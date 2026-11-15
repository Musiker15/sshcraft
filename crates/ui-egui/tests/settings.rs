//! The settings pages: the language, the shortcut editor with its conflicts, and the hosts that
//! may always write the clipboard. Everything goes through `settings.get` and `settings.save`.

use std::time::{Duration, Instant};

use egui::{Event, Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use sshcraft_engine::Caller;
use sshcraft_engine::settings::LanguageSetting;
use sshcraft_ui_egui::SshcraftApp;
use sshcraft_ui_egui::commands::ui_commands;
use sshcraft_ui_egui::state::Page;
use sshcraft_ui_egui::testing::app_harness;

type H = Harness<'static, SshcraftApp>;

fn until(h: &mut H, what: &str, done: impl Fn(&SshcraftApp) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done(h.state()) {
        assert!(Instant::now() < deadline, "timed out waiting for {what}; messages: {:?}", h.state().toasts.texts());
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    h.run_steps(2);
}

fn settings_page(h: &mut H, section: &str) {
    h.state_mut().ui.page = Page::Settings;
    h.run_steps(3);
    h.get_by_label(section).click();
    h.run_steps(3);
}

/// `key` with `modifiers`, pressed and released in frames of their own.
fn press(h: &mut H, modifiers: Modifiers, key: Key) {
    for pressed in [true, false] {
        h.input_mut().events.push(Event::Key { key, pressed, modifiers, repeat: false, physical_key: None });
        h.step();
    }
}

fn row(id: &str) -> usize {
    ui_commands().iter().position(|c| c.id == id).unwrap()
}

fn stored(app: &SshcraftApp, action: &str) -> Option<Vec<String>> {
    app.data.settings.shortcuts.bindings.get(action).cloned()
}

#[test]
fn changing_the_language_changes_the_labels() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    settings_page(&mut h, "General");
    assert!(h.query_by_label("Language").is_some());
    h.get_by_label("Deutsch").click();
    until(&mut h, "the saved language", |app| app.data.settings.general.language == LanguageSetting::De);
    assert!(h.query_by_label("Sprache").is_some(), "the page is German now");
    assert!(h.query_by_label("Language").is_none());
    h.get_by_label("English").click();
    until(&mut h, "English again", |app| app.data.settings.general.language == LanguageSetting::En);
    assert!(h.query_by_label("Language").is_some());
}

#[test]
fn a_shortcut_is_changed_removed_and_reset() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    settings_page(&mut h, "Shortcuts");
    let new_tab = row("ui.tab.new");
    let next_tab = row("ui.tab.next");
    let ctrl_alt = Modifiers::CTRL | Modifiers::ALT;

    h.get_all_by_label("Change").nth(new_tab).unwrap().click();
    h.run_steps(1);
    press(&mut h, ctrl_alt, Key::N);
    until(&mut h, "the new shortcut", |app| stored(app, "newTab") == Some(vec!["Ctrl+Alt+N".into()]));

    // Taken: the change is refused and says by whom.
    h.get_all_by_label("Change").nth(next_tab).unwrap().click();
    h.run_steps(1);
    press(&mut h, ctrl_alt, Key::N);
    h.run_steps(3);
    assert!(h.query_by_label_contains("Ctrl+Alt+N is taken by New tab").is_some(), "the conflict is named");
    assert_eq!(stored(h.state(), "nextTab"), None, "nothing changed");

    h.get_all_by_label("Remove").nth(new_tab).unwrap().click();
    until(&mut h, "the removed shortcut", |app| stored(app, "newTab") == Some(Vec::new()));
    h.get_all_by_label("Default").nth(new_tab).unwrap().click();
    until(&mut h, "the default again", |app| stored(app, "newTab").is_none());
    assert!(h.query_all_by_label_contains("outside the terminal only").next().is_some(), "Ctrl+K says where it works");
}

#[test]
fn a_clipboard_host_can_be_removed() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, session) = app_harness(dir.path());
    let host = uuid::Uuid::new_v4();
    let run = |id: &str, params| session.runtime().block_on(session.execute(Caller::User, id, params)).unwrap();
    run("host.save", json!({ "host": { "id": host, "name": "web", "group_id": null, "settings": { "address": "192.0.2.5" } } }));
    let mut settings = run("settings.get", json!({}));
    settings["terminal"]["clipboard_hosts"] = json!([host]);
    run("settings.save", json!({ "settings": settings }));
    until(&mut h, "the data", |app| !app.data.hosts.is_empty() && !app.data.settings.terminal.clipboard_hosts.is_empty());
    settings_page(&mut h, "Terminal");
    assert!(h.query_by_label("web").is_some(), "the host is listed by name");
    h.get_by_label("Remove").click();
    until(&mut h, "the removed host", |app| app.data.settings.terminal.clipboard_hosts.is_empty());
    // The page shows the change at once; the engine has it once its save ran.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let saved = run("settings.get", json!({}));
        if saved["terminal"].get("clipboard_hosts").is_none_or(|v| v.as_array().is_some_and(Vec::is_empty)) {
            break;
        }
        assert!(Instant::now() < deadline, "the engine kept the host: {saved}");
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
}

#[test]
fn leaving_a_shortcut_recording_keeps_the_shortcuts_working() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, _session) = app_harness(dir.path());
    settings_page(&mut h, "Shortcuts");
    h.get_all_by_label("Change").nth(row("ui.tab.new")).unwrap().click();
    h.run_steps(1);
    h.state_mut().ui.page = Page::Hosts;
    h.run_steps(2);
    press(&mut h, Modifiers::CTRL | Modifiers::SHIFT, Key::T);
    h.run_steps(2);
    assert_eq!(h.state().ui.tabs.len(), 1, "Ctrl+Shift+T still opens a tab");
}
