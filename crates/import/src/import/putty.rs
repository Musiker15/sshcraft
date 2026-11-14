//! PuTTY sessions: from a `.reg` export here; the app reads the Windows registry itself and
//! hands the same [`Session`]s to [`candidates`].

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{ImportCandidate, ImportPlan, KeyNote, clean, port_from};
use sshcraft_model::model::ConnectionSettings;

/// Where PuTTY keeps its sessions under `HKEY_CURRENT_USER`.
pub const SESSIONS_KEY: &str = r"Software\SimonTatham\PuTTY\Sessions";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PuttyValue {
    Text(String),
    Number(u32),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub name: String,
    pub values: BTreeMap<String, PuttyValue>,
}

/// A `.reg` file as text: UTF-16 LE with byte order mark (what regedit writes) or UTF-8.
fn decode(bytes: &[u8]) -> String {
    match bytes {
        [0xff, 0xfe, rest @ ..] => {
            let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
            String::from_utf16_lossy(&units)
        }
        [0xef, 0xbb, 0xbf, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// PuTTY stores session names URL-encoded (`%20` for a space).
pub fn decode_name(name: &str) -> String {
    let bytes = name.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        // Only `%` followed by two ASCII hex digits is an escape; anything else stays.
        if bytes[i] == b'%' && i + 2 < bytes.len() && bytes[i + 1].is_ascii_hexdigit() && bytes[i + 2].is_ascii_hexdigit() {
            let hex = |b: u8| (b as char).to_digit(16).unwrap_or(0) as u8;
            out.push(hex(bytes[i + 1]) * 16 + hex(bytes[i + 2]));
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A quoted `.reg` string: `\\` and `\"` are escapes.
fn unquote(text: &str) -> Option<String> {
    let inner = text.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            out.push(chars.next()?);
        } else {
            out.push(c);
        }
    }
    Some(out)
}

pub fn parse_reg(bytes: &[u8]) -> Vec<Session> {
    let text = decode(bytes);
    let prefix = format!(r"[HKEY_CURRENT_USER\{SESSIONS_KEY}\");
    let mut sessions: Vec<Session> = Vec::new();
    let mut current: Option<Session> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            if let Some(done) = current.take() {
                sessions.push(done);
            }
            current = line
                .strip_prefix(&prefix)
                .and_then(|rest| rest.strip_suffix(']'))
                .filter(|name| !name.is_empty() && !name.contains('\\'))
                .map(|name| Session { name: decode_name(name), values: BTreeMap::new() });
            continue;
        }
        let Some(session) = current.as_mut() else {
            continue;
        };
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let Some(key) = unquote(key) else {
            continue;
        };
        let value = if let Some(hex) = value.strip_prefix("dword:") {
            match u32::from_str_radix(hex.trim(), 16) {
                Ok(n) => PuttyValue::Number(n),
                Err(_) => continue,
            }
        } else if let Some(text) = unquote(value) {
            PuttyValue::Text(text)
        } else {
            continue;
        };
        session.values.insert(key, value);
    }
    if let Some(done) = current {
        sessions.push(done);
    }
    sessions
}

fn text<'a>(session: &'a Session, key: &str) -> &'a str {
    match session.values.get(key) {
        Some(PuttyValue::Text(t)) => t,
        _ => "",
    }
}

pub fn candidates(sessions: &[Session]) -> ImportPlan {
    let mut plan = ImportPlan::default();
    for session in sessions {
        if session.name == "Default Settings" {
            continue;
        }
        let protocol = text(session, "Protocol");
        if !protocol.is_empty() && protocol != "ssh" {
            plan.skipped.push(format!("{}: not SSH", clean(&session.name)));
            continue;
        }
        let host = text(session, "HostName").trim();
        let (user_in_host, address) = match host.rsplit_once('@') {
            Some((user, address)) => (Some(user.to_owned()), address.to_owned()),
            None => (None, host.to_owned()),
        };
        let address = clean(&address);
        if address.is_empty() {
            plan.skipped.push(format!("{}: no host name", clean(&session.name)));
            continue;
        }
        let mut warnings = Vec::new();
        let port = match session.values.get("PortNumber") {
            Some(PuttyValue::Number(n)) => port_from(&n.to_string(), &mut warnings),
            Some(PuttyValue::Text(t)) => port_from(t, &mut warnings),
            None => None,
        };
        let username = Some(clean(text(session, "UserName"))).filter(|u| !u.is_empty()).or(user_in_host.map(|u| clean(&u)).filter(|u| !u.is_empty()));
        let key_file = text(session, "PublicKeyFile").trim();
        plan.candidates.push(ImportCandidate {
            name: clean(&session.name),
            settings: ConnectionSettings {
                address: Some(address),
                port: port.filter(|p| *p != sshcraft_model::model::DEFAULT_PORT),
                username,
                key: None,
                ..Default::default()
            },
            key_note: Some(if key_file.is_empty() { KeyNote::NoIdentityFile } else { KeyNote::File { path: key_file.to_owned() } }),
            warnings,
            ..ImportCandidate::default()
        });
    }
    plan
}
