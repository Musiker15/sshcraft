//! The command palette: found by the local and the English label, keys, disabled entries last.

use egui::{Key, Modifiers};
use sshcraft_ui_egui::palette::{Entry, rank};
use sshcraft_ui_egui::state::Page;
use sshcraft_ui_egui::testing::app_harness;

fn entry(id: &'static str, local: &str, english: &'static str, enabled: bool) -> Entry {
    Entry { id, arg: None, local: local.to_string(), english, shortcut: None, enabled }
}

#[test]
fn the_palette_finds_by_german_and_english_label() {
    let entries = vec![entry("ui.page.keys", "Schlüssel", "Keys", true), entry("ui.page.hosts", "Hosts", "Hosts", true)];
    assert_eq!(rank("schlü", &entries).first().map(|e| e.id), Some("ui.page.keys"));
    assert_eq!(rank("keys", &entries).first().map(|e| e.id), Some("ui.page.keys"));
    assert_eq!(rank("page.hosts", &entries).first().map(|e| e.id), Some("ui.page.hosts"));
    assert!(rank("zzz", &entries).is_empty());
}

#[test]
fn disabled_entries_come_last() {
    let entries = vec![entry("ui.tab.close", "Close tab", "Close tab", false), entry("ui.tab.new", "New tab", "New tab", true)];
    let ranked = rank("tab", &entries);
    assert_eq!(ranked.iter().map(|e| e.id).collect::<Vec<_>>(), ["ui.tab.new", "ui.tab.close"]);
}

#[test]
fn arrows_enter_escape() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    h.run_steps(3);
    h.key_press_modifiers(Modifiers::CTRL | Modifiers::SHIFT, Key::P);
    h.run_steps(2);
    assert!(h.state().ui.palette.open);
    h.event(egui::Event::Text("settings".into()));
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(2);
    assert!(!h.state().ui.palette.open);
    assert_eq!(h.state().ui.page, Page::Settings);
    h.key_press(Key::K);
    h.key_press_modifiers(Modifiers::CTRL, Key::K);
    h.run_steps(2);
    assert!(h.state().ui.palette.open);
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(!h.state().ui.palette.open);
}
