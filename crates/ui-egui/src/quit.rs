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
pub fn running(app: &SshcraftApp) -> usize {
    app.terminals.values().filter(|t| !matches!(t.status, Some(sshcraft_engine::SessionStatus::Ended { .. }))).count()
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
        // The question needs the window: a minimized one comes back.
        if ctx.input(|i| i.viewport().minimized) == Some(true) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        ctx.request_repaint();
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
    let modal = egui::Modal::new(egui::Id::unique("sshcraft-quit")).show(ctx, |ui| {
        ui.set_width(420.0);
        ui.heading(tr("Quit SSHCraft?"));
        ui.add_space(6.0);
        let body = if count == 1 {
            tr("A session is running. It ends when SSHCraft quits.").to_string()
        } else {
            trf("{n} sessions are running. They end when SSHCraft quits.", &[("n", &count.to_string())])
        };
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
