//! Showing and hiding the window on the dropdown shortcut (registered by the app): hidden when
//! it is in front, otherwise brought to the top of the screen and focused.

use egui::{ViewportCommand, pos2};

use crate::app::SshcraftApp;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toggle {
    Hide,
    Show,
}

/// What a press does: a window that is shown, in front and not minimised goes; any other comes.
pub fn decide(shown: bool, focused: bool, minimized: bool) -> Toggle {
    if shown && focused && !minimized { Toggle::Hide } else { Toggle::Show }
}

/// The shortcut was pressed.
pub fn toggle(app: &mut SshcraftApp, ctx: &egui::Context) {
    let (focused, minimized, monitor, outer) = ctx.input(|i| {
        let v = i.viewport();
        (v.focused.unwrap_or(false), v.minimized.unwrap_or(false), v.monitor_size, v.outer_rect)
    });
    match decide(!app.dropdown_hidden, focused, minimized) {
        Toggle::Hide => {
            ctx.send_viewport_cmd(ViewportCommand::Visible(false));
            app.dropdown_hidden = true;
        }
        Toggle::Show => {
            ctx.send_viewport_cmd(ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
            // Down from the top edge, centred.
            if let (Some(monitor), Some(outer)) = (monitor, outer) {
                let x = ((monitor.x - outer.width()) / 2.0).max(0.0);
                ctx.send_viewport_cmd(ViewportCommand::OuterPosition(pos2(x, 0.0)));
            }
            ctx.send_viewport_cmd(ViewportCommand::Focus);
            app.dropdown_hidden = false;
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
