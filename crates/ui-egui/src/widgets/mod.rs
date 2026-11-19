//! Shared widgets: icons drawn in code, labels that never run over their line.

pub mod buttons;
pub mod elided;
pub mod icons;
pub mod logo;
pub mod qr;

pub use elided::{elided_label, forget_undo, one_line, secret_field, visible, visible_lines};

/// `wanted` points, or less where the space is narrower (a phone): fixed field widths never push
/// a form past the screen.
pub fn fit(ui: &egui::Ui, wanted: f32) -> f32 {
    wanted.min(ui.available_width().max(60.0))
}

/// Space between a phone's edge and a dialog.
const SHEET_MARGIN: f32 = 8.0;

/// Whether dialogs open as sheets across the window: on a phone held upright.
fn sheet(ctx: &egui::Context) -> bool {
    crate::shell::layout_for(ctx.content_rect().width()) == crate::shell::Layout::Narrow
}

/// A dialog: centred as egui places it, or on a phone a sheet across the window, placed at the
/// top so its width never depends on what egui remembered of it.
pub fn modal(ctx: &egui::Context, id: egui::Id) -> egui::Modal {
    let modal = egui::Modal::new(id);
    if !sheet(ctx) {
        return modal;
    }
    let screen = ctx.content_rect();
    let area = egui::Area::new(id)
        .kind(egui::UiKind::Modal)
        .sense(egui::Sense::hover())
        .order(egui::Order::Foreground)
        .interactable(true)
        .fixed_pos(egui::pos2(screen.left() + SHEET_MARGIN, screen.top() + 16.0));
    modal.area(area)
}

/// Keeps a dialog inside the window; on a phone it fills the width between the margins.
pub fn fit_dialog(ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let frame = 2.0 * f32::from(ui.style().spacing.menu_margin.left);
    if sheet(&ctx) {
        let room = (ctx.content_rect().width() - 2.0 * SHEET_MARGIN - frame).max(120.0);
        ui.set_min_width(room);
        ui.set_max_width(room);
    } else {
        let room = (ctx.content_rect().width() - 2.0 * f32::from(ui.style().spacing.window_margin.left) - frame).max(200.0);
        ui.set_max_width(room);
    }
}
