//! Turns the agent link on and off as the consent says (spec 9.2): the user's switch "Allow
//! agent control" and, with the suite's `AI-MCP` flag, the ArtCraft Launcher's too. The files
//! are looked at every two seconds, never every frame.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;
use sshcraft_engine::consent::{self, Consent};

use crate::app::SshcraftApp;

/// How often the consent is looked at.
pub const CHECK_EVERY: Duration = Duration::from_secs(2);

/// Reads the consent for the user's switch (the files, or a test's own).
pub type ConsentSource = Arc<dyn Fn(bool) -> Consent + Send + Sync>;

pub struct LinkWatch {
    /// The folder of the token and connection files.
    pub dir: Option<PathBuf>,
    pub source: ConsentSource,
    /// The consent at the last look.
    pub consent: Option<Consent>,
    /// Why the link could not start.
    pub error: Option<String>,
    last: Option<Instant>,
    starting: bool,
}

impl Default for LinkWatch {
    fn default() -> Self {
        Self { dir: sshcraft_platform::config_dir().ok(), source: Arc::new(consent::read), consent: None, error: None, last: None, starting: false }
    }
}

/// Looks at the consent on the next frame (the user changed the switch).
pub fn check_soon(app: &mut SshcraftApp) {
    app.link.last = None;
}

/// Starts or stops the link when the consent changed; called every frame.
pub fn tick(app: &mut SshcraftApp, ctx: &egui::Context) {
    let now = Instant::now();
    if app.link.last.is_some_and(|last| now.duration_since(last) < CHECK_EVERY) {
        return;
    }
    app.link.last = Some(now);
    ctx.request_repaint_after(CHECK_EVERY);
    let integrations = app.data.settings.integrations.clone();
    let consent = (app.link.source)(integrations.allow_agent_control);
    app.link.consent = Some(consent);
    let status = app.session.agent_link_status();
    let wrong_port = status.port.is_some_and(|p| integrations.agent_port != 0 && p != integrations.agent_port);
    if status.port.is_some() && (!consent.wanted() || wrong_port) {
        app.session.stop_agent_link();
        app.link.last = None;
        return;
    }
    if !consent.wanted() || status.port.is_some() || app.link.starting {
        if !consent.wanted() {
            app.link.error = None;
        }
        return;
    }
    let Some(dir) = app.link.dir.clone() else {
        app.link.error = Some("no settings folder for this user".into());
        return;
    };
    app.link.starting = true;
    let session = app.session.clone();
    app.runner.run_future(
        async move { session.start_agent_link(&dir, integrations.agent_port).await.map(|_| Value::Null) },
        Box::new(|app, result| {
            app.link.starting = false;
            match result {
                // The consent may have changed while it started: look again at once.
                Ok(_) => {
                    app.link.error = None;
                    check_soon(app);
                }
                // A port in use: try again at the next look, not every frame.
                Err(e) => app.link.error = Some(e.to_string()),
            }
        }),
    );
}
