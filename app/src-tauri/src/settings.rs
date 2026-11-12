//! App settings of this device (language, look, terminal font). Stored as one JSON value.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sshcraft_core::store::{Store, StoreError};

const KEY: &str = "app";
const FONT_SIZES: std::ops::RangeInclusive<u16> = 8..=32;
const MAX_FONT_FAMILY: usize = 200;

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
        };
        save(&store, wanted.clone()).unwrap();
        assert_eq!(load(&store), wanted);
    }
}
