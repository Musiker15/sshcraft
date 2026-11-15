#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
// The desktop app is a window program on Windows: no console window behind it.
#![cfg_attr(not(test), windows_subsystem = "windows")]
//! SSHCraft, the desktop app: opens the engine on the user's data and shows the window.

mod platform;

use std::process::ExitCode;
use std::sync::Arc;

use sshcraft_engine::{EngineError, Session, SessionConfig};
use sshcraft_ui_egui::SshcraftApp;
use sshcraft_ui_egui::i18n::tr;

use platform::{WindowHandle, WindowPresence};

/// Why the app does not start, in words for the person starting it.
fn start_problem(error: &EngineError) -> String {
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
    /// The engine's runtime lives as long as the window.
    _runtime: tokio::runtime::Runtime,
}

impl eframe::App for Shell {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.app.frame(ui);
        self.app.show_dialogs(Some(&*frame));
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
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(e) => return show_problem(format!("{}\n\n{e}", tr("SSHCraft could not start."))),
    };
    let data_dir = match sshcraft_platform::data_dir() {
        Ok(dir) => dir,
        Err(e) => return show_problem(format!("{}\n\n{e}", tr("SSHCraft could not open its data."))),
    };
    let window = WindowHandle::default();
    let config =
        SessionConfig::for_app(data_dir, runtime.handle().clone(), Arc::new(WindowPresence(window.clone())), platform::authenticator(window.clone()));
    let session = {
        let _inside = runtime.enter();
        match Session::open(config) {
            Ok((session, _report)) => session,
            Err(e) => return show_problem(start_problem(&e)),
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
    let result = eframe::run_native(
        "SSHCraft",
        options,
        Box::new(move |cc| {
            window.take_from(cc);
            let app = SshcraftApp::new(session, &cc.egui_ctx);
            Ok(Box::new(Shell { app, _runtime: runtime }))
        }),
    );
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
        assert!(start_problem(&error).contains("already open"));
        let legacy = start_problem(&EngineError::LegacyRunning);
        assert!(legacy.contains("previous version"), "{legacy}");
    }
}
