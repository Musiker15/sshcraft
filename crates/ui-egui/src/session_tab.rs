//! A tab per session: its terminal, fed by a [`Pump`] every frame, its status line, and what
//! programs in it ask for. The terminal's answers go back to the session; a title it sets becomes
//! the tab's; a clipboard write (OSC 52) needs the user's consent per host; a link opens only
//! through the engine's `link.open`, which takes `http` and `https` alone.

use std::time::Duration;

use egui::{Rect, Ui, UiBuilder, pos2};
use serde_json::{Value, json};
use sshcraft_engine::settings::Osc52;
use sshcraft_engine::{EndReason, SessionId, SessionStatus};
use sshcraft_model::model::HostId;
use sshcraft_term::{Notice, TermConfig, TermSize, Terminal};

use crate::app::SshcraftApp;
use crate::i18n::{tr, trf};
use crate::state::{ConnectRequest, Page, TabKind, TabState};
use crate::terminal::pump::{FrameBudget, Pump};
use crate::terminal::{TermPalette, TerminalView, install_terminal_font};
use crate::theme::Tokens;
use crate::widgets::visible;

/// Most characters of a title a program sets.
const MAX_TITLE: usize = 256;
const STATUS_HEIGHT: f32 = 28.0;
/// While a session is open the frame loop looks for output this often.
const POLL: Duration = Duration::from_millis(16);

pub struct SessionTab {
    pub id: SessionId,
    pub host: Option<HostId>,
    /// What `session.open` was given, to connect again.
    pub target: Value,
    /// The tab's own name (the host's, the shell's, the port's).
    pub name: String,
    /// A program set the title.
    titled: bool,
    pub term: Terminal,
    pump: Option<Pump>,
    pub status: Option<SessionStatus>,
    /// Text a program wants in the clipboard, waiting for the user's answer.
    pub clipboard: Option<String>,
    /// The terminal takes the keyboard the first time it is shown.
    focus: bool,
}

/// A local shell or serial port to choose before connecting.
#[derive(Debug, Clone)]
pub enum Picker {
    Local(Vec<Value>),
    Serial { ports: Vec<String>, settings: Value },
}

fn tab_index(app: &SshcraftApp, id: SessionId) -> Option<usize> {
    app.ui.tabs.iter().position(|t| matches!(t.kind, TabKind::Session { session } if session == id))
}

fn host_name(app: &SshcraftApp, host: HostId) -> Option<String> {
    app.data.hosts.iter().find(|r| r.host.id == host).map(|r| r.host.name.clone())
}

/// Opens a session for `target` in the tab `replace` (a new-tab page or the tab being
/// reconnected), or in a new tab.
fn open(app: &mut SshcraftApp, target: Value, name: String, host: Option<HostId>, replace: Option<usize>) {
    // A connection to this host is already on its way (a double click): one is enough.
    if host.is_some() && app.ui.connecting.contains(&host) {
        return;
    }
    app.ui.connecting.push(host);
    let asked_from = app.ui.page;
    let clipboard_write = app.data.settings.terminal.clipboard_osc52 != Osc52::Off;
    let params = json!({ "target": target, "cols": 100, "rows": 30 });
    let old = replace.and_then(|i| match app.ui.tabs.get(i).map(|t| &t.kind) {
        Some(TabKind::Session { session }) => Some(*session),
        _ => None,
    });
    app.runner.run(
        "session.open",
        params,
        Box::new(move |app, result| {
            if let Some(i) = app.ui.connecting.iter().position(|h| *h == host) {
                app.ui.connecting.remove(i);
            }
            let id = match result.and_then(|v| {
                serde_json::from_value::<SessionId>(v.get("session").cloned().unwrap_or_default())
                    .map_err(|e| sshcraft_engine::EngineError::bad_params("session.open", e))
            }) {
                Ok(id) => id,
                Err(e) => {
                    app.toasts.error(&e.to_string());
                    return;
                }
            };
            let pump = app.session.attach_terminal(id).map(Pump::new);
            let term = Terminal::new(TermSize::clamped(100, 30), &TermConfig { clipboard_write, ..TermConfig::default() });
            let title = host.and_then(|h| host_name(app, h)).unwrap_or_else(|| name.clone());
            let tab = TabState { title: visible(&title), kind: TabKind::Session { session: id } };
            // The tab it was asked for, if it is still what it was.
            let slot = replace.filter(|&i| match app.ui.tabs.get(i).map(|t| &t.kind) {
                Some(TabKind::New) => true,
                Some(TabKind::Session { session }) => Some(*session) == old,
                None => false,
            });
            let index = match slot {
                Some(i) => {
                    if let Some(old) = old {
                        app.terminals.remove(&old);
                        // The old session ended; closing it finishes its log and frees it.
                        app.run_reported("session.close", json!({ "id": old }));
                    }
                    if let Some(slot) = app.ui.tabs.get_mut(i) {
                        *slot = tab;
                    }
                    i
                }
                None => {
                    app.ui.tabs.push(tab);
                    app.ui.tabs.len() - 1
                }
            };
            // Shown only if the user still is where they asked from; otherwise it waits in its tab.
            let follow = app.ui.page == asked_from;
            if follow {
                app.ui.page = Page::Tab(index);
            }
            let session_tab = SessionTab { id, host, target, name, titled: false, term, pump, status: None, clipboard: None, focus: follow };
            app.terminals.insert(id, session_tab);
        }),
    );
}

