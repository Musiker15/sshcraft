//! Performance budgets of the terminal (rule 9 of the design): a flood never stalls the window,
//! and a session's grid stays in its memory budget. Only in a release build (`cargo xtask perf`):
//! a debug build measures the compiler, not the program.

#![cfg(not(debug_assertions))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Mutex;
use std::time::{Duration, Instant};

use sshcraft_term::limits::{MAX_CELLS, MAX_SCROLLBACK, SESSION_MEMORY_BUDGET};
use sshcraft_term::{TermConfig, TermSize, Terminal, cell_size};

/// Memory the process uses now (working set), in bytes.
fn in_use() -> usize {
    memory_stats::memory_stats().map_or(0, |m| m.physical_mem)
}

/// The tests measure time and memory; one at a time.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// The slice the window's pump feeds between two looks at the clock (`FEED_SLICE` in ui-egui).
const SLICE: usize = 64 * 1024;
/// The time a frame may feed (`FRAME_TIME_BUDGET` in ui-egui).
const FRAME_TIME: Duration = Duration::from_millis(6);

fn inputs() -> Vec<(&'static str, Vec<u8>)> {
    let mut ascii = Vec::new();
    let mut i = 0u64;
    while ascii.len() < 4 << 20 {
        ascii.extend_from_slice(format!("2026-11-14 12:00:{:02} INFO request {i} served in {} ms path=/api/items\n", i % 60, i % 997).as_bytes());
        i += 1;
    }
    let mut sgr = Vec::new();
    while sgr.len() < 4 << 20 {
        sgr.extend_from_slice(
            format!("\x1b[1;3{}mword{i}\x1b[0m \x1b[38;5;{}mmore\x1b[0m \x1b[38;2;{};{};9mtrue\x1b[0m\n", i % 8, i % 256, i % 256, (i * 7) % 256)
                .as_bytes(),
        );
        i += 1;
    }
    let mut cjk = Vec::new();
    while cjk.len() < 4 << 20 {
        cjk.extend_from_slice("日本語のテキスト 👩‍💻 e\u{301} 한국어 中文 ✓\n".as_bytes());
    }
    vec![("ascii", ascii), ("sgr", sgr), ("cjk", cjk)]
}

#[test]
fn a_feed_slice_stays_far_inside_a_frame() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    for (name, data) in inputs() {
        let mut term = Terminal::new(TermSize::clamped(200, 50), &TermConfig::default());
        let mut times: Vec<Duration> = data
            .chunks(SLICE)
            .map(|slice| {
                let start = Instant::now();
                term.feed(slice);
                start.elapsed()
            })
            .collect();
        times.sort();
        let median = times[times.len() / 2];
        // A frame looks at the clock after every slice; one slice is a small part of a frame.
        assert!(median < FRAME_TIME / 4, "{name}: median slice {median:?}");
    }
}

#[test]
fn one_gigabyte_of_cat_stays_responsive() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (_, data) = inputs().into_iter().next().unwrap();
    let mut term = Terminal::new(TermSize::clamped(200, 50), &TermConfig::default());
    let total = 1usize << 30;
    let start = Instant::now();
    let mut slow = 0usize;
    let mut fed = 0usize;
    while fed < total {
        for slice in data.chunks(SLICE) {
            let t = Instant::now();
            term.feed(slice);
            if t.elapsed() > FRAME_TIME {
                slow += 1;
            }
            fed += slice.len();
        }
    }
    let elapsed = start.elapsed();
    let slices = fed / SLICE;
    assert!(slow * 1000 <= slices, "{slow} of {slices} slices took longer than a frame");
    let mib_per_s = fed as f64 / (1 << 20) as f64 / elapsed.as_secs_f64();
    assert!(mib_per_s >= 50.0, "{mib_per_s:.0} MiB/s");
    let t = Instant::now();
    let _ = term.screen();
    assert!(t.elapsed() < Duration::from_millis(2), "screen() took {:?}", t.elapsed());
}

#[test]
fn scrollback_memory_stays_in_budget() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let before = in_use();
    let config = TermConfig { scrollback: MAX_SCROLLBACK, ..TermConfig::default() };
    let mut term = Terminal::new(TermSize::clamped(200, 50), &config);
    let kept = term.effective_scrollback();
    let line = format!("{}\r\n", "x".repeat(199));
    for _ in 0..300_000 {
        term.feed(line.as_bytes());
    }
    assert_eq!(term.effective_scrollback(), kept);
    let grown = in_use().saturating_sub(before);
    let grid = (kept + 50) * 200 * cell_size();
    assert!(grown <= grid + 16 * 1024 * 1024, "grew {grown} bytes for a grid of {grid}");
    assert!(grown <= SESSION_MEMORY_BUDGET, "grew {grown} bytes");
    assert!(MAX_CELLS * cell_size() <= SESSION_MEMORY_BUDGET);
}
