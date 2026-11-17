//! The window picks one of three layouts by its width: a sidebar with names, a rail of symbols,
//! or (a phone held upright) a bar at the top whose menu button folds the sidebar out over the page.

use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use sshcraft_ui_egui::SshcraftApp;
use sshcraft_ui_egui::shell::{Layout, layout_for};
use sshcraft_ui_egui::state::Page;
use sshcraft_ui_egui::testing::app_harness;

type H = Harness<'static, SshcraftApp>;

fn sized(dir: &std::path::Path, width: f32) -> (H, sshcraft_engine::Session) {
    let (mut h, session) = app_harness(dir);
    h.set_size(vec2(width, 800.0));
    h.run_steps(3);
    (h, session)
}

#[test]
fn a_narrow_window_folds_the_sidebar_behind_a_menu_button() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = sized(dir.path(), 400.0);
    assert!(h.query_by_label("Port forwarding").is_none(), "the sidebar is folded away");
    h.get_by_label("Menu").click();
    h.run_steps(3);
    assert!(h.state().ui.nav_open, "the menu opens");
    h.get_by_label("Port forwarding").click();
    h.run_steps(3);
    assert_eq!(h.state().ui.page, Page::Forwarding);
    assert!(!h.state().ui.nav_open, "choosing a page closes the menu");
}

#[test]
fn a_tap_beside_the_open_menu_closes_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = sized(dir.path(), 400.0);
    h.get_by_label("Menu").click();
    h.run_steps(3);
    assert!(h.state().ui.nav_open);
    h.get_by_label("Close the menu").click();
    h.run_steps(3);
    assert!(!h.state().ui.nav_open, "a tap beside it closes the menu");
    assert_eq!(h.state().ui.page, Page::Hosts, "and changes no page");
}

#[test]
fn a_wide_window_keeps_the_sidebar() {
    let dir = tempfile::tempdir().unwrap();
    let (h, _session) = sized(dir.path(), 1200.0);
    assert!(h.query_by_label("Port forwarding").is_some(), "the sidebar shows");
    assert!(h.query_by_label("Menu").is_none(), "no menu button");
}

#[test]
fn the_width_picks_the_layout() {
    assert_eq!(layout_for(699.9), Layout::Narrow);
    assert_eq!(layout_for(700.0), Layout::Medium);
    assert_eq!(layout_for(999.9), Layout::Medium);
    assert_eq!(layout_for(1000.0), Layout::Wide);
}

#[test]
fn a_medium_window_shows_a_rail() {
    let dir = tempfile::tempdir().unwrap();
    let (h, _session) = sized(dir.path(), 820.0);
    let button = h.get_by_label("Port forwarding").rect();
    assert!(button.width() <= 76.0, "a symbol, not a row with the name: {button:?}");
    assert!(h.query_by_label("SSHCraft").is_none(), "no names beside the logo");
    assert!(h.query_by_label("Menu").is_none(), "no menu button");
}

#[test]
fn a_wide_window_shows_the_logo_and_names() {
    let dir = tempfile::tempdir().unwrap();
    let (h, _session) = sized(dir.path(), 1200.0);
    assert!(h.query_by_label("SSHCraft").is_some());
}

#[test]
fn widening_closes_the_menu() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = sized(dir.path(), 400.0);
    h.get_by_label("Menu").click();
    h.run_steps(3);
    assert!(h.state().ui.nav_open);
    h.set_size(vec2(820.0, 800.0));
    h.run_steps(3);
    assert!(!h.state().ui.nav_open, "the menu closes when the rail shows");
    assert!(h.query_by_label("Close the menu").is_none(), "nothing is left over the page");
}

#[test]
fn every_header_fits_a_phone() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = sized(dir.path(), 390.0);
    for page in [Page::Keys, Page::Snippets, Page::Forwarding, Page::Logs, Page::Settings] {
        h.state_mut().ui.page = page;
        h.run_steps(3);
        for button in h.query_all_by_role(egui::accesskit::Role::Button) {
            let rect = button.rect();
            let name = button.accesskit_node().label().unwrap_or_default();
            assert!(rect.max.x <= 390.5, "{page:?}: {name:?} ends at {}, outside a 390 pt window", rect.max.x);
        }
    }
}
