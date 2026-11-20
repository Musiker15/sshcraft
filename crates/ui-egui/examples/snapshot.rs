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
        let tags: Vec<&str> = match name {
            "web-01" | "web-02" | "postgres-main" => vec!["prod"],
            "staging" => vec!["staging", "ci"],
            "nas" | "raspberry-pi" => vec!["home"],
            _ => Vec::new(),
        };
        let host = json!({ "id": id, "name": name, "group_id": group, "os": os, "tags": tags, "settings": { "address": address, "port": 22 } });
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
    let mut sftp = false;
    let mut theme = "dark";
    let mut language = "en".to_string();
    let mut captions = None;
    let mut size = egui::vec2(1200.0, 800.0);
    let mut tablet = false;
    let mut unlock = None;
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
                Some("sftp") => sftp = true,
                Some(name @ ("unlock" | "unlock-new")) => unlock = Some(name == "unlock-new"),
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
    if let Some(first) = unlock {
        return unlock_window(out, first, theme, &language);
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
    if sftp {
        sample_sftp(app);
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

/// An SFTP pane with sample folders on both sides and two transfers; no server behind it.
fn sample_sftp(app: &mut SshcraftApp) {
    use sshcraft_ui_egui::sftp_tab::{Entry, SftpView};
    use sshcraft_ui_egui::state::{PaneSlot, Panes, TabState};
    let entry = |name: &str, kind: &str, size: u64, modified: u64, permissions: u32| Entry {
        name: name.into(),
        kind: kind.into(),
        size,
        modified: Some(modified),
        permissions: Some(permissions),
    };
    let mut view = SftpView::new(1, None, "web-01".into(), "/home/deploy".into());
    view.remote.loading = false;
    view.remote.entries = vec![
        entry(".bashrc", "file", 3_771, 1_789_000_000, 0o644),
        entry("app", "dir", 0, 1_793_100_000, 0o755),
        entry("logs", "dir", 0, 1_793_400_000, 0o750),
        entry("backup-2026-11.tar.gz", "file", 48_211_456, 1_793_300_000, 0o600),
        entry("docker-compose.yml", "file", 2_114, 1_792_900_000, 0o644),
        entry("README.md", "file", 4_096, 1_790_000_000, 0o644),
    ];
    view.remote.selected.insert("docker-compose.yml".into());
    view.edits.push(sshcraft_ui_egui::sftp_tab::EditOpen { edit: 1, name: "docker-compose.yml".into() });
    view.local.loading = false;
    view.local.path = "/home/me/Projects".into();
    view.local.entries = vec![
        entry("sshcraft", "dir", 0, 1_793_500_000, 0o755),
        entry("notes.md", "file", 12_288, 1_793_450_000, 0o644),
        entry("deploy.sh", "file", 1_024, 1_793_000_000, 0o755),
    ];
    app.sftp.insert(1, view);
    app.transfers.insert(
        1,
        json!({ "id": 1, "sftp": 1, "direction": "upload", "label": "sshcraft (+2)", "state": { "kind": "running" },
                "files_done": 12, "files_total": 31, "bytes_done": 7_340_032, "bytes_total": 12_582_912, "skipped": 0, "errors": [] }),
    );
    app.transfers.insert(
        2,
        json!({ "id": 2, "sftp": 1, "direction": "download", "label": "backup-2026-11.tar.gz", "state": { "kind": "done" },
                "files_done": 1, "files_total": 1, "bytes_done": 48_211_456, "bytes_total": 48_211_456, "skipped": 0, "errors": [] }),
    );
    app.transfers.insert(
        3,
        json!({ "id": 3, "sftp": 1, "direction": "upload", "label": "notes.md", "state": { "kind": "cancelled" },
                "files_done": 0, "files_total": 1, "bytes_done": 4_096, "bytes_total": 12_288, "skipped": 0, "errors": [], "resumed": 0 }),
    );
    app.ui.tabs.push(TabState {
        title: sshcraft_ui_egui::sftp_tab::tab_title("web-01"),
        panes: Panes::single(PaneSlot::Sftp { id: 1 }),
        named: true,
    });
    app.ui.page = Page::Tab(app.ui.tabs.len() - 1);
}

/// Portable mode's unlock window: `first` chooses a passphrase, otherwise one is asked.
fn unlock_window(out: &str, first: bool, theme: &str, language: &str) -> Result<(), String> {
    use sshcraft_engine::portable::PassphraseMasterKey;
    use sshcraft_engine::settings::AppTheme;
    use sshcraft_store::vault::MasterKeyProvider;
    if language == "de" {
        sshcraft_ui_egui::i18n::set_language(sshcraft_ui_egui::i18n::Lang::De);
    }
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    if !first {
        PassphraseMasterKey::open(dir.path(), "a long enough passphrase")
            .and_then(|k| k.save(&[1; 32]).map_err(|e| sshcraft_engine::portable::PortableError::Io(e.to_string())))
            .map_err(|e| e.to_string())?;
    }
    let theme = if theme == "light" { AppTheme::Light } else { AppTheme::Dark };
    let unlock = sshcraft_ui_egui::unlock::Unlock::new(dir.path().to_path_buf()).with_theme(theme);
    let mut h = Harness::builder()
        .with_size(egui::vec2(480.0, if first { 330.0 } else { 250.0 }))
        .wgpu()
        .build_ui_state(|ui, u: &mut sshcraft_ui_egui::unlock::Unlock| u.ui(ui), unlock);
    h.run_steps(4);
    let image = h.render().map_err(|e| e.to_string())?;
    image.save(out).map_err(|e| e.to_string())
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(out) = args.next() else {
        eprintln!("usage: snapshot <out.png> [colors|unicode|fullscreen]");
        eprintln!(
            "       snapshot <out.png> --page hosts|keys|snippets|forwarding|logs|settings|newtab|sftp|unlock|unlock-new [--dark|--light] [--language en|de] [--captions windows|adwaita|breeze|generic]"
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
