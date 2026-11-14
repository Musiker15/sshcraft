//! Renders a fixture into a terminal and writes it as PNG, without a window:
//!
//! `cargo run -p sshcraft-ui-egui --example snapshot -- out.png [colors|unicode|fullscreen]`
//!
//! The fixtures in `tests/fixtures/` are escape sequences written for SSHCraft, in Rust escape
//! notation (`\x1b`, `\u{…}`, `\r`, `\n`). Pictures belong in a pull request, not in the
//! repository.

use std::process::ExitCode;

use egui::{FontId, Pos2};
use egui_kittest::Harness;
use sshcraft_term::{TermConfig, TermSize, Terminal};
use sshcraft_ui_egui::terminal::{CellMetrics, TermPalette, install_terminal_font, layout, paint};

/// Turns `\x1b`, `\u{…}`, `\r`, `\n` and `\\` into the characters they stand for.
fn unescape(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('x') => {
                let hex: String = chars.by_ref().take(2).collect();
                if let Some(c) = u8::from_str_radix(&hex, 16).ok().map(char::from) {
                    out.push(c);
                }
            }
            Some('u') => {
                let hex: String = chars.by_ref().skip(1).take_while(|c| *c != '}').collect();
                if let Some(c) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    out.push(c);
                }
            }
            Some('r') => out.push('\r'),
            Some('n') => out.push('\n'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(out) = args.next() else {
        eprintln!("usage: snapshot <out.png> [colors|unicode|fullscreen]");
        return ExitCode::FAILURE;
    };
    let name = args.next().unwrap_or_else(|| "colors".into());
    let path = format!("{}/tests/fixtures/{name}.txt", env!("CARGO_MANIFEST_DIR"));
    let fixture = match std::fs::read_to_string(&path) {
        Ok(text) => unescape(&text),
        Err(e) => {
            eprintln!("{path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut term = Terminal::new(TermSize::clamped(100, 30), &TermConfig::default());
    term.feed(fixture.as_bytes());
    let palette = TermPalette::dark();
    let mut font = None;
    let mut harness = Harness::builder().with_size(egui::vec2(1000.0, 560.0)).wgpu().build_ui(move |ui| {
        let font = font.get_or_insert_with(|| FontId::new(14.0, install_terminal_font(ui.ctx(), "Cascadia Mono, Consolas, monospace"))).clone();
        let metrics = CellMetrics::measure(ui.ctx(), &font);
        let rect = ui.max_rect();
        ui.painter().rect_filled(rect, 0.0, palette.bg);
        paint(ui.painter(), &layout(&term.screen(), &metrics, &palette, &font, Pos2::new(rect.min.x + 4.0, rect.min.y + 4.0), true));
    });
    harness.run_steps(20);
    match harness.render().map_err(|e| e.to_string()).and_then(|image| image.save(&out).map_err(|e| e.to_string())) {
        Ok(()) => {
            println!("{out}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
