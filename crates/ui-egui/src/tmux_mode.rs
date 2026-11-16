//! tmux control mode in the window: the engine holds one session running `tmux -CC`, and this
//! module reads its output with [`sshcraft_model::tmux::Parser`]. Every tmux window becomes a tab,
//! every pane a terminal in it; typing goes back as `send-keys` with hex keys, sizes as
//! `refresh-client` and `resize-pane`. The control session itself is never shown.
//!
//! Closing a tmux tab only stops showing it. Once no tab of a tmux session is left, SSHCraft
//! closes the control session and tmux detaches; the session keeps running on the server.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use sshcraft_engine::SessionId;
use sshcraft_model::tmux::{Event, Parser, layout_panes, resize, send_keys};

use crate::app::SshcraftApp;
use crate::state::{PaneSlot, Panes, TabState};
use crate::widgets::visible;

/// Windows one control session may show.
const MAX_WINDOWS: usize = 64;
/// Panes one control session may show.
const MAX_PANES: usize = 128;
/// Lines of history fetched for a pane that is new to the window.
const HISTORY_LINES: u32 = 200;

/// What a command's answer is for, in the order the commands went out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ask {
    Windows,
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
    asks: VecDeque<Ask>,
    started: bool,
    /// The last size told to tmux.
    client_size: Option<(u16, u16)>,
}

/// Sends a tmux command over the control session and remembers what its answer is for.
fn command(app: &mut SshcraftApp, control: SessionId, line: &str, ask: Ask) {
    if let Some(c) = app.terminals.get_mut(&control).and_then(|t| t.tmux.as_mut()) {
        c.asks.push_back(ask);
    }
    crate::session_tab::send_bytes(app, control, line.as_bytes());
}

/// The control session connected: ask for the windows that are there.
pub fn start(app: &mut SshcraftApp, control: SessionId) {
    let Some(c) = app.terminals.get_mut(&control).and_then(|t| t.tmux.as_mut()) else { return };
    if std::mem::replace(&mut c.started, true) {
        return;
    }
    command(app, control, "list-windows -F '#{window_id} #{window_layout} #{window_name}'\n", Ask::Windows);
}

/// Output of the control session.
pub fn on_bytes(app: &mut SshcraftApp, control: SessionId, bytes: &[u8]) {
    let Some(events) = app.terminals.get_mut(&control).and_then(|t| t.tmux.as_mut()).map(|c| c.parser.feed(bytes)) else { return };
    for event in events {
        apply(app, control, event);
    }
}

fn control_mut(app: &mut SshcraftApp, control: SessionId) -> Option<&mut Control> {
    app.terminals.get_mut(&control).and_then(|t| t.tmux.as_deref_mut())
}

fn apply(app: &mut SshcraftApp, control: SessionId, event: Event) {
    match event {
        Event::Output { pane, data } => {
            if let Some(id) = pane_terminal(app, control, pane)
                && let Some(tab) = app.terminals.get_mut(&id)
            {
                tab.term.feed(&data);
                tab.mark_output();
            }
        }
        Event::Reply { ok, lines } => {
            let ask = control_mut(app, control).and_then(|c| c.asks.pop_front()).unwrap_or(Ask::Other);
            match ask {
                Ask::Windows if ok => {
                    for line in lines {
                        let mut parts = line.splitn(3, ' ');
                        let (Some(window), Some(layout)) = (parts.next(), parts.next()) else { continue };
                        let Some(window) = window.strip_prefix('@').and_then(|w| w.parse::<u32>().ok()) else { continue };
                        let name = parts.next().unwrap_or_default().to_string();
                        window_changed(app, control, window, Some(name), Some(layout_panes(layout)));
                    }
                }
                Ask::Capture(pane) if ok => {
                    if let Some(id) = control_mut(app, control).and_then(|c| c.panes.get(&pane).copied())
                        && let Some(tab) = app.terminals.get_mut(&id)
                    {
                        let text = lines.join("\r\n");
                        tab.term.feed(text.as_bytes());
                    }
                }
                _ => {}
            }
        }
        Event::WindowAdd(window) => {
            command(app, control, "list-windows -F '#{window_id} #{window_layout} #{window_name}'\n", Ask::Windows);
            window_changed(app, control, window, None, None);
        }
        Event::WindowClose(window) => window_closed(app, control, window),
        Event::WindowRenamed { window, name } => window_changed(app, control, window, Some(name), None),
        Event::LayoutChange { window, panes } => window_changed(app, control, window, None, Some(panes)),
        Event::PaneFocus { .. } | Event::SessionChanged { .. } => {}
        Event::Exit(_) => {
            let windows: Vec<u32> = control_mut(app, control).map(|c| c.windows.keys().copied().collect()).unwrap_or_default();
            for window in windows {
                window_closed(app, control, window);
            }
        }
    }
}

/// The terminal of `pane`, made when the pane is new; `None` for a pane closed here or past
/// the limit.
fn pane_terminal(app: &mut SshcraftApp, control: SessionId, pane: u32) -> Option<SessionId> {
    let (known, closed, full) = {
        let c = control_mut(app, control)?;
        (c.panes.get(&pane).copied(), c.closed.contains(&pane), c.panes.len() >= MAX_PANES)
    };
    if let Some(id) = known {
        return Some(id);
    }
    if closed || full {
        return None;
    }
    let id = crate::session_tab::open_tmux_pane(app, control, pane)?;
    control_mut(app, control)?.panes.insert(pane, id);
    command(app, control, &format!("capture-pane -p -e -J -t %{pane} -S -{HISTORY_LINES}\n"), Ask::Capture(pane));
    Some(id)
}

