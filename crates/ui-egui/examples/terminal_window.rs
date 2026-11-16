//! A window with one local shell, for trying the terminal by hand until the app returns in R4:
//!
//! `cargo run -p sshcraft-ui-egui --example terminal_window`
//!
//! It runs an engine on a fresh folder in the temp directory, never on SSHCraft's own data, and
//! opens the first shell `local.shells` offers. From there `ssh <host>` reaches any server.
//! Clipboard writes from programs (OSC 52) and links are only printed: asking per host and
//! opening through `link.open` come with the interface.

use std::process::ExitCode;

use egui::FontId;
use serde_json::{Value, json};
use sshcraft_engine::{Caller, Session, SessionConfig, SessionId};
use sshcraft_term::{Notice, TermConfig, TermSize, Terminal};
use sshcraft_ui_egui::terminal::pump::{FrameBudget, Pump};
use sshcraft_ui_egui::terminal::{TermPalette, TerminalView, install_terminal_font};

struct App {
    runtime: tokio::runtime::Runtime,
    session: Session,
    id: SessionId,
    pump: Pump,
    term: Terminal,
    palette: TermPalette,
    font: Option<FontId>,
    _dir: tempfile::TempDir,
}

impl App {
    fn execute(&self, id: &str, params: Value) -> Result<Value, String> {
        self.runtime.block_on(self.session.execute(Caller::User, id, params)).map_err(|e| e.to_string())
    }

    fn send(&self, bytes: &[u8]) {
        let params = match std::str::from_utf8(bytes) {
            Ok(text) => json!({ "id": self.id, "data": text }),
            Err(_) => json!({ "id": self.id, "data_base64": data_encoding::BASE64.encode(bytes) }),
        };
        if let Err(e) = self.execute("session.send", params) {
            eprintln!("session.send: {e}");
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let _ = frame;
        let font = self.font.get_or_insert_with(|| FontId::new(14.0, install_terminal_font(ui.ctx(), "Cascadia Mono, Consolas, monospace"))).clone();
        let outcome = self.pump.frame(&mut self.term, FrameBudget::DEFAULT);
        if let Some(status) = &outcome.status {
            eprintln!("status: {status:?}");
        }
        if outcome.closed {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        for notice in self.term.take_notices() {
            match notice {
                Notice::Reply(bytes) => self.send(&bytes),
                Notice::Title(title) => ui.ctx().send_viewport_cmd(egui::ViewportCommand::Title(format!("{title} - SSHCraft terminal"))),
                Notice::ClipboardWrite(text) => eprintln!("a program wants {} bytes in the clipboard (not done here)", text.len()),
                Notice::ResetTitle
                | Notice::Bell
                | Notice::WorkingDirectory(_)
                | Notice::CommandStarted
                | Notice::CommandFinished { .. }
                | Notice::Notification { .. }
                | Notice::ProgramStatusChanged => {}
            }
        }
        let response = TerminalView { term: &mut self.term, palette: &self.palette, font, ligatures: true, highlighter: None }.show(ui);
        if !response.input.is_empty() {
            self.send(&response.input);
        }
        if let Some(size) = response.resize
            && let Err(e) = self.execute("session.resize", json!({ "id": self.id, "cols": size.cols, "rows": size.rows }))
        {
            eprintln!("session.resize: {e}");
        }
        if let Some(text) = response.copy {
            ui.ctx().copy_text(text);
        }
        if let Some(link) = response.open_link {
            eprintln!("link: {link}");
        }
        if outcome.more {
            ui.ctx().request_repaint();
        } else {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(16));
        }
    }
}

fn start() -> Result<App, String> {
    let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let session = {
        let _inside = runtime.enter();
        Session::open(SessionConfig::for_tests(dir.path())).map_err(|e| e.to_string())?.0
    };
    let shells = runtime.block_on(session.execute(Caller::User, "local.shells", json!({}))).map_err(|e| e.to_string())?;
    let shell = shells.get(0).cloned().ok_or("no local shell found")?;
    let opened = runtime
        .block_on(session.execute(Caller::User, "session.open", json!({ "target": { "kind": "local", "shell": shell }, "cols": 100, "rows": 30 })))
        .map_err(|e| e.to_string())?;
    let id: SessionId = serde_json::from_value(opened["session"].clone()).map_err(|e| e.to_string())?;
    let stream = session.attach_terminal(id).ok_or("the session has no terminal")?;
    Ok(App {
        runtime,
        session,
        id,
        pump: Pump::new(stream),
        term: Terminal::new(TermSize::clamped(100, 30), &TermConfig::default()),
        palette: TermPalette::dark(),
        font: None,
        _dir: dir,
    })
}

fn main() -> ExitCode {
    let app = match start() {
        Ok(app) => app,
        Err(e) => {
            eprintln!("terminal_window: {e}");
            return ExitCode::FAILURE;
        }
    };
    let options = eframe::NativeOptions { viewport: egui::ViewportBuilder::default().with_inner_size([960.0, 600.0]), ..Default::default() };
    match eframe::run_native("SSHCraft terminal", options, Box::new(|_| Ok(Box::new(app)))) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("terminal_window: {e}");
            ExitCode::FAILURE
        }
    }
}
