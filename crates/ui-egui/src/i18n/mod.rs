//! English and German. Texts are English in the code and go through [`tr`] or [`trf`]; the
//! German ones are in `de.rs`. The interface follows `general.language`, AUTO the system's
//! language. (craft-i18n keeps its catalogs in the separate `storytold/craft-i18n` repository;
//! SSHCraft keeps its two languages here, with the same `tr`/`trf` shape.)

pub mod de;

use std::sync::atomic::{AtomicU8, Ordering};

use sshcraft_engine::settings::LanguageSetting;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
}

impl Lang {
    /// The language for a system locale such as `de-DE` or `de_AT`: German for German, else
    /// English.
    pub fn from_locale(locale: Option<&str>) -> Lang {
        match locale {
            Some(l) if l.len() >= 2 && l.get(..2).is_some_and(|p| p.eq_ignore_ascii_case("de")) => Lang::De,
            _ => Lang::En,
        }
    }

    /// The language for the setting; AUTO asks the system.
    pub fn resolve(setting: LanguageSetting) -> Lang {
        match setting {
            LanguageSetting::En => Lang::En,
            LanguageSetting::De => Lang::De,
            LanguageSetting::Auto => Lang::from_locale(system_locale()),
        }
    }
}

/// The system's locale, asked once: the frame loop resolves the language every frame.
fn system_locale() -> Option<&'static str> {
    static LOCALE: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    LOCALE.get_or_init(sys_locale::get_locale).as_deref()
}

static LANGUAGE: AtomicU8 = AtomicU8::new(0);

pub fn set_language(lang: Lang) {
    LANGUAGE.store(lang as u8, Ordering::Relaxed);
}

pub fn language() -> Lang {
    if LANGUAGE.load(Ordering::Relaxed) == Lang::De as u8 { Lang::De } else { Lang::En }
}

/// The text in the interface language.
pub fn tr(text: &'static str) -> &'static str {
    match language() {
        Lang::En => text,
        Lang::De => de::translate(text).unwrap_or(text),
    }
}

/// [`tr`] with `{name}` placeholders filled in.
pub fn trf(template: &'static str, args: &[(&str, &str)]) -> String {
    let mut out = tr(template).to_string();
    for (name, value) in args {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

/// The `{name}` placeholders of a template, sorted.
pub fn placeholders(template: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        match after.find('}') {
            Some(end) => {
                found.push(after[..end].to_string());
                rest = &after[end + 1..];
            }
            None => break,
        }
    }
    found.sort();
    found
}
