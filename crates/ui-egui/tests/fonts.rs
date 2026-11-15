//! The craft-fonts faces are optional: a build without `CRAFT_FONTS_DIR` embeds none and the app
//! draws with egui's and the system's fonts; a build with it puts its faces in egui's families,
//! which egui asks before the system's fonts.

use craft_fonts_build::EmbeddedFace;
use egui_kittest::kittest::Queryable;
use sshcraft_ui_egui::fonts::{CRAFT_FONTS, install_embedded};
use sshcraft_ui_egui::testing::app_harness;

#[test]
fn fonts_are_optional() {
    if option_env!("CRAFT_FONTS_DIR").is_none_or(str::is_empty) {
        assert!(CRAFT_FONTS.is_empty(), "no checkout, no embedded face");
    }
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    h.run_steps(5);
    assert!(h.query_all_by_label("Hosts").next().is_some(), "the window draws with whatever fonts there are");
}

#[test]
fn embedded_faces_are_fallbacks_of_both_families() {
    // A face as craft-fonts-build writes one, made from egui's own Hack.
    let hack = egui::FontDefinitions::default().font_data.get("Hack").unwrap().font.clone();
    let bytes: &'static [u8] = Box::leak((*hack).as_ref().to_vec().into_boxed_slice());
    let faces = [EmbeddedFace { family: "Craft Test Sans", style: "Regular", scripts: &["Latn"], index: 0, bytes }];
    let mut h = egui_kittest::Harness::new_ui(|ui| {
        ui.label("x");
    });
    assert_eq!(install_embedded(&h.ctx, &faces), 1);
    h.run();
    let (data, families) = h.ctx.fonts(|f| (f.definitions().font_data.contains_key("Craft Test Sans Regular"), f.definitions().families.clone()));
    assert!(data, "the face is installed under its name");
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        let list = families.get(&family).cloned().unwrap_or_default();
        assert_eq!(list.last().map(String::as_str), Some("Craft Test Sans Regular"), "{family:?} ends with the face");
    }
}
