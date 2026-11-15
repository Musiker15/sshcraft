use egui::{Color32, FontId, Pos2};
use sshcraft_term::{GridPoint, SelectionKind, TermConfig, TermSize, Terminal};

use super::{CellMetrics, LineStyle, Primitive, layout};
use crate::terminal::TermPalette;

const M: CellMetrics = CellMetrics { width: 10.0, height: 20.0, baseline: 16.0 };

fn term(bytes: &[u8]) -> Terminal {
    let mut t = Terminal::new(TermSize::clamped(20, 3), &TermConfig::default());
    t.feed(bytes);
    t
}

fn run(t: &Terminal, focused: bool) -> Vec<Primitive> {
    layout(&t.screen(), &M, &TermPalette::dark(), &FontId::monospace(14.0), Pos2::ZERO, focused, true)
}

fn rects(p: &[Primitive]) -> Vec<(egui::Rect, Color32)> {
    p.iter().filter_map(|p| if let Primitive::Rect { rect, color } = p { Some((*rect, *color)) } else { None }).collect()
}

fn texts(p: &[Primitive]) -> Vec<(Pos2, f32, String, Color32)> {
    p.iter()
        .filter_map(|p| if let Primitive::Text { pos, width, text, color, .. } = p { Some((*pos, *width, text.clone(), *color)) } else { None })
        .collect()
}

fn lines(p: &[Primitive]) -> Vec<LineStyle> {
    p.iter().filter_map(|p| if let Primitive::Line { style, .. } = p { Some(*style) } else { None }).collect()
}

/// Hides the cursor so only what the test is about is drawn.
const NO_CURSOR: &[u8] = b"\x1b[?25l";

#[test]
fn backgrounds_are_merged_per_run() {
    let t = term(&[NO_CURSOR, b"\x1b[41m   \x1b[44m  \x1b[0m"].concat());
    let p = TermPalette::dark();
    let r = rects(&run(&t, true));
    assert_eq!(r.len(), 2, "{r:?}");
    assert_eq!(r[0].1, p.ansi[1]);
    assert_eq!(r[0].0.width(), 30.0);
    assert_eq!(r[1].1, p.ansi[4]);
    assert_eq!(r[1].0.min.x, 30.0);
}

#[test]
fn default_background_paints_nothing() {
    let t = term(b"");
    let prims = run(&t, true);
    assert_eq!(prims.len(), 1, "only the cursor: {prims:?}");
    assert!(matches!(prims[0], Primitive::Rect { color, .. } if color == TermPalette::dark().cursor));
}

#[test]
fn text_runs_split_at_style_changes() {
    let t = term(&[NO_CURSOR, b"ab\x1b[31mcd"].concat());
    let found: Vec<String> = texts(&run(&t, true)).into_iter().map(|t| t.2).collect();
    assert_eq!(found, ["ab", "cd"]);
}

#[test]
fn a_wide_character_spans_two_cells() {
    let t = term(&[NO_CURSOR, "\x1b[41m日\x1b[0mx".as_bytes()].concat());
    let prims = run(&t, true);
    let found = texts(&prims);
    assert_eq!((found[0].0.x, found[0].1, found[0].2.as_str()), (0.0, 20.0, "日"));
    assert_eq!(found[1].0.x, 20.0);
    assert_eq!(rects(&prims)[0].0.width(), 20.0);
}

#[test]
fn a_grapheme_cluster_is_one_text() {
    let t = term(&[NO_CURSOR, "ae\u{301}b \u{1F469}\u{200D}\u{1F4BB}".as_bytes()].concat());
    let found: Vec<String> = texts(&run(&t, true)).into_iter().map(|t| t.2).collect();
    assert!(found.contains(&"e\u{301}".to_string()), "{found:?}");
    assert!(found.contains(&"\u{1F469}\u{200D}\u{1F4BB}".to_string()), "{found:?}");
    let t = term(&[NO_CURSOR, "\u{1F44D}\u{1F3FD}x".as_bytes()].concat());
    let found: Vec<String> = texts(&run(&t, true)).into_iter().map(|t| t.2).collect();
    assert_eq!(found, ["\u{1F44D}\u{1F3FD}", "x"]);
}

#[test]
fn inverse_swaps_colors() {
    let t = term(&[NO_CURSOR, b"\x1b[7mx"].concat());
    let p = TermPalette::dark();
    let prims = run(&t, true);
    assert_eq!(rects(&prims)[0].1, p.fg);
    assert_eq!(texts(&prims)[0].3, p.bg);
}

#[test]
fn hidden_draws_no_text() {
    let t = term(&[NO_CURSOR, b"\x1b[8mx"].concat());
    assert!(texts(&run(&t, true)).is_empty());
}

#[test]
fn underline_styles_map_to_lines() {
    let t = term(&[NO_CURSOR, b"\x1b[4ma\x1b[4:2mb\x1b[4:3mc\x1b[4:4md\x1b[4:5me\x1b[0;9mf"].concat());
    assert_eq!(
        lines(&run(&t, true)),
        [LineStyle::Solid, LineStyle::Double, LineStyle::Curly, LineStyle::Dotted, LineStyle::Dashed, LineStyle::Solid]
    );
}

#[test]
fn the_cursor_is_hollow_without_focus() {
    let t = term(b"");
    let focused = run(&t, true);
    assert_eq!(rects(&focused).len(), 1);
    let unfocused = run(&t, false);
    assert!(rects(&unfocused).is_empty());
    assert_eq!(lines(&unfocused).len(), 4);
}

#[test]
fn the_character_under_a_block_cursor_takes_the_background_colour() {
    let mut t = term(b"ab");
    t.feed(b"\x1b[1G");
    let p = TermPalette::dark();
    let found = texts(&run(&t, true));
    assert_eq!(found[0].2, "a");
    assert_eq!(found[0].3, p.bg);
    assert_eq!(found[1].3, p.fg);
}

#[test]
fn selection_uses_the_selection_colors() {
    let mut t = term(&[NO_CURSOR, b"hi there"].concat());
    t.select_start(GridPoint { line: 0, col: 0 }, SelectionKind::Simple);
    t.select_update(GridPoint { line: 0, col: 1 });
    let p = TermPalette::dark();
    let prims = run(&t, true);
    assert_eq!(rects(&prims)[0].1, p.selection_bg);
    assert_eq!(rects(&prims)[0].0.width(), 20.0);
    let found = texts(&prims);
    assert_eq!((found[0].2.as_str(), found[0].3), ("hi", p.selection_fg));
}

#[test]
fn links_are_underlined_in_the_link_colour() {
    let t = term(&[NO_CURSOR, b"\x1b]8;;https://example.com/\x1b\\go\x1b]8;;\x1b\\"].concat());
    let p = TermPalette::dark();
    let prims = run(&t, true);
    assert_eq!(texts(&prims)[0].3, p.link);
    assert_eq!(lines(&prims), [LineStyle::Solid]);
}

#[test]
fn without_ligatures_each_cell_is_its_own_run() {
    let t = term(b"->");
    let joined = layout(&t.screen(), &M, &TermPalette::dark(), &FontId::monospace(14.0), Pos2::ZERO, false, true);
    assert_eq!(texts(&joined).iter().map(|t| t.2.as_str()).collect::<Vec<_>>(), ["->"], "one run, so the font may join it");
    let apart = layout(&t.screen(), &M, &TermPalette::dark(), &FontId::monospace(14.0), Pos2::ZERO, false, false);
    assert_eq!(texts(&apart).iter().map(|t| t.2.as_str()).collect::<Vec<_>>(), ["-", ">"], "each cell alone");
}
