//! A tab per session: its terminal, fed by a [`Pump`] every frame, its status line, and what
//! programs in it ask for. The terminal's answers go back to the session; a title it sets becomes
//! the tab's; a clipboard write (OSC 52) needs the user's consent per host; a link opens only
//! through the engine's `link.open`, which takes `http` and `https` alone.

use egui::{Rect, Ui, UiBuilder, pos2};
use serde_json::{Value, json};
use sshcraft_engine::settings::Osc52;
use sshcraft_engine::{EndReason, SessionId, SessionStatus};
use sshcraft_model::model::HostId;
use sshcraft_term::{Notice, TermConfig, TermSize, Terminal};

use crate::app::SshcraftApp;
use crate::i18n::{tr, trf};
use crate::state::{ConnectRequest, Page, PaneSlot, Panes, TabState};
use crate::terminal::pump::{FrameBudget, Pump};
use crate::terminal::{TermPalette, TerminalView, install_terminal_font};
use crate::theme::Tokens;
use crate::widgets::visible;
use sshcraft_engine::settings::TriggerAction;

/// Most characters of a title a program sets.
const MAX_TITLE: usize = 256;
const STATUS_HEIGHT: f32 = 28.0;

pub struct SessionTab {
    pub id: SessionId,
    pub host: Option<HostId>,
    /// What `session.open` was given, to connect again.
    pub target: Value,
    /// The tab's own name (the host's, the shell's, the port's).
    pub name: String,
    /// A program set the title.
    titled: bool,
    /// The pane's title: the program's, else the host's or the shell's name.
    pub title: String,
    pub term: Terminal,
    pump: Option<Pump>,
    pub status: Option<SessionStatus>,
    /// Text a program wants in the clipboard, waiting for the user's answer.
    pub clipboard: Option<String>,
    /// The terminal takes the keyboard the first time it is shown.
    focus: bool,
    /// Output came while nobody looked at the pane.
    unseen: bool,
    /// The shell's working directory, as OSC 7 reported it.
    pub cwd: Option<String>,
    /// Bytes to type once, when the session first connects (a template's `cd` and command, a
    /// snippet).
    start: Option<Vec<u8>>,
    /// The line being typed, for history and suggestions.
    capture: crate::history::Capture,
    /// This pane's history (`history.list` of its host or the local shell), newest first.
    history: Vec<String>,
    history_loaded: bool,
    suggestions: Vec<crate::completion::Suggestion>,
    selected: Option<usize>,
    /// The list was closed for the line typed so far.
    dismissed: bool,
    /// The line the suggestions were made for.
    suggested_for: Option<String>,
    paths: crate::completion::PathCache,
    path_pending: Option<String>,
    /// The session records its output (asked once it connected).
    recording: Option<bool>,
    /// The bell rang, or the cursor's line asks for a secret or a yes/no.
    waiting: bool,
    /// The terminal view's widget, to know whether it has the keyboard.
    view: Option<egui::Id>,
    /// When the running command started (shell integration), for the notification when it ends.
    command_since: Option<std::time::Instant>,
    /// Server messages of this tab shown on the desktop.
    server_limit: crate::notify::RateLimit,
    /// The labelled hints while hint mode is on.
    hints: Option<Vec<(String, crate::terminal::hints::Hint)>>,
    /// The letters of a label typed so far.
    hint_typed: String,
    /// The compose field's text while it is open.
    compose: Option<String>,
    /// The list of recent pastes is open.
    paste_picker: bool,
}

impl SessionTab {
    /// Output arrived: unseen, and waiting if the cursor's line now asks something.
    /// The compose field's text while it is open.
    pub fn compose_text(&self) -> Option<&String> {
        self.compose.as_ref()
    }

    pub fn compose_text_mut(&mut self) -> Option<&mut String> {
        self.compose.as_mut()
    }

    /// Whether hint mode shows its labels.
    pub fn hints_shown(&self) -> bool {
        self.hints.is_some()
    }

    /// The bell rang, or a trigger or the cursor's line asks for the user.
    pub fn waiting(&self) -> bool {
        self.waiting
    }

    pub fn mark_output(&mut self) {
        self.unseen = true;
        let screen = self.term.screen();
        let line = screen.text().lines().nth(usize::from(screen.cursor.row)).map(str::to_string).unwrap_or_default();
        self.waiting = crate::status::waiting_line(&line);
    }