/// The tab to replace when a connection is asked for on a new-tab page.
fn new_tab_slot(app: &SshcraftApp) -> Option<usize> {
    match app.ui.page {
        Page::Tab(i) if matches!(app.ui.tabs.get(i).map(|t| &t.kind), Some(TabKind::New)) => Some(i),
        _ => None,
    }
}

/// Acts on the connection a page asked for.
pub fn take_request(app: &mut SshcraftApp) {
    let Some(request) = app.ui.connect_request.take() else { return };
    match request {
        ConnectRequest::Host(host) => {
            // A host the cached data does not have yet: fetch it, the tab takes its name then.
            if host_name(app, host).is_none() {
                app.data.dirty |= crate::data::Refresh::HOSTS;
            }
            let name = host_name(app, host).unwrap_or_else(|| tr("Host").to_string());
            open(app, json!({ "kind": "host", "host_id": host }), name, Some(host), new_tab_slot(app));
        }
        ConnectRequest::Local => app.runner.run(
            "local.shells",
            json!({}),
            Box::new(|app, result| match result {
                Ok(Value::Array(shells)) if !shells.is_empty() => app.ui.picker = Some(Picker::Local(shells)),
                Ok(_) => app.toasts.error(tr("No local shell was found.")),
                Err(e) => app.toasts.error(&e.to_string()),
            }),
        ),
        ConnectRequest::Serial => app.runner.run_future(
            {
                let session = app.session.clone();
                async move {
                    let ports = session.execute(sshcraft_engine::Caller::User, "serial.ports", json!({})).await?;
                    let last = session.execute(sshcraft_engine::Caller::User, "serial.last", json!({})).await?;
                    Ok(json!({ "ports": ports, "last": last }))
                }
            },
            Box::new(|app, result| match result {
                Ok(v) => {
                    let ports: Vec<String> = v["ports"]
                        .as_array()
                        .map(|p| p.iter().filter_map(|p| p.get("name").and_then(Value::as_str)).map(str::to_string).collect())
                        .unwrap_or_default();
                    if ports.is_empty() {
                        app.toasts.error(tr("No serial port was found."));
                        return;
                    }
                    let mut settings = v["last"].clone();
                    let last_port = settings.get("port").and_then(Value::as_str).unwrap_or_default().to_string();
                    if !ports.contains(&last_port) {
                        settings["port"] = json!(ports[0]);
                    }
                    app.ui.picker = Some(Picker::Serial { ports, settings });
                }
                Err(e) => app.toasts.error(&e.to_string()),
            }),
        ),
    }
}

/// Sends what the terminal (or the user) typed to the session.
fn send(app: &SshcraftApp, id: SessionId, bytes: &[u8]) {
    let params = match std::str::from_utf8(bytes) {
        Ok(text) => json!({ "id": id, "data": text }),
        Err(_) => json!({ "id": id, "data_base64": data_encoding::BASE64.encode(bytes) }),
    };
    app.run_reported("session.send", params);
}

fn ended(status: Option<&SessionStatus>) -> bool {
    matches!(status, Some(SessionStatus::Ended { .. }))
}

