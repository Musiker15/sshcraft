//! The app lock: after `general.lock_after_minutes` without input the window shows only a lock
//! screen (no terminal, no host, nothing for a screen reader or a screenshot) while sessions go
//! on. Unlocking asks Windows Hello (polkit on Linux and FreeBSD); the approval for keys and
//! passwords is asked again too.

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

/// Whether the desktop can unlock with the user's presence (Windows Hello); the app starts with
/// this as `lock_available`. Android sets its own, Linux and FreeBSD theirs from polkit. macOS
/// does not offer the lock yet.
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

/// Locks after the idle time, then notes input. Runs in `logic`, so also while the window is
/// hidden or covered; the idle time is measured before this frame's input counts, so the input
/// that brings the window back (a click, a focus change) cannot reset the timer first.
pub fn tick(app: &mut SshcraftApp, ctx: &egui::Context) {
    let minutes = app.data.settings.general.lock_after_minutes;
    let limit = Duration::from_secs(u64::from(minutes) * 60);
    let idle = app.lock.last_input.elapsed();
    if minutes > 0 && app.lock_available && !app.lock.locked && idle >= limit {
        lock(app);
    }
    if !app.lock.locked && ctx.input(|i| !i.events.is_empty() || i.pointer.is_moving()) {
        app.lock.last_input = Instant::now();
    }
    // Woken again when the time is up, also without input.
    if minutes > 0 && app.lock_available && !app.lock.locked {
        ctx.request_repaint_after(limit.saturating_sub(app.lock.last_input.elapsed()).max(Duration::from_secs(1)));
    }
}

pub fn show(app: &mut SshcraftApp, ui: &mut egui::Ui) {
    let tokens = Tokens::get(ui.ctx());
    egui::CentralPanel::default().show(ui, |ui| {
        // The top of the lock screen moves the window, as a title bar does (no menus here).
        let top = egui::Rect::from_min_size(ui.max_rect().min, egui::vec2(ui.max_rect().width(), 32.0));
        let _ = craft_window::title_bar::empty_area(ui, top);
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() / 3.0);
            crate::widgets::buttons::title(ui, tr("SSHCraft is locked"));
            ui.add_space(8.0);
            let how = if app.form == crate::app::Form::Tablet {
                tr("Sessions keep running. Unlock with your fingerprint, face or screen lock to see them.")
            } else {
                if cfg!(windows) {
                    tr("Sessions keep running. Unlock with Windows Hello to see them.")
                } else {
                    tr("Sessions keep running. Unlock with your password or fingerprint to see them.")
                }
            };
            ui.label(egui::RichText::new(how).color(tokens.text_dim));
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
