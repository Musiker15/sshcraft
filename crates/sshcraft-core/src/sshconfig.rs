//! Reads OpenSSH client config files (`~/.ssh/config`) to import hosts.
//!
//! Follows OpenSSH's rule that the first value found for a keyword wins, walking the blocks
//! from top to bottom. `Include` and `Match` are not followed and are reported instead.

use std::collections::HashMap;

use crate::model::{ConnectionSettings, KeyRef};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigHost {
    pub alias: String,
    pub hostname: Option<String>,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub identity_files: Vec<String>,
    /// `ProxyJump` and `ProxyCommand` lines as written; SSHCraft does not follow them yet.
    pub proxies: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedConfig {
    pub hosts: Vec<ConfigHost>,
    /// `Include` and `Match` lines that were not followed.
    pub skipped: Vec<String>,
}

pub use crate::import::{ImportCandidate, ImportWarning, KeyNote};

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
        proxies: Vec::new(),
    };
    let mut port_seen = false;
    // First proxy directive wins, including an explicit `none`.
    let mut proxy_seen = false;
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
                "proxyjump" | "proxycommand" if !proxy_seen => {
                    proxy_seen = true;
                    // `none` switches the proxy off, so there is nothing to warn about.
                    if !value.eq_ignore_ascii_case("none") {
                        let name = if keyword == "proxyjump" {
                            "ProxyJump"
                        } else {
                            "ProxyCommand"
                        };
                        host.proxies.push(format!("{name} {value}"));
                    }
                }
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

/// Expands `%h` (the alias) and `%%` in a `HostName`. Any other token is left as written and
/// reported, because SSHCraft cannot know its value at import time.
fn expand_tokens(hostname: &str, alias: &str) -> (String, Vec<ImportWarning>) {
    let mut out = String::with_capacity(hostname.len());
    let mut warnings = Vec::new();
    let mut chars = hostname.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('h') => out.push_str(alias),
            Some('%') => out.push('%'),
            Some(other) => {
                out.push('%');
                out.push(other);
                let token = format!("%{other}");
                if !warnings.contains(&ImportWarning::UnsupportedToken {
                    token: token.clone(),
                }) {
                    warnings.push(ImportWarning::UnsupportedToken { token });
                }
            }
            None => out.push('%'),
        }
    }
    (out, warnings)
}

/// `agent_fingerprints` is `None` when the agent could not be reached.
pub fn plan_import(
    hosts: &[ConfigHost],
    fingerprint_of: &dyn Fn(&str) -> Option<String>,
    agent_fingerprints: Option<&[String]>,
) -> Vec<ImportCandidate> {
    let address = |h: &ConfigHost| match &h.hostname {
        Some(name) => expand_tokens(name, &h.alias),
        None => (h.alias.clone(), Vec::new()),
    };
    let mut per_address: HashMap<String, usize> = HashMap::new();
    for h in hosts {
        *per_address.entry(address(h).0).or_default() += 1;
    }

    hosts
        .iter()
        .map(|h| {
            let (address, mut warnings) = address(h);
            warnings.splice(
                0..0,
                h.proxies
                    .iter()
                    .map(|directive| ImportWarning::ProxyIgnored {
                        directive: directive.clone(),
                    }),
            );
            let grouped = per_address[&address] >= 2;
            let key = h
                .identity_files
                .iter()
                .filter_map(|file| fingerprint_of(file))
                .find(|fp| agent_fingerprints.is_some_and(|known| known.contains(fp)))
                .map(|fingerprint| KeyRef::Agent { fingerprint });
            let key_note = match (&key, h.identity_files.is_empty(), agent_fingerprints) {
                (Some(_), _, _) => None,
                (None, true, _) => Some(KeyNote::NoIdentityFile),
                (None, false, None) => Some(KeyNote::AgentUnavailable),
                (None, false, Some(_)) => Some(KeyNote::NotInAgent),
            };
            ImportCandidate {
                name: h.alias.clone(),
                group_path: grouped.then(|| address.clone()).into_iter().collect(),
                group_defaults: ConnectionSettings {
                    address: grouped.then(|| address.clone()),
                    ..Default::default()
                },
                settings: ConnectionSettings {
                    address: (!grouped).then_some(address),
                    port: h.port,
                    username: h.user.clone(),
                    key,
                },
                key_note,
                warnings,
                ..ImportCandidate::default()
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "sshconfig_tests.rs"]
mod tests;
