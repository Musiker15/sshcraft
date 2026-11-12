//! Groups, hosts and how a host inherits connection settings from its group.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! id_type {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
    };
}

id_type!(GroupId);
id_type!(HostId);
id_type!(KeyId);
id_type!(ServerId);
id_type!(IdentityId);
id_type!(PasswordId);

/// Where the private key for a connection comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum KeyRef {
    /// A key held in SSHCraft's own encrypted vault.
    Stored { id: KeyId },
    /// A key held by an SSH agent (Proton Pass); only its fingerprint is known.
    Agent { fingerprint: String },
}

/// Connection settings on a group or host. `None` means "not set here".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionSettings {
    pub address: Option<String>,
    pub port: Option<u16>,
    pub username: Option<String>,
    pub key: Option<KeyRef>,
    /// A password sealed in the vault.
    #[serde(default)]
    pub password: Option<PasswordId>,
    /// Fills username, key and password where this level does not set them itself.
    #[serde(default)]
    pub identity: Option<IdentityId>,
    /// Hosts to hop through, in order. `Some(vec![])` means "none", not "inherit".
    #[serde(default)]
    pub jump_hosts: Option<Vec<HostId>>,
    #[serde(default)]
    pub proxy: Option<Proxy>,
    #[serde(default)]
    pub agent_forwarding: Option<bool>,
    /// Merged with the groups above: a name set lower wins.
    #[serde(default)]
    pub env: Vec<EnvVar>,
}

impl ConnectionSettings {
    /// Every password these settings refer to: their own and the proxy's.
    pub fn password_ids(&self) -> Vec<PasswordId> {
        let proxy = match &self.proxy {
            Some(Proxy::Http(e) | Proxy::Socks5(e)) => e.password,
            _ => None,
        };
        self.password.into_iter().chain(proxy).collect()
    }
}

/// How the first connection leaves this device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Proxy {
    /// No proxy, and none inherited from a group.
    Direct,
    Http(ProxyEndpoint),
    Socks5(ProxyEndpoint),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyEndpoint {
    pub address: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<PasswordId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvVar {
    pub name: String,
    pub value: String,
}

/// A reusable bundle of user and credentials for hosts and groups.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    pub id: IdentityId,
    pub name: String,
    pub username: Option<String>,
    pub key: Option<KeyRef>,
    pub password: Option<PasswordId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<ServerId>,
}

/// At most this many environment variables per level, names up to 64 characters, values up to
/// 4096 bytes.
pub const MAX_ENV: usize = 64;
pub const MAX_ENV_NAME: usize = 64;
pub const MAX_ENV_VALUE: usize = 4096;
/// At most this many jump hosts before the target.
pub const MAX_JUMPS: usize = 8;

