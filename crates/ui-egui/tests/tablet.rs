//! The tablet form (Android): what the desktop offers and a tablet cannot do is left out.

use egui::{Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use sshcraft_ui_egui::SshcraftApp;
use sshcraft_ui_egui::app::Form;
use sshcraft_ui_egui::pages::settings::Section;
use sshcraft_ui_egui::state::Page;
use sshcraft_ui_egui::testing::app_harness;

fn harness(dir: &std::path::Path, form: Form) -> Harness<'static, SshcraftApp> {
    let (mut h, _session) = app_harness(dir);
    h.state_mut().form = form;
    h.run_steps(3);
    h
}

fn settings(h: &mut Harness<'static, SshcraftApp>, section: Section) {
    h.state_mut().ui.page = Page::Settings;
    h.state_mut().ui.settings.section = section;
    h.run_steps(3);
}

#[test]
fn a_tablet_offers_no_local_terminal() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness(dir.path(), Form::Tablet);
    h.key_press_modifiers(Modifiers::CTRL | Modifiers::SHIFT, Key::T);
    h.run_steps(2);
    assert!(h.query_by_label("Quick connect").is_some(), "the new-tab page is open");
    assert!(h.query_by_label("Local terminal").is_none());
    assert!(h.query_by_label("Serial port").is_none());
}

#[test]
fn a_tablet_offers_no_files_and_no_agents() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness(dir.path(), Form::Tablet);
    settings(&mut h, Section::General);
    assert!(h.query_by_label("Create backup…").is_none());
    assert!(h.query_by_label("AI agents").is_none(), "no agents on a tablet");
    settings(&mut h, Section::Appearance);
    assert!(h.query_by_label("Dropdown window").is_none());
    h.state_mut().ui.page = Page::Hosts;
    h.run_steps(3);
    assert!(h.query_by_label("New group").is_some(), "the hosts page is open");
    assert!(h.query_by_label("Import").is_none());
    assert!(h.query_by_label("Export").is_none());
}

/// The counter-check: the desktop keeps all of it.
#[test]
fn the_desktop_still_offers_them() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness(dir.path(), Form::Desktop);
    settings(&mut h, Section::General);
    assert!(h.query_by_label("Create backup…").is_some());
    assert!(h.query_by_label("AI agents").is_some());
    settings(&mut h, Section::Appearance);
    assert!(h.query_by_label("Dropdown window").is_some());
    h.state_mut().ui.page = Page::Hosts;
    h.run_steps(3);
    assert!(h.query_by_label("Import").is_some());
    h.key_press_modifiers(Modifiers::CTRL | Modifiers::SHIFT, Key::T);
    h.run_steps(2);
    assert!(h.query_by_label("Local terminal").is_some());
}
