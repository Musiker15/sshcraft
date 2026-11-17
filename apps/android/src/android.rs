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
}

impl eframe::App for Tablet {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.app.frame(ui);
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
        let wanted = ui.ctx().output(|o| o.ime.is_some());
        if wanted != self.keyboard {
            crate::keyboard::show(wanted);
            self.keyboard = wanted;
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
        let edges = crate::unpack_covered(crate::keyboard::covered());
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
        for text in crate::keyboard::take_text() {
            raw_input.events.push(crate::committed(text));
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
    eframe::NativeOptions { android_app: Some(android.clone()), clipboard_shortcuts: false, ..Default::default() }
}

fn show_problem(android: &AndroidApp, text: String) {
    let _ = eframe::run_native("SSHCraft", options(android), Box::new(move |_| Ok(Box::new(Problem(text)))));
}

/// Called by android-activity on its own thread once the activity exists. The process ends with
/// the activity: winit cannot make a second event loop in a process, so an activity started
/// again in a process Android kept (after Back) would otherwise stay blank.
#[allow(unsafe_code)] // Exporting the symbol android-activity looks up; nothing else is unsafe.
#[unsafe(no_mangle)]
fn android_main(android: AndroidApp) {
    run(android);
    std::process::exit(0);
}

fn run(android: AndroidApp) {
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(e) => return show_problem(&android, format!("{}\n\n{e}", tr("SSHCraft could not start."))),
    };
    let Some(data_dir) = android.internal_data_path() else {
        return show_problem(&android, tr("SSHCraft could not open its data.").to_string());
    };
    let config = SessionConfig::for_app(data_dir, runtime.handle().clone(), Arc::new(NoPresence), None);
    let (session, vault) = {
        let _inside = runtime.enter();
        match Session::open(config) {
            Ok((session, report)) => (session, report.vault),
            Err(e) => return show_problem(&android, format!("{}\n\n{e}", tr("SSHCraft could not open its data."))),
        }
    };
    let handle = android.clone();
    let _ = eframe::run_native(
        "SSHCraft",
        options(&android),
        Box::new(move |cc| {
            let mut app = SshcraftApp::new(session, &cc.egui_ctx);
            app.form = Form::Tablet;
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
            }))
        }),
    );
    // The activity is gone; the engine's tasks end with the runtime.
    runtime.shutdown_timeout(std::time::Duration::from_secs(2));
}
