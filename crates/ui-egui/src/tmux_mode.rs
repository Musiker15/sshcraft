//! tmux control mode in the window: the engine holds one session running `tmux -CC`, and this
//! module reads its output with [`sshcraft_model::tmux::Parser`]. Every tmux window becomes a tab,
//! every pane a terminal in it; typing goes back as `send-keys` with hex keys, sizes as
//! `refresh-client` and `resize-pane`. The control session's own tab shows only what came before
//! control mode started (`tmux: not found`), and gives way to the first window.
//!
//! Closing a tmux tab only stops showing it. Once no tab of a tmux session is left, SSHCraft
//! closes the control session and tmux detaches; the session keeps running on the server.
//!
//! Every command tmux answers with a `%begin`/`%end` block, in the order the commands went out;
//! each command sent here notes what its answer is for, so the answers line up.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::{Mutex, PoisonError};

use sshcraft_engine::SessionId;
use sshcraft_model::tmux::{Event, Parser, clean_name, layout_panes, resize, send_keys};

use crate::app::SshcraftApp;
use crate::state::{Page, PaneSlot, Panes, TabState};
use crate::widgets::visible;

/// Windows one control session may show.
const MAX_WINDOWS: usize = 64;
/// Panes one control session may show.
const MAX_PANES: usize = 128;
/// Lines of history fetched for a pane that is new to the window.
const HISTORY_LINES: u32 = 200;
/// Answers waited for at once; past this tmux is not answering, and the oldest is dropped.
const MAX_ASKS: usize = 256;
/// Output kept for a pane while its history is on its way.
const MAX_HELD: usize = 1 << 20;

const LIST_WINDOWS: &str = "list-windows -F '#{window_id} #{window_layout} #{window_name}'\n";

/// What a command's answer is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ask {
    /// The windows; `full` when the list is all of them (after connecting), so windows missing
    /// from it are gone.
    Windows {
        full: bool,
    },
    Capture(u32),
    Other,
}

/// The state of one control session.
#[derive(Debug, Default)]
pub struct Control {
    parser: Parser,
    /// tmux pane → the terminal showing it.
    pub panes: HashMap<u32, SessionId>,
    /// tmux window → its name and panes.
    pub windows: BTreeMap<u32, (String, Vec<u32>)>,
    /// Panes the user closed here: their output is not shown again.
    closed: HashSet<u32>,
    /// What the answers on their way are for, oldest first. Behind a lock so typing (which only
    /// reads the app) can note its command too.
    asks: Mutex<VecDeque<Ask>>,
    /// Control mode answered its own start: commands may go out.
    started: bool,
    /// A `list-windows` is on its way.
    listing: bool,
    /// The last size told to tmux.
    client_size: Option<(u16, u16)>,
    /// Output of panes whose history is still on its way, shown after it.
    held: HashMap<u32, Vec<u8>>,
}

impl Control {
    fn note(&self, ask: Ask) {
        let mut asks = self.asks.lock().unwrap_or_else(PoisonError::into_inner);
        if asks.len() >= MAX_ASKS {
            asks.pop_front();
        }
        asks.push_back(ask);
    }

    fn answered(&self) -> Option<Ask> {
        self.asks.lock().unwrap_or_else(PoisonError::into_inner).pop_front()
    }
}

fn control_mut(app: &mut SshcraftApp, control: SessionId) -> Option<&mut Control> {
    app.terminals.get_mut(&control).and_then(|t| t.tmux.as_deref_mut())
}

fn control_ref(app: &SshcraftApp, control: SessionId) -> Option<&Control> {
    app.terminals.get(&control).and_then(|t| t.tmux.as_deref())
}

/// Sends a tmux command over the control session and notes what its answer is for.
fn command(app: &SshcraftApp, control: SessionId, line: &str, ask: Ask) {
    let Some(c) = control_ref(app, control) else { return };
    c.note(ask);
    crate::session_tab::send_bytes(app, control, line.as_bytes());
}

/// Asks for the windows, unless a list is already on its way.
fn list_windows(app: &mut SshcraftApp, control: SessionId, full: bool) {
    let Some(c) = control_mut(app, control) else { return };
    if std::mem::replace(&mut c.listing, true) {
        return;
    }
    command(app, control, LIST_WINDOWS, Ask::Windows { full });
}

