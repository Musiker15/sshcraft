//! Showing and hiding the window on the dropdown shortcut (registered by the app): hidden when
//! it is in front, otherwise brought to the top of the screen and focused.

use egui::{ViewportCommand, pos2};

use crate::app::SshcraftApp;

/// Whether hiding takes the window off the screen (Windows, macOS) or minimises it.
const HIDES: bool = !cfg!(any(target_os = "linux", target_os = "freebsd"));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toggle {
    Hide,
    Show,
}

/// What a press does: a window that is shown, in front and not minimised goes; any other comes.
pub fn decide(shown: bool, focused: bool, minimized: bool) -> Toggle {
    if shown && focused && !minimized { Toggle::Hide } else { Toggle::Show }
}

/// Brings the window back: shown, not minimised, in front. Every way back (the shortcut, a
/// second start, a quit question) goes through here, so a hidden window is never lost.
pub fn reveal(app: &mut SshcraftApp, ctx: &egui::Context) {
    ctx.send_viewport_cmd(ViewportCommand::Visible(true));
    ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd(ViewportCommand::Focus);
    app.dropdown_hidden = false;
    ctx.request_repaint();
}

/// The shortcut was pressed.
pub fn toggle(app: &mut SshcraftApp, ctx: &egui::Context) {
    let (focused, minimized, monitor, outer) = ctx.input(|i| {
        let v = i.viewport();
        (v.focused.unwrap_or(false), v.minimized.unwrap_or(false), v.monitor_size, v.outer_rect)
    });
    match decide(!app.dropdown_hidden, focused, minimized) {
        Toggle::Hide if HIDES => {
            ctx.send_viewport_cmd(ViewportCommand::Visible(false));
            app.dropdown_hidden = true;
        }
        // On Linux and FreeBSD a global shortcut can stop working unnoticed (Wayland): the
        // window goes to the taskbar instead, where it can always be found.
        Toggle::Hide => ctx.send_viewport_cmd(ViewportCommand::Minimized(true)),
        Toggle::Show => {
            // Down from the top edge, centred, when the window is on the monitor egui measures
            // (its origin is unknown, so a window on another monitor stays where it is).
            if let (Some(monitor), Some(outer)) = (monitor, outer)
                && (0.0..monitor.x).contains(&outer.min.x)
            {
                let x = ((monitor.x - outer.width()) / 2.0).max(0.0);
                ctx.send_viewport_cmd(ViewportCommand::OuterPosition(pos2(x, 0.0)));
            }
            reveal(app, ctx);
        }
    }
    ctx.request_repaint();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_in_front_goes_and_any_other_comes() {
        assert_eq!(decide(true, true, false), Toggle::Hide);
        assert_eq!(decide(true, false, false), Toggle::Show, "behind another program");
        assert_eq!(decide(true, true, true), Toggle::Show, "minimised");
        assert_eq!(decide(false, false, false), Toggle::Show, "hidden");
    }
}
