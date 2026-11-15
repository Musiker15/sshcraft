//! Renders a fixture into a terminal, or the whole window on a page, and writes it as PNG, without
//! a window on screen:
//!
//! `cargo run -p sshcraft-ui-egui --example snapshot -- out.png [colors|unicode|fullscreen]`
//!
//! `cargo run -p sshcraft-ui-egui --example snapshot -- out.png --page hosts|keys|snippets|forwarding|logs|settings|newtab
//! [--dark|--light] [--language en|de]`
//!
//! The window runs on an engine in a fresh temp folder with made-up sample data, never on
//! SSHCraft's own data.
//!
//! The fixtures in `tests/fixtures/` are escape sequences written for SSHCraft, in Rust escape
//! notation (`\x1b`, `\u{…}`, `\r`, `\n`). Pictures belong in a pull request, not in the
//! repository.

use std::process::ExitCode;

use egui::{FontId, Pos2};
use egui_kittest::Harness;
use serde_json::{Value, json};
use sshcraft_engine::{Caller, Session};
use sshcraft_term::{TermConfig, TermSize, Terminal};
use sshcraft_ui_egui::SshcraftApp;
use sshcraft_ui_egui::state::Page;
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

fn run(session: &Session, id: &str, params: Value) -> Result<Value, String> {
    session.runtime().block_on(session.execute(Caller::User, id, params)).map_err(|e| format!("{id}: {e}"))
}

/// Made-up hosts, groups and a key, so a page has something to show.
fn sample_data(session: &Session) -> Result<(), String> {
    let web = uuid::Uuid::new_v4();
    let db = uuid::Uuid::new_v4();
    for (id, name) in [(web, "Web servers"), (db, "Databases")] {
        run(session, "group.save", json!({ "group": { "id": id, "name": name, "defaults": { "username": "deploy" } } }))?;
    }
    let hosts = [
        ("gateway", "198.51.100.1", None, vec!["edge"]),
        ("build-runner", "198.51.100.20", None, vec!["ci"]),
        ("web-01", "192.0.2.11", Some(web), vec!["prod"]),
        ("web-02", "192.0.2.12", Some(web), vec!["prod"]),
        ("postgres-main", "192.0.2.31", Some(db), vec!["prod", "backup"]),
        ("raspberry-pi", "pi.home.example", None, vec!["home"]),
        ("staging", "203.0.113.5", None, vec!["staging"]),
    ];
    for (name, address, group, tags) in hosts {
        let host =
            json!({ "id": uuid::Uuid::new_v4(), "name": name, "group_id": group, "tags": tags, "settings": { "address": address, "port": 22 } });
        run(session, "host.save", json!({ "host": host }))?;
    }
    run(session, "snippet.save", json!({ "snippet": { "id": uuid::Uuid::new_v4(), "name": "restart app", "package": "ops", "content": "sudo systemctl restart app", "default_targets": [] } }))?;
    run(session, "snippet.save", json!({ "snippet": { "id": uuid::Uuid::new_v4(), "name": "disk usage", "content": "df -h", "default_targets": [] } }))?;
    run(session, "key.generate", json!({ "name": "laptop", "spec": { "type": "ed25519" }, "passphrase": null }))?;
    run(session, "key.generate", json!({ "name": "deploy-ci", "spec": { "type": "ecdsa", "curve": "nistp256" }, "passphrase": null }))?;
    Ok(())
}

/// The whole window on `page`, after enough frames for the data to arrive.
fn window(out: &str, mut args: impl Iterator<Item = String>) -> Result<(), String> {
    let mut page = Page::Hosts;
    let mut new_tab = false;
    let mut theme = "dark";
    let mut language = "en".to_string();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--page" => match args.next().as_deref() {
                Some("hosts") => page = Page::Hosts,
                Some("keys") => page = Page::Keys,
                Some("settings") => page = Page::Settings,
                Some("snippets") => page = Page::Snippets,
                Some("forwarding") => page = Page::Forwarding,
                Some("logs") => page = Page::Logs,
                Some("newtab") => new_tab = true,
                other => return Err(format!("unknown page {other:?}")),
            },
            "--dark" => theme = "dark",
            "--light" => theme = "light",
            "--language" => language = args.next().ok_or("--language needs en or de")?,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let (mut h, session) = sshcraft_ui_egui::testing::app_harness_from(Harness::builder().wgpu(), dir.path());
    sample_data(&session)?;
    let mut settings = run(&session, "settings.get", json!({}))?;
    settings["general"]["language"] = json!(language);
    settings["appearance"]["app_theme"] = json!(theme);
    run(&session, "settings.save", json!({ "settings": settings }))?;
    let app: &mut SshcraftApp = h.state_mut();
    app.ui.page = page;
    if new_tab {
        app.open_new_tab();
    }
    for _ in 0..40 {
        h.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(15));
    }
    let image = h.render().map_err(|e| e.to_string())?;
    image.save(out).map_err(|e| e.to_string())
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(out) = args.next() else {
        eprintln!("usage: snapshot <out.png> [colors|unicode|fullscreen]");
        eprintln!("       snapshot <out.png> --page hosts|keys|snippets|forwarding|logs|settings|newtab [--dark|--light] [--language en|de]");
        return ExitCode::FAILURE;
    };
    let name = args.next().unwrap_or_else(|| "colors".into());
    if name.starts_with("--") {
        return match window(&out, std::iter::once(name).chain(args)) {
            Ok(()) => {
                println!("{out}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        };
    }
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
        paint(ui.painter(), &layout(&term.screen(), &metrics, &palette, &font, Pos2::new(rect.min.x + 4.0, rect.min.y + 4.0), true, true));
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
