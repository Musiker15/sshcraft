//! The settings of this device in `config.toml` (through craft-settings): language, look,
//! terminal, shortcuts, log defaults, the default shell and the last serial settings. The data
//! (hosts, keys, templates, agent rights) stays in the database.
//!
//! The Tauri version kept these settings as JSON rows in the database. They are taken over once
//! ([`take_over`]); the old rows stay where they are.

use std::collections::BTreeMap;

use craft_settings::{LoadReport, Settings, SettingsStore, Source};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sshcraft_local::pty::ShellCommand;
use sshcraft_local::serial::SerialSettings;
use sshcraft_store::store::Store;

/// The database row of the Tauri version's app settings.
const TAURI_APP: &str = "app";
const TAURI_LOCAL_SHELL: &str = "terminal.local_shell";
const TAURI_SERIAL_LAST: &str = "serial.last";
/// Set in the database once the takeover is done (or not needed).
pub const TAKEOVER_MARKER: &str = "migration.tauri_settings";

pub const FONT_SIZES: std::ops::RangeInclusive<u16> = 8..=32;
const MAX_FONT_FAMILY: usize = 200;

/// Terminal types a session may ask for (`TERM`); the first is the default.
pub const TERMINAL_TYPES: [&str; 7] = ["xterm-256color", "xterm", "screen-256color", "tmux-256color", "linux", "vt220", "vt100"];

/// Actions a shortcut can trigger.
pub const SHORTCUT_ACTIONS: [&str; 21] = [
    "palette",
    "newTab",
    "closeTab",
    "nextTab",
    "prevTab",
    "splitPane",
    "closePane",
    "nextPane",
    "prevPane",
    "toggleLayout",
    "toggleBroadcast",
    "toggleSidebar",
    "bookmark",
    // Added by the egui window (R4); the names above are the Tauri version's.
    "hostsPage",
    "keysPage",
    "settingsPage",
    "copy",
    "paste",
    "snippetsPage",
    "forwardingPage",
    "logsPage",
];
/// Days a finished session log is kept; 0 keeps it until it is deleted.
pub const LOG_RETENTION_DAYS: [u32; 5] = [0, 7, 30, 90, 365];
const MAX_COMBOS: usize = 4;
/// Scrollback lines a terminal keeps, as the setting allows them.
pub const SCROLLBACK: std::ops::RangeInclusive<usize> = 1000..=100_000;
/// Most hosts that may always write the clipboard (OSC 52) without asking.
pub const MAX_CLIPBOARD_HOSTS: usize = 1000;
const MAX_COMBO_LEN: usize = 40;
const MODIFIERS: [&str; 4] = ["Ctrl", "Alt", "Shift", "Meta"];

