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
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.app.logic(ctx);
    }

    /// The parts the system covers (status bar, navigation bar, on-screen keyboard) stay free,
    /// and a hardware keyboard's clipboard keys use the system clipboard.
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        let content = self.android.content_rect();
        if let Some(window) = raw_input.screen_rect {
            let covered = crate::covered(window, (content.left, content.top, content.right, content.bottom), ctx.pixels_per_point());
            raw_input.safe_area_insets = Some(egui::SafeAreaInsets(covered));
        }
        crate::clipboard_keys(&mut raw_input.events, clipboard_text);
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

/// Called by android-activity on its own thread once the activity exists.
#[allow(unsafe_code)] // Exporting the symbol android-activity looks up; nothing else is unsafe.
#[unsafe(no_mangle)]
fn android_main(android: AndroidApp) {
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
            Ok(Box::new(Tablet { app, android: handle }))
        }),
    );
    // The activity is gone; the engine's tasks end with the runtime.
    runtime.shutdown_timeout(std::time::Duration::from_secs(2));
}