/// The control session (re)connected: everything half read is forgotten, and the windows are
/// listed again once tmux answered its start.
pub fn start(app: &mut SshcraftApp, control: SessionId) {
    let Some(c) = control_mut(app, control) else { return };
    c.parser.reset();
    c.asks.lock().unwrap_or_else(PoisonError::into_inner).clear();
    c.started = false;
    c.listing = false;
    c.client_size = None;
    c.held.clear();
}

/// Output of the control session.
pub fn on_bytes(app: &mut SshcraftApp, control: SessionId, bytes: &[u8]) {
    let Some(events) = control_mut(app, control).map(|c| c.parser.feed(bytes)) else { return };
    for event in events {
        apply(app, control, event);
    }
}

/// The first answer (to tmux's own start) or the session's name: control mode is up.
fn started(app: &mut SshcraftApp, control: SessionId) {
    let Some(c) = control_mut(app, control) else { return };
    if !std::mem::replace(&mut c.started, true) {
        list_windows(app, control, true);
    }
}

fn apply(app: &mut SshcraftApp, control: SessionId, event: Event) {
    match event {
        Event::Plain(text) => {
            if let Some(tab) = app.terminals.get_mut(&control) {
                tab.term.feed(format!("{text}\r\n").as_bytes());
            }
        }
        Event::Output { pane, data } => output(app, control, pane, &data),
        Event::Reply { ok, lines } => {
            let ask = control_ref(app, control).and_then(Control::answered).unwrap_or(Ask::Other);
            answered(app, control, ask, ok, lines);
            started(app, control);
        }
        Event::SessionChanged { .. } => started(app, control),
        Event::WindowAdd(window) => {
            list_windows(app, control, false);
            window_changed(app, control, window, None, None);
        }
        Event::WindowClose(window) => window_closed(app, control, window),
        Event::WindowRenamed { window, name } => window_changed(app, control, window, Some(name), None),
        Event::LayoutChange { window, panes } => window_changed(app, control, window, None, Some(panes)),
        Event::PaneFocus { .. } => {}
        Event::Exit(_) => close_control(app, control),
    }
}

fn answered(app: &mut SshcraftApp, control: SessionId, ask: Ask, ok: bool, lines: Vec<String>) {
    match ask {
        Ask::Windows { full } => {
            if let Some(c) = control_mut(app, control) {
                c.listing = false;
            }
            if !ok {
                return;
            }
            let mut listed = Vec::new();
            for line in lines {
                let mut parts = line.splitn(3, ' ');
                let (Some(window), Some(layout)) = (parts.next(), parts.next()) else { continue };
                let Some(window) = window.strip_prefix('@').and_then(|w| w.parse::<u32>().ok()) else { continue };
                listed.push(window);
                let name = clean_name(parts.next().unwrap_or_default());
                window_changed(app, control, window, Some(name), Some(layout_panes(layout)));
            }
            // After connecting, the list is all of them: what closed meanwhile goes.
            if full {
                let gone: Vec<u32> =
                    control_ref(app, control).map(|c| c.windows.keys().filter(|w| !listed.contains(w)).copied().collect()).unwrap_or_default();
                for window in gone {
                    window_closed(app, control, window);
                }
            }
        }
        Ask::Capture(pane) => {
            let held = control_mut(app, control).and_then(|c| c.held.remove(&pane)).unwrap_or_default();
            let Some(id) = control_ref(app, control).and_then(|c| c.panes.get(&pane).copied()) else { return };
            let Some(tab) = app.terminals.get_mut(&id) else { return };
            if ok {
                tab.term.feed(lines.join("\r\n").as_bytes());
            }
            // What came while the history was on its way follows it.
            tab.term.feed(&held);
        }
        Ask::Other => {}
    }
}

fn output(app: &mut SshcraftApp, control: SessionId, pane: u32, data: &[u8]) {
    let Some(id) = pane_terminal(app, control, pane) else { return };
    if let Some(c) = control_mut(app, control)
        && let Some(held) = c.held.get_mut(&pane)
    {
        if held.len().saturating_add(data.len()) <= MAX_HELD {
            held.extend_from_slice(data);
        }
        return;
    }
    if let Some(tab) = app.terminals.get_mut(&id) {
        tab.term.feed(data);
        tab.mark_output();
    }
}

