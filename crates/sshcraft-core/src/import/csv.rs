//! Hosts from a CSV table: SSHCraft's own export, Termius-style tables and similar ones.

use super::{ImportCandidate, ImportPlan, clean, port_from};
use crate::model::{ConnectionSettings, HostOs};

/// The columns SSHCraft exports, in this order; the importer reads them back.
pub const CSV_HEADER: &str = "name,group,tags,address,port,username,key,os";

#[derive(Default)]
struct Columns {
    name: Option<usize>,
    group: Option<usize>,
    tags: Option<usize>,
    address: Option<usize>,
    port: Option<usize>,
    username: Option<usize>,
    os: Option<usize>,
    protocol: Option<usize>,
}

impl Columns {
    fn from_header(header: &::csv::StringRecord) -> Self {
        let mut columns = Columns::default();
        for (index, title) in header.iter().enumerate() {
            let title = title.trim().trim_start_matches('\u{feff}').to_lowercase();
            let slot = match title.as_str() {
                "name" | "label" | "alias" => &mut columns.name,
                "group" | "groups" | "folder" => &mut columns.group,
                "tags" | "tag" => &mut columns.tags,
                "address" | "hostname" | "hostname/ip" | "host" | "ip" => &mut columns.address,
                "port" => &mut columns.port,
                "username" | "user" | "login" => &mut columns.username,
                "os" => &mut columns.os,
                "protocol" => &mut columns.protocol,
                _ => continue,
            };
            slot.get_or_insert(index);
        }
        columns
    }
}

fn os_from(text: &str) -> Option<HostOs> {
    let text = text.trim().to_lowercase();
    (!text.is_empty())
        .then(|| serde_json::from_value(serde_json::Value::String(text)).ok())
        .flatten()
}

/// A cell the export guarded against spreadsheet formulas (`'=`, `'+`, `'-`, `'@`).
pub(crate) fn unguard(cell: &str) -> &str {
    match cell.strip_prefix('\'') {
        Some(rest) if rest.starts_with(['=', '+', '-', '@', '\t', '\r']) => rest,
        _ => cell,
    }
}

/// Guards a cell that a spreadsheet would run as a formula.
pub(crate) fn guard(cell: &str) -> String {
    if cell.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{cell}")
    } else {
        cell.to_owned()
    }
}

/// A group path written with `/` between groups; `\/` and `\\` stand for a slash or a
/// backslash inside a name.
pub(crate) fn split_path(path: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut chars = path.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            '/' => parts.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    parts.push(current);
    parts
        .iter()
        .map(|p| clean(p))
        .filter(|p| !p.is_empty())
        .collect()
}

pub(crate) fn join_path<'a>(names: impl Iterator<Item = &'a str>) -> String {
    names
        .map(|n| n.replace('\\', "\\\\").replace('/', "\\/"))
        .collect::<Vec<_>>()
        .join("/")
}

pub fn parse(text: &str) -> ImportPlan {
    let text = text.trim_start_matches('\u{feff}');
    let header_line = text.lines().next().unwrap_or_default();
    let delimiter = if header_line.matches(';').count() > header_line.matches(',').count() {
        b';'
    } else {
        b','
    };
    let mut reader = ::csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .flexible(true)
        .from_reader(text.as_bytes());
    let mut plan = ImportPlan::default();
    let columns = match reader.headers() {
        Ok(header) => Columns::from_header(header),
        Err(_) => {
            plan.skipped.push("header: unreadable".into());
            return plan;
        }
    };
    if columns.address.is_none() {
        plan.skipped.push("header: no address column".into());
        return plan;
    }
    for (index, record) in reader.records().enumerate() {
        // Line in the file: the header is line 1.
        let row = index + 2;
        let Ok(record) = record else {
            plan.skipped.push(format!("row {row}: unreadable"));
            continue;
        };
        let field = |column: Option<usize>| -> String {
            column
                .and_then(|c| record.get(c))
                .map(|v| clean(unguard(v)))
                .unwrap_or_default()
        };
        let protocol = field(columns.protocol);
        if !protocol.is_empty() && !protocol.eq_ignore_ascii_case("ssh") {
            plan.skipped.push(format!("row {row}: not SSH"));
            continue;
        }
        let address = field(columns.address);
        if address.is_empty() {
            plan.skipped.push(format!("row {row}: no address"));
            continue;
        }
        let mut warnings = Vec::new();
        let port = columns
            .port
            .and_then(|c| record.get(c))
            .and_then(|p| port_from(p, &mut warnings));
        let name = Some(field(columns.name))
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| address.clone());
        let username = Some(field(columns.username)).filter(|u| !u.is_empty());
        let group_path = columns
            .group
            .and_then(|c| record.get(c))
            .map(|g| split_path(unguard(g)))
            .unwrap_or_default();
        let tags = columns
            .tags
            .and_then(|c| record.get(c))
            .map(|t| {
                t.split([';', ','])
                    .map(clean)
                    .filter(|p| !p.is_empty())
                    .collect()
            })
            .unwrap_or_default();
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
            tags,
            os: os_from(&field(columns.os)),
            warnings,
            ..ImportCandidate::default()
        });
    }
    if plan.candidates.is_empty() && plan.skipped.is_empty() {
        plan.skipped.push("no rows".into());
    }
    plan
}
