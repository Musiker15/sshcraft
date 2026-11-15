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
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, tokens.button_border);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, tokens.separator);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, tokens.text_dim);
    ctx.set_visuals(visuals);
}

/// The terminal's background for the tokens, so a terminal tab sits flush with the app.
pub fn terminal_bg(tokens: &Tokens) -> Color32 {
    tokens.bg
}

#[cfg(test)]
mod contrast_tests;
