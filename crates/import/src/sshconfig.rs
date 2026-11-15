//! Reads OpenSSH client config files (`~/.ssh/config`) to import hosts.
//!
//! Follows OpenSSH's rule that the first value found for a keyword wins, walking the blocks
//! from top to bottom. `Include` is followed by [`parse_tree`]; `Match` is not followed and is
//! reported instead.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use sshcraft_model::model::{ConnectionSettings, EnvVar, KeyRef};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigHost {
    pub alias: String,
    pub hostname: Option<String>,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub identity_files: Vec<String>,
    /// `ProxyJump` and `ProxyCommand` lines as written, for the warning when they cannot be
    /// followed.
    pub proxies: Vec<String>,
    /// The hops of `ProxyJump`, in order.
    pub jump: Vec<String>,
    /// `ForwardAgent yes` or `no`.
    pub forward_agent: Option<bool>,
    /// The variables of `SetEnv`.
    pub set_env: Vec<EnvVar>,
    /// `ProxyCommand` as arguments, when it needs no shell.
    pub proxy_command: Option<Vec<String>>,
    /// The file of the first `Host` line naming this alias (only with [`parse_tree`]).
    pub file: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedConfig {
    pub hosts: Vec<ConfigHost>,
    /// `Include` and `Match` lines that were not followed, and files that could not be read.
    pub skipped: Vec<String>,
    /// The files that were read, so a change to any of them can be noticed.
    pub files: Vec<PathBuf>,
}

pub use crate::import::{ImportCandidate, ImportWarning, KeyNote};

struct Block {
    /// Index into the files read; the block came from that file.
    file: usize,
    patterns: Vec<String>,
    /// `Match` blocks are skipped, together with everything inside them.
    skip: bool,
    entries: Vec<(String, String)>,
}

/// How deep `Include` may nest (as in OpenSSH).
pub const MAX_INCLUDE_DEPTH: usize = 16;
/// The most files read for one configuration.
pub const MAX_CONFIG_FILES: usize = 256;
/// The largest file read.
pub const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

/// Where configuration files come from: the disk, or memory in tests.
pub trait ConfigFs {
    fn read(&self, path: &Path) -> Option<String>;
    /// The files matching a pattern with `*` or `?` in its last part, sorted.
    fn glob(&self, pattern: &Path) -> Vec<PathBuf>;
}

/// The files on disk.
pub struct RealFs;

impl ConfigFs for RealFs {
    fn read(&self, path: &Path) -> Option<String> {
        use std::io::Read;
        let file = std::fs::File::open(path).ok()?;
        if file.metadata().ok()?.len() > MAX_CONFIG_BYTES {
            return None;
        }
        let mut text = String::new();
        file.take(MAX_CONFIG_BYTES).read_to_string(&mut text).ok()?;
        Some(text)
    }

    fn glob(&self, pattern: &Path) -> Vec<PathBuf> {
        let (Some(dir), Some(name)) = (pattern.parent(), pattern.file_name().and_then(|n| n.to_str())) else {
            return Vec::new();
        };
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut found: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            // As in glob(3): `*` does not match a leading dot (backups and temporary files).
            .filter(|e| e.file_name().to_str().is_some_and(|n| (name.starts_with('.') || !n.starts_with('.')) && glob_matches(name, n)))
            .map(|e| e.path())
            .collect();
        found.sort();
        found
    }
}

/// Whether `text` matches a pattern with `*` and `?` (as in `Host` lines and `Include`).
pub fn glob_matches(pattern: &str, text: &str) -> bool {
    glob(pattern, text)
}

/// What the reading has gathered so far.
struct Reading {
    blocks: Vec<Block>,
    skipped: Vec<String>,
    files: Vec<PathBuf>,
}

impl Reading {
    fn new() -> Self {
        // Lines before the first `Host` apply to every host.
        Self {
            blocks: vec![Block { file: 0, patterns: vec!["*".to_owned()], skip: false, entries: Vec::new() }],
            skipped: Vec::new(),
            files: Vec::new(),
        }
    }

