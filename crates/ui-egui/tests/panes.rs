//! Panes: up to 16 sessions in one tab, in a near-square grid or one large; dividers resize,
//! a title bar dragged onto another pane swaps them, panes move between tabs without
//! reconnecting.

use std::time::{Duration, Instant};

use egui::{Event, Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use sshcraft_engine::testing::test_host;
use sshcraft_engine::{Session, SessionId, SessionStatus};
use sshcraft_model::model::HostId;
use sshcraft_ssh::testing::{TestAuth, TestServer};
use sshcraft_ui_egui::SshcraftApp;
use sshcraft_ui_egui::panes::{MAX_PANES, grid};
use sshcraft_ui_egui::state::{ConnectRequest, Page, PaneSlot};
use sshcraft_ui_egui::testing::app_harness;

type H = Harness<'static, SshcraftApp>;

fn until(h: &mut H, what: &str, done: impl Fn(&SshcraftApp) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !done(h.state()) {
        assert!(Instant::now() < deadline, "timed out waiting for {what}; messages: {:?}", h.state().toasts.texts());
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    h.run_steps(2);
}

fn host(session: &Session, name: &str) -> (TestServer, HostId) {
    session.runtime().block_on(test_host(session, name, TestAuth::default(), true))
}

fn connected(app: &SshcraftApp, id: SessionId) -> bool {
    app.terminals.get(&id).is_some_and(|t| matches!(t.status, Some(SessionStatus::Connected { .. })))
}

/// Connects the focused pane (a new-pane page) to `host`.
fn connect_here(h: &mut H, host: HostId) -> SessionId {
    let before: Vec<SessionId> = h.state().terminals.keys().copied().collect();
    h.state_mut().ui.connect_request = Some(ConnectRequest::Host(host));
    until(h, "the session", |app| app.terminals.keys().any(|id| !before.contains(id)));
    let id = *h.state().terminals.keys().find(|id| !before.contains(id)).unwrap();
    until(h, "the connection", |app| connected(app, id));
    id
}

fn slots(app: &SshcraftApp, tab: usize) -> Vec<PaneSlot> {
    app.ui.tabs[tab].panes.panes.clone()
}

fn drag(h: &mut H, from: Pos2, to: Pos2) {
    h.event(Event::PointerMoved(from));
    h.run_steps(1);
    h.event(Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(1);
    for i in 1..=6 {
        h.event(Event::PointerMoved(from + (to - from) * (i as f32 / 6.0)));
        h.run_steps(1);
    }
    h.event(Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
}

/// A tab with two connected panes.
fn two_panes(h: &mut H, session: &Session) -> (SessionId, SessionId, Vec<TestServer>) {
    let (a_server, a) = host(session, "alpha");
    let (b_server, b) = host(session, "beta");
    h.state_mut().open_new_tab();
    h.run_steps(2);
    let first = connect_here(h, a);
    h.state_mut().split();
    h.run_steps(2);
    let second = connect_here(h, b);
    (first, second, vec![a_server, b_server])
}

#[test]
fn the_grid_is_near_square() {
    let expected = [
        (1, (1, 1)),
        (2, (2, 1)),
        (3, (2, 2)),
        (4, (2, 2)),
        (5, (3, 2)),
        (6, (3, 2)),
        (7, (3, 3)),
        (9, (3, 3)),
        (10, (4, 3)),
        (12, (4, 3)),
        (13, (4, 4)),
        (16, (4, 4)),
    ];
    for (count, cols_rows) in expected {
        assert_eq!(grid(count), cols_rows, "{count} panes");
    }
}

#[test]
fn split_opens_a_new_pane_page_and_stops_at_16() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    h.state_mut().open_new_tab();
    h.run_steps(2);
    for _ in 0..20 {
        h.state_mut().split();
    }
    h.run_steps(2);
    let panes = slots(h.state(), 0);
    assert_eq!(panes.len(), MAX_PANES);
    assert!(panes.iter().all(|p| matches!(p, PaneSlot::New { .. })), "each a page to pick a connection");
    assert_eq!(h.state().ui.tabs[0].panes.focused, MAX_PANES - 1, "the newest pane has the focus");
    assert!(h.state().toasts.texts().iter().any(|t| t.contains("16")), "it says why: {:?}", h.state().toasts.texts());
}

#[test]
fn closing_the_last_pane_closes_the_tab() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (first, second, _servers) = two_panes(&mut h, &session);
    h.state_mut().close_pane();
    h.run_steps(2);
    assert_eq!(slots(h.state(), 0), [PaneSlot::Session { session: first }]);
    h.state_mut().close_pane();
    until(&mut h, "the closed tab", |app| app.ui.tabs.is_empty());
    let deadline = Instant::now() + Duration::from_secs(10);
    while session.unacknowledged(first).is_some() || session.unacknowledged(second).is_some() {
        assert!(Instant::now() < deadline, "a session is still open in the engine");
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
}

#[test]
fn a_tab_with_one_pane_looks_like_a_plain_tab() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    h.state_mut().open_new_tab();
    h.run_steps(3);
    assert!(h.query_by_label("Close pane").is_none(), "no title bar with one pane");
    h.state_mut().split();
    h.run_steps(3);
    assert_eq!(h.query_all_by_label("Close pane").count(), 2, "a title bar on each pane");
}

#[test]
fn focus_layout_shows_one_pane_large_and_lists_the_others() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (first, _second, _servers) = two_panes(&mut h, &session);
    sshcraft_ui_egui::commands::run(h.state_mut(), "ui.pane.layout");
    h.run_steps(3);
    assert!(h.state().ui.tabs[0].panes.focus_layout);
    let terminals: Vec<_> = h.query_all_by_role(egui::accesskit::Role::Terminal).map(|n| n.rect()).collect();
    assert_eq!(terminals.len(), 1, "one pane is drawn large");
    assert!(terminals[0].width() > 700.0, "the whole width but the list: {:?}", terminals[0]);
    h.get_by_label("alpha").click();
    h.run_steps(3);
    assert_eq!(h.state().ui.tabs[0].panes.focused_slot(), Some(PaneSlot::Session { session: first }), "a click on the list brings it to the front");
}

#[test]
fn dragging_a_divider_resizes() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    h.state_mut().open_new_tab();
    h.state_mut().split();
    h.run_steps(3);
    let divider = h.get_by_label("Resize columns").rect().center();
    drag(&mut h, divider, divider + egui::vec2(200.0, 0.0));
    let sizes = h.state().ui.tabs[0].panes.col_sizes.clone();
    assert!(sizes[0] > 0.6, "{sizes:?}");
    let divider = h.get_by_label("Resize columns").rect().center();
    drag(&mut h, divider, Pos2::new(1190.0, 300.0));
    let sizes = h.state().ui.tabs[0].panes.col_sizes.clone();
    assert!(sizes[1] >= 0.1 - 1e-4, "a column keeps a tenth: {sizes:?}");
}