/// A window appeared or changed its name or panes: its tab follows.
fn window_changed(app: &mut SshcraftApp, control: SessionId, window: u32, name: Option<String>, panes: Option<Vec<u32>>) {
    {
        let Some(c) = control_mut(app, control) else { return };
        if !c.windows.contains_key(&window) && c.windows.len() >= MAX_WINDOWS {
            return;
        }
        let entry = c.windows.entry(window).or_insert_with(|| (format!("tmux {window}"), Vec::new()));
        if let Some(name) = name.filter(|n| !n.is_empty()) {
            entry.0 = name;
        }
        if let Some(panes) = panes {
            entry.1 = panes;
        }
    }
    let Some((name, panes)) = control_mut(app, control).and_then(|c| c.windows.get(&window).cloned()) else { return };
    let ids: Vec<SessionId> = panes.iter().filter_map(|&p| pane_terminal(app, control, p)).collect();
    if ids.is_empty() {
        return;
    }
    let title = visible(&name);
    // The tab that shows one of these panes, or the control session's own tab for the first
    // window, or a new tab.
    let existing = app.ui.tabs.iter().position(|t| t.panes.sessions().any(|s| ids.contains(&s)));
    let own = app.ui.tabs.iter().position(|t| t.panes.sessions().any(|s| s == control));
    let index = match existing.or(own) {
        Some(i) => i,
        None => {
            app.ui.tabs.push(TabState { title: title.clone(), panes: Panes::single(PaneSlot::Session { session: ids[0] }), named: true });
            app.ui.tabs.len() - 1
        }
    };
    if let Some(tab) = app.ui.tabs.get_mut(index) {
        tab.title = title;
        tab.named = true;
        let mut grid = Panes::single(PaneSlot::Session { session: ids[0] });
        for id in ids.iter().skip(1) {
            grid.add(PaneSlot::Session { session: *id });
        }
        grid.focused = 0;
        tab.panes = grid;
    }
}

fn window_closed(app: &mut SshcraftApp, control: SessionId, window: u32) {
    let Some((_, panes)) = control_mut(app, control).and_then(|c| c.windows.remove(&window)) else { return };
    let ids: Vec<SessionId> = panes.iter().filter_map(|p| control_mut(app, control).and_then(|c| c.panes.remove(p))).collect();
    if let Some(i) = app.ui.tabs.iter().position(|t| t.panes.sessions().any(|s| ids.contains(&s))) {
        app.ui.tabs.remove(i);
        if let crate::state::Page::Tab(shown) = app.ui.page
            && shown >= app.ui.tabs.len()
        {
            app.ui.page = if app.ui.tabs.is_empty() { crate::state::Page::Hosts } else { crate::state::Page::Tab(app.ui.tabs.len() - 1) };
        }
    }
    for id in ids {
        app.terminals.remove(&id);
    }
}

/// Typing in a tmux pane: hex keys through the control session.
pub fn type_into(app: &SshcraftApp, control: SessionId, pane: u32, bytes: &[u8]) {
    for line in send_keys(pane, bytes) {
        crate::session_tab::send_bytes(app, control, line.as_bytes());
    }
}

/// A tmux pane's terminal took `cols` × `rows`: tmux is told the window's size first (so the
/// pane can grow) and then the pane's.
pub fn pane_resized(app: &mut SshcraftApp, control: SessionId, pane: u32, cols: u16, rows: u16) {
    let tab_size = app
        .ui
        .tabs
        .iter()
        .find(|t| t.panes.sessions().any(|s| control_mut_ref(app, control).is_some_and(|c| c.panes.get(&pane) == Some(&s))))
        .map(|t| crate::panes::grid(t.panes.panes.len()));
    let (grid_cols, grid_rows) = tab_size.unwrap_or((1, 1));
    let as_u16 = |n: usize| u16::try_from(n).unwrap_or(u16::MAX);
    let client = (
        cols.saturating_mul(as_u16(grid_cols)).saturating_add(as_u16(grid_cols.saturating_sub(1))),
        rows.saturating_mul(as_u16(grid_rows)).saturating_add(as_u16(grid_rows.saturating_sub(1))),
    );
    let changed = control_mut(app, control).is_some_and(|c| c.client_size.replace(client) != Some(client));
    if changed {
        crate::session_tab::send_bytes(app, control, resize(client.0, client.1).as_bytes());
    }
    crate::session_tab::send_bytes(app, control, format!("resize-pane -t %{pane} -x {} -y {}\n", cols.max(1), rows.max(1)).as_bytes());
}

fn control_mut_ref(app: &SshcraftApp, control: SessionId) -> Option<&Control> {
    app.terminals.get(&control).and_then(|t| t.tmux.as_deref())
}

/// The user closed the terminal of tmux pane `pane`: it is not shown again, and once no tab of
/// this tmux session is left, the control session closes and tmux detaches.
pub fn pane_closed(app: &mut SshcraftApp, control: SessionId, pane: u32) {
    let id = control_mut(app, control).and_then(|c| {
        c.closed.insert(pane);
        c.panes.remove(&pane)
    });
    if let Some(id) = id {
        app.terminals.remove(&id);
    }
    let any_left = control_mut_ref(app, control).is_some_and(|c| !c.panes.is_empty());
    if !any_left {
        app.terminals.remove(&control);
        app.run_reported("session.close", serde_json::json!({ "id": control }));
    }
}