/// Feeds every session's output into its terminal and acts on what programs asked for.
pub fn pump_all(app: &mut SshcraftApp, ctx: &egui::Context) {
    if app.terminals.is_empty() {
        return;
    }
    let mut more = false;
    let ids: Vec<SessionId> = app.terminals.keys().copied().collect();
    for id in ids {
        let Some(tab) = app.terminals.get_mut(&id) else { continue };
        if let Some(pump) = tab.pump.as_mut() {
            let outcome = pump.frame(&mut tab.term, FrameBudget::DEFAULT);
            if let Some(status) = outcome.status {
                tab.status = Some(status);
            }
            more |= outcome.more;
            if outcome.closed {
                tab.pump = None;
            }
        }
        let notices = tab.term.take_notices();
        let host = tab.host;
        let can_send = !ended(tab.status.as_ref());
        for notice in notices {
            match notice {
                Notice::Reply(bytes) if can_send => send(app, id, &bytes),
                Notice::Reply(_) | Notice::Bell => {}
                Notice::Title(title) => {
                    let title: String = visible(&title).chars().take(MAX_TITLE).collect();
                    if let Some(tab) = app.terminals.get_mut(&id) {
                        tab.titled = true;
                    }
                    if let Some(i) = tab_index(app, id) {
                        app.ui.tabs[i].title = title;
                    }
                }
                Notice::ResetTitle => {
                    if let Some(tab) = app.terminals.get_mut(&id) {
                        tab.titled = false;
                    }
                }
                Notice::ClipboardWrite(text) => clipboard_write(app, ctx, id, host, text),
            }
        }
        // The host's name once the data has it, unless a program named the tab.
        if let Some(tab) = app.terminals.get(&id)
            && !tab.titled
        {
            let title = visible(&tab.host.and_then(|h| host_name(app, h)).unwrap_or_else(|| tab.name.clone()));
            if let Some(i) = tab_index(app, id)
                && app.ui.tabs[i].title != title
            {
                app.ui.tabs[i].title = title;
            }
        }
    }
    if more {
        ctx.request_repaint();
    } else if app.terminals.values().any(|t| t.pump.is_some()) {
        ctx.request_repaint_after(POLL);
    }
}

/// OSC 52: never when turned off, at once for a host the user always allows, else asked.
fn clipboard_write(app: &mut SshcraftApp, ctx: &egui::Context, id: SessionId, host: Option<HostId>, text: String) {
    let terminal = &app.data.settings.terminal;
    if terminal.clipboard_osc52 == Osc52::Off {
        return;
    }
    if host.is_some_and(|h| terminal.clipboard_hosts.contains(&h)) {
        ctx.copy_text(text);
        return;
    }
    if let Some(tab) = app.terminals.get_mut(&id) {
        tab.clipboard = Some(text);
    }
}

/// Asks about the first clipboard write that waits for an answer.
pub fn show_clipboard_question(app: &mut SshcraftApp, ctx: &egui::Context) {
    let Some((id, host, count)) = app.terminals.values().find_map(|t| t.clipboard.as_ref().map(|c| (t.id, t.host, c.chars().count()))) else {
        return;
    };
    let name = app.terminals.get(&id).map(|t| t.host.and_then(|h| host_name(app, h)).unwrap_or_else(|| t.name.clone())).unwrap_or_default();
    #[derive(PartialEq)]
    enum Choice {
        Once,
        Always,
        No,
    }
    let mut choice = None;
    egui::Modal::new(egui::Id::unique("sshcraft-clipboard")).show(ctx, |ui| {
        ui.set_width(460.0);
        ui.heading(tr("Clipboard"));
        ui.add_space(6.0);
        ui.label(trf(
            "A program on {host} wants to put {count} characters in the clipboard. Allow it?",
            &[("host", &visible(&name)), ("count", &count.to_string())],
        ));
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button(tr("Once")).clicked() {
                choice = Some(Choice::Once);
            }
            if host.is_some() && ui.button(tr("Always for this host")).clicked() {
                choice = Some(Choice::Always);
            }
            if ui.button(tr("No")).clicked() {
                choice = Some(Choice::No);
            }
        });
    });
    let Some(choice) = choice else { return };
    let Some(text) = app.terminals.get_mut(&id).and_then(|t| t.clipboard.take()) else { return };
    if choice == Choice::No {
        return;
    }
    ctx.copy_text(text);
    if choice == Choice::Always
        && let Some(host) = host
    {
        let mut settings = app.data.settings.clone();
        if !settings.terminal.clipboard_hosts.contains(&host) {
            settings.terminal.clipboard_hosts.push(host);
        }
        app.data.settings.terminal.clipboard_hosts.clone_from(&settings.terminal.clipboard_hosts);
        app.run_reported("settings.save", json!({ "settings": settings }));
    }
}

