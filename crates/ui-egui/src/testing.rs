//! The app in egui_kittest on an engine for tests (built with the `testing` feature).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::sync::Arc;

use egui_kittest::Harness;
use sshcraft_engine::{Session, SessionConfig};

use crate::app::SshcraftApp;

/// The app on a fresh engine in `dir`, 1200 by 800 points, and the engine's session.
pub fn app_harness(dir: &Path) -> (Harness<'static, SshcraftApp>, Session) {
    let runtime = Arc::new(tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap());
    let session = {
        let _inside = runtime.enter();
        Session::open(SessionConfig::for_tests(dir)).unwrap().0
    };
    let ctx = egui::Context::default();
    let mut app = SshcraftApp::new(session.clone(), &ctx);
    app.keep_runtime(runtime);
    let harness = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut SshcraftApp| app.frame(ui), app);
    (harness, session)
}
