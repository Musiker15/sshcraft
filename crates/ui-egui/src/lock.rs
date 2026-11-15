//! The app lock: after `general.lock_after_minutes` without input the window shows only a lock
//! screen (no terminal, no host, nothing for a screen reader or a screenshot) while sessions go
//! on. Unlocking asks Windows Hello; the approval for keys and passwords is asked again too.

use std::time::{Duration, Instant};

use serde_json::json;

use crate::app::SshcraftApp;
use crate::i18n::tr;
use crate::theme::Tokens;

pub struct LockState {
    pub last_input: Instant,
    pub locked: bool,
    pub unlocking: bool,
    pub error: Option<String>,
}

impl Default for LockState {
    fn default() -> Self {
        Self { last_input: Instant::now(), locked: false, unlocking: false, error: None }
    }
}

/// Whether this system can unlock with the user's presence (Windows Hello). Elsewhere the lock
/// is not offered yet.
pub fn supported() -> bool {
    cfg!(windows)
}

/// Locks now: the window shows the lock screen and keys and passwords need the presence again.
pub fn lock(app: &mut SshcraftApp) {
    if app.lock.locked {
        return;
    }
    app.lock.locked = true;
    app.lock.error = None;
    app.run_reported("app.lock", json!({}));
}

/// Notes input and locks after the idle time.
pub fn tick(app: &mut SshcraftApp, ctx: &egui::Context) {
    if ctx.input(|i| !i.events.is_empty() || i.pointer.is_moving()) {
        app.lock.last_input = Instant::now();
    }
    let minutes = app.data.settings.general.lock_after_minutes;
    if minutes > 0 && supported() && !app.lock.locked && app.lock.last_input.elapsed() >= Duration::from_secs(u64::from(minutes) * 60) {
        lock(app);
    }
    // Checked again when the time is up, also without input.
    if minutes > 0 && !app.lock.locked {
        ctx.request_repaint_after(Duration::from_secs(30));
    }
}

pub fn show(app: &mut SshcraftApp, ui: &mut egui::Ui) {
    let tokens = Tokens::get(ui.ctx());
    egui::CentralPanel::default().show(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() / 3.0);
            ui.heading(tr("SSHCraft is locked"));
            ui.add_space(8.0);
            ui.label(egui::RichText::new(tr("Sessions keep running. Unlock with Windows Hello to see them.")).color(tokens.text_dim));
            ui.add_space(12.0);
            if let Some(error) = &app.lock.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            let button = ui.add_enabled(!app.lock.unlocking, egui::Button::new(tr("Unlock")));
            if button.clicked() {
                app.lock.unlocking = true;
                app.runner.run(
                    "app.unlock",
                    json!({}),
                    Box::new(|app, result| {
                        app.lock.unlocking = false;
                        match result {
                            Ok(_) => {
                                app.lock.locked = false;
                                app.lock.error = None;
                                app.lock.last_input = Instant::now();
                            }
                            Err(e) => app.lock.error = Some(e.to_string()),
                        }
                    }),
                );
            }
        });
    });
}
