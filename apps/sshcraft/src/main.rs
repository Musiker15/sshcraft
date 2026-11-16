#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
// The desktop app is a window program on Windows: no console window behind it.
#![cfg_attr(not(test), windows_subsystem = "windows")]
//! SSHCraft, the desktop app: opens the engine on the user's data and shows the window.

mod control;
mod platform;

use std::process::ExitCode;
use std::sync::Arc;

use sshcraft_engine::{EngineError, Session, SessionConfig};
use sshcraft_ui_egui::SshcraftApp;
use sshcraft_ui_egui::i18n::{Lang, set_language, tr};

use platform::{WindowHandle, WindowPresence};

/// How long closing the window waits for the engine's last tasks before the process ends.
const SHUTDOWN_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

/// Ends the engine's runtime without waiting for a blocking task that never ends: the process
/// must not live on without a window and keep the data locked.
fn finish(runtime: tokio::runtime::Runtime) {
    runtime.shutdown_timeout(SHUTDOWN_WAIT);
}

/// The language of the start windows: the settings are not open yet, so the system's.
fn system_language() -> Lang {
    Lang::resolve(sshcraft_engine::settings::LanguageSetting::Auto)
}

/// Why the app does not start, in words for the person starting it, in `lang`.
fn start_problem(error: &EngineError, lang: Lang) -> String {
    set_language(lang);
    match error {
        EngineError::DataInUse => tr("SSHCraft is already open. Switch to its window.").to_string(),
        EngineError::LegacyRunning => {
            tr("The previous version of SSHCraft is still running. Quit it (also in the notification area), then start SSHCraft again.").to_string()
        }
        other => format!("{}\n\n{other}", tr("SSHCraft could not open its data.")),
    }
}

/// Leaves what to open for the SSHCraft that already runs; whether that worked.
fn hand_over(open: &control::Open) -> bool {
    let Ok(config_dir) = sshcraft_platform::config_dir() else { return false };
    let dir = sshcraft_ui_egui::handoff::dir_in(&config_dir);
    match open {
        control::Open::Link((url, _)) => sshcraft_ui_egui::handoff::write(&dir, url),
        control::Open::Host(id) => sshcraft_ui_egui::handoff::write_host(&dir, *id),
    }
    .is_ok()
}

/// The window around the app.
struct Shell {
    app: SshcraftApp,
    /// The control channel, while the app runs with `--control`.
    _control: Option<sshcraft_link::Server>,
}

impl eframe::App for Shell {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.app.frame(ui);
        self.app.show_dialogs(Some(&*frame));
    }

    /// Also while the window is minimized or covered (no frame then): the agent link follows the
    /// consent, and the control channel answers.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.app.logic(ctx);
        // Last, so it sees the quit question's `CancelClose` (craft-quit: macOS quits through
        // AppKit, not by closing the window).
        craft_quit::on_close_request(ctx);
    }

    /// The native menu's keys and the control channel's clicks and keys go in front of the
    /// frame, as a person's would.
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        sshcraft_ui_egui::menus::native_raw_input(&mut self.app, raw_input);
        raw_input.events.extend(self.app.take_control_input());
    }
}

/// The setting "Use system title bar", read before the window exists. Unreadable settings keep
/// the app's own bar, the default.
fn title_bar_pref(runtime: &tokio::runtime::Runtime, session: &Session) -> craft_window::policy::TitleBarPref {
    let read = runtime.block_on(session.execute(sshcraft_engine::Caller::User, "settings.get", serde_json::json!({})));
    match read.ok().and_then(|v| serde_json::from_value::<sshcraft_engine::settings::EngineSettings>(v).ok()) {
        Some(settings) => sshcraft_ui_egui::window::title_bar_pref(&settings),
        None => craft_window::policy::TitleBarPref::default(),
    }
}

/// The main window: its size, icon and frame. The app id names the window to the desktop (the
/// Wayland app id, the X11 class), so it matches `de.musiker15.sshcraft.desktop` and the Flatpak.
fn main_viewport(frame: &craft_window::policy::Frame) -> egui::ViewportBuilder {
    let viewport = egui::ViewportBuilder::default()
        .with_title("SSHCraft")
        .with_app_id(APP_ID)
        .with_inner_size([1200.0, 780.0])
        .with_min_inner_size([720.0, 480.0])
        .with_icon(sshcraft_ui_egui::widgets::icons::app_icon());
    craft_window::policy::apply(viewport, frame)
}

/// SSHCraft's id on every platform: keyring service, data folder, desktop entry, bundle id.
const APP_ID: &str = "de.musiker15.sshcraft";

/// A small window that explains why SSHCraft did not start.
struct Problem(String);

