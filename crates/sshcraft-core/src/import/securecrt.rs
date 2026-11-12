//! SecureCRT sessions: one `.ini` file per session under `Config\Sessions`, folders are groups.
//! Password fields are never read.

use std::path::Path;

use super::{ImportCandidate, ImportPlan, ImportWarning, KeyNote, MAX_FILE, clean, port_from};
use crate::model::ConnectionSettings;

/// Folders deeper than this are not walked.
const MAX_DEPTH: usize = 16;

fn value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().find_map(|line| {
        let line = line.trim_end_matches('\r');
        let (name, value) = line.split_once('=')?;
        let name = name.split_once(':').map_or(name, |(_, n)| n);
        (name.trim_matches('"') == key).then_some(value)
    })
}

fn session(text: &str, name: String, group_path: Vec<String>, plan: &mut ImportPlan) {
    let protocol = value(text, "Protocol Name").unwrap_or_default();
    if !protocol.eq_ignore_ascii_case("SSH2") {
        plan.skipped.push(format!("{name}: not SSH"));
        return;
    }
    let address = value(text, "Hostname").map(clean).unwrap_or_default();
    if address.is_empty() {
        plan.skipped.push(format!("{name}: no host name"));
        return;
    }
    let mut warnings = Vec::new();
    let port = value(text, "[SSH2] Port")
        .and_then(|hex| u32::from_str_radix(hex.trim(), 16).ok())
        .and_then(|p| port_from(&p.to_string(), &mut warnings))
        .filter(|p| *p != crate::model::DEFAULT_PORT);
    let username = value(text, "Username").map(clean).filter(|u| !u.is_empty());
    let key = value(text, "Identity Filename V2")
        .map(|v| v.split("::").next().unwrap_or_default().trim().to_owned())
        .filter(|v| !v.is_empty());
    let stored_password = ["Password V2", "Password"]
        .iter()
        .any(|k| value(text, k).is_some_and(|v| !v.trim().is_empty()));
    if stored_password {
        warnings.push(ImportWarning::PasswordNotImported);
    }
    plan.candidates.push(ImportCandidate {
        name,
        group_path,
        settings: ConnectionSettings {
            address: Some(address),
            port,
            username,
            key: None,
            ..Default::default()
        },
        key_note: Some(match key {
            Some(path) => KeyNote::File { path },
            None => KeyNote::NoIdentityFile,
        }),
        warnings,
        ..ImportCandidate::default()
    });
}

fn walk(dir: &Path, group_path: &[String], depth: usize, plan: &mut ImportPlan) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let file_name = entry.file_name().to_string_lossy().into_owned();
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() {
            let mut inner = group_path.to_vec();
            inner.push(clean(&file_name));
            walk(&path, &inner, depth + 1, plan);
            continue;
        }
        let Some(stem) = file_name.strip_suffix(".ini") else {
            continue;
        };
        if stem == "__FolderData__" || (stem == "Default" && group_path.is_empty()) {
            continue;
        }
        if meta.len() > MAX_FILE {
            plan.skipped.push(format!("{}: too large", clean(stem)));
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
        session(&text, clean(stem), group_path.to_vec(), plan);
    }
}

/// All sessions under `root` (SecureCRT's `Config\Sessions` folder).
pub fn parse_dir(root: &Path) -> ImportPlan {
    let mut plan = ImportPlan::default();
    walk(root, &[], 0, &mut plan);
    plan
}