    /// What the list under the cursor offers now.
    pub fn suggestions(&self) -> &[crate::completion::Suggestion] {
        if self.dismissed { &[] } else { &self.suggestions }
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// The pane's history as loaded, newest first.
    pub fn history(&self) -> &[String] {
        &self.history
    }

    /// Fetches this pane's history again.
    pub fn reload_history(&mut self) {
        self.history_loaded = false;
    }

    /// The history scope of this pane: `host:<id>`, `local`, or none for a serial line.
    fn scope(&self) -> Option<String> {
        match self.target.get("kind").and_then(Value::as_str)? {
            "host" => self.target.get("host_id").and_then(Value::as_str).map(|id| format!("host:{id}")),
            "local" => Some("local".into()),
            _ => None,
        }
    }

    /// The pane is looked at: blue and amber go, red stays.
    pub fn looked_at(&mut self) {
        self.unseen = false;
        self.waiting = false;
    }

    pub fn dot(&self) -> crate::status::Dot {
        use crate::status::Dot;
        match &self.status {
            Some(SessionStatus::Ended { reason: EndReason::Exited, .. }) => Dot::Ended,
            Some(SessionStatus::Ended { .. }) => Dot::Error,
            None | Some(SessionStatus::Connecting | SessionStatus::Reconnecting { .. }) => Dot::Connecting,
            Some(SessionStatus::Connected { .. }) if self.waiting => Dot::Waiting,
            Some(SessionStatus::Connected { .. }) if self.unseen => Dot::Unseen,
            Some(SessionStatus::Connected { .. }) => Dot::Connected,
        }
    }
}

/// The dot of a pane: its session's, or none for a new-pane page.
pub fn slot_dot(app: &SshcraftApp, slot: PaneSlot) -> Option<crate::status::Dot> {
    match slot {
        PaneSlot::Session { session } => app.terminals.get(&session).map(SessionTab::dot),
        PaneSlot::New { .. } => None,
    }
}

/// The focused pane of the shown tab is looked at.
pub fn mark_looked_at(app: &mut SshcraftApp) {
    if let Page::Tab(i) = app.ui.page
        && let Some(PaneSlot::Session { session }) = app.ui.tabs.get(i).and_then(|t| t.panes.focused_slot())
        && let Some(tab) = app.terminals.get_mut(&session)
    {
        tab.looked_at();
    }
}

/// A local shell or serial port to choose before connecting.
#[derive(Debug, Clone)]
pub enum Picker {
    Local(Vec<Value>),
    Serial { ports: Vec<String>, settings: Value },
}

/// The tab and pane that show session `id`.
pub fn pane_of(app: &SshcraftApp, id: SessionId) -> Option<(usize, usize)> {
    let slot = PaneSlot::Session { session: id };
    app.ui.tabs.iter().enumerate().find_map(|(t, tab)| tab.panes.position(slot).map(|p| (t, p)))
}

/// Each tab's title is the title of its focused pane.
pub fn refresh_titles(app: &mut SshcraftApp) {
    for tab in app.ui.tabs.iter_mut().filter(|t| !t.named) {
        let title = match tab.panes.focused_slot() {
            Some(PaneSlot::Session { session }) => app.terminals.get(&session).map(|t| t.title.clone()).unwrap_or_default(),
            // A new-pane page keeps the tab's title, or names it when it has none.
            _ if !tab.title.is_empty() => continue,
            _ => tr("New tab").to_string(),
        };
        if tab.title != title {
            tab.title = title;
        }
    }
}

fn host_name(app: &SshcraftApp, host: HostId) -> Option<String> {
    app.data.hosts.iter().find(|r| r.host.id == host).map(|r| r.host.name.clone())
}

/// Opens a session for `target` in the pane `replace` (a new-pane page, or the pane being
/// reconnected), or in a new tab when no pane asked. A pane closed while it connects takes its
/// connection with it.
fn open(app: &mut SshcraftApp, target: Value, name: String, host: Option<HostId>, replace: Option<PaneSlot>) {
    open_with(app, target, name, host, replace, None);
}

/// [`open`] with a line to type once the session first connects.
pub fn open_with(app: &mut SshcraftApp, target: Value, name: String, host: Option<HostId>, replace: Option<PaneSlot>, start: Option<Vec<u8>>) {
    // A connection for this pane (or, without one, to this host) is already on its way: one is
    // enough (a double click asks twice).
    let key = (replace, if replace.is_some() { None } else { host });
    if (key.0.is_some() || key.1.is_some()) && app.ui.connecting.contains(&key) {
        return;
    }
    app.ui.connecting.push(key);
    let asked_from = app.ui.page;
    let clipboard_write = app.data.settings.terminal.clipboard_osc52 != Osc52::Off;
    let scrollback = app.data.settings.terminal.scrollback;
    let params = json!({ "target": target, "cols": 100, "rows": 30 });
    let old = match replace {
        Some(PaneSlot::Session { session }) => Some(session),
        _ => None,
    };
    app.runner.run(
        "session.open",
        params,
        Box::new(move |app, result| {
            if let Some(i) = app.ui.connecting.iter().position(|k| *k == key) {
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
            // The pane it was asked for, if it is still there; without one, a tab of its own.
            let place = replace.and_then(|wanted| app.ui.tabs.iter().enumerate().find_map(|(t, tab)| tab.panes.position(wanted).map(|p| (t, p))));
            if replace.is_some() && place.is_none() {
                // The user closed that pane while it connected: the connection goes too, and
                // nothing it was to type runs.
                app.run_reported("session.close", json!({ "id": id }));
                return;
            }
            let pump = app.session.attach_terminal(id).map(Pump::new);
            let term = Terminal::new(TermSize::clamped(100, 30), &TermConfig { clipboard_write, scrollback });
            let title = host.and_then(|h| host_name(app, h)).unwrap_or_else(|| name.clone());
            let slot = PaneSlot::Session { session: id };
            let (index, pane) = match place {
                Some((t, p)) => {
                    if let Some(old) = old {
                        app.terminals.remove(&old);
                        // The old session ended; closing it finishes its log and frees it.
                        app.run_reported("session.close", json!({ "id": old }));
                    }
                    if let Some(tab) = app.ui.tabs.get_mut(t)
                        && let Some(cell) = tab.panes.panes.get_mut(p)
                    {
                        *cell = slot;
                    }
                    (t, p)
                }
                None => {
                    app.ui.tabs.push(TabState { title: visible(&title), panes: Panes::single(slot), named: false });
                    (app.ui.tabs.len() - 1, 0)
                }
            };
            // Shown only if the user still is where they asked from; otherwise it waits in its tab.
            let follow = app.ui.page == asked_from;
            if follow {
                app.ui.page = Page::Tab(index);
                if let Some(tab) = app.ui.tabs.get_mut(index) {
                    tab.panes.focused = pane;
                }
            }
            let session_tab = SessionTab {
                id,
                host,
                target,
                name,
                titled: false,
                title: visible(&title),
                term,
                pump,
                status: None,
                clipboard: None,
                focus: follow,
                unseen: false,
                waiting: false,
                view: None,
                cwd: None,
                start,
                capture: crate::history::Capture::default(),
                history: Vec::new(),
                history_loaded: false,
                suggestions: Vec::new(),
                selected: None,
                dismissed: false,
                suggested_for: None,
                paths: crate::completion::PathCache::default(),
                path_pending: None,
                recording: None,
                command_since: None,
                server_limit: crate::notify::RateLimit::default(),
                hints: None,
                hint_typed: String::new(),
                compose: None,
                paste_picker: false,
            };
            app.terminals.insert(id, session_tab);
        }),
    );
}

/// The new-pane page a connection asked for on it goes into: the focused pane of the shown tab.
fn new_tab_slot(app: &SshcraftApp) -> Option<PaneSlot> {
    match app.ui.page {
        Page::Tab(i) => app.ui.tabs.get(i).and_then(|t| t.panes.focused_slot()).filter(|s| matches!(s, PaneSlot::New { .. })),
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

/// Types `bytes` into session `id` (a snippet, a paste).
pub fn send_bytes(app: &SshcraftApp, id: SessionId, bytes: &[u8]) {
    send(app, id, bytes);
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
/// The texts pasted in this frame.
fn ctx_pastes(ctx: &egui::Context) -> Vec<String> {
    ctx.input(|i| i.events.iter().filter_map(|e| if let egui::Event::Paste(text) = e { Some(text.clone()) } else { None }).collect())
}

/// The compose field in `rect` and the list of recent pastes over the terminal; returns the bytes
/// one of them sends.
fn compose_and_pastes(app: &mut SshcraftApp, ui: &mut Ui, id: SessionId, rect: Rect, term_rect: Rect, active: bool, send: bool) -> Option<Vec<u8>> {
    let pastes: Vec<String> = app.pastes.entries().iter().cloned().collect();
    let tab = app.terminals.get_mut(&id)?;
    let modes = tab.term.modes();
    let mut out = None;
    if let Some(text) = tab.compose.as_mut() {
        let edit_id = egui::Id::unique(("sshcraft-compose", id));
        // In the frame it opens the field has no room yet; egui must never get a negative size.
        let area = rect.shrink(4.0);
        if area.width() > 0.0 && area.height() > 0.0 {
            ui.scope_builder(UiBuilder::new().max_rect(area), |ui| {
                let edit = egui::TextEdit::multiline(text)
                    .id(edit_id)
                    .hint_text(tr("Write here; Ctrl+Enter sends it, Esc closes"))
                    .desired_width(f32::INFINITY)
                    .desired_rows(3)
                    .font(egui::TextStyle::Monospace);
                ui.add_sized(area.size(), edit);
            });
        }
        let send = active && send;
        let close = active && ui.input(|i| i.key_pressed(egui::Key::Escape)) && ui.memory(|m| m.has_focus(edit_id));
        if send && !text.trim().is_empty() {
            out = Some(crate::terminal::compose::compose_bytes(text, modes));
            text.clear();
        }
        if close {
            tab.compose = None;
        }
    }
    if tab.paste_picker {
        let mut open = true;
        egui::Window::new(tr("Recent pastes"))
            .id(egui::Id::unique(("sshcraft-pastes", id)))
            .fixed_pos(term_rect.min + egui::vec2(24.0, 24.0))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ui.ctx(), |ui| {
                if pastes.is_empty() {
                    ui.label(tr("Nothing pasted yet."));
                }
                for text in &pastes {
                    if ui.button(crate::terminal::compose::preview(text)).clicked() {
                        out = Some(sshcraft_term::input::encode_paste(text, modes));
                        tab.paste_picker = false;
                    }
                }
            });
        if !open || (active && ui.input(|i| i.key_pressed(egui::Key::Escape))) {
            tab.paste_picker = false;
        }
    }
    out
}

/// Takes the keys hint mode needs before the terminal sees them: letters choose a label (in
/// capitals: to type its text in), Escape leaves. Returns the text chosen and whether to type it.
fn hint_keys(app: &mut SshcraftApp, ctx: &egui::Context, id: SessionId) -> Option<(String, bool)> {
    let tab = app.terminals.get_mut(&id)?;
    let hints = tab.hints.as_ref()?;
    let events = ctx.input_mut(|i| {
        let taken: Vec<egui::Event> = i.events.iter().filter(|e| matches!(e, egui::Event::Text(_) | egui::Event::Key { .. })).cloned().collect();
        i.events.retain(|e| !matches!(e, egui::Event::Text(_) | egui::Event::Key { .. }));
        taken
    });
    let mut picked = None;
    let mut close = false;
    for event in events {
        match event {
            egui::Event::Key { key: egui::Key::Escape, pressed: true, .. } => close = true,
            egui::Event::Text(text) => {
                for c in text.chars().filter(char::is_ascii_alphabetic) {
                    let paste = c.is_ascii_uppercase();
                    tab.hint_typed.push(c.to_ascii_lowercase());
                    if let Some((_, hint)) = hints.iter().find(|(label, _)| *label == tab.hint_typed) {
                        picked = Some((hint.text.clone(), paste));
                    } else if !hints.iter().any(|(label, _)| label.starts_with(tab.hint_typed.as_str())) {
                        // No label goes on like this: start over.
                        tab.hint_typed.clear();
                    }
                }
            }
            _ => {}
        }
    }
    if picked.is_some() || close {
        tab.hints = None;
        tab.hint_typed.clear();
    }
    picked
}

/// The look of `host`'s terminals: its own appearance with what its groups give.
pub fn host_appearance(app: &SshcraftApp, host: Option<HostId>) -> sshcraft_model::appearance::Appearance {
    let Some(row) = host.and_then(|id| app.data.hosts.iter().find(|r| r.host.id == id)) else { return Default::default() };
    let groups: Vec<sshcraft_model::model::Group> = app.data.groups.iter().map(|g| g.group.clone()).collect();
    sshcraft_model::appearance::appearance_for(&row.host, &groups)
}

/// The badge in the terminal's top right corner, on the host's colour.
fn paint_badge(ui: &Ui, rect: Rect, badge: &str, color: Option<[u8; 3]>) {
    let painter = ui.painter_at(rect);
    let galley = painter.layout_no_wrap(badge.to_string(), egui::FontId::proportional(12.0), egui::Color32::WHITE);
    let size = galley.size() + egui::vec2(12.0, 6.0);
    let area = Rect::from_min_size(pos2(rect.max.x - size.x - 10.0, rect.min.y + 8.0), size);
    let [r, g, b] = color.unwrap_or([0x55, 0x5a, 0x66]);
    painter.rect_filled(area, 4.0, egui::Color32::from_rgba_unmultiplied(r, g, b, 210));
    painter.galley(area.min + egui::vec2(6.0, 3.0), galley, egui::Color32::WHITE);
}

/// Whether the user looks at tab `id`: the window has the focus and the tab is the one shown.
fn watched(app: &SshcraftApp, ctx: &egui::Context, id: SessionId) -> bool {
    let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
    focused && app.ui.shown_terminal == Some(id)
}

/// The name a notification of tab `id` starts with.
fn tab_label(app: &SshcraftApp, id: SessionId) -> String {
    app.terminals.get(&id).map(|t| visible(&t.host.and_then(|h| host_name(app, h)).unwrap_or_else(|| t.name.clone()))).unwrap_or_default()
}

/// A command ended: a notification when it ran long and nobody watched.
fn command_finished(app: &mut SshcraftApp, ctx: &egui::Context, id: SessionId, exit: Option<i32>) {
    let since = app.terminals.get_mut(&id).and_then(|t| t.command_since.take());
    let Some(since) = since else { return };
    let after = app.data.settings.terminal.notify_after_secs;
    if !crate::notify::command_worth_it(after, since.elapsed(), watched(app, ctx, id)) {
        return;
    }
    let title = trf("Command finished on {host}", &[("host", &tab_label(app, id))]);
    let body = match exit {
        Some(0) => tr("It succeeded.").to_string(),
        Some(code) => trf("It ended with exit code {code}.", &[("code", &code.to_string())]),
        None => tr("It ended.").to_string(),
    };
    app.notifier.notify(&title, &body);
}

/// A message from the server (OSC 9, OSC 777, a program waiting), shown only while servers may.
pub(crate) fn server_message(app: &mut SshcraftApp, ctx: &egui::Context, id: SessionId, title: Option<&str>, body: &str) {
    if app.data.settings.terminal.server_notifications {
        desktop_message(app, ctx, id, title, body);
    }
}

/// A message about tab `id` for the desktop: the host's name first, limited per tab, a toast
/// instead while the user watches the tab.
fn desktop_message(app: &mut SshcraftApp, ctx: &egui::Context, id: SessionId, title: Option<&str>, body: &str) {
    let allowed = app.terminals.get_mut(&id).is_some_and(|t| t.server_limit.allow(std::time::Instant::now()));
    if !allowed {
        return;
    }
    let host = tab_label(app, id);
    let title = match title.filter(|t| !t.is_empty()) {
        Some(title) => format!("{host}: {title}"),
        None => host,
    };
    if watched(app, ctx, id) {
        app.toasts.info(&format!("{title}: {body}"));
    } else {
        app.notifier.notify(&title, body);
    }
}

pub fn pump_all(app: &mut SshcraftApp, ctx: &egui::Context) {
    if app.terminals.is_empty() {
        return;
    }
    let terminal = &app.data.settings.terminal;
    app.patterns.refresh(&terminal.highlights, &terminal.triggers);
    let capture = !app.patterns.triggers.is_empty();
    let mut more = false;
    let mut starts = Vec::new();
    let mut ask_recording = Vec::new();
    let ids: Vec<SessionId> = app.terminals.keys().copied().collect();
    for id in ids {
        let Some(tab) = app.terminals.get_mut(&id) else { continue };
        // Finished lines are kept only while a trigger wants them.
        tab.term.set_line_capture(capture);
        if let Some(pump) = tab.pump.as_mut() {
            let outcome = pump.frame(&mut tab.term, FrameBudget::DEFAULT);
            if outcome.fed > 0 {
                tab.mark_output();
            }
            if let Some(status) = outcome.status {
                // The template's line goes out once, when the session first connects.
                if matches!(status, SessionStatus::Connected { .. })
                    && let Some(line) = tab.start.take()
                {
                    starts.push((id, line));
                }
                if matches!(status, SessionStatus::Connected { .. }) && tab.recording.is_none() {
                    tab.recording = Some(false);
                    ask_recording.push(id);
                }
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
                Notice::Reply(_) => {}
                Notice::Bell => {
                    if let Some(tab) = app.terminals.get_mut(&id) {
                        tab.waiting = true;
                    }
                }
                Notice::Title(title) => {
                    let title: String = visible(&title).chars().take(MAX_TITLE).collect();
                    if let Some(tab) = app.terminals.get_mut(&id) {
                        tab.titled = true;
                        tab.title = title;
                    }
                }
                Notice::ResetTitle => {
                    if let Some(tab) = app.terminals.get_mut(&id) {
                        tab.titled = false;
                    }
                }
                Notice::ClipboardWrite(text) => clipboard_write(app, ctx, id, host, text),
                Notice::WorkingDirectory(dir) => {
                    if let Some(tab) = app.terminals.get_mut(&id) {
                        tab.cwd = Some(dir);
                    }
                }
                Notice::CommandStarted => {
                    if let Some(tab) = app.terminals.get_mut(&id) {
                        tab.command_since = Some(std::time::Instant::now());
                    }
                }
                Notice::CommandFinished { exit } => command_finished(app, ctx, id, exit),
                Notice::Notification { title, body } => server_message(app, ctx, id, title.as_deref(), &body),
                Notice::ProgramStatusChanged => {}
            }
        }
        let lines = app.terminals.get_mut(&id).map(|t| t.term.take_completed_lines()).unwrap_or_default();
        for line in lines {
            for action in app.patterns.triggers.actions(&line) {
                match action {
                    TriggerAction::Notify => desktop_message(app, ctx, id, Some(tr("Trigger")), &line),
                    TriggerAction::Bell => {
                        if let Some(tab) = app.terminals.get_mut(&id) {
                            tab.waiting = true;
                        }
                    }
                }
            }
        }
        // The host's name once the data has it, unless a program named the pane.
        let named =
            app.terminals.get(&id).filter(|t| !t.titled).map(|t| visible(&t.host.and_then(|h| host_name(app, h)).unwrap_or_else(|| t.name.clone())));
        if let Some(title) = named
            && let Some(tab) = app.terminals.get_mut(&id)
        {
            tab.title = title;
        }
    }
    for (id, bytes) in starts {
        send(app, id, &bytes);
    }
    for id in ask_recording {
        app.runner.run(
            "session.recording",
            json!({ "id": id }),
            Box::new(move |app, result| {
                if let (Ok(Value::Bool(on)), Some(tab)) = (result, app.terminals.get_mut(&id)) {
                    tab.recording = Some(on);
                }
            }),
        );
    }
    // More output than one frame takes: the next frame continues. New output wakes the window
    // through the engine's terminal waker.
    if more {
        ctx.request_repaint();
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

fn status_line(app: &mut SshcraftApp, ui: &mut Ui, id: SessionId) {
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
        open(app, tab.target.clone(), tab.name.clone(), tab.host, Some(PaneSlot::Session { session: id }));
    }
}

/// Whether the cursor's line of `tab` asks for a secret or a yes/no.
fn asks(tab: &SessionTab) -> bool {
    let screen = tab.term.screen();
    let line = screen.text().lines().nth(usize::from(screen.cursor.row)).map(str::to_string).unwrap_or_default();
    crate::status::waiting_line(&line)
}

/// Where typing in session `id` goes: with broadcast on in its tab, to every pane of the tab
/// whose session is connected and that asks a question exactly when `id` does (a command never
/// goes into another pane's password question, a password never to another pane's prompt);
/// else to `id` alone.
pub fn broadcast_targets(app: &SshcraftApp, id: SessionId) -> Vec<SessionId> {
    let Some((tab, _)) = pane_of(app, id) else { return vec![id] };
    let Some(panes) = app.ui.tabs.get(tab).map(|t| &t.panes) else { return vec![id] };
    if !panes.broadcast {
        return vec![id];
    }
    let here = app.terminals.get(&id).is_some_and(asks);
    panes
        .sessions()
        .filter(|s| app.terminals.get(s).is_some_and(|t| matches!(t.status, Some(SessionStatus::Connected { .. })) && asks(t) == here))
        .collect()
}

/// A question, another window or an open menu is over the terminal: it gets no keys.
pub fn modal_open(app: &SshcraftApp, ctx: &egui::Context) -> bool {
    !app.prompts.open.is_empty()
        || app.ui.picker.is_some()
        || app.ui.palette.open
        || app.terminals.values().any(|t| t.clipboard.is_some())
        || craft_menu::bar::is_open(ctx)
}

/// Whether `id` is another terminal than the one shown last frame.
fn app_shown_changed(shown: &mut Option<SessionId>, id: SessionId) -> bool {
    let changed = *shown != Some(id);
    *shown = Some(id);
    changed
}

/// The page of a session tab: the terminal and its status line.
/// Sets a bookmark named with the time in the recording of the focused pane (Ctrl+Shift+L).
pub fn bookmark(app: &mut SshcraftApp) {
    let Page::Tab(i) = app.ui.page else { return };
    let Some(PaneSlot::Session { session }) = app.ui.tabs.get(i).and_then(|t| t.panes.focused_slot()) else { return };
    let name = chrono::Local::now().format("%H:%M:%S").to_string();
    app.runner.run(
        "session.bookmark",
        json!({ "id": session, "text": name }),
        Box::new(|app, result| match result {
            Ok(_) => app.toasts.info(tr("Bookmark added.")),
            Err(e) => app.toasts.error(&e.to_string()),
        }),
    );
}

/// The text of the cursor's line: the prompt a new line is typed after.
/// The cursor's line up to the cursor (what a shell's grey suggestion after it is not part of)
/// and the line above it.
fn before_cursor(term: &Terminal) -> (String, String) {
    let screen = term.screen();
    let cols = usize::from(screen.cols);
    let row = usize::from(screen.cursor.row);
    let cells = |r: usize, upto: usize| -> String {
        let start = r.saturating_mul(cols);
        screen.cells.get(start..start.saturating_add(upto.min(cols))).map(|c| c.iter().map(|c| c.text.as_str()).collect()).unwrap_or_default()
    };
    let above = row.checked_sub(1).map(|r| cells(r, cols)).unwrap_or_default();
    (cells(row, usize::from(screen.cursor.col)), above)
}

/// The keys the suggestion list takes: Down selects, Up moves back, Tab or Enter take the
/// selected one, Escape closes. Without a selection Enter, Tab and Escape stay the shell's.
fn completion_keys(ctx: &egui::Context, tab: &mut SessionTab) -> Option<Vec<u8>> {
    if tab.suggestions().is_empty() {
        return None;
    }
    let count = tab.suggestions.len();
    let mut taken = None;
    ctx.input_mut(|input| {
        input.events.retain(|event| {
            let egui::Event::Key { key, pressed: true, modifiers, .. } = event else { return true };
            if !modifiers.is_none() {
                return true;
            }
            match (key, tab.selected) {
                (egui::Key::ArrowDown, sel) => {
                    tab.selected = Some(sel.map_or(0, |s| (s + 1).min(count.saturating_sub(1))));
                    false
                }
                (egui::Key::ArrowUp, Some(s)) => {
                    tab.selected = s.checked_sub(1);
                    false
                }
                (egui::Key::Tab | egui::Key::Enter, Some(s)) => {
                    taken = tab.suggestions.get(s).map(|sug| sug.insert.clone());
                    tab.selected = None;
                    tab.dismissed = true;
                    false
                }
                (egui::Key::Escape, sel) => {
                    tab.dismissed = true;
                    tab.selected = None;
                    // With nothing selected, Escape goes on to the shell.
                    sel.is_none()
                }
                _ => true,
            }
        });
    });
    taken
}

/// Follows the line typed into `id`; a finished line that counts goes into the history.
fn track_line(app: &mut SshcraftApp, id: SessionId, bytes: &[u8], ctx: crate::history::LineContext) {
    let keep = app.data.settings.terminal.history;
    let Some(tab) = app.terminals.get_mut(&id) else { return };
    let Some(line) = tab.capture.input(bytes, &ctx) else { return };
    let Some(scope) = tab.scope().filter(|_| keep) else { return };
    tab.history.retain(|h| *h != line);
    tab.history.insert(0, line.clone());
    crate::pages::snippets::sidebar_recorded(app, &scope, &line);
    app.run_reported("history.record", json!({ "scope": scope, "command": line }));
}

/// Takes a cleared history out of every pane of `scope` (every pane for `None`) and out of the
/// sidebar, so a cleared line is never suggested again.
pub fn forget_history(app: &mut SshcraftApp, scope: Option<&str>) {
    for tab in app.terminals.values_mut() {
        if scope.is_none() || tab.scope().as_deref() == scope {
            tab.history.clear();
            tab.suggestions.clear();
            tab.selected = None;
            tab.suggested_for = None;
        }
    }
    crate::pages::snippets::sidebar_forget(app, scope);
}

/// Makes the suggestions for the line typed so far: history, snippets, then paths.
fn update_suggestions(app: &mut SshcraftApp, id: SessionId) {
    let enabled = app.data.settings.terminal.autocomplete;
    let snippets = app.data.snippets.clone();
    let Some(tab) = app.terminals.get_mut(&id) else { return };
    if !tab.history_loaded
        && let Some(scope) = tab.scope()
    {
        tab.history_loaded = true;
        app.runner.run(
            "history.list",
            json!({ "scope": scope }),
            Box::new(move |app, result| {
                if let (Ok(Value::Array(rows)), Some(tab)) = (result, app.terminals.get_mut(&id)) {
                    tab.history = rows.iter().filter_map(|r| r.get("command").and_then(Value::as_str)).map(str::to_string).collect();
                }
            }),
        );
        return;
    }
    let line = if enabled { tab.capture.line().map(str::to_string) } else { None };
    if line == tab.suggested_for && tab.path_pending.is_none() {
        return;
    }
    if line != tab.suggested_for {
        tab.dismissed = false;
        tab.selected = None;
    }
    tab.suggested_for.clone_from(&line);
    let Some(line) = line else {
        tab.suggestions.clear();
        return;
    };
    let now = std::time::Instant::now();
    let mut paths: Vec<String> = Vec::new();
    if let Some(dir) = crate::completion::path_word(&line).and_then(|(dir, _)| crate::completion::resolve_dir(&dir, tab.cwd.as_deref())) {
        match tab.paths.get(&dir, now) {
            Some(names) => paths = names.to_vec(),
            None if tab.path_pending.as_deref() != Some(dir.as_str()) => {
                tab.path_pending = Some(dir.clone());
                app.runner.run(
                    "session.list_dir",
                    json!({ "id": id, "dir": dir, "prefix": "" }),
                    Box::new(move |app, result| {
                        if let Some(tab) = app.terminals.get_mut(&id) {
                            let names = match result {
                                Ok(Value::Array(names)) => names.iter().filter_map(|n| n.as_str()).map(str::to_string).collect(),
                                _ => Vec::new(),
                            };
                            tab.paths.put(&dir, names, std::time::Instant::now());
                            tab.path_pending = None;
                            tab.suggested_for = None;
                        }
                    }),
                );
            }
            None => {}
        }
    }
    let Some(tab) = app.terminals.get_mut(&id) else { return };
    let bracketed = tab.term.modes().contains(sshcraft_term::TermModes::BRACKETED_PASTE);
    tab.suggestions = crate::completion::suggest(&line, &tab.history, &snippets, &paths, bracketed);
}

/// The list under the cursor.
fn draw_suggestions(app: &SshcraftApp, ui: &Ui, id: SessionId, rect: Rect, font: &egui::FontId) {
    let Some(tab) = app.terminals.get(&id) else { return };
    let list = tab.suggestions();
    if list.is_empty() {
        return;
    }
    let tokens = Tokens::get(ui.ctx());
    let metrics = crate::terminal::CellMetrics::measure(ui.ctx(), font);
    let cursor = tab.term.screen().cursor;
    let at = rect.min + egui::vec2(f32::from(cursor.col) * metrics.width, (f32::from(cursor.row) + 1.0) * metrics.height);
    egui::Area::new(egui::Id::unique(("sshcraft-suggestions", id))).order(egui::Order::Foreground).fixed_pos(at).show(ui.ctx(), |ui| {
        egui::Frame::new().fill(tokens.panel).stroke(egui::Stroke::new(1.0, tokens.separator)).inner_margin(4.0).show(ui, |ui| {
            for (i, s) in list.iter().enumerate() {
                let color = if tab.selected == Some(i) { tokens.text_strong } else { tokens.text };
                let text = egui::RichText::new(visible(&s.label)).monospace().color(color);
                let text = if tab.selected == Some(i) { text.background_color(tokens.row_selected) } else { text };
                ui.label(text);
            }
        });
    });
}

/// The terminal of session `id` and its status line in the rest of `ui`; `focused` is the pane
/// the keyboard and the terminal commands go to.
pub fn show(app: &mut SshcraftApp, ui: &mut Ui, id: SessionId, focused: bool) {
    let full = ui.available_rect_before_wrap();
    let composing = app.terminals.get(&id).is_some_and(|t| t.compose.is_some());
    let compose_height = if composing { crate::terminal::compose::COMPOSE_HEIGHT } else { 0.0 };
    let term_rect = Rect::from_min_max(full.min, pos2(full.max.x, (full.max.y - STATUS_HEIGHT - compose_height).max(full.min.y)));
    let compose_rect = Rect::from_min_max(pos2(full.min.x, term_rect.max.y), pos2(full.max.x, (term_rect.max.y + compose_height).min(full.max.y)));
    let status_rect = Rect::from_min_max(pos2(full.min.x, compose_rect.max.y), full.max);
    // Ctrl+Enter sends the compose field, before the terminal or the field itself sees the key.
    let compose_send = focused && composing && ui.ctx().input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter));
    if focused {
        // Every paste goes into the history (in memory only).
        let pasted: Vec<String> = ctx_pastes(ui.ctx());
        for text in pasted {
            app.pastes.push(&text);
        }
    }
    let ctx = ui.ctx().clone();
    let pending = if focused { std::mem::take(&mut app.ui.pending) } else { Vec::new() };
    let appearance = &app.data.settings.appearance;
    // Loading a font is slow: only when the setting changes.
    if app.term_font.as_ref().is_none_or(|(name, _)| *name != appearance.font_family) {
        app.term_font = Some((appearance.font_family.clone(), install_terminal_font(&ctx, &appearance.font_family)));
    }
    let family = app.term_font.as_ref().map_or(egui::FontFamily::Monospace, |(_, f)| f.clone());
    let font = egui::FontId::new(f32::from(appearance.font_size), family);
    let ligatures = appearance.ligatures;
    let app_highlighter = app.patterns.highlighter.clone();
    let look = host_appearance(app, app.terminals.get(&id).and_then(|t| t.host));
    let theme = look.theme.as_deref().unwrap_or(appearance.terminal_theme.as_str());
    let palette = if theme == "sshcraft-light" { TermPalette::light() } else { TermPalette::dark() };
    let blocked = modal_open(app, &ctx);
    if focused
        && !blocked
        && let Some((text, paste)) = hint_keys(app, &ctx, id)
    {
        if paste {
            let modes = app.terminals.get(&id).map(|t| t.term.modes()).unwrap_or_default();
            send(app, id, &sshcraft_term::input::encode_paste(&text, modes));
        } else {
            ctx.copy_text(text);
        }
    }
    let Some(tab) = app.terminals.get_mut(&id) else {
        ui.label(tr("This session is gone."));
        return;
    };
    // Commands get the time they were first shown running or finished.
    tab.term.stamp_commands(chrono::Utc::now());
    for command in pending {
        match command {
            "ui.terminal.copy" => {
                if let Some(text) = tab.term.selection_text() {
                    ctx.copy_text(text);
                }
            }
            "ui.terminal.paste" => ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste),
            "ui.terminal.prev_command" => {
                tab.term.jump_to_prompt(sshcraft_term::Direction::Up);
            }
            "ui.terminal.next_command" => {
                tab.term.jump_to_prompt(sshcraft_term::Direction::Down);
            }
            "ui.terminal.compose" => {
                tab.compose = if tab.compose.is_some() { None } else { Some(String::new()) };
                if tab.compose.is_some() {
                    ctx.memory_mut(|m| m.request_focus(egui::Id::unique(("sshcraft-compose", id))));
                }
            }
            "ui.terminal.paste_history" => tab.paste_picker = !tab.paste_picker,
            "ui.terminal.hints" => {
                let found = crate::terminal::hints::find_hints(&tab.term.screen());
                let labels = crate::terminal::hints::labels(found.len());
                tab.hints = (!found.is_empty()).then(|| labels.into_iter().zip(found).collect());
                tab.hint_typed.clear();
            }
            "ui.terminal.copy_output" => {
                if let Some(text) = tab.term.last_finished_output() {
                    ctx.copy_text(text);
                }
            }
            _ => {}
        }
    }
    // The suggestion list takes its keys before the terminal sees them, and only while the
    // terminal has the keyboard (not a search field or a dialog).
    let has_keys = tab.view.is_some_and(|v| ctx.memory(|m| m.has_focus(v)));
    let taken = if focused && !blocked && has_keys { completion_keys(&ctx, tab) } else { None };
    let line_context = crate::history::LineContext {
        prompt: before_cursor(&tab.term).0,
        above: before_cursor(&tab.term).1,
        alt_screen: tab.term.modes().contains(sshcraft_term::TermModes::ALT_SCREEN),
        broadcast: false,
    };
    let response = ui
        .scope_builder(UiBuilder::new().max_rect(term_rect).id_salt(id), |ui| {
            TerminalView { term: &mut tab.term, palette: &palette, font: font.clone(), ligatures, highlighter: Some(&app_highlighter) }.show(ui)
        })
        .inner;
    if let Some(badge) = &look.badge {
        paint_badge(ui, term_rect, badge, look.color);
    }
    if let Some(hints) = &tab.hints {
        let metrics = crate::terminal::CellMetrics::measure(&ctx, &font);
        crate::terminal::hints::paint_labels(&ui.painter_at(term_rect), term_rect, &metrics, hints, &tab.hint_typed);
    }
    // Only the focused pane takes the keyboard.
    let first = focused && std::mem::take(&mut tab.focus);
    let switched = focused && app_shown_changed(&mut app.ui.shown_terminal, id);
    let Some(tab) = app.terminals.get_mut(&id) else { return };
    tab.view = Some(response.response.id);
    if blocked || (!focused && response.response.has_focus()) {
        // A question is open: what is typed belongs to it, never to the shell behind it.
        response.response.surrender_focus();
    } else if focused && (first || switched || ctx.memory(|m| m.focused().is_none())) {
        response.response.request_focus();
    }
    let can_send = !ended(tab.status.as_ref()) && !blocked;
    // The terminal's own reports go to its own session only.
    if !response.reports.is_empty() && can_send {
        send(app, id, &response.reports);
    }
    let mut typed = taken.unwrap_or_default();
    typed.extend_from_slice(&response.input);
    if !typed.is_empty() && can_send {
        let targets = broadcast_targets(app, id);
        let broadcast = targets.len() > 1 || targets.first() != Some(&id);
        for target in targets {
            send(app, target, &typed);
        }
        track_line(app, id, &typed, crate::history::LineContext { broadcast, ..line_context });
    }
    if let Some(bytes) = compose_and_pastes(app, ui, id, compose_rect, term_rect, focused && can_send, compose_send)
        && can_send
    {
        for target in broadcast_targets(app, id) {
            send(app, target, &bytes);
        }
    }
    if focused {
        update_suggestions(app, id);
        draw_suggestions(app, ui, id, response.response.rect, &font);
    }
    // "Rec" in the lower right corner while the output is recorded.
    if app.terminals.get(&id).is_some_and(|t| t.recording == Some(true)) {
        let tokens = Tokens::get(ui.ctx());
        let rect = response.response.rect;
        let at = egui::Rect::from_min_size(rect.right_bottom() - egui::vec2(44.0, 20.0), egui::vec2(40.0, 16.0));
        let mark = ui.put(at, egui::Label::new(egui::RichText::new(tr("Rec")).small().color(tokens.danger)));
        mark.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, tr("Rec")));
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
    ui.scope_builder(UiBuilder::new().max_rect(status_rect), |ui| status_line(app, ui, id));
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