/// The terminal of `pane`, made when the pane is new; `None` for a pane closed here or past
/// the limit.
fn pane_terminal(app: &mut SshcraftApp, control: SessionId, pane: u32) -> Option<SessionId> {
    let (known, closed, full) = {
        let c = control_ref(app, control)?;
        (c.panes.get(&pane).copied(), c.closed.contains(&pane), c.panes.len() >= MAX_PANES)
    };
    if let Some(id) = known {
        return Some(id);
    }
    if closed || full {
        return None;
    }
    let id = crate::session_tab::open_tmux_pane(app, control, pane)?;
    let c = control_mut(app, control)?;
    c.panes.insert(pane, id);
    // Output until the history arrives waits behind it.
    c.held.insert(pane, Vec::new());
    command(app, control, &format!("capture-pane -p -e -J -t %{pane} -S -{HISTORY_LINES}\n"), Ask::Capture(pane));
    Some(id)
}

/// A window appeared or changed its name or panes: its tab follows. Panes that left the layout
/// go with their terminals.
fn window_changed(app: &mut SshcraftApp, control: SessionId, window: u32, name: Option<String>, panes: Option<Vec<u32>>) {
    let left: Vec<SessionId> = {
        let Some(c) = control_mut(app, control) else { return };
        if !c.windows.contains_key(&window) && c.windows.len() >= MAX_WINDOWS {
            return;
        }
        let entry = c.windows.entry(window).or_insert_with(|| (format!("tmux {window}"), Vec::new()));
        if let Some(name) = name.filter(|n| !n.is_empty()) {
            entry.0 = name;
        }
        let mut gone = Vec::new();
        if let Some(panes) = panes {
            gone = entry.1.iter().filter(|p| !panes.contains(p)).copied().collect();
            entry.1 = panes;
        }
        gone.iter()
            .filter_map(|p| {
                c.held.remove(p);
                c.panes.remove(p)
            })
            .collect()
    };
    for id in &left {
        app.terminals.remove(id);
    }
    let Some((name, panes)) = control_ref(app, control).and_then(|c| c.windows.get(&window).cloned()) else { return };
    let ids: Vec<SessionId> = panes.iter().filter_map(|&p| pane_terminal(app, control, p)).collect();
    let title = visible(&name);
    let existing = app.ui.tabs.iter().position(|t| t.panes.sessions().any(|s| ids.contains(&s) || left.contains(&s)));
    let Some(&first) = ids.first() else {
        // No pane left to show: the window's tab goes.
        if let Some(i) = existing {
            remove_tab(app, i);
        }
        return;
    };
    // The tab that shows this window, or the control session's own tab for the first window, or
    // a new tab.
    let own = app.ui.tabs.iter().position(|t| t.panes.sessions().any(|s| s == control));
    let index = match existing.or(own) {
        Some(i) => i,
        None => {
            app.ui.tabs.push(TabState { title: title.clone(), panes: Panes::single(PaneSlot::Session { session: first }), named: true });
            app.ui.tabs.len() - 1
        }
    };
    if let Some(tab) = app.ui.tabs.get_mut(index) {
        // The focus stays on the pane it was on.
        let focused = tab.panes.focused_slot();
        let mut grid = Panes::single(PaneSlot::Session { session: first });
        for id in ids.iter().skip(1) {
            grid.add(PaneSlot::Session { session: *id });
        }
        grid.focused = focused.and_then(|slot| grid.position(slot)).unwrap_or(0);
        tab.title = title;
        tab.named = true;
        tab.panes = grid;
    }
}

/// Removes tab `i`; the shown page stays on the tab it showed, or moves to the one before.
fn remove_tab(app: &mut SshcraftApp, i: usize) {
    if i >= app.ui.tabs.len() {
        return;
    }
    app.ui.tabs.remove(i);
    app.ui.page = match app.ui.page {
        Page::Tab(shown) if shown == i => {
            if app.ui.tabs.is_empty() {
                Page::Hosts
            } else {
                Page::Tab(i.saturating_sub(1).min(app.ui.tabs.len() - 1))
            }
        }
        Page::Tab(shown) if shown > i => Page::Tab(shown - 1),
        other => other,
    };
}

