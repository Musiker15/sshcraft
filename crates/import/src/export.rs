//! Exporting hosts and groups as CSV or JSON, without secrets: a stored key appears only with
//! its name and fingerprint, an agent key only with its fingerprint.

use serde::Serialize;

use sshcraft_model::model::{ConnectionSettings, DEFAULT_PORT, Group, GroupId, Host, HostOs, KeyRef, ServerId, group_chain};
use sshcraft_store::store::{Store, StoreError};

pub use crate::import::csv::CSV_HEADER;

/// A place to export from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExportPlace {
    /// Only on this device.
    Local,
    Server {
        id: ServerId,
    },
}

/// What to export.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
pub struct ExportFilter {
    /// Only items at this place.
    #[serde(default)]
    pub location: Option<ExportPlace>,
    /// Only this group and everything below it.
    #[serde(default)]
    pub group: Option<GroupId>,
}

/// A key as it may leave the device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
enum KeyOut {
    Stored { name: String, fingerprint: String },
    Agent { agent_fingerprint: String },
}

#[derive(Serialize)]
struct SettingsOut {
    address: Option<String>,
    port: Option<u16>,
    username: Option<String>,
    key: Option<KeyOut>,
    /// Only whether one is stored, never the password nor its id.
    has_password: bool,
    /// The identity by name.
    identity: Option<String>,
    jump_hosts: Option<Vec<String>>,
    proxy: Option<ProxyOut>,
    agent_forwarding: Option<bool>,
    env: Vec<sshcraft_model::model::EnvVar>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ProxyOut {
    Direct,
    Http(ProxyEndpointOut),
    Socks5(ProxyEndpointOut),
    Command { argv: Vec<String> },
}

#[derive(Serialize)]
struct ProxyEndpointOut {
    address: String,
    port: u16,
    username: Option<String>,
    has_password: bool,
}

fn proxy_out(proxy: &Option<sshcraft_model::model::Proxy>) -> Option<ProxyOut> {
    use sshcraft_model::model::Proxy;
    let endpoint = |e: &sshcraft_model::model::ProxyEndpoint| ProxyEndpointOut {
        address: e.address.clone(),
        port: e.port,
        username: e.username.clone(),
        has_password: e.password.is_some(),
    };
    proxy.as_ref().map(|p| match p {
        Proxy::Direct => ProxyOut::Direct,
        Proxy::Http(e) => ProxyOut::Http(endpoint(e)),
        Proxy::Socks5(e) => ProxyOut::Socks5(endpoint(e)),
        Proxy::Command { argv } => ProxyOut::Command { argv: argv.clone() },
    })
}

#[derive(Serialize)]
struct GroupOut {
    id: String,
    name: String,
    parent_id: Option<String>,
    tags: Vec<String>,
    defaults: SettingsOut,
}

#[derive(Serialize)]
struct HostOut {
    id: String,
    name: String,
    group_id: Option<String>,
    tags: Vec<String>,
    os: Option<HostOs>,
    settings: SettingsOut,
}

#[derive(Serialize)]
struct Document {
    format: &'static str,
    version: u32,
    groups: Vec<GroupOut>,
    hosts: Vec<HostOut>,
}

struct Selection {
    groups: Vec<Group>,
    /// Every group, for paths and inheritance.
    all_groups: Vec<Group>,
    hosts: Vec<Host>,
}

fn select(store: &Store, filter: &ExportFilter) -> Result<Selection, StoreError> {
    let all_groups = store.list_groups()?;
    let tree: Option<Vec<GroupId>> = match filter.group {
        Some(id) => {
            let mut ids = store.subgroups(id)?;
            ids.push(id);
            Some(ids)
        }
        None => None,
    };
    let at = |location: &Option<ServerId>| match filter.location {
        None => true,
        Some(ExportPlace::Local) => location.is_none(),
        Some(ExportPlace::Server { id }) => *location == Some(id),
    };
    let groups = all_groups.iter().filter(|g| at(&g.location) && tree.as_ref().is_none_or(|t| t.contains(&g.id))).cloned().collect();
    let hosts = store
        .list_hosts()?
        .into_iter()
        .filter(|h| at(&h.location) && tree.as_ref().is_none_or(|t| h.group_id.is_some_and(|g| t.contains(&g))))
        .collect();
    Ok(Selection { groups, all_groups, hosts })
}

fn key_out(store: &Store, key: &Option<KeyRef>) -> Result<Option<KeyOut>, StoreError> {
    Ok(match key {
        Some(KeyRef::Stored { id }) => store.key_record(*id)?.map(|record| KeyOut::Stored { name: record.name, fingerprint: record.fingerprint }),
        Some(KeyRef::Agent { fingerprint }) => Some(KeyOut::Agent { agent_fingerprint: fingerprint.clone() }),
        None => None,
    })
}

fn settings_out(store: &Store, s: &ConnectionSettings) -> Result<SettingsOut, StoreError> {
    Ok(SettingsOut {
        address: s.address.clone(),
        port: s.port,
        username: s.username.clone(),
        key: key_out(store, &s.key)?,
        has_password: s.password.is_some(),
        identity: match s.identity {
            Some(id) => store.list_identities()?.into_iter().find(|i| i.id == id).map(|i| i.name),
            None => None,
        },
        jump_hosts: s.jump_hosts.as_ref().map(|j| j.iter().map(|h| h.0.to_string()).collect()),
        proxy: proxy_out(&s.proxy),
        agent_forwarding: s.agent_forwarding,
        env: s.env.clone(),
    })
}

/// The whole tree with each item's own values (inheritance stays in the tree).
pub fn export_json(store: &Store, filter: &ExportFilter) -> Result<String, StoreError> {
    let selection = select(store, filter)?;
    let in_export = |id: &GroupId| selection.groups.iter().any(|g| g.id == *id);
    let document = Document {
        format: "sshcraft-hosts",
        version: 1,
        groups: selection
            .groups
            .iter()
            .map(|g| {
                // The top of an exported part carries what it inherits from above it.
                let mut defaults = g.defaults.clone();
                if !g.parent_id.is_some_and(|p| in_export(&p)) {
                    for above in group_chain(&selection.all_groups, g.id).iter().skip(1) {
                        let d = &above.defaults;
                        defaults.address = defaults.address.or_else(|| d.address.clone());
                        defaults.port = defaults.port.or(d.port);
                        defaults.username = defaults.username.or_else(|| d.username.clone());
                        defaults.key = defaults.key.or_else(|| d.key.clone());
                    }
                }
                Ok(GroupOut {
                    id: g.id.0.to_string(),
                    name: g.name.clone(),
                    parent_id: g.parent_id.filter(in_export).map(|p| p.0.to_string()),
                    tags: g.tags.clone(),
                    defaults: settings_out(store, &defaults)?,
                })
            })
            .collect::<Result<_, StoreError>>()?,
        hosts: selection
            .hosts
            .iter()
            .map(|h| {
                Ok(HostOut {
                    id: h.id.0.to_string(),
                    name: h.name.clone(),
                    group_id: h.group_id.map(|g| g.0.to_string()),
                    tags: h.tags.clone(),
                    os: h.os,
                    settings: settings_out(store, &h.settings)?,
                })
            })
            .collect::<Result<_, StoreError>>()?,
    };
    serde_json::to_string_pretty(&document).map_err(|e| StoreError::Malformed(e.to_string()))
}

/// One row per host with what it effectively uses (a flat table has no inheritance), in the
/// columns of [`CSV_HEADER`].
pub fn export_csv(store: &Store, filter: &ExportFilter) -> Result<String, StoreError> {
    let selection = select(store, filter)?;
    let mut writer = ::csv::Writer::from_writer(Vec::new());
    let bad = |e: ::csv::Error| StoreError::Malformed(e.to_string());
    writer.write_record(CSV_HEADER.split(',')).map_err(bad)?;
    for host in &selection.hosts {
        let chain = host.group_id.map(|g| group_chain(&selection.all_groups, g)).unwrap_or_default();
        let path = crate::import::csv::join_path(chain.iter().rev().map(|g| g.name.as_str()));
        let text = |own: &Option<String>, get: fn(&ConnectionSettings) -> &Option<String>| {
            own.clone().or_else(|| chain.iter().find_map(|g| get(&g.defaults).clone())).unwrap_or_default()
        };
        let port = host.settings.port.or_else(|| chain.iter().find_map(|g| g.defaults.port)).unwrap_or(DEFAULT_PORT);
        let key = host.settings.key.clone().or_else(|| chain.iter().find_map(|g| g.defaults.key.clone()));
        let key = match key_out(store, &key)? {
            Some(KeyOut::Stored { name, .. }) => name,
            Some(KeyOut::Agent { agent_fingerprint }) => format!("agent:{agent_fingerprint}"),
            None => String::new(),
        };
        let os = host.os.and_then(|os| serde_json::to_value(os).ok()).and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default();
        writer
            .write_record(
                [
                    host.name.clone(),
                    path,
                    host.tags.join(";"),
                    text(&host.settings.address, |d| &d.address),
                    port.to_string(),
                    text(&host.settings.username, |d| &d.username),
                    key,
                    os,
                ]
                .iter()
                .map(|cell| crate::import::csv::guard(cell)),
            )
            .map_err(bad)?;
    }
    let bytes = writer.into_inner().map_err(|e| StoreError::Malformed(e.to_string()))?;
    String::from_utf8(bytes).map_err(|e| StoreError::Malformed(e.to_string()))
}

#[cfg(test)]
#[path = "export_tests.rs"]
mod tests;
