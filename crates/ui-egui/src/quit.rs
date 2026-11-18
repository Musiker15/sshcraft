//! Quitting: the window's close button, the system's close (Alt+F4, the taskbar) and the menu's
//! Quit all end here. With sessions running, the app asks first; Cancel keeps everything running.
//! The decision runs in `logic`, also while the window is minimized or covered (eframe draws no
//! frame then), and the question is drawn in the frame.

use crate::app::SshcraftApp;
use crate::i18n::{tr, trf};

/// Where quitting stands.
#[derive(Debug, Clone, Default)]
pub struct Quit {
    /// The question is shown.
    pub asking: bool,
    /// The user said yes: the next close goes through.
    confirmed: bool,
    /// The menu's Quit asked for a close.
    wanted: bool,
}

/// The sessions a quit would end: open terminals whose session has not ended.
pub fn sessions(app: &SshcraftApp) -> usize {
    app.terminals.values().filter(|t| !matches!(t.status, Some(sshcraft_engine::SessionStatus::Ended { .. }))).count()
}

/// The port forwards a quit would end: rules that run or are starting, also without a terminal.
pub fn forwards(app: &SshcraftApp) -> usize {
    app.data.forwards.values().filter(|s| matches!(s.get("state").and_then(serde_json::Value::as_str), Some("running" | "starting"))).count()
}

/// Everything a quit would end.
pub fn running(app: &SshcraftApp) -> usize {
    sessions(app).saturating_add(forwards(app))
}

/// What runs, in words: "2 sessions and 1 port forward".
fn running_text(sessions: usize, forwards: usize) -> String {
    let count = |n: usize, one: &'static str, many: &'static str| if n == 1 { tr(one).to_string() } else { trf(many, &[("n", &n.to_string())]) };
    let parts: Vec<String> = [(sessions, "1 session", "{n} sessions"), (forwards, "1 port forward", "{n} port forwards")]
        .into_iter()
        .filter(|(n, ..)| *n > 0)
        .map(|(n, one, many)| count(n, one, many))
        .collect();
    parts.join(tr(" and "))
}

/// The menu's Quit: asks like the close button does.
pub fn request(app: &mut SshcraftApp) {
    app.ui.quit.wanted = true;
}

/// Decides a close request of this frame. Call it in `logic`, before `craft_quit::on_close_request`,
/// which then sees the `CancelClose`. Running twice in a frame changes nothing.
pub fn logic(app: &mut SshcraftApp, ctx: &egui::Context) {
    let close = ctx.input(|i| i.viewport().close_requested());
    let wanted = std::mem::take(&mut app.ui.quit.wanted);
    if !(close || wanted) {
        return;
    }
    if app.ui.quit.confirmed || running(app) == 0 {
        if wanted && !close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        return;
    }
    if close {
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
    }
    if !app.ui.quit.asking {
        app.ui.quit.asking = true;
        // The question needs the window: a minimised or hidden one comes back.
        crate::dropdown::reveal(app, ctx);
    }
}

/// The question, while it is asked.
pub fn show(app: &mut SshcraftApp, ctx: &egui::Context) {
    if !app.ui.quit.asking {
        return;
    }
    let count = running(app);
    if count == 0 {
        // The sessions ended while the question was open: nothing left to ask about.
        app.ui.quit.asking = false;
        app.ui.quit.confirmed = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        return;
    }
    let (mut cancel, mut quit) = (false, false);
    let modal = crate::widgets::modal(ctx, egui::Id::unique("sshcraft-quit")).show(ctx, |ui| {
        crate::widgets::fit_dialog(ui);
        ui.set_width(crate::widgets::fit(ui, 420.0));
        crate::widgets::buttons::title(ui, tr("Quit SSHCraft?"));
        ui.add_space(6.0);
        let body = trf("Still running: {what}. Quitting ends it.", &[("what", &running_text(sessions(app), forwards(app)))]);
        ui.label(body);
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button(tr("Cancel")).clicked() {
                cancel = true;
            }
            if ui.button(tr("End sessions and quit")).clicked() {
                quit = true;
            }
        });
    });
    if quit {
        app.ui.quit.asking = false;
        app.ui.quit.confirmed = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    } else if cancel || modal.should_close() {
        app.ui.quit.asking = false;
    }
}