#[test]
fn dragging_a_title_bar_onto_another_pane_swaps_them() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (first, second, _servers) = two_panes(&mut h, &session);
    let bars: Vec<Pos2> = h.query_all_by_label("Pane title").map(|n| n.rect().center()).collect();
    assert_eq!(bars.len(), 2);
    drag(&mut h, bars[0], bars[1] + egui::vec2(0.0, 150.0));
    assert_eq!(slots(h.state(), 0), [PaneSlot::Session { session: second }, PaneSlot::Session { session: first }]);
}

#[test]
fn move_to_own_tab_and_merging_keep_the_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let (first, second, _servers) = two_panes(&mut h, &session);
    h.get_all_by_label("Move to its own tab").nth(1).unwrap().click();
    h.run_steps(3);
    assert_eq!(h.state().ui.tabs.len(), 2);
    assert_eq!(slots(h.state(), 0), [PaneSlot::Session { session: first }]);
    assert_eq!(slots(h.state(), 1), [PaneSlot::Session { session: second }]);
    // Drag the second tab onto the first: one tab with both panes again.
    let tabs: Vec<Pos2> = h.query_all_by_label("Tab").map(|n| n.rect().center()).collect();
    drag(&mut h, tabs[1], tabs[0]);
    assert_eq!(h.state().ui.tabs.len(), 1);
    assert_eq!(slots(h.state(), 0), [PaneSlot::Session { session: first }, PaneSlot::Session { session: second }]);
    assert!(connected(h.state(), first) && connected(h.state(), second), "nothing reconnected");
    assert_eq!(h.state().terminals.len(), 2);
    assert_eq!(h.state().ui.page, Page::Tab(0));
}
