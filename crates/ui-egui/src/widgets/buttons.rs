//! The larger controls of the redesign: the accent button, the outlined one, a square button with
//! a symbol, and the search field with its magnifier. Sizes come from the caller, colours from the
//! tokens.

use egui::{Align, FontId, Layout, Rect, Response, Sense, Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2};

use crate::theme::Tokens;
use crate::widgets::icons::{Icon, draw};

/// How wide a button with `label` (and a symbol) is at `height` points.
pub fn width_for(ui: &Ui, label: &str, icon: bool, height: f32) -> f32 {
    let text = ui.painter().layout_no_wrap(label.to_string(), FontId::proportional(15.0), egui::Color32::WHITE).size().x;
    text + if icon { 26.0 } else { 0.0 } + height * 0.72
}

fn labelled(ui: &mut Ui, rect: Rect, icon: Option<Icon>, label: &str, color: egui::Color32) {
    let galley = ui.painter().layout_no_wrap(label.to_string(), FontId::proportional(15.0), color);
    let icon_width = if icon.is_some() { 26.0 } else { 0.0 };
    let start = rect.center().x - (galley.size().x + icon_width) / 2.0;
    if let Some(icon) = icon {
        draw(ui.painter(), icon, Rect::from_center_size(pos2(start + 9.0, rect.center().y), vec2(18.0, 18.0)), color);
    }
    ui.painter().galley(pos2(start + icon_width, rect.center().y - galley.size().y / 2.0), galley, color);
}

/// The accent button: white words (and a symbol) on blue.
pub fn primary(ui: &mut Ui, icon: Option<Icon>, label: &str, height: f32) -> Response {
    let tokens = Tokens::get(ui.ctx());
    let width = width_for(ui, label, icon.is_some(), height);
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    ui.painter().rect_filled(rect, (height * 0.28).round(), if response.hovered() { tokens.accent_strong } else { tokens.accent });
    labelled(ui, rect, icon, label, egui::Color32::WHITE);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    response
}

/// The outlined button on the panel colour.
pub fn ghost(ui: &mut Ui, label: &str, height: f32) -> Response {
    let tokens = Tokens::get(ui.ctx());
    let width = width_for(ui, label, false, height);
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    let radius = (height * 0.24).round();
    ui.painter().rect(
        rect,
        radius,
        if response.hovered() { tokens.row_hover } else { tokens.panel },
        Stroke::new(1.0, tokens.button_border),
        StrokeKind::Inside,
    );
    labelled(ui, rect, None, label, tokens.text_strong);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    response
}

/// A square button with only a symbol, named `label` for screen readers and on hover; blue when
/// `primary`, outlined otherwise.
pub fn square(ui: &mut Ui, icon: Icon, label: &str, side: f32, primary: bool) -> Response {
    let tokens = Tokens::get(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(vec2(side, side), Sense::click());
    let radius = (side * 0.28).round();
    if primary {
        ui.painter().rect_filled(rect, radius, if response.hovered() { tokens.accent_strong } else { tokens.accent });
    } else {
        ui.painter().rect(
            rect,
            radius,
            if response.hovered() { tokens.row_hover } else { tokens.panel },
            Stroke::new(1.0, tokens.button_border),
            StrokeKind::Inside,
        );
    }
    let color = if primary { egui::Color32::WHITE } else { tokens.text };
    draw(ui.painter(), icon, Rect::from_center_size(rect.center(), vec2(20.0, 20.0)), color);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    response.on_hover_text(label)
}

/// The search field: a magnifier, then the text, in a rounded box `size` large.
pub fn search(ui: &mut Ui, text: &mut String, hint: &str, size: egui::Vec2) -> Response {
    let tokens = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let radius = (size.y * 0.28).round();
    ui.painter().rect(rect, radius, tokens.field, Stroke::new(1.0, tokens.field_border), StrokeKind::Inside);
    draw(ui.painter(), Icon::Search, Rect::from_center_size(pos2(rect.left() + 26.0, rect.center().y), vec2(20.0, 20.0)), tokens.text_dim);
    // One line of 15 point text, centred in the box; a child so the row's cursor stays put.
    let field = Rect::from_min_max(pos2(rect.left() + 46.0, rect.center().y - 11.0), pos2(rect.right() - 14.0, rect.center().y + 11.0));
    let mut child = ui.new_child(UiBuilder::new().max_rect(field).layout(Layout::left_to_right(Align::Center)));
    child.add(
        egui::TextEdit::singleline(text)
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .hint_text(hint)
            .font(FontId::proportional(15.0))
            .desired_width(field.width()),
    )
}

/// A heading in `size` points, drawn twice a hair apart so it reads as bold (egui has no
/// weights, and a bold face is only there with craft-fonts).
pub fn heading(ui: &mut Ui, text: &str, size: f32) -> Response {
    let tokens = Tokens::get(ui.ctx());
    let galley = ui.painter().layout(text.to_string(), FontId::proportional(size), tokens.text_strong, ui.available_width());
    let (rect, response) = ui.allocate_exact_size(galley.size() + vec2(1.0, 0.0), Sense::hover());
    ui.painter().galley(rect.min, galley.clone(), tokens.text_strong);
    ui.painter().galley(rect.min + vec2(0.6, 0.0), galley, tokens.text_strong);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, text));
    response
}

/// A filter chip: rounded, outlined, blue when on.
pub fn chip(ui: &mut Ui, label: &str, on: bool) -> Response {
    let tokens = Tokens::get(ui.ctx());
    let galley = ui.painter().layout_no_wrap(label.to_string(), FontId::proportional(13.0), if on { tokens.accent } else { tokens.text });
    let (rect, response) = ui.allocate_exact_size(vec2(galley.size().x + 24.0, 30.0), Sense::click());
    let fill = if on {
        tokens.accent_dim
    } else if response.hovered() {
        tokens.row_hover
    } else {
        tokens.panel
    };
    ui.painter().rect(rect, 15.0, fill, Stroke::new(1.0, if on { tokens.accent } else { tokens.separator }), StrokeKind::Inside);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, tokens.text);
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, label));
    response
}

/// A page's title: 28 points, as on the Hosts page.
pub fn page_title(ui: &mut Ui, text: impl AsRef<str>) -> Response {
    // On a phone the bar at the top already names the page.
    if crate::shell::layout_for(ui.ctx().content_rect().width()) == crate::shell::Layout::Narrow {
        return ui.allocate_response(vec2(0.0, 0.0), Sense::hover());
    }
    let response = heading(ui, text.as_ref(), 28.0);
    ui.add_space(6.0);
    response
}

/// A section's or a dialog's title: 20 points.
pub fn title(ui: &mut Ui, text: impl AsRef<str>) -> Response {
    heading(ui, text.as_ref(), 20.0)
}

/// The main action of a form (save, create, connect): white words on the accent.
pub fn main_action(label: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(label.to_string()).color(egui::Color32::WHITE)).fill(crate::theme::Tokens::dark().accent)
}