fn status_text(status: Option<&SessionStatus>) -> (String, bool) {
    match status {
        None | Some(SessionStatus::Connecting) => (tr("Connecting…").to_string(), false),
        Some(SessionStatus::Connected { .. }) => (tr("Connected").to_string(), false),
        Some(SessionStatus::Reconnecting { attempt, delay_secs }) => (
            trf(
                "Connection lost. Reconnecting (attempt {attempt}) in {seconds} s.",
                &[("attempt", &attempt.to_string()), ("seconds", &delay_secs.to_string())],
            ),
            false,
        ),
        Some(SessionStatus::Ended { reason, .. }) => {
            let text = match reason {
                EndReason::Exited => tr("The session ended."),
                EndReason::Lost => tr("The connection was lost."),
                EndReason::HostKeyRejected => tr("The host key was not accepted."),
                EndReason::HostKeyMismatch => tr(
                    "The host key of this server changed. Someone may be listening in, or the server was set up again. SSHCraft does not connect; if the change is expected, forget the old key in the host's settings.",
                ),
                EndReason::AuthFailed => tr("Sign-in failed."),
                EndReason::AuthCancelled => tr("Sign-in was cancelled."),
                EndReason::ConnectFailed => tr("The server could not be reached."),
                EndReason::Error => tr("The session could not start."),
            };
            (text.to_string(), true)
        }
    }
}

fn status_line(app: &mut SshcraftApp, ui: &mut Ui, id: SessionId, index: usize) {
    let tokens = Tokens::get(ui.ctx());
    let Some(tab) = app.terminals.get(&id) else { return };
    let (text, is_end) = status_text(tab.status.as_ref());
    let mismatch = matches!(tab.status, Some(SessionStatus::Ended { reason: EndReason::HostKeyMismatch, .. }));
    let detail = match &tab.status {
        Some(SessionStatus::Ended { detail: Some(d), .. }) => Some(visible(d)),
        _ => None,
    };
    let mut reconnect = false;
    ui.horizontal_centered(|ui| {
        let color = if mismatch {
            tokens.danger
        } else if is_end {
            tokens.warning
        } else {
            tokens.text_dim
        };
        ui.label(egui::RichText::new(text).color(color));
        if let Some(detail) = detail {
            // A server's detail can be long: cut, so Reconnect stays on screen.
            let room = (ui.available_width() - 160.0).max(80.0);
            crate::widgets::elided_label(ui, &detail, room, tokens.text_dim, egui::FontId::proportional(11.0));
        }
        if is_end && !mismatch && ui.button(tr("Reconnect")).clicked() {
            reconnect = true;
        }
    });
    if reconnect && let Some(tab) = app.terminals.get(&id) {
        open(app, tab.target.clone(), tab.name.clone(), tab.host, Some(index));
    }
}

/// A question or another window is over the terminal: it gets no keys.
pub fn modal_open(app: &SshcraftApp) -> bool {
    !app.prompts.open.is_empty() || app.ui.picker.is_some() || app.ui.palette.open || app.terminals.values().any(|t| t.clipboard.is_some())
}

/// Whether `id` is another terminal than the one shown last frame.
fn app_shown_changed(shown: &mut Option<SessionId>, id: SessionId) -> bool {
    let changed = *shown != Some(id);
    *shown = Some(id);
    changed
}