fn valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    name.len() <= MAX_ENV_NAME
        && (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Environment variables as they are stored: invalid names and too long values dropped, a name
/// given twice keeps the last value at the place of the first, at most [`MAX_ENV`].
pub fn normalize_env(vars: &[EnvVar]) -> Vec<EnvVar> {
    let mut out: Vec<EnvVar> = Vec::new();
    for var in vars {
        if !valid_env_name(&var.name) || var.value.len() > MAX_ENV_VALUE {
            continue;
        }
        if let Some(existing) = out.iter_mut().find(|v| v.name == var.name) {
            existing.value = var.value.clone();
        } else if out.len() < MAX_ENV {
            out.push(var.clone());
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    pub id: GroupId,
    pub name: String,
    pub defaults: ConnectionSettings,
    /// The sync server this group belongs to; `None` means "only on this device". A subgroup
    /// always has its parent's location.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<ServerId>,
    /// The group this one is nested in.
    #[serde(default)]
    pub parent_id: Option<GroupId>,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// The operating system or distribution a host runs, for its symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostOs {
    Linux,
    Debian,
    Ubuntu,
    Fedora,
    RedHat,
    CentOs,
    Rocky,
    Alma,
    Arch,
    Alpine,
    OpenSuse,
    Raspbian,
    FreeBsd,
    OpenBsd,
    MacOs,
    Windows,
    Other,
}

/// How deep groups may nest; deeper chains (or a cycle) are cut off here.
pub const MAX_GROUP_DEPTH: usize = 64;
/// At most this many tags per item, each at most this many characters.
pub const MAX_TAGS: usize = 32;
pub const MAX_TAG_LEN: usize = 64;

/// Tags as they are stored: trimmed, empty ones dropped, duplicates (ignoring case) merged,
/// each cut to [`MAX_TAG_LEN`] characters, at most [`MAX_TAGS`].
pub fn normalize_tags(tags: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in tags {
        let tag: String = tag.trim().chars().take(MAX_TAG_LEN).collect();
        if tag.is_empty() || out.iter().any(|t| t.to_lowercase() == tag.to_lowercase()) {
            continue;
        }
        out.push(tag);
        if out.len() == MAX_TAGS {
            break;
        }
    }
    out
}

/// A group and the groups above it, nearest first. Stops at a missing group, at a cycle and
/// after [`MAX_GROUP_DEPTH`] levels.
pub fn group_chain(groups: &[Group], start: GroupId) -> Vec<Group> {
    let mut chain: Vec<Group> = Vec::new();
    let mut next = Some(start);
    while let Some(id) = next {
        if chain.len() >= MAX_GROUP_DEPTH || chain.iter().any(|g| g.id == id) {
            break;
        }
        let Some(group) = groups.iter().find(|g| g.id == id) else {
            break;
        };
        next = group.parent_id;
        chain.push(group.clone());
    }
    chain
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Host {
    pub id: HostId,
    pub name: String,
    pub group_id: Option<GroupId>,
    pub settings: ConnectionSettings,
    /// The sync server this host belongs to; `None` means "only on this device". A host in a
    /// group always has its group's location.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<ServerId>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// A system a newer client knows reads as `Other` here.
    #[serde(default, deserialize_with = "os_or_other")]
    pub os: Option<HostOs>,
}

fn os_or_other<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<HostOs>, D::Error> {
    let value = Option::<serde_json::Value>::deserialize(d)?;
    Ok(match value {
        None | Some(serde_json::Value::Null) => None,
        Some(v) => Some(serde_json::from_value(v).unwrap_or(HostOs::Other)),
    })
}

/// A host with every setting filled in, ready to connect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedHost {
    pub host_id: HostId,
    pub name: String,
    pub address: String,
    pub port: u16,
    pub username: String,
    pub key: Option<KeyRef>,
    pub password: Option<PasswordId>,
    /// Never [`Proxy::Direct`]: that resolves to `None`.
    pub proxy: Option<Proxy>,
    pub agent_forwarding: bool,
    pub env: Vec<EnvVar>,
    pub jump_hosts: Vec<HostId>,
}

/// Everything a route is resolved from.
pub struct Directory<'a> {
    pub groups: &'a [Group],
    pub hosts: &'a [Host],
    pub identities: &'a [Identity],
}

/// How to reach a host: through `jumps` in order, then `target`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    pub target: ResolvedHost,
    pub jumps: Vec<ResolvedHost>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ResolveError {
    #[error("no address set on the host or its group")]
    MissingAddress,
    #[error("no username set on the host or its group")]
    MissingUsername,
    #[error("the given group is not the host's group")]
    WrongGroup,
    #[error("the host does not exist")]
    NotFound,
    #[error("the jump hosts contain the host itself or one host twice")]
    JumpLoop,
    #[error("more than {MAX_JUMPS} jump hosts")]
    TooManyJumps,
    #[error("a jump host does not exist")]
    JumpMissing,
    #[error("a jump host cannot be resolved: {error}")]
    Jump {
        host_id: HostId,
        error: Box<ResolveError>,
    },
}

pub const DEFAULT_PORT: u16 = 22;

/// Checks that `chain` is the host's group and the groups above it, nearest first.
fn check_chain(host: &Host, chain: &[&Group]) -> Result<(), ResolveError> {
    match (host.group_id, chain.first()) {
        (None, None) => {}
        (Some(id), Some(first)) if first.id == id => {}
        _ => return Err(ResolveError::WrongGroup),
    }
    for pair in chain.windows(2) {
        if pair[0].parent_id != Some(pair[1].id) {
            return Err(ResolveError::WrongGroup);
        }
    }
    Ok(())
}

/// One level of settings (the host or one of its groups) with the identity it names.
struct Level<'a> {
    settings: &'a ConnectionSettings,
    identity: Option<&'a Identity>,
}

/// Combines a host with its groups (`chain`: its group, then that group's parent and so on):
/// a value set on the host wins, then the nearest group that sets it; empty strings count as
/// unset. On each level, the level's identity fills username, key and password where the level
/// does not set them itself. An identity that does not exist is ignored.
pub fn resolve(
    host: &Host,
    chain: &[&Group],
    identities: &[Identity],
) -> Result<ResolvedHost, ResolveError> {
    check_chain(host, chain)?;
    let (address, port) = endpoint(host, chain)?;
    let identity = |settings: &ConnectionSettings| {
        settings
            .identity
            .and_then(|id| identities.iter().find(|i| i.id == id))
    };
    let levels: Vec<Level> = std::iter::once(&host.settings)
        .chain(chain.iter().map(|g| &g.defaults))
        .map(|settings| Level {
            settings,
            identity: identity(settings),
        })
        .collect();
    let username = levels
        .iter()
        .find_map(|l| {
            non_empty(&l.settings.username)
                .or_else(|| l.identity.and_then(|i| non_empty(&i.username)))
        })
        .ok_or(ResolveError::MissingUsername)?;
    let key = levels.iter().find_map(|l| {
        l.settings
            .key
            .clone()
            .or_else(|| l.identity.and_then(|i| i.key.clone()))
    });
    let password = levels.iter().find_map(|l| {
        l.settings
            .password
            .or_else(|| l.identity.and_then(|i| i.password))
    });
    let proxy = levels
        .iter()
        .find_map(|l| l.settings.proxy.clone())
        .filter(|p| *p != Proxy::Direct);
    let agent_forwarding = levels
        .iter()
        .find_map(|l| l.settings.agent_forwarding)
        .unwrap_or(false);
    let jump_hosts = levels
        .iter()
        .find_map(|l| l.settings.jump_hosts.clone())
        .unwrap_or_default();
    let env: Vec<EnvVar> = levels
        .iter()
        .rev()
        .flat_map(|l| l.settings.env.iter().cloned())
        .collect();
    Ok(ResolvedHost {
        host_id: host.id,
        name: host.name.clone(),
        address,
        port,
        username,
        key,
        password,
        proxy,
        agent_forwarding,
        env: normalize_env(&env),
        jump_hosts,
    })
}

/// Resolves one host of the directory with its groups.
fn resolve_in(dir: &Directory, host_id: HostId) -> Result<ResolvedHost, ResolveError> {
    let host = dir
        .hosts
        .iter()
        .find(|h| h.id == host_id)
        .ok_or(ResolveError::NotFound)?;
    let chain = match host.group_id {
        Some(gid) => group_chain(dir.groups, gid),
        None => Vec::new(),
    };
    let chain: Vec<&Group> = chain.iter().collect();
    resolve(host, &chain, dir.identities)
}

/// How to reach a host. The jump hosts come from the target and are resolved with their own
/// address and credentials; their own jump hosts and proxy do not apply here.
pub fn resolve_route(dir: &Directory, host_id: HostId) -> Result<Route, ResolveError> {
    let target = resolve_in(dir, host_id)?;
    if target.jump_hosts.len() > MAX_JUMPS {
        return Err(ResolveError::TooManyJumps);
    }
    let mut jumps: Vec<ResolvedHost> = Vec::new();
    for &jump in &target.jump_hosts {
        if jump == host_id {
            return Err(ResolveError::JumpLoop);
        }
        if jumps.iter().any(|j| j.host_id == jump) {
            return Err(ResolveError::JumpLoop);
        }
        let mut hop = match resolve_in(dir, jump) {
            Ok(hop) => hop,
            Err(ResolveError::NotFound) => return Err(ResolveError::JumpMissing),
            Err(error) => {
                return Err(ResolveError::Jump {
                    host_id: jump,
                    error: Box::new(error),
                });
            }
        };
        hop.jump_hosts.clear();
        hop.proxy = None;
        jumps.push(hop);
    }
    Ok(Route { target, jumps })
}

/// Address and port of a host after inheriting from its groups. Needs neither a username nor
/// a key, so it also works for half-filled hosts (for example to forget a stored host key).
pub fn effective_endpoint(host: &Host, chain: &[&Group]) -> Result<(String, u16), ResolveError> {
    check_chain(host, chain)?;
    endpoint(host, chain)
}

fn endpoint(host: &Host, chain: &[&Group]) -> Result<(String, u16), ResolveError> {
    let address = non_empty(&host.settings.address)
        .or_else(|| chain.iter().find_map(|g| non_empty(&g.defaults.address)))
        .ok_or(ResolveError::MissingAddress)?;
    let port = host
        .settings
        .port
        .or_else(|| chain.iter().find_map(|g| g.defaults.port))
        .unwrap_or(DEFAULT_PORT);
    Ok((address, port))
}

fn non_empty(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> KeyRef {
        KeyRef::Stored { id: KeyId::new() }
    }

    fn group(settings: ConnectionSettings) -> Group {
        Group {
            location: None,
            id: GroupId::new(),
            name: "srv".into(),
            defaults: settings,
            parent_id: None,
            tags: Vec::new(),
        }
    }

    fn host_in(group: &Group, settings: ConnectionSettings) -> Host {
        Host {
            location: None,
            id: HostId::new(),
            name: "h".into(),
            group_id: Some(group.id),
            settings,
            tags: Vec::new(),
            os: None,
        }
    }

    fn full_group(k: &KeyRef) -> Group {
        group(ConnectionSettings {
            address: Some("example.org".into()),
            port: Some(2222),
            username: Some("root".into()),
            key: Some(k.clone()),
            ..Default::default()
        })
    }

    #[test]
    fn host_inherits_group_defaults() {
        let k = key();
        let g = full_group(&k);
        let h = host_in(&g, ConnectionSettings::default());
        let r = resolve(&h, &[&g], &[]).unwrap();
        assert_eq!(r.address, "example.org");
        assert_eq!(r.port, 2222);
        assert_eq!(r.username, "root");
        assert_eq!(r.key, Some(k));
        assert_eq!(r.host_id, h.id);
    }

    #[test]
    fn host_overrides_group() {
        let g = full_group(&key());
        let h = host_in(
            &g,
            ConnectionSettings {
                username: Some("deploy".into()),
                ..Default::default()
            },
        );
        assert_eq!(resolve(&h, &[&g], &[]).unwrap().username, "deploy");
    }

    #[test]
    fn empty_string_counts_as_unset() {
        let g = full_group(&key());
        let h = host_in(
            &g,
            ConnectionSettings {
                username: Some("  ".into()),
                ..Default::default()
            },
        );
        assert_eq!(resolve(&h, &[&g], &[]).unwrap().username, "root");
    }

    #[test]
    fn port_defaults_to_22() {
        let g = group(ConnectionSettings {
            address: Some("a".into()),
            username: Some("u".into()),
            key: Some(key()),
            port: None,
            ..Default::default()
        });
        let h = host_in(&g, ConnectionSettings::default());
        assert_eq!(resolve(&h, &[&g], &[]).unwrap().port, 22);
    }

    #[test]
    fn missing_address_is_error() {
        let h = Host {
            location: None,
            id: HostId::new(),
            name: "h".into(),
            group_id: None,
            settings: ConnectionSettings {
                username: Some("u".into()),
                key: Some(key()),
                ..Default::default()
            },
            tags: Vec::new(),
            os: None,
        };
        assert_eq!(resolve(&h, &[], &[]), Err(ResolveError::MissingAddress));
    }

    #[test]
    fn wrong_group_is_error() {
        let g = full_group(&key());
        let other = full_group(&key());
        let h = host_in(&g, ConnectionSettings::default());
        assert_eq!(resolve(&h, &[&other], &[]), Err(ResolveError::WrongGroup));
    }

    #[test]
    fn host_json_without_location_still_parses() {
        let json = r#"{"id":"6f1c2c4e-8d3a-4b7e-9a51-2f0e7c1d9b00","name":"web","group_id":null,
            "settings":{"address":null,"port":null,"username":null,"key":null}}"#;
        let host: Host = serde_json::from_str(json).unwrap();
        assert_eq!(host.location, None);
    }

    #[test]
    fn agent_key_ref_roundtrips_json() {
        let r = KeyRef::Agent {
            fingerprint: "SHA256:x".into(),
        };
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(json, r#"{"kind":"agent","fingerprint":"SHA256:x"}"#);
        assert_eq!(serde_json::from_str::<KeyRef>(&json).unwrap(), r);
    }

    #[test]
    fn host_inherits_agent_key_from_group() {
        let agent = KeyRef::Agent {
            fingerprint: "SHA256:y".into(),
        };
        let g = full_group(&agent);
        let h = host_in(&g, ConnectionSettings::default());
        assert_eq!(resolve(&h, &[&g], &[]).unwrap().key, Some(agent));
    }

    #[test]
    fn effective_endpoint_inherits_address_and_port() {
        let g = group(ConnectionSettings {
            address: Some("10.0.0.5".into()),
            port: Some(2222),
            ..Default::default()
        });
        let h = host_in(&g, ConnectionSettings::default());
        assert_eq!(
            effective_endpoint(&h, &[&g]).unwrap(),
            ("10.0.0.5".to_owned(), 2222)
        );
    }

    #[test]
    fn effective_endpoint_needs_no_username_or_key() {
        let mut h = host_in(
            &group(ConnectionSettings::default()),
            ConnectionSettings {
                address: Some(" example.org ".into()),
                ..Default::default()
            },
        );
        h.group_id = None;
        assert_eq!(
            effective_endpoint(&h, &[]).unwrap(),
            ("example.org".to_owned(), 22)
        );
        h.settings.address = Some("  ".into());
        assert_eq!(
            effective_endpoint(&h, &[]),
            Err(ResolveError::MissingAddress)
        );
    }
}

#[cfg(test)]
#[path = "route_tests.rs"]
mod route_tests;
