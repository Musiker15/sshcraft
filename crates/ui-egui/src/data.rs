//! What the pages show, as the engine's list commands return it, fetched again when the engine
//! says data or settings changed. The rows are the commands' JSON, read with defaults, so a field
//! a newer engine adds does not break the interface.

use bitflags::bitflags;
use serde::Deserialize;
use serde_json::{Value, json};
use sshcraft_engine::settings::EngineSettings;
use sshcraft_model::model::{Group, Host};

use crate::app::SshcraftApp;

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct Refresh: u16 {
        const HOSTS = 1;
        const GROUPS = 1 << 1;
        const KEYS = 1 << 2;
        const IDENTITIES = 1 << 3;
        const SETTINGS = 1 << 4;
        const TEMPLATES = 1 << 5;
        const SNIPPETS = 1 << 6;
        const LOGS = 1 << 7;
        const SERVERS = 1 << 8;
        const DATA = Self::HOSTS.bits()
            | Self::GROUPS.bits()
            | Self::KEYS.bits()
            | Self::IDENTITIES.bits()
            | Self::TEMPLATES.bits()
            | Self::SNIPPETS.bits()
            | Self::LOGS.bits()
            | Self::SERVERS.bits();
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct HostRow {
    #[serde(flatten)]
    pub host: Host,
    #[serde(default)]
    pub has_password: bool,
    #[serde(default)]
    pub proxy_has_password: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GroupRow {
    #[serde(flatten)]
    pub group: Group,
    #[serde(default)]
    pub has_password: bool,
    #[serde(default)]
    pub proxy_has_password: bool,
}

/// A key as `key.list` shows it: public facts only, never secret material.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct KeyRow {
    pub id: String,
    pub name: String,
    pub algorithm: String,
    pub fingerprint: String,
    pub public_key: String,
    pub kind: Value,
    pub protected: bool,
    pub certificate: Option<Value>,
    pub location: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct IdentityRow {
    pub id: String,
    pub name: String,
    pub username: Option<String>,
    pub key: Option<Value>,
    pub has_password: bool,
    pub location: Option<String>,
}

/// A snippet as `snippet.list` returns it.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SnippetRow {
    pub id: String,
    pub name: String,
    pub package: String,
    pub content: String,
    pub default_targets: Vec<sshcraft_model::model::HostId>,
}

#[derive(Default)]
pub struct Data {
    pub hosts: Vec<HostRow>,
    pub groups: Vec<GroupRow>,
    pub keys: Vec<KeyRow>,
    pub identities: Vec<IdentityRow>,
    /// Workspace templates as `workspace.list` returns them.
    pub templates: Vec<Value>,
    pub snippets: Vec<SnippetRow>,
    /// Session logs as `log.list` returns them.
    pub logs: Vec<Value>,
    /// Sync servers as `sync.servers` returns them.
    pub servers: Vec<Value>,
    /// The last state each sync server reported, by server id.
    pub sync_status: std::collections::HashMap<String, Value>,
    /// The last state of each forwarding rule that ran, as the engine reports it.
    pub forwards: std::collections::HashMap<sshcraft_model::forward::ForwardId, Value>,
    /// The states of running rules were asked for once.
    pub forwards_loaded: bool,
    pub settings: EngineSettings,
    /// What this device can make besides vault keys; asked once.
    pub security: Option<crate::pages::keys::SecuritySupport>,
    pub security_loading: bool,
    /// What to fetch again; set by the engine's events, cleared when the fetch starts.
    pub dirty: Refresh,
    /// What is being fetched now, so a burst of changes starts one fetch each.
    pub loading: Refresh,
    /// Counts the fetches that came back, so a page can tell its cached lists are old.
    pub version: u64,
}

fn rows<T: serde::de::DeserializeOwned>(value: Value) -> Vec<T> {
    // A row the interface cannot read is left out rather than losing the whole list.
    match value {
        Value::Array(items) => items.into_iter().filter_map(|item| serde_json::from_value(item).ok()).collect(),
        _ => Vec::new(),
    }
}

/// Starts the fetches for what is dirty and not already loading.
pub fn refresh(app: &mut SshcraftApp) {
    let due = app.data.dirty - app.data.loading;
    if due.is_empty() {
        return;
    }
    app.data.dirty -= due;
    app.data.loading |= due;
    let fetch = |app: &SshcraftApp, what: Refresh, id: &str, apply: fn(&mut Data, Value)| {
        app.runner.run(
            id,
            json!({}),
            Box::new(move |app, result| {
                app.data.loading -= what;
                match result {
                    Ok(value) => {
                        apply(&mut app.data, value);
                        app.data.version = app.data.version.wrapping_add(1);
                    }
                    Err(e) => app.toasts.error(&e.to_string()),
                }
            }),
        );
    };
    if due.contains(Refresh::HOSTS) {
        fetch(app, Refresh::HOSTS, "host.list", |d, v| d.hosts = rows(v));
    }
    if due.contains(Refresh::GROUPS) {
        fetch(app, Refresh::GROUPS, "group.list", |d, v| d.groups = rows(v));
    }
    if due.contains(Refresh::KEYS) {
        fetch(app, Refresh::KEYS, "key.list", |d, v| d.keys = rows(v));
    }
    if due.contains(Refresh::SERVERS) {
        fetch(app, Refresh::SERVERS, "sync.servers", |d, v| d.servers = rows(v));
    }
    if due.contains(Refresh::LOGS) {
        fetch(app, Refresh::LOGS, "log.list", |d, v| d.logs = rows(v));
    }
    if due.contains(Refresh::SNIPPETS) {
        fetch(app, Refresh::SNIPPETS, "snippet.list", |d, v| d.snippets = rows(v));
    }
    if due.contains(Refresh::TEMPLATES) {
        fetch(app, Refresh::TEMPLATES, "workspace.list", |d, v| d.templates = rows(v));
    }
    if due.contains(Refresh::IDENTITIES) {
        fetch(app, Refresh::IDENTITIES, "identity.list", |d, v| d.identities = rows(v));
    }
    if due.contains(Refresh::SETTINGS) {
        fetch(app, Refresh::SETTINGS, "settings.get", |d, v| {
            if let Ok(settings) = serde_json::from_value(v) {
                d.settings = settings;
            }
        });
    }
}
