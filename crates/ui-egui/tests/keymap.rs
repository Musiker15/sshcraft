//! Shortcuts: the defaults of docs/shortcuts.md, the terminal keeping its keys, own bindings with
//! conflicts refused.

use std::collections::BTreeMap;

use craft_keymap::Chord;
use sshcraft_engine::settings::Shortcuts;
use sshcraft_ui_egui::keymap::{BindError, Bindings};

fn chord(text: &str) -> Chord {
    Chord::parse(text).unwrap()
}

#[test]
fn defaults_match_the_shortcut_table() {
    let table = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/shortcuts.md")).unwrap();
    let b = Bindings::default();
    for (id, keys) in [
        ("ui.palette", &["Ctrl+Shift+P", "Ctrl+K"][..]),
        ("ui.tab.new", &["Ctrl+Shift+T"]),
        ("ui.tab.close", &["Ctrl+Shift+W"]),
        ("ui.tab.next", &["Ctrl+Tab", "Ctrl+PageDown"]),
        ("ui.tab.previous", &["Ctrl+Shift+Tab", "Ctrl+PageUp"]),
    ] {
        for key in keys {
            assert_eq!(b.command_for(&chord(key), false), Some(id), "{key}");
            assert!(table.contains(key), "{key} is in docs/shortcuts.md");
        }
    }
    assert_eq!(b.command_for(&chord("Ctrl+Shift+C"), true), Some("ui.terminal.copy"));
    assert_eq!(b.command_for(&chord("Ctrl+Shift+V"), true), Some("ui.terminal.paste"));
}

#[test]
fn inside_a_terminal_only_ctrl_shift_and_ctrl_tab_are_taken() {
    let b = Bindings::default();
    assert_eq!(b.command_for(&chord("Ctrl+K"), true), None, "Ctrl+K belongs to the shell");
    assert_eq!(b.command_for(&chord("Ctrl+PageDown"), true), None);
    assert_eq!(b.command_for(&chord("Ctrl+Shift+P"), true), Some("ui.palette"));
    assert_eq!(b.command_for(&chord("Ctrl+Tab"), true), Some("ui.tab.next"));
    assert_eq!(b.command_for(&chord("Ctrl+K"), false), Some("ui.palette"));
}

#[test]
fn a_taken_combination_is_refused_with_its_owner() {
    let mut b = Bindings::default();
    let Err(BindError::Taken { owner }) = b.assign("ui.tab.new", "Ctrl+Shift+P") else { panic!("not refused") };
    assert_eq!(owner, "Open the command palette");
    assert_eq!(b.command_for(&chord("Ctrl+Shift+T"), false), Some("ui.tab.new"), "the old binding stays");
}

#[test]
fn a_shortcut_needs_a_modifier_or_a_function_key() {
    let mut b = Bindings::default();
    assert_eq!(b.assign("ui.tab.new", "K"), Err(BindError::NeedsModifier));
    assert_eq!(b.assign("ui.tab.new", "Shift+K"), Err(BindError::NeedsModifier));
    assert!(matches!(b.assign("ui.tab.new", "Ctrl+Shift+"), Err(BindError::Invalid(_))));
    assert_eq!(b.assign("ui.tab.new", "F7"), Ok(()));
    assert_eq!(b.command_for(&chord("F7"), false), Some("ui.tab.new"));
    assert_eq!(b.command_for(&chord("Ctrl+Shift+T"), false), None, "change replaces");
}

#[test]
fn own_bindings_round_trip_through_the_settings() {
    // Stored under the engine's action names (the Tauri version's), not the command ids.
    let mut stored = BTreeMap::new();
    stored.insert("newTab".to_string(), vec!["Ctrl+Alt+N".to_string()]);
    stored.insert("unknownAction".to_string(), vec!["Ctrl+Alt+U".to_string()]);
    let b = Bindings::from_settings(&Shortcuts { bindings: stored });
    assert_eq!(b.command_for(&chord("Ctrl+Alt+N"), false), Some("ui.tab.new"));
    assert_eq!(b.command_for(&chord("Ctrl+Shift+T"), false), None);
    let saved = b.to_settings();
    assert_eq!(saved.bindings.get("newTab"), Some(&vec!["Ctrl+Alt+N".to_string()]));
    assert!(!saved.bindings.contains_key("unknownAction"), "unknown actions are dropped");
    assert!(!saved.bindings.contains_key("palette"), "defaults are not stored");
    let mut b = b;
    b.reset("ui.tab.new");
    assert_eq!(b.command_for(&chord("Ctrl+Shift+T"), false), Some("ui.tab.new"));
    b.remove("ui.tab.new");
    assert_eq!(b.command_for(&chord("Ctrl+Shift+T"), false), None);
    assert_eq!(b.to_settings().bindings.get("newTab"), Some(&Vec::new()), "a removed binding is stored as none");
}

#[test]
fn every_command_has_an_action_the_engine_keeps() {
    for command in sshcraft_ui_egui::commands::ui_commands() {
        assert!(sshcraft_engine::settings::SHORTCUT_ACTIONS.contains(&command.action), "{} ({})", command.id, command.action);
    }
    // What the window stores survives the engine's own check of the settings.
    let mut b = Bindings::default();
    b.assign("ui.page.keys", "Ctrl+Alt+K").unwrap();
    let settings = sshcraft_engine::settings::EngineSettings { shortcuts: b.to_settings(), ..Default::default() };
    let kept = settings.sanitized().shortcuts;
    assert_eq!(Bindings::from_settings(&kept).command_for(&chord("Ctrl+Alt+K"), false), Some("ui.page.keys"));
}
