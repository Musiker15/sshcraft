//! Renders a fixture into a terminal, or the whole window on a page, and writes it as PNG, without
//! a window on screen:
//!
//! `cargo run -p sshcraft-ui-egui --example snapshot -- out.png [colors|unicode|fullscreen]`
//!
//! `cargo run -p sshcraft-ui-egui --example snapshot -- out.png --page hosts|keys|snippets|forwarding|logs|settings|newtab
//! [--dark|--light] [--language en|de] [--captions windows|adwaita|breeze|generic] [--size WIDTHxHEIGHT] [--tablet] [--click LABEL|right:LABEL]...`
//!
//! `--captions` draws the window's own title bar with that family's window buttons, as on a
//! desktop where the app draws the frame.
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
    let home = uuid::Uuid::new_v4();
    for (id, name) in [(web, "Web servers"), (db, "Databases"), (home, "Home")] {
        run(session, "group.save", json!({ "group": { "id": id, "name": name, "defaults": { "username": "deploy" } } }))?;
    }
    let hosts = [
        ("gateway", "198.51.100.1", None, "debian"),
        ("build-runner", "198.51.100.20", None, "ubuntu"),
        ("web-01", "192.0.2.11", Some(web), "debian"),
        ("web-02", "192.0.2.12", Some(web), "debian"),
        ("mail", "mail.example.org", Some(web), "debian"),
        ("postgres-main", "192.0.2.31", Some(db), "debian"),
        ("redis", "192.0.2.32", Some(db), "alpine"),
        ("raspberry-pi", "pi.home.example", Some(home), "raspbian"),
        ("nas", "nas.home.example", Some(home), "free_bsd"),
        ("staging", "203.0.113.5", None, "ubuntu"),
        ("jump", "203.0.113.30", None, "alpine"),
    ];
    let mut ids = Vec::new();
    for (name, address, group, os) in hosts {
        let id = uuid::Uuid::new_v4();
        let host = json!({ "id": id, "name": name, "group_id": group, "os": os, "settings": { "address": address, "port": 22 } });
        run(session, "host.save", json!({ "host": host }))?;
        ids.push((name, id));
    }
    // "Recently connected": the newest first.
    for (n, name) in ["web-01", "postgres-main", "nas", "staging"].iter().enumerate() {
        if let Some((_, id)) = ids.iter().find(|(h, _)| h == name) {
            sshcraft_engine::testing::touch_recent(session, sshcraft_model::model::HostId(*id), 1_000_000 - n as i64);
        }
    }
    run(
        session,
        "snippet.save",
        json!({ "snippet": { "id": uuid::Uuid::new_v4(), "name": "restart app", "package": "ops", "content": "sudo systemctl restart app", "default_targets": [] } }),
    )?;
    run(
        session,
        "snippet.save",
        json!({ "snippet": { "id": uuid::Uuid::new_v4(), "name": "disk usage", "content": "df -h", "default_targets": [] } }),
    )?;
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
    let mut captions = None;
    let mut size = egui::vec2(1200.0, 800.0);
    let mut tablet = false;
    let mut clicks = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--click" => clicks.push(args.next().ok_or("--click needs a label")?),
            "--size" => {
                let text = args.next().ok_or("--size needs WIDTHxHEIGHT")?;
                let (w, h) = text.split_once('x').ok_or("--size needs WIDTHxHEIGHT")?;
                size = egui::vec2(w.parse().map_err(|_| "bad width")?, h.parse().map_err(|_| "bad height")?);
            }
            "--tablet" => tablet = true,
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
            "--captions" => {
                let name = args.next().unwrap_or_default();
                captions = Some(craft_window::policy::CaptionFamily::parse(&name).ok_or(format!("unknown caption family {name:?}"))?);
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let (mut h, session) = sshcraft_ui_egui::testing::app_harness_from(Harness::builder().wgpu(), dir.path());
    h.set_size(size);
    sample_data(&session)?;
    let mut settings = run(&session, "settings.get", json!({}))?;
    settings["general"]["language"] = json!(language);
    settings["appearance"]["app_theme"] = json!(theme);
    run(&session, "settings.save", json!({ "settings": settings }))?;
    let app: &mut SshcraftApp = h.state_mut();
    app.ui.page = page;
    if tablet {
        app.form = sshcraft_ui_egui::app::Form::Tablet;
    }
    if let Some(family) = captions {
        let layout = craft_window::Layout {
            left: Vec::new(),
            right: vec![craft_window::Button::Minimize, craft_window::Button::Maximize, craft_window::Button::Close],
        };
        app.window_frame = craft_window::policy::Frame::Integrated(craft_window::policy::Captions { family, layout });
    }
    if new_tab {
        app.open_new_tab();
    }
    for _ in 0..40 {
        h.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(15));
    }
    // Open menus and the like: a click (or `right:` a right click) on the last node so named.
    for label in clicks {
        use egui_kittest::kittest::Queryable;
        let (right, name) = match label.strip_prefix("right:") {
            Some(name) => (true, name.to_string()),
            None => (false, label.clone()),
        };
        let node = h.query_all_by_label(&name).last().ok_or(format!("nothing labelled {name:?}"))?;
        if right {
            node.click_secondary();
        } else {
            node.click();
        }
        for _ in 0..6 {
            h.run_steps(1);
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
    }
    let image = h.render().map_err(|e| e.to_string())?;
    image.save(out).map_err(|e| e.to_string())
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(out) = args.next() else {
        eprintln!("usage: snapshot <out.png> [colors|unicode|fullscreen]");
        eprintln!(
            "       snapshot <out.png> --page hosts|keys|snippets|forwarding|logs|settings|newtab [--dark|--light] [--language en|de] [--captions windows|adwaita|breeze|generic]"
        );
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
