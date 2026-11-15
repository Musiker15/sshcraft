use sshcraft_term::limits::{MAX_CELLS, MAX_SCROLLBACK, SESSION_MEMORY_BUDGET};
use sshcraft_term::{CellFlags, TermColor, TermConfig, TermModes, TermSize, Terminal};

use crate::{row_text, term};

#[test]
fn plain_text_lands_in_the_first_row() {
    let mut t = term(80, 24);
    t.feed(b"hello");
    let s = t.screen();
    assert_eq!(row_text(&s, 0), "hello");
    assert_eq!((s.cursor.col, s.cursor.row), (5, 0));
}

#[test]
fn sgr_colors_reach_the_cells() {
    let mut t = term(80, 24);
    t.feed(b"\x1b[31mr\x1b[38;2;1;2;3mt\x1b[0;1;3;4mb");
    let s = t.screen();
    assert_eq!(s.cells[0].fg, TermColor::Indexed(1));
    assert_eq!(s.cells[1].fg, TermColor::Rgb(1, 2, 3));
    assert!(s.cells[2].flags.contains(CellFlags::BOLD | CellFlags::ITALIC | CellFlags::UNDERLINE));
    assert_eq!(s.cells[2].fg, TermColor::Default);
    assert_eq!(s.cells[3].bg, TermColor::DefaultBg);
}

#[test]
fn wide_characters_take_two_cells() {
    let mut t = term(80, 24);
    t.feed("日".as_bytes());
    let s = t.screen();
    assert_eq!(s.cells[0].text, "日");
    assert!(s.cells[0].flags.contains(CellFlags::WIDE));
    assert!(s.cells[1].flags.contains(CellFlags::WIDE_SPACER));
    assert_eq!(s.cells[1].text, "");
}

#[test]
fn combining_marks_stay_in_their_cell() {
    let mut t = term(80, 24);
    t.feed("e\u{301}x".as_bytes());
    let s = t.screen();
    assert_eq!(s.cells[0].text, "e\u{301}");
    assert_eq!(s.cells[1].text, "x");
}

#[test]
fn alt_screen_is_reported_and_restored() {
    let mut t = term(80, 24);
    t.feed(b"main");
    t.feed(b"\x1b[?1049h\x1b[2J\x1b[Halt");
    let s = t.screen();
    assert!(s.modes.contains(TermModes::ALT_SCREEN));
    assert_eq!(row_text(&s, 0), "alt");
    t.feed(b"\x1b[?1049l");
    let s = t.screen();
    assert!(!s.modes.contains(TermModes::ALT_SCREEN));
    assert_eq!(row_text(&s, 0), "main");
}

#[test]
fn sizes_are_clamped() {
    assert_eq!(TermSize::clamped(0, 0), TermSize { cols: 2, rows: 1 });
    assert_eq!(TermSize::clamped(5000, 5000), TermSize { cols: 1000, rows: 500 });
    let mut t = term(80, 24);
    t.resize(TermSize { cols: 0, rows: 9999 });
    assert_eq!(t.size(), TermSize { cols: 2, rows: 500 });
    let s = t.screen();
    assert_eq!(s.cells.len(), 2 * 500);
}

#[test]
fn the_grid_fits_the_memory_budget_at_every_size() {
    let config = TermConfig { scrollback: MAX_SCROLLBACK, ..TermConfig::default() };
    for cols in [2u16, 80, 200, 1000] {
        for rows in [1u16, 50, 500] {
            let t = Terminal::new(TermSize::clamped(cols, rows), &config);
            let cells = (t.effective_scrollback() + usize::from(rows)) * usize::from(cols);
            assert!(cells <= MAX_CELLS, "{cols}x{rows}: {cells} cells");
        }
    }
    assert!(MAX_CELLS * sshcraft_term::cell_size() <= SESSION_MEMORY_BUDGET);
}

#[test]
fn huge_csi_parameters_stay_inside_the_grid() {
    let mut t = term(80, 24);
    for seq in
        [&b"\x1b[4294967295@"[..], b"\x1b[65535;65535H", b"\x1b[999999S", b"x\x1b[1000000b", b"\x1b[999999L", b"\x1b[999999P", b"\x1b[99999999X"]
    {
        t.feed(seq);
        let s = t.screen();
        assert!(s.cursor.col < s.cols && s.cursor.row < s.rows);
        assert_eq!(s.cells.len(), 80 * 24);
    }
}
