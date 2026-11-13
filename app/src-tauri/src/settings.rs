//! App settings of this device (language, look, terminal, shortcuts). Stored as one JSON value.

use std::collections::BTreeMap;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sshcraft_core::store::{Store, StoreError};

const KEY: &str = "app";
const FONT_SIZES: std::ops::RangeInclusive<u16> = 8..=32;
const MAX_FONT_FAMILY: usize = 200;

/// Terminal types a session may ask for (`TERM`); the first is the default.
pub const TERMINAL_TYPES: [&str; 7] = [
    "xterm-256color",
    "xterm",
    "screen-256color",
    "tmux-256color",
    "linux",
    "vt220",
    "vt100",
];

/// Actions a shortcut can trigger; the same names as `ShortcutAction` in the UI.
pub const SHORTCUT_ACTIONS: [&str; 12] = [
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
];
const MAX_COMBOS: usize = 4;
const MAX_COMBO_LEN: usize = 40;
const MODIFIERS: [&str; 4] = ["Ctrl", "Alt", "Shift", "Meta"];

/// Whether `combo` is a shortcut in SSHCraft's notation: modifiers in the order
/// Ctrl, Alt, Shift, Meta, then one key; with Ctrl, Alt or Meta, or an F-key on its own.
pub fn valid_combo(combo: &str) -> bool {
    if combo.is_empty() || combo.len() > MAX_COMBO_LEN {
        return false;
    }
    let parts: Vec<&str> = combo.split('+').collect();
    let (key, modifiers) = parts.split_last().expect("split gives at least one part");
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
    // One character, or a named key from `KeyboardEvent.key` (letters and digits only).
    let single = key.chars().count() == 1;
    if !single && !key.chars().all(|c| c.is_ascii_alphanumeric()) {
        return false;
    }
    let function_key = key
        .strip_prefix('F')
        .and_then(|n| n.parse::<u8>().ok())
        .is_some_and(|n| (1..=24).contains(&n));
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppSettings {
    pub language: LanguageSetting,
    pub app_theme: AppTheme,
    /// Id of a terminal theme; the UI falls back to its default for unknown ids.
    pub terminal_theme: String,
    pub font_family: String,
    pub font_size: u16,
    pub host_view: HostView,
    /// One of [`TERMINAL_TYPES`].
    pub terminal_type: String,
    pub clipboard_osc52: Osc52,
    /// Shortcuts that differ from the defaults, by action; an empty list unbinds the action.
    pub shortcuts: BTreeMap<String, Vec<String>>,
    /// Commands typed in terminals are kept (sealed) for the sidebar and completion.
    pub history: bool,
    /// Suggestions while typing in a terminal.
    pub autocomplete: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            language: LanguageSetting::Auto,
            app_theme: AppTheme::System,
            terminal_theme: "sshcraft-dark".into(),
            font_family: "Cascadia Mono, Consolas, monospace".into(),
            font_size: 14,
            host_view: HostView::Grid,
            terminal_type: TERMINAL_TYPES[0].into(),
            clipboard_osc52: Osc52::Write,
            shortcuts: BTreeMap::new(),
            history: true,
            autocomplete: true,
        }
    }
}

impl AppSettings {
    /// Keeps values in their allowed ranges.
    fn sanitized(mut self) -> Self {
        self.font_size = self.font_size.clamp(*FONT_SIZES.start(), *FONT_SIZES.end());
        let family = self.font_family.trim();
        self.font_family = if family.is_empty() || family.chars().count() > MAX_FONT_FAMILY {
            Self::default().font_family
        } else {
            family.to_owned()
        };
        if !TERMINAL_TYPES.contains(&self.terminal_type.as_str()) {
            self.terminal_type = TERMINAL_TYPES[0].into();
        }
        self.shortcuts = std::mem::take(&mut self.shortcuts)
            .into_iter()
            .filter(|(action, _)| SHORTCUT_ACTIONS.contains(&action.as_str()))
            .map(|(action, combos)| {
                let combos = combos
                    .into_iter()
                    .filter(|c| valid_combo(c))
                    .take(MAX_COMBOS)
                    .collect();
                (action, combos)
            })
            .collect();
        self
    }
}

