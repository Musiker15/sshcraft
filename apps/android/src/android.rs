//! The Android entry point: the engine on the app's private storage, eframe on the activity.

use std::sync::Arc;

use sshcraft_engine::{Session, SessionConfig};
use sshcraft_fido::presence::{Presence, UserPresence};
use sshcraft_ui_egui::SshcraftApp;
use sshcraft_ui_egui::app::Form;
use sshcraft_ui_egui::i18n::tr;
use winit::platform::android::activity::AndroidApp;

/// Android has no Windows Hello; the engine asks nothing of it.
struct NoPresence;

#[async_trait::async_trait]
impl UserPresence for NoPresence {
    async fn verify(&self, _reason: &str) -> Presence {
        Presence::Unavailable
    }
}

/// The system clipboard's text, if there is any.
fn clipboard_text() -> Option<String> {
    android_clipboard::get_text().ok()
}

/// The activity around the app.
struct Tablet {
    app: SshcraftApp,
    android: AndroidApp,
    /// The content rect seen last, to notice the keyboard or a rotation.
    content: (i32, i32, i32, i32),
    /// What the system covered last (keyboard, bars), from MainActivity.
    edges: (i32, i32, i32, i32),
    content_changed: bool,
    last_input: std::time::Instant,
    /// Whether the on-screen keyboard was asked for last.
    keyboard: bool,
    /// The last request for text and when it was made (see crate::keep_keyboard).
    last_ime: Option<(egui::output::IMEOutput, std::time::Instant)>,
    /// The field (and its input context) that had the keyboard last.
    owner: Option<(egui::Id, u64)>,
    /// When to show the keyboard again after another field took it.
    reshow: Option<std::time::Instant>,
    /// The number of open sessions told to the activity last (crate::background).
    sessions: usize,
    /// Calls into MainActivity that failed, each said once.
    reported: Vec<String>,
    /// The word the keyboard is composing (crate::Composer).
    composer: crate::Composer,
}

impl Tablet {
    /// Says once why a call into the Java side failed: without it the keyboard or the insets
    /// stop working with nothing to show for it.
    fn report<T>(&mut self, result: Result<T, String>) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(why) => {
                if !self.reported.contains(&why) && self.reported.len() < 8 {
                    self.app.toasts.error(&format!("Android: {why}"));
                    self.reported.push(why);
                }
                None
            }
        }
    }
}

