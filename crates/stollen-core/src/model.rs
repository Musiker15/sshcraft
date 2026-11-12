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

/// Where the private key for a connection comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum KeyRef {
    /// A key held in Stollen's own encrypted vault.
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    pub id: GroupId,
    pub name: String,
    pub defaults: ConnectionSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Host {
    pub id: HostId,
    pub name: String,
    pub group_id: Option<GroupId>,
    pub settings: ConnectionSettings,
}

/// A host with every setting filled in, ready to connect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedHost {
    pub host_id: HostId,
    pub name: String,
    pub address: String,
    pub port: u16,
    pub username: String,
    pub key: KeyRef,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ResolveError {
    #[error("no address set on the host or its group")]
    MissingAddress,
    #[error("no username set on the host or its group")]
    MissingUsername,
    #[error("no key set on the host or its group")]
    MissingKey,
    #[error("the given group is not the host's group")]
    WrongGroup,
}

pub const DEFAULT_PORT: u16 = 22;

/// Combines a host with its group: host values win, empty strings count as unset.
pub fn resolve(host: &Host, group: Option<&Group>) -> Result<ResolvedHost, ResolveError> {
    if let Some(g) = group
        && host.group_id != Some(g.id)
    {
        return Err(ResolveError::WrongGroup);
    }
    let inherited = group.map(|g| &g.defaults);

    let text = |own: &Option<String>, from_group: Option<&Option<String>>| {
        non_empty(own).or_else(|| from_group.and_then(non_empty))
    };

    let address = text(&host.settings.address, inherited.map(|d| &d.address))
        .ok_or(ResolveError::MissingAddress)?;
    let username = text(&host.settings.username, inherited.map(|d| &d.username))
        .ok_or(ResolveError::MissingUsername)?;
    let port = host
        .settings
        .port
        .or_else(|| inherited.and_then(|d| d.port))
        .unwrap_or(DEFAULT_PORT);
    let key = host
        .settings
        .key
        .clone()
        .or_else(|| inherited.and_then(|d| d.key.clone()))
        .ok_or(ResolveError::MissingKey)?;

    Ok(ResolvedHost {
        host_id: host.id,
        name: host.name.clone(),
        address,
        port,
        username,
        key,
    })
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
            id: GroupId::new(),
            name: "srv".into(),
            defaults: settings,
        }
    }

    fn host_in(group: &Group, settings: ConnectionSettings) -> Host {
        Host {
            id: HostId::new(),
            name: "h".into(),
            group_id: Some(group.id),
            settings,
        }
    }

    fn full_group(k: &KeyRef) -> Group {
        group(ConnectionSettings {
            address: Some("example.org".into()),
            port: Some(2222),
            username: Some("root".into()),
            key: Some(k.clone()),
        })
    }

    #[test]
    fn host_inherits_group_defaults() {
        let k = key();
        let g = full_group(&k);
        let h = host_in(&g, ConnectionSettings::default());
        let r = resolve(&h, Some(&g)).unwrap();
        assert_eq!(r.address, "example.org");
        assert_eq!(r.port, 2222);
        assert_eq!(r.username, "root");
        assert_eq!(r.key, k);
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
        assert_eq!(resolve(&h, Some(&g)).unwrap().username, "deploy");
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
        assert_eq!(resolve(&h, Some(&g)).unwrap().username, "root");
    }

    #[test]
    fn port_defaults_to_22() {
        let g = group(ConnectionSettings {
            address: Some("a".into()),
            username: Some("u".into()),
            key: Some(key()),
            port: None,
        });
        let h = host_in(&g, ConnectionSettings::default());
        assert_eq!(resolve(&h, Some(&g)).unwrap().port, 22);
    }

    #[test]
    fn missing_address_is_error() {
        let h = Host {
            id: HostId::new(),
            name: "h".into(),
            group_id: None,
            settings: ConnectionSettings {
                username: Some("u".into()),
                key: Some(key()),
                ..Default::default()
            },
        };
        assert_eq!(resolve(&h, None), Err(ResolveError::MissingAddress));
    }

    #[test]
    fn missing_key_is_error() {
        let h = Host {
            id: HostId::new(),
            name: "h".into(),
            group_id: None,
            settings: ConnectionSettings {
                address: Some("a".into()),
                username: Some("u".into()),
                ..Default::default()
            },
        };
        assert_eq!(resolve(&h, None), Err(ResolveError::MissingKey));
    }

    #[test]
    fn wrong_group_is_error() {
        let g = full_group(&key());
        let other = full_group(&key());
        let h = host_in(&g, ConnectionSettings::default());
        assert_eq!(resolve(&h, Some(&other)), Err(ResolveError::WrongGroup));
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
        assert_eq!(resolve(&h, Some(&g)).unwrap().key, agent);
    }
}
