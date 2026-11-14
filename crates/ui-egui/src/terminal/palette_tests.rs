use egui::Color32;
use sshcraft_term::{CellFlags, TermColor};

use super::{TermPalette, contrast};

#[test]
fn indexed_colors_follow_the_xterm_cube() {
    let p = TermPalette::dark();
    assert_eq!(p.indexed(16), Color32::from_rgb(0, 0, 0));
    assert_eq!(p.indexed(196), Color32::from_rgb(255, 0, 0));
    assert_eq!(p.indexed(232), Color32::from_rgb(8, 8, 8));
    assert_eq!(p.indexed(255), Color32::from_rgb(238, 238, 238));
    assert_eq!(p.indexed(1), p.ansi[1]);
    assert_eq!(p.resolve(TermColor::Rgb(1, 2, 3), CellFlags::empty()), Color32::from_rgb(1, 2, 3));
    assert_eq!(p.resolve(TermColor::Default, CellFlags::empty()), p.fg);
    assert_eq!(p.resolve(TermColor::DefaultBg, CellFlags::empty()), p.bg);
}

#[test]
fn default_palettes_are_readable() {
    for (name, p) in [("dark", TermPalette::dark()), ("light", TermPalette::light())] {
        assert!(contrast(p.fg, p.bg) >= 4.5, "{name}: fg on bg {}", contrast(p.fg, p.bg));
        assert!(contrast(p.selection_fg, p.selection_bg) >= 4.5, "{name}: selection {}", contrast(p.selection_fg, p.selection_bg));
        assert!(contrast(p.link, p.bg) >= 4.5, "{name}: link {}", contrast(p.link, p.bg));
        assert!(contrast(p.cursor, p.bg) >= 3.0, "{name}: cursor {}", contrast(p.cursor, p.bg));
    }
}
