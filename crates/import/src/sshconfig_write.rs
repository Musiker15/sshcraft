//! Writes a host back into an OpenSSH client config, changing as little as possible: the
//! lines SSHCraft manages (`HostName`, `Port`, `User`, `ProxyJump`, `ProxyCommand`,
//! `ForwardAgent`) in the host's own block are replaced, every other line of the file stays as
//! it was, comments and order included.

use crate::sshconfig::{split_args, split_keyword};

/// The keywords SSHCraft writes; any other line of a block is kept.
const MANAGED: [&str; 6] = ["hostname", "port", "user", "proxyjump", "proxycommand", "forwardagent"];

/// A host as it goes into the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteHost {
    pub alias: String,
    pub hostname: Option<String>,
    pub port: Option<u16>,
    pub user: Option<String>,
    /// Aliases of the jump hosts, in order.
    pub jump: Vec<String>,
    pub proxy_command: Option<Vec<String>>,
    pub forward_agent: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WriteError {
    #[error("'{0}' cannot be a host name in ~/.ssh/config (no spaces, no * ? ! or quotes)")]
    BadAlias(String),
    #[error("a value would break ~/.ssh/config: {0}")]
    BadValue(String),
}

fn check_alias(alias: &str) -> Result<(), WriteError> {
    let bad = alias.is_empty() || alias.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, '*' | '?' | '!' | '"' | ',' | '#'));
    if bad { Err(WriteError::BadAlias(alias.to_owned())) } else { Ok(()) }
}

/// A value as ssh_config reads it: quoted when it has blanks; never a line break or a quote.
fn value(text: &str) -> Result<String, WriteError> {
    if text.is_empty() || text.chars().any(|c| c.is_control() || c == '"') {
        return Err(WriteError::BadValue(text.escape_default().to_string()));
    }
    Ok(if text.contains(char::is_whitespace) { format!("\"{text}\"") } else { text.to_owned() })
}

/// The managed lines of a host, in a fixed order.
fn entries(host: &WriteHost) -> Result<Vec<(&'static str, String)>, WriteError> {
    let mut out = Vec::new();
    if let Some(h) = &host.hostname {
        out.push(("HostName", value(h)?));
    }
    if let Some(p) = host.port {
        out.push(("Port", p.to_string()));
    }
    if let Some(u) = &host.user {
        out.push(("User", value(u)?));
    }
    if !host.jump.is_empty() {
        for hop in &host.jump {
            check_alias(hop)?;
        }
        out.push(("ProxyJump", host.jump.join(",")));
    }
    if let Some(argv) = &host.proxy_command {
        if argv.is_empty() || argv.iter().any(|a| a.chars().any(char::is_control)) {
            return Err(WriteError::BadValue("proxy command".into()));
        }
        out.push(("ProxyCommand", sshcraft_model::proxy_command::join(argv)));
    }
    if let Some(on) = host.forward_agent {
        out.push(("ForwardAgent", if on { "yes" } else { "no" }.to_owned()));
    }
    Ok(out)
}

fn keyword_of(line: &str) -> String {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return String::new();
    }
    split_keyword(trimmed).0
}

/// `text` with `host` written into it: its own block (`Host <alias>` alone) updated in place,
/// or a new block before the first `Host` or `Match` line, so it wins over `Host *` and the
/// lines for every host stay before it.
pub fn write_host(text: &str, host: &WriteHost) -> Result<String, WriteError> {
    check_alias(&host.alias)?;
    let managed = entries(host)?;
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let starts_block = |line: &str| matches!(keyword_of(line).as_str(), "host" | "match");
    let own =
        lines.iter().position(|line| keyword_of(line) == "host" && split_args(split_keyword(line.trim()).1) == std::slice::from_ref(&host.alias));
    let mut out = String::with_capacity(text.len() + 256);
    match own {
        Some(start) => {
            let end = lines.iter().skip(start + 1).position(|l| starts_block(l)).map_or(lines.len(), |i| start + 1 + i);
            let body = lines.get(start + 1..end).unwrap_or_default();
            let indent: String = body
                .iter()
                .find(|l| !keyword_of(l).is_empty())
                .map(|l| l.chars().take_while(|c| *c == ' ' || *c == '\t').collect())
                .unwrap_or_else(|| "    ".to_owned());
            for line in lines.get(..=start).unwrap_or_default() {
                out.push_str(line);
            }
            if !out.ends_with('\n') {
                out.push_str(newline);
            }
            for (k, v) in &managed {
                out.push_str(&format!("{indent}{k} {v}{newline}"));
            }
            for line in body.iter().filter(|l| !MANAGED.contains(&keyword_of(l).as_str())) {
                out.push_str(line);
            }
            for line in lines.get(end..).unwrap_or_default() {
                out.push_str(line);
            }
        }
        None => {
            let mut block = format!("Host {}{newline}", host.alias);
            for (k, v) in &managed {
                block.push_str(&format!("    {k} {v}{newline}"));
            }
            match lines.iter().position(|l| starts_block(l)) {
                Some(first) => {
                    for line in lines.get(..first).unwrap_or_default() {
                        out.push_str(line);
                    }
                    out.push_str(&block);
                    out.push_str(newline);
                    for line in lines.get(first..).unwrap_or_default() {
                        out.push_str(line);
                    }
                }
                None => {
                    out.push_str(text);
                    if !text.is_empty() {
                        if !text.ends_with('\n') {
                            out.push_str(newline);
                        }
                        out.push_str(newline);
                    }
                    out.push_str(&block);
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
#[path = "sshconfig_write_tests.rs"]
mod tests;