/// Reads the settings field by field: a field that is missing or does not fit (written by an
/// older or newer version, or damaged) takes its default, the others are kept.
pub fn load(store: &Store) -> AppSettings {
    let defaults = AppSettings::default();
    let stored = store
        .get_setting(KEY)
        .ok()
        .flatten()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
    let Some(serde_json::Value::Object(map)) = stored else {
        return defaults;
    };
    fn field<T: DeserializeOwned>(
        map: &serde_json::Map<String, serde_json::Value>,
        name: &str,
    ) -> Option<T> {
        map.get(name)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }
    AppSettings {
        language: field(&map, "language").unwrap_or(defaults.language),
        app_theme: field(&map, "app_theme").unwrap_or(defaults.app_theme),
        terminal_theme: field(&map, "terminal_theme").unwrap_or(defaults.terminal_theme),
        font_family: field(&map, "font_family").unwrap_or(defaults.font_family),
        font_size: field(&map, "font_size").unwrap_or(defaults.font_size),
        host_view: field(&map, "host_view").unwrap_or(defaults.host_view),
        terminal_type: field(&map, "terminal_type").unwrap_or(defaults.terminal_type),
        clipboard_osc52: field(&map, "clipboard_osc52").unwrap_or(defaults.clipboard_osc52),
        shortcuts: field(&map, "shortcuts").unwrap_or(defaults.shortcuts),
        history: field(&map, "history").unwrap_or(defaults.history),
        autocomplete: field(&map, "autocomplete").unwrap_or(defaults.autocomplete),
    }
    .sanitized()
}