impl eframe::App for Problem {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.add_space(12.0);
        ui.label(&self.0);
        ui.add_space(12.0);
        if ui.button(tr("Close")).clicked() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn show_problem(text: String) -> ExitCode {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("SSHCraft").with_inner_size([460.0, 160.0]).with_resizable(false),
        ..Default::default()
    };
    let _ = eframe::run_native("SSHCraft", options, Box::new(move |_| Ok(Box::new(Problem(text)))));
    ExitCode::FAILURE
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (args, open) = match control::take_open(&args) {
        Ok(found) => found,
        Err(problem) => {
            set_language(system_language());
            return show_problem(problem);
        }
    };
    let control_args = match control::parse(&args) {
        Ok(found) => found,
        Err(problem) => {
            set_language(system_language());
            return show_problem(problem);
        }
    };
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(e) => {
            set_language(system_language());
            return show_problem(format!("{}\n\n{e}", tr("SSHCraft could not start.")));
        }
    };
    let data_dir = match sshcraft_platform::data_dir() {
        Ok(dir) => dir,
        Err(e) => {
            set_language(system_language());
            return show_problem(format!("{}\n\n{e}", tr("SSHCraft could not open its data.")));
        }
    };
    let window = WindowHandle::default();
    let config =
        SessionConfig::for_app(data_dir, runtime.handle().clone(), Arc::new(WindowPresence(window.clone())), platform::authenticator(window.clone()));
    let (session, vault) = {
        let _inside = runtime.enter();
        match Session::open(config) {
            Ok((session, report)) => (session, report.vault),
            // SSHCraft runs already: it takes the link from the handoff folder.
            Err(sshcraft_engine::EngineError::DataInUse) if open.as_ref().is_some_and(hand_over) => return ExitCode::SUCCESS,
            Err(e) => return show_problem(start_problem(&e, system_language())),
        }
    };
    // Who draws the title bar: the app's own bar unless the user chose the system's.
    let pref = title_bar_pref(&runtime, &session);
    let decision = craft_window::policy::decide_for(craft_window::Prefix::new("SSHCRAFT"), craft_window::policy::Backend::from_env(), pref);
    let window_frame = decision.frame;
    let options = eframe::NativeOptions { viewport: main_viewport(&window_frame), ..Default::default() };
    let handle = runtime.handle().clone();
    let result = eframe::run_native(
        "SSHCraft",
        options,
        Box::new(move |cc| {
            window.take_from(cc);
            let mut app = SshcraftApp::new(session.clone(), &cc.egui_ctx);
            app.window_frame = window_frame;
            app.report_vault(vault);
            app.handoff_dir = sshcraft_platform::config_dir().ok().map(|d| sshcraft_ui_egui::handoff::dir_in(&d));
            match &open {
                Some(control::Open::Link((_, target))) => sshcraft_ui_egui::quick_connect::open(&mut app, target.name()),
                Some(control::Open::Host(id)) => app.ui.connect_request = Some(sshcraft_ui_egui::state::ConnectRequest::Host(*id)),
                None => {}
            }
            #[cfg(target_os = "macos")]
            sshcraft_ui_egui::menus::install_native(&mut app, &cc.egui_ctx);
            let control = match &control_args {
                Some(control_args) => {
                    let queue = app.enable_control(&cc.egui_ctx);
                    match handle.block_on(control::start(control_args, session, queue)) {
                        Ok(server) => Some(server),
                        Err(problem) => {
                            app.toasts.error(&format!("--control: {problem}"));
                            None
                        }
                    }
                }
                None => None,
            };
            Ok(Box::new(Shell { app, _control: control }))
        }),
    );
    // The window and the app are gone; the engine's tasks get a moment to end.
    finish(runtime);
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_names_the_app_to_the_desktop() {
        let viewport = main_viewport(&craft_window::policy::Frame::Native);
        assert_eq!(viewport.app_id.as_deref(), Some("de.musiker15.sshcraft"));
    }

    /// A second start on the same data does not open it and explains why.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_second_start_explains_and_stops() {
        let dir = tempfile::tempdir().unwrap();
        let (_first, _) = Session::open(SessionConfig::for_tests(dir.path())).unwrap();
        let Err(error) = Session::open(SessionConfig::for_tests(dir.path())) else { panic!("a second engine opened the same data") };
        assert_eq!(error, EngineError::DataInUse);
        assert!(start_problem(&error, Lang::En).contains("already open"));
        let legacy = start_problem(&EngineError::LegacyRunning, Lang::En);
        assert!(legacy.contains("previous version"), "{legacy}");
    }

    /// The window that explains a failed start speaks the system's language.
    #[test]
    fn the_start_problem_is_in_the_given_language() {
        assert!(start_problem(&EngineError::DataInUse, Lang::De).contains("schon geöffnet"));
        assert!(start_problem(&EngineError::DataInUse, Lang::En).contains("already open"));
    }

    /// A blocking task that never ends does not keep a closed window's process (and its data
    /// lock) alive.
    #[test]
    fn closing_does_not_wait_for_a_stuck_task() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.spawn_blocking(|| std::thread::sleep(std::time::Duration::from_secs(30)));
        let start = std::time::Instant::now();
        finish(runtime);
        assert!(start.elapsed() < std::time::Duration::from_secs(10), "{:?}", start.elapsed());
    }
}