/// The page of a session tab: the terminal and its status line.
pub fn show(app: &mut SshcraftApp, ui: &mut Ui, id: SessionId, index: usize) {
    let full = ui.available_rect_before_wrap();
    let term_rect = Rect::from_min_max(full.min, pos2(full.max.x, (full.max.y - STATUS_HEIGHT).max(full.min.y)));
    let status_rect = Rect::from_min_max(pos2(full.min.x, term_rect.max.y), full.max);
    let ctx = ui.ctx().clone();
    let pending = std::mem::take(&mut app.ui.pending);
    let appearance = &app.data.settings.appearance;
    // Loading a font is slow: only when the setting changes.
    if app.term_font.as_ref().is_none_or(|(name, _)| *name != appearance.font_family) {
        app.term_font = Some((appearance.font_family.clone(), install_terminal_font(&ctx, &appearance.font_family)));
    }
    let family = app.term_font.as_ref().map_or(egui::FontFamily::Monospace, |(_, f)| f.clone());
    let font = egui::FontId::new(f32::from(appearance.font_size), family);
    let palette = if appearance.terminal_theme == "sshcraft-light" { TermPalette::light() } else { TermPalette::dark() };
    let blocked = modal_open(app);
    let Some(tab) = app.terminals.get_mut(&id) else {
        ui.label(tr("This session is gone."));
        return;
    };
    for command in pending {
        match command {
            "ui.terminal.copy" => {
                if let Some(text) = tab.term.selection_text() {
                    ctx.copy_text(text);
                }
            }
            "ui.terminal.paste" => ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste),
            _ => {}
        }
    }
    let response = ui
        .scope_builder(UiBuilder::new().max_rect(term_rect).id_salt(id), |ui| TerminalView { term: &mut tab.term, palette: &palette, font }.show(ui))
        .inner;
    let first = std::mem::take(&mut tab.focus);
    let switched = app_shown_changed(&mut app.ui.shown_terminal, id);
    let Some(tab) = app.terminals.get_mut(&id) else { return };
    if blocked {
        // A question is open: what is typed belongs to it, never to the shell behind it.
        response.response.surrender_focus();
    } else if first || switched || ctx.memory(|m| m.focused().is_none()) {
        response.response.request_focus();
    }
    let can_send = !ended(tab.status.as_ref()) && !blocked;
    if !response.input.is_empty() && can_send {
        send(app, id, &response.input);
    }
    if let Some(size) = response.resize
        && can_send
    {
        app.run_reported("session.resize", json!({ "id": id, "cols": size.cols, "rows": size.rows }));
    }
    if let Some(text) = response.copy {
        ctx.copy_text(text);
    }
    if let Some(url) = response.open_link {
        app.run_reported("link.open", json!({ "url": url }));
    }
    ui.scope_builder(UiBuilder::new().max_rect(status_rect), |ui| status_line(app, ui, id, index));
}

/// The shell and serial pickers.
pub fn show_picker(app: &mut SshcraftApp, ctx: &egui::Context) {
    let Some(mut picker) = app.ui.picker.take() else { return };
    let mut keep = true;
    let mut chosen: Option<(Value, String)> = None;
    let modal = egui::Modal::new(egui::Id::unique("sshcraft-picker")).show(ctx, |ui| {
        ui.set_width(420.0);
        match &mut picker {
            Picker::Local(shells) => {
                ui.heading(tr("Local terminal"));
                ui.add_space(6.0);
                for shell in shells.iter() {
                    let label = shell.get("label").and_then(Value::as_str).unwrap_or_default().to_string();
                    if ui.button(visible(&label)).clicked() {
                        chosen = Some((json!({ "kind": "local", "shell": shell }), label));
                    }
                }
            }
            Picker::Serial { ports, settings } => {
                ui.heading(tr("Serial port"));
                ui.add_space(6.0);
                let mut port = settings.get("port").and_then(Value::as_str).unwrap_or_default().to_string();
                let mut baud = settings.get("baud").and_then(Value::as_u64).unwrap_or(115_200);
                egui::Grid::new("sshcraft-serial").num_columns(2).min_col_width(130.0).show(ui, |ui| {
                    ui.label(tr("Port"));
                    egui::ComboBox::from_id_salt("sshcraft-serial-port").selected_text(visible(&port)).show_ui(ui, |ui| {
                        for p in ports.iter() {
                            ui.selectable_value(&mut port, p.clone(), visible(p));
                        }
                    });
                    ui.end_row();
                    ui.label(tr("Baud rate"));
                    ui.add(egui::DragValue::new(&mut baud).range(50..=4_000_000));
                    ui.end_row();
                });
                settings["port"] = json!(port);
                settings["baud"] = json!(baud);
                if ui.button(tr("Connect")).clicked() {
                    chosen = Some((json!({ "kind": "serial", "settings": settings.clone() }), port));
                }
            }
        }
        ui.add_space(8.0);
        if ui.button(tr("Cancel")).clicked() {
            keep = false;
        }
    });
    if modal.should_close() {
        keep = false;
    }
    if let Some((target, name)) = chosen {
        if let Some(settings) = target.get("settings") {
            app.run_reported("serial.remember", json!({ "settings": settings }));
        }
        open(app, target, name, None, new_tab_slot(app));
        return;
    }
    if keep {
        app.ui.picker = Some(picker);
    }
}
