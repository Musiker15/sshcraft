//! The craft-fonts faces the build embedded (`build.rs`, empty without `CRAFT_FONTS_DIR`), put
//! into egui's families after its own faces. egui asks the system's fonts
//! (`egui_system_font_provider`) only for characters none of these has, so an embedded face comes
//! first.

pub use craft_fonts_build::EmbeddedFace;
use egui::epaint::text::{FontData, FontInsert, FontPriority, InsertFontFamily};

/// The faces embedded at build time.
pub static CRAFT_FONTS: &[EmbeddedFace] = include!(concat!(env!("OUT_DIR"), "/craft_fonts.rs"));

/// Adds `faces` at the end of the proportional and the monospace family; returns how many.
pub fn install_embedded(ctx: &egui::Context, faces: &[EmbeddedFace]) -> usize {
    for face in faces {
        let families = [egui::FontFamily::Proportional, egui::FontFamily::Monospace]
            .into_iter()
            .map(|family| InsertFontFamily { family, priority: FontPriority::Lowest })
            .collect();
        ctx.add_font(FontInsert { name: face.name(), data: FontData { index: face.index, ..FontData::from_static(face.bytes) }, families });
    }
    faces.len()
}
