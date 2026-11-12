//! Reads OpenSSH client config files (`~/.ssh/config`) to import hosts.
//!
//! Follows OpenSSH's rule that the first value found for a keyword wins, walking the blocks
//! from top to bottom. `Include` and `Match` are not followed and are reported instead.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::model::{ConnectionSettings, Group, GroupId, Host, HostId, KeyRef};
use crate::store::{Store, StoreError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigHost {
    pub alias: String,
    pub hostname: Option<String>,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub identity_files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedConfig {
    pub hosts: Vec<ConfigHost>,
    /// `Include` and `Match` lines that were not followed.
    pub skipped: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportCandidate {
    pub alias: String,
    pub group: Option<String>,
    pub settings: ConnectionSettings,
    pub key_note: Option<String>,
}

struct Block {
    patterns: Vec<String>,
    /// `Match` blocks are skipped, together with everything inside them.
    skip: bool,
    entries: Vec<(String, String)>,
}

pub fn parse(text: &str) -> ParsedConfig {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    // Lines before the first `Host` apply to every host.
    let mut blocks = vec![Block {
        patterns: vec!["*".to_owned()],
        skip: false,
        entries: Vec::new(),
    }];
    let mut skipped = Vec::new();

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (keyword, value) = split_keyword(line);
        if keyword.is_empty() {
            continue;
        }
        match keyword.as_str() {
            "host" => blocks.push(Block {
                patterns: split_args(value),
                skip: false,
                entries: Vec::new(),
            }),
            "match" => {
                skipped.push(line.to_owned());
                blocks.push(Block {
                    patterns: Vec::new(),
                    skip: true,
                    entries: Vec::new(),
                });
            }
            "include" => skipped.push(line.to_owned()),
            _ => {
                let block = blocks.last_mut().expect("there is always a block");
                if !block.skip {
                    block.entries.push((keyword, unquote(value)));
                }
            }
        }
    }

    let mut aliases: Vec<String> = Vec::new();
    for block in &blocks[1..] {
        for pattern in &block.patterns {
            let concrete = !pattern.contains(['*', '?']) && !pattern.starts_with('!');
            if concrete && !aliases.contains(pattern) {
                aliases.push(pattern.clone());
            }
        }
    }

    let hosts = aliases
        .into_iter()
        .map(|alias| collect(&blocks, alias))
        .collect();
    ParsedConfig { hosts, skipped }
}

fn collect(blocks: &[Block], alias: String) -> ConfigHost {
    let mut host = ConfigHost {
        alias,
        hostname: None,
        port: None,
        user: None,
        identity_files: Vec::new(),
    };
    let mut port_seen = false;
    for block in blocks.iter().filter(|b| !b.skip && applies(b, &host.alias)) {
        for (keyword, value) in &block.entries {
            if value.is_empty() {
                continue;
            }
            match keyword.as_str() {
                "hostname" if host.hostname.is_none() => host.hostname = Some(value.clone()),
                "user" if host.user.is_none() => host.user = Some(value.clone()),
                "port" if !port_seen => {
                    port_seen = true;
                    host.port = value.parse().ok().filter(|p| *p != 0);
                }
                "identityfile" => host.identity_files.push(value.clone()),
                _ => {}
            }
        }
    }
    host
}

fn applies(block: &Block, alias: &str) -> bool {
    let mut matched = false;
    for pattern in &block.patterns {
        if let Some(negated) = pattern.strip_prefix('!') {
            if glob(negated, alias) {
                return false;
            }
        } else if glob(pattern, alias) {
            matched = true;
        }
    }
    matched
}

/// Case-insensitive match with `*` (any run) and `?` (one character).
fn glob(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

/// Splits `Keyword value`, `Keyword=value` and `Keyword = value`; the keyword is lowercased.
fn split_keyword(line: &str) -> (String, &str) {
    let end = line
        .find(|c: char| c.is_whitespace() || c == '=')
        .unwrap_or(line.len());
    let keyword = line[..end].to_lowercase();
    let rest = line[end..].trim_start();
    let rest = rest.strip_prefix('=').unwrap_or(rest).trim();
    (keyword, rest)
}

/// Splits whitespace-separated arguments, keeping quoted parts together.
fn split_args(value: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in value.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    let value = value.strip_prefix('"').unwrap_or(value);
    value.strip_suffix('"').unwrap_or(value).to_owned()
}

pub fn plan_import(
    hosts: &[ConfigHost],
    fingerprint_of: &dyn Fn(&str) -> Option<String>,
    agent_fingerprints: &[String],
) -> Vec<ImportCandidate> {
    let address = |h: &ConfigHost| h.hostname.clone().unwrap_or_else(|| h.alias.clone());
    let mut per_address: HashMap<String, usize> = HashMap::new();
    for h in hosts {
        *per_address.entry(address(h)).or_default() += 1;
    }

    hosts
        .iter()
        .map(|h| {
            let address = address(h);
            let grouped = per_address[&address] >= 2;
            let key = h
                .identity_files
                .iter()
                .filter_map(|file| fingerprint_of(file))
                .find(|fp| agent_fingerprints.contains(fp))
                .map(|fingerprint| KeyRef::Agent { fingerprint });
            let key_note = match (&key, h.identity_files.is_empty()) {
                (Some(_), _) => None,
                (None, true) => Some("Kein Schlüssel in der ssh-config, bitte zuordnen".to_owned()),
                (None, false) => Some("Schlüssel nicht in Proton Pass, bitte zuordnen".to_owned()),
            };
            ImportCandidate {
                alias: h.alias.clone(),
                group: grouped.then(|| address.clone()),
                settings: ConnectionSettings {
                    address: (!grouped).then_some(address),
                    port: h.port,
                    username: h.user.clone(),
                    key,
                },
                key_note,
            }
        })
        .collect()
}

/// Creates exactly the given candidates (as confirmed in the preview) and the groups they
/// name. A host that already exists with the same name and address is skipped; a group with
/// the same name is reused. Returns the number of hosts created.
pub fn apply_import(store: &Store, candidates: &[ImportCandidate]) -> Result<usize, StoreError> {
    let mut groups = store.list_groups()?;
    let hosts = store.list_hosts()?;
    let mut created = 0;

    for candidate in candidates {
        let group_id = match &candidate.group {
            Some(name) => Some(match groups.iter().find(|g| &g.name == name) {
                Some(g) => g.id,
                None => {
                    let group = Group {
                        id: GroupId::new(),
                        name: name.clone(),
                        defaults: ConnectionSettings {
                            address: Some(name.clone()),
                            ..Default::default()
                        },
                    };
                    store.upsert_group(&group)?;
                    let id = group.id;
                    groups.push(group);
                    id
                }
            }),
            None => None,
        };
        let address_of = |h: &Host| {
            h.settings.address.clone().or_else(|| {
                h.group_id
                    .and_then(|gid| groups.iter().find(|g| g.id == gid))
                    .and_then(|g| g.defaults.address.clone())
            })
        };
        let new_address = candidate
            .settings
            .address
            .clone()
            .or_else(|| candidate.group.clone());
        let exists = hosts
            .iter()
            .any(|h| h.name == candidate.alias && address_of(h) == new_address);
        if exists {
            continue;
        }
        store.upsert_host(&Host {
            id: HostId::new(),
            name: candidate.alias.clone(),
            group_id,
            settings: candidate.settings.clone(),
        })?;
        created += 1;
    }
    Ok(created)
}

#[cfg(test)]
#[path = "sshconfig_tests.rs"]
mod tests;
