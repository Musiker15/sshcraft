//! Every pair of foreground and background the interface draws stays readable (WCAG 2): text 7:1
//! on the main surfaces, 4.5:1 for dim text and on selections, 3:1 for symbols and states.

use super::{Tokens, contrast};

fn check(name: &str, t: &Tokens) {
    let pairs = [
        ("text on bg", t.text, t.bg, 7.0),
        ("text on panel", t.text, t.panel, 7.0),
        ("text on sidebar", t.text, t.sidebar, 7.0),
        ("text on row", t.text, t.row, 7.0),
        ("text on row_hover", t.text, t.row_hover, 4.5),
        ("text on row_selected", t.text, t.row_selected, 4.5),
        ("text on field", t.text, t.field, 4.5),
        ("text on button", t.text, t.button, 4.5),
        ("text_dim on panel", t.text_dim, t.panel, 4.5),
        ("text_dim on bg", t.text_dim, t.bg, 4.5),
        ("text_strong on accent_dim", t.text_strong, t.accent_dim, 4.5),
        ("accent on panel", t.accent, t.panel, 3.0),
        ("accent on bg", t.accent, t.bg, 3.0),
        ("danger on panel", t.danger, t.panel, 3.0),
        ("warning on panel", t.warning, t.panel, 3.0),
        ("success on panel", t.success, t.panel, 3.0),
        ("info on panel", t.info, t.panel, 3.0),
        ("info on bg", t.info, t.bg, 3.0),
        ("field_border on field", t.field_border, t.field, 1.6),
        ("button_border on panel", t.button_border, t.panel, 1.6),
    ];
    for (pair, fg, bg, minimum) in pairs {
        let ratio = contrast(fg, bg);
        assert!(ratio >= minimum, "{name}: {pair} is {ratio:.2}, needs {minimum}");
    }
}

#[test]
fn every_theme_reads() {
    check("dark", &Tokens::dark());
    check("light", &Tokens::light());
}

#[test]
fn the_accent_is_the_same_blue_in_both_themes() {
    for t in [Tokens::dark(), Tokens::light()] {
        assert_eq!(t.accent, egui::Color32::from_rgb(0x00, 0x71, 0xFF));
        assert_eq!((t.radius, t.radius_card), (10, 16));
    }
    assert_eq!(Tokens::dark().bg, egui::Color32::from_rgb(0x18, 0x19, 0x1E));
    assert_eq!(Tokens::light().sidebar, egui::Color32::from_rgb(0xEF, 0xEF, 0xF2));
}