    /// Reads one file's lines; `include` handles an `Include` line's arguments.
    fn lines(&mut self, text: &str, include: &mut dyn FnMut(&mut Self, &str, &str)) {
        let file = self.files.len().saturating_sub(1);
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
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
                "host" => self.blocks.push(Block { file, patterns: split_args(value), skip: false, entries: Vec::new() }),
                "match" => {
                    self.skipped.push(line.to_owned());
                    self.blocks.push(Block { file, patterns: Vec::new(), skip: true, entries: Vec::new() });
                }
                // An Include inside a skipped Match block is skipped with it.
                "include" if self.blocks.last().is_some_and(|b| b.skip) => {}
                "include" => include(self, line, value),
                _ => {
                    // There is always a block (the one before the first Host line).
                    let Some(block) = self.blocks.last_mut() else {
                        continue;
                    };
                    if !block.skip {
                        block.entries.push((keyword, unquote(value)));
                    }
                }
            }
        }
    }

    fn finish(self) -> ParsedConfig {
        let mut aliases: Vec<String> = Vec::new();
        for block in self.blocks.iter().skip(1) {
            for pattern in &block.patterns {
                let concrete = !pattern.contains(['*', '?']) && !pattern.starts_with('!');
                if concrete && !aliases.contains(pattern) {
                    aliases.push(pattern.clone());
                }
            }
        }
        let hosts = aliases
            .into_iter()
            .map(|alias| {
                let mut host = collect(&self.blocks, alias);
                host.file = self.blocks.iter().skip(1).find(|b| b.patterns.contains(&host.alias)).and_then(|b| self.files.get(b.file)).cloned();
                host
            })
            .collect();
        ParsedConfig { hosts, skipped: self.skipped, files: self.files }
    }
}

/// Reads one configuration text; `Include` lines are not followed and are reported.
pub fn parse(text: &str) -> ParsedConfig {
    let mut reading = Reading::new();
    reading.lines(text, &mut |reading, line, _| reading.skipped.push(line.to_owned()));
    reading.finish()
}

/// Reads `root` (the user's `~/.ssh/config`) and the files it includes, as OpenSSH does: an
/// `Include` stands for the included lines where it is, relative paths are in `~/.ssh`,
/// patterns match in sorted order. Nesting, the number of files and their size are bounded.
pub fn parse_tree(fs: &dyn ConfigFs, root: &Path, home: &Path) -> ParsedConfig {
    let mut reading = Reading::new();
    read_file(fs, root, home, 0, &mut reading);
    reading.finish()
}

fn read_file(fs: &dyn ConfigFs, path: &Path, home: &Path, depth: usize, reading: &mut Reading) {
    if reading.files.len() >= MAX_CONFIG_FILES {
        if !reading.skipped.iter().any(|s| s.starts_with("too many")) {
            reading.skipped.push(format!("too many included files, stopped at {MAX_CONFIG_FILES}"));
        }
        return;
    }
    let Some(text) = fs.read(path) else {
        reading.skipped.push(format!("cannot read {}", path.display()));
        return;
    };
    reading.files.push(path.to_path_buf());
    reading.lines(&text, &mut |reading, line, value| {
        if depth >= MAX_INCLUDE_DEPTH {
            reading.skipped.push(format!("{line}: includes nest too deep"));
            return;
        }
        for arg in split_args(value) {
            let target = if let Some(rest) = arg.strip_prefix("~/") {
                home.join(rest)
            } else if Path::new(&arg).is_absolute() {
                PathBuf::from(&arg)
            } else {
                home.join(".ssh").join(&arg)
            };
            let paths = if arg.contains(['*', '?']) { fs.glob(&target) } else { vec![target] };
            for p in paths {
                read_file(fs, &p, home, depth + 1, reading);
            }
        }
    });
}

