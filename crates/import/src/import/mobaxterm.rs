//! MobaXterm bookmarks from `MobaXterm.ini`. Stored passwords are never read.

use super::{ImportCandidate, ImportPlan, clean, port_from};
use sshcraft_model::model::ConnectionSettings;

/// Session type of an SSH bookmark (`#<icon>#<type>%…`).
const SSH: u32 = 0;

pub fn parse(bytes: &[u8]) -> ImportPlan {
    let text = String::from_utf8_lossy(bytes);
    let mut plan = ImportPlan::default();
    let mut in_bookmarks = false;
    let mut folder: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(section) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            in_bookmarks = section == "Bookmarks" || section.starts_with("Bookmarks_");
            folder.clear();
            if section == "Passwords" {
                plan.skipped
                    .push("stored passwords are not imported".into());
            }
            continue;
        }
        if !in_bookmarks {
            continue;
        }
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        match name {
            "SubRep" => {
                folder = value
                    .split('\\')
                    .map(clean)
                    .filter(|p| !p.is_empty())
                    .collect();
                continue;
            }
            "ImgNum" => continue,
            _ => {}
        }
        let name = clean(name);
        let Some(session) = value.strip_prefix('#') else {
            continue;
        };
        // `<icon>#<type>%host%port%user%…`; the part after `#` up to the next `#` is settings.
        let settings = session.split('#').nth(1).unwrap_or_default();
        let fields: Vec<&str> = settings.split('%').collect();
        let kind = fields.first().and_then(|t| t.trim().parse::<u32>().ok());
        if kind != Some(SSH) {
            plan.skipped.push(format!("{name}: not SSH"));
            continue;
        }
        if fields.len() < 4 || clean(fields[1]).is_empty() {
            plan.skipped.push(format!("{name}: incomplete"));
            continue;
        }
        let mut warnings = Vec::new();
        let port = port_from(fields[2], &mut warnings)
            .filter(|p| *p != sshcraft_model::model::DEFAULT_PORT);
        let username = Some(clean(fields[3])).filter(|u| !u.is_empty());
        plan.candidates.push(ImportCandidate {
            name,
            group_path: folder.clone(),
            settings: ConnectionSettings {
                address: Some(clean(fields[1])),
                port,
                username,
                key: None,
                ..Default::default()
            },
            warnings,
            ..ImportCandidate::default()
        });
    }
    plan
}