impl eframe::App for Tablet {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.app.frame(ui);
        // While sessions are open, a foreground service keeps them and their network running in
        // the background; told only when the number changes.
        let open = sshcraft_ui_egui::notify::open_sessions(&self.app);
        if open != self.sessions {
            match crate::background::sessions(open) {
                Ok(()) => self.sessions = open,
                Err(e) => log::warn!("the session service could not be told: {e}"),
            }
        }
        // What the app copied goes on the system clipboard; egui's own clipboard on Android
        // reaches no other app.
        let copied: Vec<String> = ui.ctx().output(|o| {
            o.commands
                .iter()
                .filter_map(|c| match c {
                    egui::OutputCommand::CopyText(text) => Some(text.clone()),
                    _ => None,
                })
                .collect()
        });
        for text in copied {
            // A clipboard the system refuses keeps the text in egui's clipboard only.
            let _ = android_clipboard::set_text(text);
        }
        // The keyboard follows whether a terminal or a field wants text, through the view with
        // the text connection (crate::keyboard).
        let ime = ui.ctx().output(|o| o.ime.clone());
        let tapped = ui.input(|i| i.pointer.any_pressed());
        // A tap into the terminal or field that wants text, not onto the key bar beside it.
        let press = ui.input(|i| i.pointer.press_origin());
        let asked_now = ime.as_ref().is_some_and(|ime| {
            // The widget's whole area: a field reports only where its text is.
            let area = ui.ctx().read_response(ime.owner).map_or(ime.rect, |r| r.rect.union(ime.rect));
            press.is_some_and(|at| area.contains(at))
        });
        match ime {
            Some(ime) => self.last_ime = Some((ime, std::time::Instant::now())),
            None => {
                // A frame without a request does not close the keyboard at once: opening it moves
                // the interface up and the field may lose its focus for a frame, and a tap from one
                // field to the next has a frame in between. Closing and opening again at once is
                // what Android drops. The request of a moment ago stands, for egui-winit too, and
                // a field that lost its focus without a tap gets it back.
                match &self.last_ime {
                    Some((ime, at)) if crate::keep_keyboard(at.elapsed()) => {
                        let ime = ime.clone();
                        if !tapped && ui.ctx().memory(|m| m.focused().is_none()) {
                            ui.ctx().memory_mut(|m| m.request_focus(ime.owner));
                        }
                        ui.ctx().output_mut(|o| o.ime = Some(ime));
                        ui.ctx().request_repaint_after(crate::KEEP_KEYBOARD);
                    }
                    _ => self.last_ime = None,
                }
            }
        }
        // Another field took the keyboard: egui-winit resets the text input for it, which hides
        // the keyboard through NativeActivity's own view. It is shown again a moment later, after
        // that reset went through.
        let owner = self.last_ime.as_ref().map(|(ime, _)| (ime.owner, ime.context_generation));
        if owner.is_some() && owner != self.owner {
            if self.owner.is_some() {
                self.reshow = Some(std::time::Instant::now() + crate::RESHOW_AFTER);
                ui.ctx().request_repaint_after(crate::RESHOW_AFTER);
            }
            self.owner = owner;
        }
        let wanted = self.last_ime.is_some();
        // The user closed the keyboard and tapped the terminal or field again: nothing changed
        // for egui, so it is asked for here. The height is the whole window's, not the part the
        // keyboard leaves free.
        let window_px = ui.ctx().viewport_rect().height() * ui.ctx().pixels_per_point();
        let up = crate::keyboard_up(self.edges.3, window_px);
        // Asked a moment after the tap, as after another field took the keyboard: asked during
        // the tap itself, Android drops it.
        if wanted && self.reshow.is_none() && crate::show_again(tapped, asked_now, up) {
            self.reshow = Some(std::time::Instant::now() + crate::RESHOW_AFTER);
            ui.ctx().request_repaint_after(crate::RESHOW_AFTER);
        }
        let reshow = wanted && self.reshow.is_some_and(|at| std::time::Instant::now() >= at);
        if wanted != self.keyboard || reshow {
            let shown = crate::keyboard::show(wanted);
            self.report(shown);
            self.keyboard = wanted;
            self.reshow = None;
        }
        if !wanted {
            self.owner = None;
            self.reshow = None;
        }
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.app.logic(ctx);
        // winit tells nothing when the on-screen keyboard opens or closes, so the frame looks
        // for it itself (crate::repaint_after).
        ctx.request_repaint_after(crate::repaint_after(std::mem::take(&mut self.content_changed), self.last_input.elapsed()));
    }

    /// The parts the system covers (status bar, navigation bar, on-screen keyboard) stay free,
    /// and a hardware keyboard's clipboard keys use the system clipboard.
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        let rect = self.android.content_rect();
        let content = (rect.left, rect.top, rect.right, rect.bottom);
        let covered = crate::keyboard::covered();
        let edges = self.report(covered).map_or(self.edges, crate::unpack_covered);
        if (content, edges) != (self.content, self.edges) {
            self.content = content;
            self.edges = edges;
            self.content_changed = true;
        }
        if !raw_input.events.is_empty() {
            self.last_input = std::time::Instant::now();
        }
        if let Some(window) = raw_input.screen_rect {
            let ppp = ctx.pixels_per_point();
            // The window's insets (Android 15 and later draw edge to edge and no longer shrink
            // the window for the keyboard), and the content rect where an older one still does.
            let covered = crate::widest(crate::covered_edges(edges, ppp), crate::covered(window, content, ppp));
            raw_input.safe_area_insets = Some(egui::SafeAreaInsets(covered));
        }
        crate::clipboard_keys(&mut raw_input.events, clipboard_text);
        let taken = crate::keyboard::take_text();
        for entry in self.report(taken).unwrap_or_default() {
            raw_input.events.extend(self.composer.events(entry));
        }
    }
}

/// A screen that says why SSHCraft did not start.
struct Problem(String);

impl eframe::App for Problem {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.add_space(48.0);
        ui.label(&self.0);
    }
}

