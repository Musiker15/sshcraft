//! Where to connect from text: an `ssh://[user@]host[:port]` link (RFC 3986 with the ssh URI
//! scheme) or what the user types into quick connect (`user@host:port`, `host`, `[::1]:22`).
//! Both are foreign input: a link can come from any web page. A user or host that starts with
//! `-` (an option to some program), contains control characters or whitespace, or a password in
//! the link is refused, never cleaned up.

use serde::{Deserialize, Serialize};

/// The longest user name.
pub const MAX_USER: usize = 64;
/// The longest host name.
pub const MAX_HOST: usize = 253;
/// The longest link or typed text.
pub const MAX_TEXT: usize = 1024;

/// A host to connect to, read from a link or typed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    pub user: Option<String>,
    /// A name, an IPv4 address or an IPv6 address without brackets.
    pub host: String,
    pub port: Option<u16>,
}

impl Target {
    /// The name a host saved from it gets: `user@host`, with `:port` when one was given.
    pub fn name(&self) -> String {
        let host = if self.host.contains(':') { format!("[{}]", self.host) } else { self.host.clone() };
        let at = self.user.as_ref().map(|u| format!("{u}@")).unwrap_or_default();
        match self.port {
            Some(port) => format!("{at}{host}:{port}"),
            None => format!("{at}{host}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TargetError {
    #[error("not an ssh:// link")]
    NotSsh,
    #[error("nothing to connect to")]
    Empty,
    #[error("the text is too long")]
    TooLong,
    #[error("a link carries no password")]
    Password,
    #[error("the user name is not valid")]
    User,
    #[error("the host is not valid")]
    Host,
    #[error("the port must be 1 to 65535")]
    Port,
    #[error("the link has a path, query or fragment")]
    Extra,
}

/// Reads an `ssh://` link. Percent escapes are decoded; a trailing `/` is allowed.
pub fn parse_url(url: &str) -> Result<Target, TargetError> {
    let url = url.trim();
    if url.len() > MAX_TEXT {
        return Err(TargetError::TooLong);
    }
    let rest = match url.get(..6) {
        Some(scheme) if scheme.eq_ignore_ascii_case("ssh://") => url.get(6..).unwrap_or_default(),
        _ => return Err(TargetError::NotSsh),
    };
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    if rest.contains(['/', '?', '#']) {
        return Err(TargetError::Extra);
    }
    authority(rest, true)
}

/// Reads what the user typed: `user@host:port`, `host`, `[v6]:port`, or an `ssh://` link.
pub fn parse_quick(text: &str) -> Result<Target, TargetError> {
    let text = text.trim();
    if text.len() > MAX_TEXT {
        return Err(TargetError::TooLong);
    }
    if text.get(..6).is_some_and(|s| s.eq_ignore_ascii_case("ssh://")) {
        return parse_url(text);
    }
    authority(text, false)
}

fn authority(text: &str, escaped: bool) -> Result<Target, TargetError> {
    if text.is_empty() {
        return Err(TargetError::Empty);
    }
    let (user, hostport) = match text.rsplit_once('@') {
        Some((user, hostport)) => (Some(user), hostport),
        None => (None, text),
    };
    let user = match user {
        Some(info) => {
            // `;fingerprint=…` and other parameters of the ssh URI scheme are not used.
            let info = info.split(';').next().unwrap_or_default();
            if info.contains(':') {
                return Err(TargetError::Password);
            }
            let user = if escaped { decode(info).ok_or(TargetError::User)? } else { info.to_string() };
            Some(check_user(user)?)
        }
        None => None,
    };
    let (host, port) = host_port(hostport, escaped)?;
    Ok(Target { user, host, port })
}

fn host_port(text: &str, escaped: bool) -> Result<(String, Option<u16>), TargetError> {
    let (host, port) = if let Some(inner) = text.strip_prefix('[') {
        let (address, after) = inner.split_once(']').ok_or(TargetError::Host)?;
        if address.parse::<std::net::Ipv6Addr>().is_err() {
            return Err(TargetError::Host);
        }
        let port = match after {
            "" => None,
            _ => Some(after.strip_prefix(':').ok_or(TargetError::Host)?),
        };
        (address.to_string(), port)
    } else {
        let (host, port) = match text.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (text, None),
        };
        let host = if escaped { decode(host).ok_or(TargetError::Host)? } else { host.to_string() };
        (check_host(host)?, port)
    };
    let port = match port {
        None => None,
        Some(p) if !p.is_empty() && p.len() <= 5 && p.bytes().all(|b| b.is_ascii_digit()) => {
            Some(p.parse::<u16>().ok().filter(|p| *p != 0).ok_or(TargetError::Port)?)
        }
        Some(_) => return Err(TargetError::Port),
    };
    Ok((host, port))
}

/// Characters that change how text reads without being seen: bidi controls, zero-width
/// characters, the soft hyphen, the byte order mark.
fn invisible(c: char) -> bool {
    matches!(c, '\u{00AD}' | '\u{061C}' | '\u{180E}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{206F}' | '\u{FEFF}')
}

fn check_user(user: String) -> Result<String, TargetError> {
    let ok = !user.is_empty()
        && user.chars().count() <= MAX_USER
        && !user.starts_with('-')
        && !user.chars().any(|c| c.is_control() || c.is_whitespace() || invisible(c) || matches!(c, '@' | ':' | '/' | '\\'));
    if ok { Ok(user) } else { Err(TargetError::User) }
}

/// Host names in ASCII only (an international name in its `xn--` form): a lookalike letter from
/// another script cannot pass for a known host.
fn check_host(host: String) -> Result<String, TargetError> {
    let ok = !host.is_empty()
        && host.len() <= MAX_HOST
        && !host.starts_with('-')
        && host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
    if ok { Ok(host) } else { Err(TargetError::Host) }
}

/// Decodes `%XX` escapes; `None` for a broken escape or text that is not UTF-8.
fn decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        if b == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(user: Option<&str>, host: &str, port: Option<u16>) -> Target {
        Target { user: user.map(str::to_string), host: host.into(), port }
    }

    #[test]
    fn links_are_read() {
        assert_eq!(parse_url("ssh://web.example.org"), Ok(target(None, "web.example.org", None)));
        assert_eq!(parse_url("SSH://root@10.0.0.5:2222/"), Ok(target(Some("root"), "10.0.0.5", Some(2222))));
        assert_eq!(parse_url("ssh://[2001:db8::1]:22"), Ok(target(None, "2001:db8::1", Some(22))));
        assert_eq!(parse_url("ssh://j%C3%BCrgen@host"), Ok(target(Some("jürgen"), "host", None)));
        assert_eq!(parse_url("ssh://me;fingerprint=SHA256-abc@host"), Ok(target(Some("me"), "host", None)));
    }

    #[test]
    fn hostile_links_are_refused() {
        assert_eq!(parse_url("ssh://-oProxyCommand=calc@host"), Err(TargetError::User));
        assert_eq!(parse_url("ssh://%2DoProxyCommand=calc@host"), Err(TargetError::User));
        assert_eq!(parse_url("ssh://-oProxyCommand=calc"), Err(TargetError::Host));
        assert_eq!(parse_url("ssh://a%0Ab@host"), Err(TargetError::User));
        assert_eq!(parse_url("ssh://user@ho%20st"), Err(TargetError::Host));
        assert_eq!(parse_url("ssh://user:secret@host"), Err(TargetError::Password));
        assert_eq!(parse_url("ssh://host:0"), Err(TargetError::Port));
        assert_eq!(parse_url("ssh://host:99999"), Err(TargetError::Port));
        assert_eq!(parse_url("ssh://host:"), Err(TargetError::Port));
        assert_eq!(parse_url("ssh://host/path"), Err(TargetError::Extra));
        assert_eq!(parse_url("ssh://host?x=1"), Err(TargetError::Extra));
        assert_eq!(parse_url("ssh://[not-v6]"), Err(TargetError::Host));
        assert_eq!(parse_url("ssh://%ZZ@host"), Err(TargetError::User));
        assert_eq!(parse_url("https://host"), Err(TargetError::NotSsh));
        assert_eq!(parse_url("ssh://"), Err(TargetError::Empty));
        assert_eq!(parse_url(&format!("ssh://{}", "a".repeat(2000))), Err(TargetError::TooLong));
        assert_eq!(parse_url("ssh://\u{0456}.example.org"), Err(TargetError::Host), "a Cyrillic lookalike");
        assert_eq!(parse_url("ssh://admin%E2%80%AEtoor@host"), Err(TargetError::User), "a bidi control");
        assert_eq!(parse_url("ssh://xn--bcher-kva.example"), Ok(target(None, "xn--bcher-kva.example", None)));
    }

    #[test]
    fn typed_text_is_read_without_escapes() {
        assert_eq!(parse_quick(" admin@db:2200 "), Ok(target(Some("admin"), "db", Some(2200))));
        assert_eq!(parse_quick("db"), Ok(target(None, "db", None)));
        assert_eq!(parse_quick("[::1]"), Ok(target(None, "::1", None)));
        assert_eq!(parse_quick("ssh://db"), Ok(target(None, "db", None)));
        assert_eq!(parse_quick("a b@db"), Err(TargetError::User));
        assert_eq!(parse_quick("::1"), Err(TargetError::Host));
        assert_eq!(parse_quick("-p 22 host"), Err(TargetError::Host));
    }

    #[test]
    fn a_target_names_its_host() {
        assert_eq!(target(Some("root"), "db", None).name(), "root@db");
        assert_eq!(target(None, "::1", Some(2222)).name(), "[::1]:2222");
    }
}
