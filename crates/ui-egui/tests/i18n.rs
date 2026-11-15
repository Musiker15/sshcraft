//! Every text of the interface has German, with the same placeholders; AUTO follows the system.

use std::path::{Path, PathBuf};

use sshcraft_ui_egui::i18n::{Lang, de, placeholders};

/// The `.rs` files below `dir`.
fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The string literals passed to `tr(` and `trf(` in `text` (plain literals, no escapes but `\"`).
fn literals(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for call in ["tr(\"", "trf(\""] {
        let mut rest = text;
        while let Some(at) = rest.find(call) {
            // `ltr("` or `attr("` are not calls of tr.
            let before = rest[..at].chars().last();
            rest = &rest[at + call.len()..];
            if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                continue;
            }
            let mut literal = String::new();
            let mut chars = rest.chars();
            while let Some(c) = chars.next() {
                match c {
                    '\\' => {
                        if let Some(next) = chars.next() {
                            literal.push(next);
                        }
                    }
                    '"' => break,
                    other => literal.push(other),
                }
            }
            found.push(literal);
        }
    }
    found
}

fn all_texts() -> Vec<(PathBuf, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    sources(&root.join("src"), &mut files);
    sources(&root.join("../../apps/sshcraft/src"), &mut files);
    let mut texts = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        for literal in literals(&text) {
            texts.push((file.clone(), literal));
        }
    }
    texts
}

#[test]
fn every_text_has_german() {
    let texts = all_texts();
    assert!(texts.len() > 5, "the collector found the texts: {}", texts.len());
    let missing: Vec<String> = texts.iter().filter(|(_, t)| de::translate(t).is_none()).map(|(f, t)| format!("{}: {t:?}", f.display())).collect();
    assert!(missing.is_empty(), "no German for:\n{}", missing.join("\n"));
}

#[test]
fn placeholders_match() {
    for (file, text) in all_texts() {
        if let Some(german) = de::translate(&text) {
            assert_eq!(placeholders(&text), placeholders(german), "{}: {text:?}", file.display());
        }
    }
}

#[test]
fn auto_follows_the_system() {
    assert_eq!(Lang::from_locale(Some("de-DE")), Lang::De);
    assert_eq!(Lang::from_locale(Some("de")), Lang::De);
    assert_eq!(Lang::from_locale(Some("de_AT")), Lang::De);
    assert_eq!(Lang::from_locale(Some("fr-FR")), Lang::En);
    assert_eq!(Lang::from_locale(None), Lang::En);
}

#[test]
fn the_interface_speaks_the_chosen_language() {
    sshcraft_ui_egui::i18n::set_language(Lang::De);
    assert_eq!(sshcraft_ui_egui::i18n::tr("Settings"), "Einstellungen");
    sshcraft_ui_egui::i18n::set_language(Lang::En);
    assert_eq!(sshcraft_ui_egui::i18n::tr("Settings"), "Settings");
    assert_eq!(sshcraft_ui_egui::i18n::trf("{count} hosts", &[("count", "3")]), "3 hosts");
}
