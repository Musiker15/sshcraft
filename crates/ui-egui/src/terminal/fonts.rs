//! The terminal's font: the first family of the setting `appearance.font_family` that is
//! installed, loaded from the system; egui's own monospace font when none is. Characters the font
//! lacks (CJK, emoji) come from the system's fallback through `egui_system_font_provider`. No
//! font file is part of SSHCraft.

use std::sync::Arc;

use egui::epaint::text::{FontData, FontInsert, FontPriority, InsertFontFamily, MissingGlyphPolicy};
use egui::{Context, FontFamily};
use fontique::{Collection, CollectionOptions, SourceCache, SourceCacheOptions};

/// The family name the terminal's font is installed under.
pub const TERMINAL_FAMILY: &str = "terminal";

/// Installs the first installed family of the comma-separated `families` and returns the family
/// to draw with. Generic names (`monospace`) and unknown families are skipped.
pub fn install_terminal_font(ctx: &Context, families: &str) -> FontFamily {
    // A server may print any character; one no font has is drawn as a box, never a panic
    // (test harnesses switch egui to panicking).
    ctx.set_missing_glyph_policy(MissingGlyphPolicy::Tofu);
    ctx.add_font_provider(Arc::new(egui_system_font_provider::SystemFontProvider::new()));
    let mut collection = Collection::new(CollectionOptions { shared: false, system_fonts: true });
    let mut cache = SourceCache::new(SourceCacheOptions::default());
    for name in families.split(',').map(str::trim).filter(|n| !n.is_empty() && !n.eq_ignore_ascii_case("monospace")) {
        let Some(family) = collection.family_by_name(name) else { continue };
        let Some(font) = family.default_font() else { continue };
        let Some(blob) = font.load(Some(&mut cache)) else { continue };
        let (blob, _) = blob.into_raw_parts();
        ctx.add_font(FontInsert {
            name: format!("{TERMINAL_FAMILY}:{name}"),
            data: FontData::from_blob(blob, font.index()),
            families: vec![InsertFontFamily { family: FontFamily::Name(TERMINAL_FAMILY.into()), priority: FontPriority::Highest }],
        });
        return FontFamily::Name(TERMINAL_FAMILY.into());
    }
    FontFamily::Monospace
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_families_fall_back_to_egui_monospace() {
        let ctx = Context::default();
        assert_eq!(install_terminal_font(&ctx, "No Such Font 3f9c, monospace"), FontFamily::Monospace);
        assert_eq!(install_terminal_font(&ctx, ""), FontFamily::Monospace);
    }

    #[test]
    fn a_character_no_font_has_is_a_box_not_a_panic() {
        // The test harness makes egui panic on a missing glyph; the terminal switches that off.
        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            let family = install_terminal_font(ui.ctx(), "No Such Font 3f9c");
            let font = egui::FontId::new(14.0, family);
            let _ = ui.painter().layout_no_wrap("\u{e000}\u{10fffd}".into(), font, egui::Color32::WHITE);
        });
        harness.run_steps(2);
    }
}
