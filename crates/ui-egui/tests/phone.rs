//! A phone held upright (390 points wide): dialogs fit the screen, the page's name shows once,
//! and on a touch screen the menu bar gives way to a button that opens the command palette.

use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use sshcraft_ui_egui::SshcraftApp;
use sshcraft_ui_egui::app::Form;
use sshcraft_ui_egui::state::Page;
use sshcraft_ui_egui::testing::app_harness;

type H = Harness<'static, SshcraftApp>;

fn phone(dir: &std::path::Path) -> H {
    let (mut h, _session) = app_harness(dir);
    h.state_mut().form = Form::Tablet;
    h.set_size(vec2(390.0, 844.0));
    h.run_steps(4);
    h
}

#[test]
fn the_host_form_fits_a_phone() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = phone(dir.path());
    sshcraft_ui_egui::pages::hosts::new_host(h.state_mut());
    h.run_steps(4);
    let fields: Vec<_> = h.query_all_by_role(egui::accesskit::Role::TextInput).map(|n| n.rect()).collect();
    assert!(!fields.is_empty(), "the form is open");
    // Fields, their labels and the title: nothing of the form may leave the screen.
    for rect in fields {
        assert!(rect.left() >= -0.5 && rect.right() <= 390.5, "a field at {rect:?} leaves the 390 pt screen");
    }
    for label in h.query_all_by_role(egui::accesskit::Role::Label) {
        let rect = label.rect();
        let text = label.accesskit_node().label().unwrap_or_default();
        assert!(rect.left() >= -0.5 && rect.right() <= 390.5, "{text:?} at {rect:?} leaves the 390 pt screen");
    }
}

#[test]
fn the_page_name_shows_once_on_a_phone() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = phone(dir.path());
    h.state_mut().ui.page = Page::Settings;
    h.run_steps(4);
    assert_eq!(h.query_all_by_label("Settings").count(), 1, "the bar names the page; the page does not repeat it");
}

#[test]
fn a_touch_screen_has_commands_instead_of_the_menu_bar() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = phone(dir.path());
    assert!(h.query_by_label("File").is_none(), "no menu bar on a touch screen");
    h.get_by_label("Commands").click();
    h.run_steps(3);
    assert!(h.state().ui.palette.open, "Commands opens the command palette");
}

#[test]
fn the_desktop_keeps_its_menu_bar() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    h.run_steps(3);
    assert!(h.query_by_label("File").is_some());
}

/// A long message stays inside a phone's screen: it wraps instead of running off the edges.
#[test]
fn a_long_message_wraps_on_a_phone() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = phone(dir.path());
    let text = "The connection was lost. (could not connect: Disconnected) and then some more words";
    h.state_mut().toasts.error(text);
    h.run_steps(3);
    let rect = h.get_by_label(text).rect();
    assert!(rect.left() >= 0.0 && rect.right() <= 390.5, "the message runs off the screen: {rect:?}");
}
