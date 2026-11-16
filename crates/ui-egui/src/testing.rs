//! The app in egui_kittest on an engine for tests (built with the `testing` feature).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::sync::Arc;

use egui_kittest::Harness;
use sshcraft_engine::{Session, SessionConfig};

use crate::app::SshcraftApp;

/// No window: tests answer dialogs through `dialogs.set_override`.
struct NoWindow;

impl raw_window_handle::HasWindowHandle for NoWindow {
    fn window_handle(&self) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        Err(raw_window_handle::HandleError::Unavailable)
    }
}

impl raw_window_handle::HasDisplayHandle for NoWindow {
    fn display_handle(&self) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        Err(raw_window_handle::HandleError::Unavailable)
    }
}

/// The app on a fresh engine in `dir` (English, whatever the machine's language), its engine
/// session and the context it was made with, without a harness: for budgets that time frames.
pub fn app_on(dir: &Path) -> (SshcraftApp, Session, egui::Context) {
    app_on_with(dir, |_| {})
}

/// [`app_on`] on an engine whose test configuration `configure` changed (an agent, a pass-cli).
pub fn app_on_with(dir: &Path, configure: impl FnOnce(&mut SessionConfig)) -> (SshcraftApp, Session, egui::Context) {
    let runtime = Arc::new(tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap());
    let session = {
        let _inside = runtime.enter();
        let mut config = SessionConfig::for_tests(dir);
        configure(&mut config);
        Session::open(config).unwrap().0
    };
    // Tests read English labels, whatever the machine's language.
    let mut settings = runtime.block_on(session.execute(sshcraft_engine::Caller::User, "settings.get", serde_json::json!({}))).unwrap();
    settings["general"]["language"] = serde_json::json!("en");
    runtime.block_on(session.execute(sshcraft_engine::Caller::User, "settings.save", serde_json::json!({ "settings": settings }))).unwrap();
    let ctx = egui::Context::default();
    let mut app = SshcraftApp::new(session.clone(), &ctx);
    // Never the real settings folder: the agent link of a test lives in its own.
    app.link.dir = Some(dir.join("link"));
    // Never a real notification on the desktop of whoever runs the tests.
    app.notifier = Box::new(crate::notify::Recording::default());
    app.keep_runtime(runtime);
    (app, session, ctx)
}

/// The app on a fresh engine in `dir`, 1200 by 800 points, and the engine's session.
pub fn app_harness(dir: &Path) -> (Harness<'static, SshcraftApp>, Session) {
    app_harness_from(Harness::builder(), dir)
}

/// [`app_harness`] from `builder` (a renderer for pictures, another size).
pub fn app_harness_from(builder: egui_kittest::HarnessBuilder<SshcraftApp>, dir: &Path) -> (Harness<'static, SshcraftApp>, Session) {
    harness_for(builder, app_on(dir))
}

/// [`app_harness`] on an engine whose test configuration `configure` changed.
pub fn app_harness_with(dir: &Path, configure: impl FnOnce(&mut SessionConfig)) -> (Harness<'static, SshcraftApp>, Session) {
    harness_for(Harness::builder(), app_on_with(dir, configure))
}

fn harness_for(
    builder: egui_kittest::HarnessBuilder<SshcraftApp>,
    (app, session, _ctx): (SshcraftApp, Session, egui::Context),
) -> (Harness<'static, SshcraftApp>, Session) {
    // A frame as long as on a 60 Hz display, so double clicks fall inside egui's time for them.
    let harness = builder.with_size(egui::vec2(1200.0, 800.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SshcraftApp| {
            app.frame(ui);
            app.show_dialogs(None::<&NoWindow>);
        },
        app,
    );
    // The app was made with a context of its own: the harness's needs the system's fonts too.
    harness.ctx.add_font_provider(Arc::new(egui_system_font_provider::SystemFontProvider::new()));
    (harness, session)
}