/// Stores the settings within their limits and returns what was stored.
pub fn save(store: &Store, settings: AppSettings) -> Result<AppSettings, StoreError> {
    let settings = settings.sanitized();
    let json =
        serde_json::to_string(&settings).map_err(|e| StoreError::Malformed(e.to_string()))?;
    store.put_setting(KEY, &json)?;
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_with(json: &str) -> Store {
        let store = Store::open_in_memory().unwrap();
        store.put_setting(KEY, json).unwrap();
        store
    }

    #[test]
    fn nothing_stored_gives_defaults() {
        assert_eq!(
            load(&Store::open_in_memory().unwrap()),
            AppSettings::default()
        );
    }

    #[test]
    fn missing_fields_take_defaults() {
        let settings = load(&store_with(r#"{"language":"de"}"#));
        assert_eq!(settings.language, LanguageSetting::De);
        assert_eq!(settings.font_size, 14);
        assert_eq!(settings.terminal_theme, "sshcraft-dark");
    }

    #[test]
    fn unknown_value_only_resets_that_field() {
        let settings = load(&store_with(
            r#"{"language":"fr","font_size":16,"app_theme":"light"}"#,
        ));
        assert_eq!(settings.language, LanguageSetting::Auto);
        assert_eq!(settings.font_size, 16);
        assert_eq!(settings.app_theme, AppTheme::Light);
    }

    #[test]
    fn not_json_gives_defaults() {
        assert_eq!(load(&store_with("not json")), AppSettings::default());
        assert_eq!(load(&store_with("[1,2]")), AppSettings::default());
    }

    #[test]
    fn font_size_is_clamped() {
        let store = Store::open_in_memory().unwrap();
        let small = save(
            &store,
            AppSettings {
                font_size: 0,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(small.font_size, 8);
        let big = save(
            &store,
            AppSettings {
                font_size: 500,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(big.font_size, 32);
        assert_eq!(load(&store).font_size, 32);
        assert_eq!(load(&store_with(r#"{"font_size":500}"#)).font_size, 32);
    }

    #[test]
    fn blank_or_huge_font_family_gives_default() {
        let store = Store::open_in_memory().unwrap();
        let blank = save(
            &store,
            AppSettings {
                font_family: "   ".into(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(blank.font_family, AppSettings::default().font_family);
        let huge = save(
            &store,
            AppSettings {
                font_family: "x".repeat(201),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(huge.font_family, AppSettings::default().font_family);
        let trimmed = save(
            &store,
            AppSettings {
                font_family: " Consolas ".into(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(trimmed.font_family, "Consolas");
    }

    #[test]
    fn saved_settings_come_back() {
        let store = Store::open_in_memory().unwrap();
        let wanted = AppSettings {
            language: LanguageSetting::En,
            app_theme: AppTheme::Dark,
            terminal_theme: "nord".into(),
            font_family: "JetBrains Mono".into(),
            font_size: 15,
            host_view: HostView::List,
            terminal_type: "vt100".into(),
            clipboard_osc52: Osc52::Off,
            shortcuts: BTreeMap::from([("newTab".to_owned(), vec!["Ctrl+Shift+N".to_owned()])]),
            history: false,
            autocomplete: false,
        };
        save(&store, wanted.clone()).unwrap();
        assert_eq!(load(&store), wanted);
    }

    #[test]
    fn unknown_terminal_type_falls_back() {
        let settings = load(&store_with(
            r#"{"terminal_type":"evil
"}"#,
        ));
        assert_eq!(settings.terminal_type, "xterm-256color");
        let store = Store::open_in_memory().unwrap();
        let saved = save(
            &store,
            AppSettings {
                terminal_type: "ansi".into(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(saved.terminal_type, "xterm-256color");
    }

    #[test]
    fn terminal_type_kept_when_known() {
        for term in TERMINAL_TYPES {
            let json = format!(r#"{{"terminal_type":"{term}"}}"#);
            assert_eq!(load(&store_with(&json)).terminal_type, term);
        }
    }

    #[test]
    fn osc52_off_is_kept() {
        assert_eq!(AppSettings::default().clipboard_osc52, Osc52::Write);
        let settings = load(&store_with(r#"{"clipboard_osc52":"off"}"#));
        assert_eq!(settings.clipboard_osc52, Osc52::Off);
    }

    #[test]
    fn shortcuts_drop_unknown_actions_and_invalid_combos() {
        let settings = load(&store_with(
            r#"{"shortcuts":{"palette":["Ctrl+Shift+P","P","Ctrl+"],"bogus":["Ctrl+Shift+Q"],"newTab":["Ctrl+1","Ctrl+2","Ctrl+3","Ctrl+4","Ctrl+5"]}}"#,
        ));
        assert_eq!(settings.shortcuts["palette"], vec!["Ctrl+Shift+P"]);
        assert!(!settings.shortcuts.contains_key("bogus"));
        assert_eq!(
            settings.shortcuts["newTab"],
            vec!["Ctrl+1", "Ctrl+2", "Ctrl+3", "Ctrl+4"]
        );
    }

    #[test]
    fn empty_shortcut_list_means_unbound() {
        let settings = load(&store_with(r#"{"shortcuts":{"closeTab":[]}}"#));
        assert_eq!(settings.shortcuts["closeTab"], Vec::<String>::new());
    }

    #[test]
    fn old_settings_without_new_fields_load() {
        let settings = load(&store_with(r#"{"language":"de"}"#));
        assert_eq!(settings.terminal_type, "xterm-256color");
        assert_eq!(settings.clipboard_osc52, Osc52::Write);
        assert!(settings.shortcuts.is_empty());
    }

    #[test]
    fn combos_are_checked() {
        for good in [
            "Ctrl+Shift+P",
            "Alt+X",
            "F5",
            "Ctrl+Tab",
            "Meta+K",
            "Ctrl+Alt+Shift+Meta+PageDown",
        ] {
            assert!(valid_combo(good), "{good}");
        }
        let long = format!("Ctrl+{}", "X".repeat(36));
        for bad in [
            "P",
            "Shift+P",
            "Ctrl+",
            "Shift+Ctrl+P",
            "Ctrl+Shift+P+Q",
            "Ctrl+Ctrl+P",
            "F25",
            "",
            long.as_str(),
        ] {
            assert!(!valid_combo(bad), "{bad}");
        }
    }

    #[test]
    fn every_action_is_known() {
        assert_eq!(SHORTCUT_ACTIONS.len(), 12);
        assert!(SHORTCUT_ACTIONS.contains(&"toggleSidebar"));
        assert!(SHORTCUT_ACTIONS.contains(&"toggleBroadcast"));
    }

    #[test]
    fn history_and_autocomplete_are_on_by_default_and_kept() {
        let defaults = AppSettings::default();
        assert!(defaults.history && defaults.autocomplete);
        let old = load(&store_with(r#"{"language":"de"}"#));
        assert!(old.history && old.autocomplete);
        let off = load(&store_with(r#"{"history":false,"autocomplete":false}"#));
        assert!(!off.history && !off.autocomplete);
    }
}
