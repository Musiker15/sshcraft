use std::path::Path;

use craft_settings::{Location, SettingsStore};
use serde_json::json;
use sshcraft_store::store::Store;

use super::*;
use crate::{Caller, EngineError, Session, SessionConfig};

fn store_with(json: &str) -> Store {
    let store = Store::open_in_memory().unwrap();
    store.put_setting(TAURI_APP, json).unwrap();
    store
}

fn load(store: &Store) -> EngineSettings {
    read_tauri_settings(store)
}

fn with(f: impl FnOnce(&mut EngineSettings)) -> EngineSettings {
    let mut s = EngineSettings::default();
    f(&mut s);
    s.sanitized()
}

#[test]
fn nothing_stored_gives_defaults() {
    assert_eq!(load(&Store::open_in_memory().unwrap()), EngineSettings::default());
}

#[test]
fn missing_fields_take_defaults() {
    let settings = load(&store_with(r#"{"language":"de"}"#));
    assert_eq!(settings.general.language, LanguageSetting::De);
    assert_eq!(settings.appearance.font_size, 14);
    assert_eq!(settings.appearance.terminal_theme, "sshcraft-dark");
}

#[test]
fn unknown_value_only_resets_that_field() {
    let settings = load(&store_with(r#"{"language":"fr","font_size":16,"app_theme":"light"}"#));
    assert_eq!(settings.general.language, LanguageSetting::Auto);
    assert_eq!(settings.appearance.font_size, 16);
    assert_eq!(settings.appearance.app_theme, AppTheme::Light);
}

#[test]
fn not_json_gives_defaults() {
    assert_eq!(load(&store_with("not json")), EngineSettings::default());
    assert_eq!(load(&store_with("[1,2]")), EngineSettings::default());
}

#[test]
fn font_size_is_clamped() {
    assert_eq!(with(|s| s.appearance.font_size = 0).appearance.font_size, 8);
    assert_eq!(with(|s| s.appearance.font_size = 500).appearance.font_size, 32);
    assert_eq!(load(&store_with(r#"{"font_size":500}"#)).appearance.font_size, 32);
}

#[test]
fn blank_or_huge_font_family_gives_default() {
    let default = Appearance::default().font_family;
    assert_eq!(with(|s| s.appearance.font_family = "   ".into()).appearance.font_family, default);
    assert_eq!(with(|s| s.appearance.font_family = "x".repeat(201)).appearance.font_family, default);
    assert_eq!(with(|s| s.appearance.font_family = " Consolas ".into()).appearance.font_family, "Consolas");
}

#[tokio::test]
async fn saved_settings_come_back() {
    let dir = tempfile::tempdir().unwrap();
    let wanted = with(|s| {
        s.general.language = LanguageSetting::En;
        s.general.system_title_bar = true;
        s.appearance = Appearance {
            app_theme: AppTheme::Dark,
            terminal_theme: "nord".into(),
            font_family: "JetBrains Mono".into(),
            font_size: 15,
            host_view: HostView::List,
            ligatures: false,
        };
        s.terminal.terminal_type = "vt100".into();
        s.terminal.clipboard_osc52 = Osc52::Off;
        s.terminal.history = false;
        s.terminal.autocomplete = false;
        s.shortcuts.bindings = BTreeMap::from([("newTab".to_owned(), vec!["Ctrl+Shift+N".to_owned()])]);
        s.logs = Logs { session_logs: true, log_retention_days: 90 };
    });
    let (session, _) = Session::open(SessionConfig::for_tests(dir.path())).unwrap();
    session.execute(Caller::User, "settings.save", json!({ "settings": wanted })).await.unwrap();
    let back: EngineSettings = serde_json::from_value(session.execute(Caller::User, "settings.get", json!({})).await.unwrap()).unwrap();
    assert_eq!(back, wanted);
}

#[test]
fn unknown_terminal_type_falls_back() {
    assert_eq!(load(&store_with("{\"terminal_type\":\"evil\n\"}")).terminal.terminal_type, "xterm-256color");
    assert_eq!(with(|s| s.terminal.terminal_type = "ansi".into()).terminal.terminal_type, "xterm-256color");
}

#[test]
fn terminal_type_kept_when_known() {
    for term in TERMINAL_TYPES {
        let json = format!(r#"{{"terminal_type":"{term}"}}"#);
        assert_eq!(load(&store_with(&json)).terminal.terminal_type, term);
    }
}

#[test]
fn osc52_off_is_kept() {
    assert_eq!(EngineSettings::default().terminal.clipboard_osc52, Osc52::Write);
    assert_eq!(load(&store_with(r#"{"clipboard_osc52":"off"}"#)).terminal.clipboard_osc52, Osc52::Off);
}

#[test]
fn shortcuts_drop_unknown_actions_and_invalid_combos() {
    let settings = load(&store_with(
        r#"{"shortcuts":{"palette":["Ctrl+Shift+P","P","Ctrl+"],"bogus":["Ctrl+Shift+Q"],"newTab":["Ctrl+1","Ctrl+2","Ctrl+3","Ctrl+4","Ctrl+5"]}}"#,
    ));
    let b = &settings.shortcuts.bindings;
    assert_eq!(b["palette"], vec!["Ctrl+Shift+P"]);
    assert!(!b.contains_key("bogus"));
    assert_eq!(b["newTab"], vec!["Ctrl+1", "Ctrl+2", "Ctrl+3", "Ctrl+4"]);
}

#[test]
fn empty_shortcut_list_means_unbound() {
    let settings = load(&store_with(r#"{"shortcuts":{"closeTab":[]}}"#));
    assert_eq!(settings.shortcuts.bindings["closeTab"], Vec::<String>::new());
}

#[test]
fn old_settings_without_new_fields_load() {
    let settings = load(&store_with(r#"{"language":"de"}"#));
    assert_eq!(settings.terminal.terminal_type, "xterm-256color");
    assert_eq!(settings.terminal.clipboard_osc52, Osc52::Write);
    assert!(settings.shortcuts.bindings.is_empty());
}

#[test]
fn combos_are_checked() {
    for good in ["Ctrl+Shift+P", "Alt+X", "F5", "Ctrl+Tab", "Meta+K", "Ctrl+Alt+Shift+Meta+PageDown"] {
        assert!(valid_combo(good), "{good}");
    }
    let long = format!("Ctrl+{}", "X".repeat(36));
    for bad in ["P", "Shift+P", "Ctrl+", "Shift+Ctrl+P", "Ctrl+Shift+P+Q", "Ctrl+Ctrl+P", "F25", "", long.as_str()] {
        assert!(!valid_combo(bad), "{bad}");
    }
}

#[test]
fn every_action_is_known() {
    assert_eq!(SHORTCUT_ACTIONS.len(), 26);
    for added in ["hostsPage", "keysPage", "settingsPage", "copy", "paste", "quit", "about"] {
        assert!(SHORTCUT_ACTIONS.contains(&added), "{added}");
    }
    assert!(SHORTCUT_ACTIONS.contains(&"bookmark"));
    assert!(SHORTCUT_ACTIONS.contains(&"toggleSidebar"));
    assert!(SHORTCUT_ACTIONS.contains(&"toggleBroadcast"));
}

#[test]
fn history_and_autocomplete_are_on_by_default_and_kept() {
    let defaults = EngineSettings::default();
    assert!(defaults.terminal.history && defaults.terminal.autocomplete);
    let old = load(&store_with(r#"{"language":"de"}"#));
    assert!(old.terminal.history && old.terminal.autocomplete);
    let off = load(&store_with(r#"{"history":false,"autocomplete":false}"#));
    assert!(!off.terminal.history && !off.terminal.autocomplete);
}

#[test]
fn log_settings_default_off_and_keep_known_retention() {
    let defaults = EngineSettings::default();
    assert!(!defaults.logs.session_logs);
    assert_eq!(defaults.logs.log_retention_days, 0);
    let loaded = load(&store_with(r#"{"session_logs": true, "log_retention_days": 30}"#));
    assert!(loaded.logs.session_logs);
    assert_eq!(loaded.logs.log_retention_days, 30);
    assert_eq!(load(&store_with(r#"{"log_retention_days": 45}"#)).logs.log_retention_days, 0);
}

// --- The takeover from the Tauri version ---

const OLD_APP: &str = r#"{"language":"de","app_theme":"light","terminal_theme":"nord","font_family":"Fira Code","font_size":17,"host_view":"list","terminal_type":"vt220","clipboard_osc52":"off","shortcuts":{"newTab":["Ctrl+Shift+T"]},"history":false,"autocomplete":false,"session_logs":true,"log_retention_days":30}"#;
const OLD_SHELL: &str = r#"{"program":"pwsh.exe","args":["-NoLogo"],"label":"PowerShell 7"}"#;
const OLD_SERIAL: &str = r#"{"port":"COM7","baud":9600,"data_bits":7,"stop_bits":2,"parity":"even","flow":"hardware"}"#;

/// A data folder with the rows the Tauri version wrote.
fn tauri_data(data: &Path) {
    std::fs::create_dir_all(data).unwrap();
    let store = Store::open(&data.join("sshcraft.db")).unwrap();
    store.put_setting(TAURI_APP, OLD_APP).unwrap();
    store.put_setting(TAURI_LOCAL_SHELL, OLD_SHELL).unwrap();
    store.put_setting(TAURI_SERIAL_LAST, OLD_SERIAL).unwrap();
}

fn config_at(data: &Path, config: &Path) -> SessionConfig {
    let mut c = SessionConfig::for_tests(data);
    c.settings = Location::Dir(config.to_path_buf());
    c
}

fn marker(data: &Path) -> Option<String> {
    Store::open(&data.join("sshcraft.db")).unwrap().get_setting(TAKEOVER_MARKER).unwrap()
}

fn on_disk(config: &Path) -> EngineSettings {
    SettingsStore::<EngineSettings>::open(Location::Dir(config.to_path_buf())).0.get().clone()
}

#[tokio::test]
async fn takeover_copies_every_field() {
    let dir = tempfile::tempdir().unwrap();
    let (data, config) = (dir.path().join("data"), dir.path().join("config"));
    tauri_data(&data);
    let (_session, report) = Session::open(config_at(&data, &config)).unwrap();
    assert_eq!(report.takeover, Takeover::Done);
    let s = on_disk(&config);
    assert_eq!(s.general.language, LanguageSetting::De);
    assert_eq!(
        s.appearance,
        Appearance {
            app_theme: AppTheme::Light,
            terminal_theme: "nord".into(),
            font_family: "Fira Code".into(),
            font_size: 17,
            host_view: HostView::List,
            ligatures: true,
        }
    );
    assert_eq!(s.terminal.terminal_type, "vt220");
    assert_eq!(s.terminal.clipboard_osc52, Osc52::Off);
    assert!(!s.terminal.history && !s.terminal.autocomplete);
    assert_eq!(s.shortcuts.bindings["newTab"], vec!["Ctrl+Shift+T"]);
    assert_eq!(s.logs, Logs { session_logs: true, log_retention_days: 30 });
    assert_eq!(s.terminal.local_shell.as_ref().map(|c| c.program.as_str()), Some("pwsh.exe"));
    assert_eq!(s.serial.last.as_ref().map(|p| (p.port.as_str(), p.baud, p.data_bits)), Some(("COM7", 9600, 7)));
    drop(_session);
    assert!(marker(&data).is_some());
}

#[tokio::test]
async fn takeover_runs_once() {
    let dir = tempfile::tempdir().unwrap();
    let (data, config) = (dir.path().join("data"), dir.path().join("config"));
    tauri_data(&data);
    drop(Session::open(config_at(&data, &config)).unwrap());
    let before = std::fs::read(config.join("config.toml")).unwrap();
    Store::open(&data.join("sshcraft.db")).unwrap().put_setting(TAURI_APP, r#"{"language":"en"}"#).unwrap();
    let (_session, report) = Session::open(config_at(&data, &config)).unwrap();
    assert_eq!(report.takeover, Takeover::AlreadyDone);
    assert_eq!(std::fs::read(config.join("config.toml")).unwrap(), before);
}

#[tokio::test]
async fn an_existing_config_wins_and_is_marked() {
    let dir = tempfile::tempdir().unwrap();
    let (data, config) = (dir.path().join("data"), dir.path().join("config"));
    tauri_data(&data);
    let (mut existing, _) = SettingsStore::<EngineSettings>::open(Location::Dir(config.clone()));
    existing.set(with(|s| s.appearance.font_size = 20));
    existing.flush().unwrap();
    drop(existing);
    let (session, report) = Session::open(config_at(&data, &config)).unwrap();
    assert_eq!(report.takeover, Takeover::ConfigWins);
    assert_eq!(on_disk(&config).appearance.font_size, 20);
    assert_eq!(on_disk(&config).general.language, LanguageSetting::Auto);
    drop(session);
    assert!(marker(&data).is_some());
}

#[tokio::test]
async fn a_failed_save_leaves_no_marker() {
    let dir = tempfile::tempdir().unwrap();
    let (data, config) = (dir.path().join("data"), dir.path().join("config"));
    tauri_data(&data);
    let not_a_dir = dir.path().join("a-file");
    std::fs::write(&not_a_dir, "x").unwrap();
    let (session, report) = Session::open(config_at(&data, &not_a_dir)).unwrap();
    assert!(matches!(report.takeover, Takeover::Failed(_)), "{:?}", report.takeover);
    drop(session);
    assert!(marker(&data).is_none());
    let (_session, report) = Session::open(config_at(&data, &config)).unwrap();
    assert_eq!(report.takeover, Takeover::Done);
}

#[tokio::test]
async fn memory_settings_take_over_nothing() {
    let dir = tempfile::tempdir().unwrap();
    tauri_data(dir.path());
    let (session, report) = Session::open(SessionConfig::for_tests(dir.path())).unwrap();
    assert_eq!(report.takeover, Takeover::NotPersistent);
    drop(session);
    assert!(marker(dir.path()).is_none());
}

#[tokio::test]
async fn newer_config_is_read_only_and_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let (data, config) = (dir.path().join("data"), dir.path().join("config"));
    tauri_data(&data);
    std::fs::create_dir_all(&config).unwrap();
    let text = "schema_version = 99\n\n[appearance]\nfont_size = 22\n";
    std::fs::write(config.join("config.toml"), text).unwrap();
    let (session, report) = Session::open(config_at(&data, &config)).unwrap();
    assert!(report.settings_read_only);
    let got: EngineSettings = serde_json::from_value(session.execute(Caller::User, "settings.get", json!({})).await.unwrap()).unwrap();
    assert_eq!(got.appearance.font_size, 22);
    let err = session.execute(Caller::User, "settings.save", json!({ "settings": EngineSettings::default() })).await.unwrap_err();
    assert!(matches!(err, EngineError::Settings(_)), "{err:?}");
    assert_eq!(std::fs::read_to_string(config.join("config.toml")).unwrap(), text);
    drop(session);
    assert!(marker(&data).is_none());
}

#[tokio::test]
async fn settings_survive_a_reload() {
    let dir = tempfile::tempdir().unwrap();
    let (data, config) = (dir.path().join("data"), dir.path().join("config"));
    let (session, _) = Session::open(config_at(&data, &config)).unwrap();
    let wanted = with(|s| s.appearance.font_size = 19);
    session.execute(Caller::User, "settings.save", json!({ "settings": wanted })).await.unwrap();
    session.execute(Caller::User, "local.set_default_shell", json!({ "shell": { "program": "bash", "args": [], "label": "Bash" } })).await.unwrap();
    drop(session);
    let (session, _) = Session::open(config_at(&data, &config)).unwrap();
    let got: EngineSettings = serde_json::from_value(session.execute(Caller::User, "settings.get", json!({})).await.unwrap()).unwrap();
    assert_eq!(got.appearance.font_size, 19);
    let shell = session.execute(Caller::User, "local.default_shell", json!({})).await.unwrap();
    assert_eq!(shell["program"], "bash");
}

#[test]
fn clipboard_hosts_round_trip() {
    let hosts: Vec<sshcraft_model::model::HostId> = (0..MAX_CLIPBOARD_HOSTS + 5).map(|_| sshcraft_model::model::HostId::new()).collect();
    let mut wanted = hosts.clone();
    wanted.insert(1, hosts[0]);
    let s = with(|s| s.terminal.clipboard_hosts = wanted);
    assert_eq!(s.terminal.clipboard_hosts.len(), MAX_CLIPBOARD_HOSTS, "at most 1000");
    assert_eq!(s.terminal.clipboard_hosts[..2], [hosts[0], hosts[1]], "a host once, in order");
    let back: EngineSettings = serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
    assert_eq!(back, s);
    assert!(EngineSettings::default().terminal.clipboard_hosts.is_empty(), "nobody is allowed by default");
}

#[test]
fn scrollback_is_clamped() {
    assert_eq!(EngineSettings::default().terminal.scrollback, 10_000);
    assert_eq!(with(|s| s.terminal.scrollback = 0).terminal.scrollback, 1000);
    assert_eq!(with(|s| s.terminal.scrollback = 10_000_000).terminal.scrollback, 100_000);
    assert_eq!(with(|s| s.terminal.scrollback = 50_000).terminal.scrollback, 50_000);
    assert!(EngineSettings::default().appearance.ligatures, "ligatures on by default");
    assert!(!EngineSettings::default().general.system_title_bar, "the app's own title bar by default");
}

#[test]
fn integrations_default_off() {
    let fresh = EngineSettings::default();
    assert!(!fresh.integrations.allow_agent_control, "agents are off until the user turns them on");
    assert_eq!(fresh.integrations.agent_port, 0, "any free port");
}

#[test]
fn notifications_have_defaults_and_bounds() {
    let d = EngineSettings::default();
    assert_eq!(d.terminal.notify_after_secs, 10);
    assert!(d.terminal.server_notifications);
    assert_eq!(with(|s| s.terminal.notify_after_secs = 1_000_000).terminal.notify_after_secs, 86_400);
    assert_eq!(with(|s| s.terminal.notify_after_secs = 0).terminal.notify_after_secs, 0);
}