fn options(android: &AndroidApp) -> eframe::NativeOptions {
    // egui-winit's clipboard keys use its own clipboard, which on Android reaches no other app;
    // clipboard_keys handles them with Android's instead.
    let mut options = eframe::NativeOptions { android_app: Some(android.clone()), clipboard_shortcuts: false, ..Default::default() };
    // Ask the GPU for what it has, not for wgpu's desktop defaults: weaker mobile GPUs (and the
    // emulator's) have smaller limits, and asking for more ends the app before its first frame.
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup {
        // No debug labels or validation: some mobile Vulkan drivers (the emulator's among them)
        // crash naming GPU objects, and nobody reads the names on a tablet.
        setup.instance_descriptor.flags = eframe::wgpu::InstanceFlags::empty();
        #[cfg(feature = "gl")]
        {
            setup.instance_descriptor.backends = eframe::wgpu::Backends::GL;
        }
        setup.device_descriptor = Arc::new(|adapter| {
            let limits = adapter.limits();
            eframe::wgpu::DeviceDescriptor {
                label: Some("sshcraft"),
                required_limits: eframe::wgpu::Limits { max_texture_dimension_2d: limits.max_texture_dimension_2d.min(8192), ..limits },
                ..Default::default()
            }
        });
    }
    options
}

fn show_problem(android: &AndroidApp, text: String) {
    log::error!("{text}");
    if let Err(e) = eframe::run_native("SSHCraft", options(android), Box::new(move |_| Ok(Box::new(Problem(text))))) {
        log::error!("the window could not open: {e}");
    }
}

/// Called by android-activity on its own thread once the activity exists. The process ends with
/// the activity: winit cannot make a second event loop in a process, so an activity started
/// again in a process Android kept (after Back) would otherwise stay blank.
#[allow(unsafe_code)] // Exporting the symbol android-activity looks up; nothing else is unsafe.
#[unsafe(no_mangle)]
fn android_main(android: AndroidApp) {
    // Rust's log (the app's, eframe's, wgpu's) goes to logcat under the tag "sshcraft"; without it
    // a failure on Android leaves no trace (`adb logcat -s sshcraft`).
    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("sshcraft"));
    log::info!("SSHCraft {} starts", env!("CARGO_PKG_VERSION"));
    run(android);
    log::info!("SSHCraft ends");
    std::process::exit(0);
}

fn run(android: AndroidApp) {
    crate::keyboard::set_activity(android.activity_as_ptr());
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(e) => return show_problem(&android, format!("{}\n\n{e}", tr("SSHCraft could not start."))),
    };
    let Some(data_dir) = android.internal_data_path() else {
        return show_problem(&android, tr("SSHCraft could not open its data.").to_string());
    };
    let mut config = SessionConfig::for_app(data_dir.clone(), runtime.handle().clone(), Arc::new(NoPresence), None);
    config.settings = crate::settings_location(&data_dir);
    let (session, vault) = {
        let _inside = runtime.enter();
        match Session::open(config) {
            Ok((session, report)) => (session, report.vault),
            Err(e) => return show_problem(&android, format!("{}\n\n{e}", tr("SSHCraft could not open its data."))),
        }
    };
    let handle = android.clone();
    let shown = eframe::run_native(
        "SSHCraft",
        options(&android),
        Box::new(move |cc| {
            let mut app = SshcraftApp::new(session, &cc.egui_ctx);
            app.form = Form::Tablet;
            app.notifier = Box::new(crate::background::Notices);
            app.paste_source = Some(Arc::new(clipboard_text));
            app.report_vault(vault);
            Ok(Box::new(Tablet {
                app,
                android: handle,
                content: (0, 0, 0, 0),
                edges: (0, 0, 0, 0),
                content_changed: true,
                last_input: std::time::Instant::now(),
                keyboard: false,
                last_ime: None,
                owner: None,
                reshow: None,
                sessions: 0,
                reported: Vec::new(),
                composer: crate::Composer::default(),
            }))
        }),
    );
    if let Err(e) = shown {
        log::error!("the window could not open: {e}");
    }
    // The activity is gone; the engine's tasks end with the runtime.
    runtime.shutdown_timeout(std::time::Duration::from_secs(2));
}
