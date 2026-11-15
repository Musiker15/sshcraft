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

    /// The control channel's clicks and keys go in front of the frame, as a person's would.
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        raw_input.events.extend(self.app.take_control_input());
    }
}

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
    let session = {
        let _inside = runtime.enter();
        match Session::open(config) {
            Ok((session, _report)) => session,
            Err(e) => return show_problem(start_problem(&e, system_language())),
        }
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("SSHCraft")
            .with_inner_size([1200.0, 780.0])
            .with_min_inner_size([720.0, 480.0])
            .with_icon(sshcraft_ui_egui::widgets::icons::app_icon()),
        ..Default::default()
    };
    let handle = runtime.handle().clone();
    let result = eframe::run_native(
        "SSHCraft",
        options,
        Box::new(move |cc| {
            window.take_from(cc);
            let mut app = SshcraftApp::new(session.clone(), &cc.egui_ctx);
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