fn collect(blocks: &[Block], alias: String) -> ConfigHost {
    let mut host = ConfigHost {
        alias,
        hostname: None,
        port: None,
        user: None,
        identity_files: Vec::new(),
        proxies: Vec::new(),
        jump: Vec::new(),
        forward_agent: None,
        set_env: Vec::new(),
        proxy_command: None,
        file: None,
    };
    let mut env_seen = false;
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
                            host.jump = value.split(',').map(str::trim).filter(|hop| !hop.is_empty()).map(str::to_owned).collect();
                            "ProxyJump"
                        } else {
                            host.proxy_command = sshcraft_model::proxy_command::split(value).ok();
                            "ProxyCommand"
                        };
                        host.proxies.push(format!("{name} {value}"));
                    }
                }
                "forwardagent" if host.forward_agent.is_none() => {
                    host.forward_agent = match value.to_ascii_lowercase().as_str() {
                        "yes" => Some(true),
                        "no" => Some(false),
                        _ => None,
                    };
                }
                "setenv" if !env_seen => {
                    env_seen = true;
                    host.set_env = parse_set_env(value);
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
pub(crate) fn split_keyword(line: &str) -> (String, &str) {
    let end = line.find(|c: char| c.is_whitespace() || c == '=').unwrap_or(line.len());
    let keyword = line[..end].to_lowercase();
    let rest = line[end..].trim_start();
    let rest = rest.strip_prefix('=').unwrap_or(rest).trim();
    (keyword, rest)
}

/// Splits whitespace-separated arguments, keeping quoted parts together.
pub(crate) fn split_args(value: &str) -> Vec<String> {
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
                if !warnings.contains(&ImportWarning::UnsupportedToken { token: token.clone() }) {
                    warnings.push(ImportWarning::UnsupportedToken { token });
                }
            }
            None => out.push('%'),
        }
    }
    (out, warnings)
}

/// `SetEnv A=1 B="x y"`: words split at blanks outside double quotes, each `NAME=value`.
fn parse_set_env(value: &str) -> Vec<EnvVar> {
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut started = false;
    for c in value.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            c if c.is_whitespace() && !quoted => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            c => {
                word.push(c);
                started = true;
            }
        }
    }
    if started {
        words.push(word);
    }
    let vars: Vec<EnvVar> = words
        .into_iter()
        .filter_map(|w| {
            let (name, value) = w.split_once('=')?;
            Some(EnvVar { name: name.to_owned(), value: value.to_owned() })
        })
        .collect();
    sshcraft_model::model::normalize_env(&vars)
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
    let aliases: Vec<&str> = hosts.iter().map(|h| h.alias.as_str()).collect();
    let mut per_address: HashMap<String, usize> = HashMap::new();
    for h in hosts {
        *per_address.entry(address(h).0).or_default() += 1;
    }

    hosts
        .iter()
        .map(|h| {
            let (address, mut warnings) = address(h);
            // A ProxyJump through hosts of this same file becomes jump hosts; anything else
            // (user@host:port, a host not in the file, ProxyCommand) stays a warning.
            let jumps_known = !h.jump.is_empty() && h.jump.iter().all(|hop| aliases.contains(&hop.as_str()));
            warnings.splice(
                0..0,
                h.proxies
                    .iter()
                    .filter(|directive| !(jumps_known && directive.starts_with("ProxyJump ")))
                    .filter(|directive| !(h.proxy_command.is_some() && directive.starts_with("ProxyCommand ")))
                    .map(|directive| ImportWarning::ProxyIgnored { directive: directive.clone() }),
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
                group_defaults: ConnectionSettings { address: grouped.then(|| address.clone()), ..Default::default() },
                settings: ConnectionSettings {
                    address: (!grouped).then_some(address),
                    port: h.port,
                    username: h.user.clone(),
                    key,
                    agent_forwarding: h.forward_agent,
                    env: h.set_env.clone(),
                    proxy: h.proxy_command.clone().map(|argv| sshcraft_model::model::Proxy::Command { argv }),
                    ..Default::default()
                },
                jump_aliases: if jumps_known { h.jump.clone() } else { Vec::new() },
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
