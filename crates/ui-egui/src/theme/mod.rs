//! Design tokens: every colour and radius the interface draws with. Widgets never hard-code a
//! colour; they read [`Tokens::get`]. [`apply`] turns the tokens into egui's visuals.

mod tokens;

use egui::{Color32, CornerRadius, Stroke};
use sshcraft_engine::settings::{AppTheme, EngineSettings};

pub use crate::terminal::palette::contrast;
pub use tokens::Tokens;

/// Whether the interface is dark for `setting`; `system` is what the platform reports.
pub fn is_dark(setting: AppTheme, system: Option<egui::Theme>) -> bool {
    match setting {
        AppTheme::Dark => true,
        AppTheme::Light => false,
        AppTheme::System => system != Some(egui::Theme::Light),
    }
}

/// Sets the tokens and egui's visuals for the settings; cheap to call when nothing changed.
pub fn apply(ctx: &egui::Context, settings: &EngineSettings) {
    let dark = is_dark(settings.appearance.app_theme, ctx.system_theme());
    let tokens = if dark { Tokens::dark() } else { Tokens::light() };
    if ctx.data(|d| d.get_temp::<Tokens>(egui::Id::NULL)) == Some(tokens) {
        return;
    }
    tokens.set(ctx);
    let mut visuals = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    visuals.panel_fill = tokens.panel;
    visuals.window_fill = tokens.panel;
    visuals.extreme_bg_color = tokens.field;
    visuals.faint_bg_color = tokens.row;
    visuals.code_bg_color = tokens.field;
    visuals.hyperlink_color = tokens.accent;
    visuals.warn_fg_color = tokens.warning;
    visuals.error_fg_color = tokens.danger;
    visuals.selection.bg_fill = tokens.row_selected;
    visuals.selection.stroke = Stroke::new(1.0, tokens.text_strong);
    visuals.window_stroke = Stroke::new(1.0, tokens.separator);
    visuals.override_text_color = Some(tokens.text);
    let radius = CornerRadius::same(tokens.radius);
    craft_ui::theme::WidgetPalette {
        noninteractive: tokens.panel,
        inactive: tokens.button,
        hovered: tokens.row_hover,
        active: tokens.row_selected,
        open: tokens.row_selected,
    }
    .apply_to(&mut visuals.widgets, radius, Stroke::new(1.0, tokens.text));
    // Every state of every egui widget in the design's terms, nothing left at egui's defaults:
    // controls sit on the panel colour with a border, hover lifts them, a pressed or open one
    // takes the selection colour, and none grows on hover.
    let state = |fill: Color32, border: Color32, text: Color32| egui::style::WidgetVisuals {
        bg_fill: fill,
        weak_bg_fill: fill,
        bg_stroke: Stroke::new(1.0, border),
        corner_radius: radius,
        fg_stroke: Stroke::new(1.0, text),
        expansion: 0.0,
    };
    visuals.widgets.noninteractive = state(tokens.panel, tokens.separator, tokens.text_dim);
    visuals.widgets.inactive = state(tokens.panel, tokens.button_border, tokens.text);
    visuals.widgets.hovered = state(tokens.row_hover, tokens.button_border, tokens.text_strong);
    visuals.widgets.active = state(tokens.row_selected, tokens.accent, tokens.text_strong);
    visuals.widgets.open = state(tokens.row_selected, tokens.button_border, tokens.text_strong);
    // A checkbox or radio is drawn in the field colour so it reads as something to fill in.
    visuals.widgets.inactive.bg_fill = tokens.field;
    // Selected text and selected entries: the accent's dim tone; focus rings in the accent.
    visuals.selection.bg_fill = tokens.accent_dim;
    visuals.selection.stroke = Stroke::new(1.0, tokens.accent);
    visuals.text_cursor.stroke = Stroke::new(2.0, tokens.accent);
    visuals.text_edit_bg_color = Some(tokens.field);
    // Dialogs and popups: panel colour, a thin border, round corners and a soft shadow.
    visuals.window_corner_radius = CornerRadius::same(16);
    visuals.menu_corner_radius = CornerRadius::same(12);
    visuals.window_shadow = egui::Shadow { offset: [0, 8], blur: 32, spread: 0, color: Color32::from_black_alpha(if dark { 140 } else { 40 }) };
    visuals.popup_shadow = egui::Shadow { offset: [0, 6], blur: 20, spread: 0, color: Color32::from_black_alpha(if dark { 120 } else { 32 }) };
    visuals.window_highlight_topmost = false;
    visuals.indent_has_left_vline = false;
    visuals.collapsing_header_frame = true;
    visuals.handle_shape = egui::style::HandleShape::Circle;
    ctx.set_visuals(visuals);
    // Type and room of the redesign: 14 point text, controls (menu entries too, whose padding
    // egui fixes) at least 32 points high.
    ctx.all_styles_mut(|style| {
        use egui::{FontFamily, FontId, TextStyle};
        style.text_styles.insert(TextStyle::Small, FontId::new(12.0, FontFamily::Proportional));
        style.text_styles.insert(TextStyle::Body, FontId::new(14.0, FontFamily::Proportional));
        style.text_styles.insert(TextStyle::Button, FontId::new(14.0, FontFamily::Proportional));
        style.text_styles.insert(TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace));
        style.text_styles.insert(TextStyle::Heading, FontId::new(22.0, FontFamily::Proportional));
        style.spacing.button_padding = egui::vec2(12.0, 7.0);
        style.spacing.interact_size.y = 32.0;
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.menu_margin = egui::Margin::same(6);
        style.spacing.window_margin = egui::Margin::same(20);
        style.spacing.menu_width = 200.0;
        style.spacing.icon_width = 18.0;
        style.spacing.icon_width_inner = 10.0;
        style.spacing.icon_spacing = 8.0;
        style.spacing.combo_height = 320.0;
        // Thin bars that float over the content and take the border colour (no bright block).
        style.spacing.scroll = egui::style::ScrollStyle::floating();
        style.spacing.scroll.bar_width = 8.0;
    });
}

/// The terminal's background for the tokens, so a terminal tab sits flush with the app.
pub fn terminal_bg(tokens: &Tokens) -> Color32 {
    tokens.bg
}

#[cfg(test)]
mod contrast_tests;