/// Whether `combo` is a shortcut in SSHCraft's notation: modifiers in the order Ctrl, Alt,
/// Shift, Meta, then one key; with Ctrl, Alt or Meta, or an F-key on its own.
pub fn valid_combo(combo: &str) -> bool {
    if combo.is_empty() || combo.len() > MAX_COMBO_LEN {
        return false;
    }
    let parts: Vec<&str> = combo.split('+').collect();
    let Some((key, modifiers)) = parts.split_last() else {
        return false;
    };
    // Modifiers must come in the fixed order, each at most once.
    let mut last = None;
    for m in modifiers {
        let Some(position) = MODIFIERS.iter().position(|x| x == m) else {
            return false;
        };
        if last.is_some_and(|l| position <= l) {
            return false;
        }
        last = Some(position);
    }
    if key.is_empty() || MODIFIERS.contains(key) {
        return false;
    }
    // One character, or a named key (letters and digits only).
    let single = key.chars().count() == 1;
    if !single && !key.chars().all(|c| c.is_ascii_alphanumeric()) {
        return false;
    }
    let function_key = key.strip_prefix('F').and_then(|n| n.parse::<u8>().ok()).is_some_and(|n| (1..=24).contains(&n));
    if key.starts_with('F') && key.len() > 1 && !function_key {
        return false;
    }
    modifiers.iter().any(|m| *m != "Shift") || function_key
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Osc52 {
    /// Programs on the server may write the clipboard (never read it).
    #[default]
    Write,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LanguageSetting {
    #[default]
    Auto,
    En,
    De,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppTheme {
    #[default]
    System,
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostView {
    #[default]
    Grid,
    List,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct General {
    pub language: LanguageSetting,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    pub app_theme: AppTheme,
    /// Id of a terminal theme; the interface falls back to its default for unknown ids.
    pub terminal_theme: String,
    pub font_family: String,
    pub font_size: u16,
    pub host_view: HostView,
    /// Neighbouring characters may join into one glyph where the font has ligatures (`->`).
    pub ligatures: bool,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            app_theme: AppTheme::System,
            terminal_theme: "sshcraft-dark".into(),
            font_family: "Cascadia Mono, Consolas, monospace".into(),
            font_size: 14,
            host_view: HostView::Grid,
            ligatures: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Terminal {
    /// One of [`TERMINAL_TYPES`].
    pub terminal_type: String,
    pub clipboard_osc52: Osc52,
    /// Hosts whose programs may write the clipboard without asking each time (the user chose
    /// "Always for this host"). Empty by default: every host asks first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub clipboard_hosts: Vec<sshcraft_model::model::HostId>,
    /// Lines a terminal keeps above the screen, within [`SCROLLBACK`].
    pub scrollback: usize,
    /// Commands typed in terminals are kept (sealed) for the sidebar and completion.
    pub history: bool,
    /// Suggestions while typing in a terminal.
    pub autocomplete: bool,
    /// The shell a local terminal starts; none means the system default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_shell: Option<ShellCommand>,
}

impl Default for Terminal {
    fn default() -> Self {
        Self {
            terminal_type: TERMINAL_TYPES[0].into(),
            clipboard_osc52: Osc52::Write,
            clipboard_hosts: Vec::new(),
            scrollback: 10_000,
            history: true,
            autocomplete: true,
            local_shell: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Shortcuts {
    /// Shortcuts that differ from the defaults, by action; an empty list unbinds the action.
    pub bindings: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Logs {
    /// Record sessions unless a host or group says otherwise.
    pub session_logs: bool,
    /// One of [`LOG_RETENTION_DAYS`].
    pub log_retention_days: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Serial {
    /// The serial settings used last.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last: Option<SerialSettings>,
}

/// Agents and other programs reaching the running app.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Integrations {
    /// AI agents may connect (through `sshcraft-cli` and MCP). Off until the user turns it on.
    pub allow_agent_control: bool,
    /// The loopback port agents connect on; 0 takes any free port.
    pub agent_port: u16,
}

/// Everything in `config.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct EngineSettings {
    pub general: General,
    pub appearance: Appearance,
    pub terminal: Terminal,
    pub shortcuts: Shortcuts,
    pub logs: Logs,
    pub serial: Serial,
    pub integrations: Integrations,
}

impl Settings for EngineSettings {
    const APP_ID: &'static str = "sshcraft";
    const SCHEMA_VERSION: u32 = 1;
}

impl EngineSettings {
    /// Keeps values in their allowed ranges.
    pub fn sanitized(mut self) -> Self {
        let defaults = Appearance::default();
        let a = &mut self.appearance;
        a.font_size = a.font_size.clamp(*FONT_SIZES.start(), *FONT_SIZES.end());
        let family = a.font_family.trim();
        a.font_family = if family.is_empty() || family.chars().count() > MAX_FONT_FAMILY { defaults.font_family } else { family.to_owned() };
        if !LOG_RETENTION_DAYS.contains(&self.logs.log_retention_days) {
            self.logs.log_retention_days = 0;
        }
        if !TERMINAL_TYPES.contains(&self.terminal.terminal_type.as_str()) {
            self.terminal.terminal_type = TERMINAL_TYPES[0].into();
        }
        let mut seen = std::collections::HashSet::new();
        self.terminal.clipboard_hosts.retain(|h| seen.insert(*h));
        self.terminal.clipboard_hosts.truncate(MAX_CLIPBOARD_HOSTS);
        self.terminal.scrollback = self.terminal.scrollback.clamp(*SCROLLBACK.start(), *SCROLLBACK.end());
        self.shortcuts.bindings = std::mem::take(&mut self.shortcuts.bindings)
            .into_iter()
            .filter(|(action, _)| SHORTCUT_ACTIONS.contains(&action.as_str()))
            .map(|(action, combos)| (action, combos.into_iter().filter(|c| valid_combo(c)).take(MAX_COMBOS).collect()))
            .collect();
        self
    }
}

/// The settings the Tauri version kept in the database, field by field: a field that is missing
/// or does not fit takes its default, the others are kept.
pub fn read_tauri_settings(store: &Store) -> EngineSettings {
    fn row<T: DeserializeOwned>(store: &Store, key: &str) -> Option<T> {
        store.get_setting(key).ok().flatten().and_then(|text| serde_json::from_str(&text).ok())
    }
    fn field<T: DeserializeOwned>(map: &serde_json::Map<String, serde_json::Value>, name: &str) -> Option<T> {
        map.get(name).and_then(|v| serde_json::from_value(v.clone()).ok())
    }
    let mut out = EngineSettings::default();
    if let Some(serde_json::Value::Object(map)) = row::<serde_json::Value>(store, TAURI_APP) {
        let d = EngineSettings::default();
        out.general.language = field(&map, "language").unwrap_or(d.general.language);
        out.appearance = Appearance {
            app_theme: field(&map, "app_theme").unwrap_or(d.appearance.app_theme),
            terminal_theme: field(&map, "terminal_theme").unwrap_or(d.appearance.terminal_theme),
            font_family: field(&map, "font_family").unwrap_or(d.appearance.font_family),
            font_size: field(&map, "font_size").unwrap_or(d.appearance.font_size),
            host_view: field(&map, "host_view").unwrap_or(d.appearance.host_view),
            ligatures: d.appearance.ligatures,
        };
        out.terminal.terminal_type = field(&map, "terminal_type").unwrap_or(d.terminal.terminal_type);
        out.terminal.clipboard_osc52 = field(&map, "clipboard_osc52").unwrap_or(d.terminal.clipboard_osc52);
        out.terminal.history = field(&map, "history").unwrap_or(d.terminal.history);
        out.terminal.autocomplete = field(&map, "autocomplete").unwrap_or(d.terminal.autocomplete);
        out.shortcuts.bindings = field(&map, "shortcuts").unwrap_or(d.shortcuts.bindings);
        out.logs.session_logs = field(&map, "session_logs").unwrap_or(d.logs.session_logs);
        out.logs.log_retention_days = field(&map, "log_retention_days").unwrap_or(d.logs.log_retention_days);
    }
    out.terminal.local_shell = row(store, TAURI_LOCAL_SHELL);
    out.serial.last = row(store, TAURI_SERIAL_LAST);
    out.sanitized()
}

/// What happened to the settings of the Tauri version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Takeover {
    /// Copied into `config.toml` now.
    Done,
    /// Done on an earlier start.
    AlreadyDone,
    /// A `config.toml` was already there; it stays as it is.
    ConfigWins,
    /// The settings live in memory only (tests, an empty `SSHCRAFT_CONFIG_DIR`): nothing to do.
    NotPersistent,
    /// `config.toml` could not be written; the next start tries again.
    Failed(String),
}

/// Copies the Tauri version's settings into `config.toml` once. The marker in the database is
/// set only when the copy is saved (or a `config.toml` already existed).
pub fn take_over(store: &Store, settings: &mut SettingsStore<EngineSettings>, report: &LoadReport, now_ms: i64) -> Takeover {
    if store.get_setting(TAKEOVER_MARKER).ok().flatten().is_some() {
        return Takeover::AlreadyDone;
    }
    if !settings.is_persistent() {
        return Takeover::NotPersistent;
    }
    if report.read_only.is_some() || settings.is_read_only() {
        return Takeover::Failed("config.toml is read-only".to_string());
    }
    let mark = |store: &Store| store.put_setting(TAKEOVER_MARKER, &now_ms.to_string()).map_err(|e| e.to_string());
    if matches!(report.source, Source::File) {
        return match mark(store) {
            Ok(()) => Takeover::ConfigWins,
            Err(e) => Takeover::Failed(e),
        };
    }
    settings.set(read_tauri_settings(store));
    if let Err(e) = settings.flush() {
        return Takeover::Failed(e.to_string());
    }
    match mark(store) {
        Ok(()) => Takeover::Done,
        Err(e) => Takeover::Failed(e),
    }
}

#[cfg(test)]
mod tests;
