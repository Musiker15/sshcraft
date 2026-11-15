//! Embeds the optional craft-fonts faces: with `CRAFT_FONTS_DIR` naming a checkout of
//! storytold/craft-fonts, its faces for the interface go into the binary; without it the table is
//! empty and the app draws with egui's and the system's fonts. `CRAFT_FONTS_REQUIRED=1` (release
//! builds) makes a missing checkout an error.

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    // Interface faces only: CJK faces would need translation catalogs to be cut to, and the
    // interface is English and German; host names in other scripts come from the system's fonts.
    if let Err(e) = craft_fonts_build::Embed::new().for_ui().write("craft_fonts.rs") {
        println!("cargo::error={e}");
    }
}