fn window_closed(app: &mut SshcraftApp, control: SessionId, window: u32) {
    let Some((_, panes)) = control_mut(app, control).and_then(|c| c.windows.remove(&window)) else { return };
    let ids: Vec<SessionId> = panes.iter().filter_map(|p| control_mut(app, control).and_then(|c| c.panes.remove(p))).collect();
    if let Some(i) = app.ui.tabs.iter().position(|t| t.panes.sessions().any(|s| ids.contains(&s))) {
        remove_tab(app, i);
    }
    for id in ids {
        app.terminals.remove(&id);
    }
}

/// Ends the control session here: its tabs and terminals go, and the engine closes it (tmux
/// detaches, or it already ended).
fn close_control(app: &mut SshcraftApp, control: SessionId) {
    let panes: Vec<SessionId> = app.terminals.values().filter(|t| t.tmux_pane.is_some_and(|(c, _)| c == control)).map(|t| t.id).collect();
    while let Some(i) = app.ui.tabs.iter().position(|t| t.panes.sessions().any(|s| s == control || panes.contains(&s))) {
        remove_tab(app, i);
    }
    for id in panes {
        app.terminals.remove(&id);
    }
    app.terminals.remove(&control);
    app.run_reported("session.close", serde_json::json!({ "id": control }));
}

/// Typing in a tmux pane: hex keys through the control session.
pub fn type_into(app: &SshcraftApp, control: SessionId, pane: u32, bytes: &[u8]) {
    for line in send_keys(pane, bytes) {
        command(app, control, &line, Ask::Other);
    }
}

/// A tmux pane's terminal took `cols` × `rows`: tmux is told the window's size first (so the
/// pane can grow) and then the pane's.
pub fn pane_resized(app: &mut SshcraftApp, control: SessionId, pane: u32, cols: u16, rows: u16) {
    let pane_id = control_ref(app, control).and_then(|c| c.panes.get(&pane).copied());
    let (grid_cols, grid_rows) = app
        .ui
        .tabs
        .iter()
        .find(|t| pane_id.is_some_and(|id| t.panes.sessions().any(|s| s == id)))
        .map_or((1, 1), |t| crate::panes::grid(t.panes.panes.len()));
    let as_u16 = |n: usize| u16::try_from(n).unwrap_or(u16::MAX);
    let client = (
        cols.saturating_mul(as_u16(grid_cols)).saturating_add(as_u16(grid_cols.saturating_sub(1))),
        rows.saturating_mul(as_u16(grid_rows)).saturating_add(as_u16(grid_rows.saturating_sub(1))),
    );
    let changed = control_mut(app, control).is_some_and(|c| c.client_size.replace(client) != Some(client));
    if changed {
        command(app, control, &resize(client.0, client.1), Ask::Other);
    }
    command(app, control, &format!("resize-pane -t %{pane} -x {} -y {}\n", cols.max(1), rows.max(1)), Ask::Other);
}

/// The user closed the terminal of tmux pane `pane`: it is not shown again, and once no tab
/// shows a pane of this tmux session, the control session closes and tmux detaches.
pub fn pane_closed(app: &mut SshcraftApp, control: SessionId, pane: u32) {
    let id = control_mut(app, control).and_then(|c| {
        c.closed.insert(pane);
        c.panes.remove(&pane)
    });
    if let Some(id) = id {
        app.terminals.remove(&id);
    }
    let shown: Vec<SessionId> = control_ref(app, control).map(|c| c.panes.values().copied().collect()).unwrap_or_default();
    let any_shown = app.ui.tabs.iter().any(|t| t.panes.sessions().any(|s| shown.contains(&s)));
    if !any_shown {
        close_control(app, control);
    }
}

/// Whether tab `tab` shows tmux panes: its grid follows tmux, so it takes no other panes.
pub fn is_tmux_tab(app: &SshcraftApp, tab: usize) -> bool {
    app.ui.tabs.get(tab).is_some_and(|t| t.panes.sessions().any(|s| app.terminals.get(&s).is_some_and(|x| x.tmux_pane.is_some())))
}
