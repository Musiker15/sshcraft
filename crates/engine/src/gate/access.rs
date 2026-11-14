//! What a command needs to be reachable for agents, and the sixteen commands that are: their
//! category, how an agent's parameters are checked and turned into the command's, and what the
//! agent gets back.

use std::time::Duration;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use sshcraft_model::model::{Group, GroupId, Host, HostId, KeyId, KeyRef};
use sshcraft_store::agent::{AgentCategory, AgentOutcome, MAX_DETAIL};
use uuid::Uuid;

use super::scope::Connection;
use super::view::{GateView, group_view, host_view, name_ok, signs_in_with_stored};
use super::wire::{DEFAULT_EXEC_SECS, GroupInput, HostInput, KeyType, KeyView, MAX_EXEC_SECS, MAX_OUTPUT, MAX_WAIT_MS};
use crate::error::EngineError;
use crate::sessions::SessionId;

/// When an agent's request needs the user's answer in the SSHCraft window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskRule {
    /// The level of the category decides (allow, ask, deny).
    Level,
    /// Acting on a session the same connection opened needs no new question.
    OwnSession,
    /// Cleaning up what the agent itself opened (closing its session) is always allowed.
    Cleanup,
    /// Reading within the category (a status, a list, a log) needs no question; "deny" still
    /// blocks it.
    Read,
}

/// What running the command does to the connection's sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionEffect {
    None,
    /// The answer holds `session`: it now belongs to the connection.
    Opens,
    /// The session in `id` is gone from the connection.
    Closes,
}

/// The agent's parameters, checked and turned into the command's parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    pub params: Value,
    /// What the request is about, for the question and the log (a host name, a key name).
    pub target: String,
    /// What it does, for the question and the log (the command line of `host.exec`).
    pub detail: String,
}

/// Why a request was refused, and how it goes into the agent log.
#[derive(Debug, Clone, PartialEq)]
pub struct Refusal {
    pub error: EngineError,
    pub outcome: AgentOutcome,
    /// What the request was about, as far as it got.
    pub target: String,
}

impl Refusal {
    fn on(mut self, target: &str) -> Self {
        if self.target.is_empty() {
            target.clone_into(&mut self.target);
        }
        self
    }
}

pub fn not_found(what: &str, hidden: bool) -> Refusal {
    Refusal {
        error: EngineError::NotFound(what.to_string()),
        outcome: if hidden { AgentOutcome::Hidden } else { AgentOutcome::Failed },
        target: String::new(),
    }
}

pub fn invalid(message: impl ToString) -> Refusal {
    Refusal { error: EngineError::bad_params("agent", message), outcome: AgentOutcome::Failed, target: String::new() }
}

pub fn denied(message: &str) -> Refusal {
    Refusal { error: EngineError::Denied(message.to_string()), outcome: AgentOutcome::Denied, target: String::new() }
}

pub type Resolve = fn(&GateView, &Connection, Value) -> Result<Resolved, Refusal>;
/// What the agent gets back, from what the command returned and what was resolved.
pub type Present = fn(&GateView, &Resolved, Value) -> Value;

/// How a command is reachable for agents. A command without one is never reachable for them.
#[derive(Debug)]
pub struct AgentAccess {
    pub category: AgentCategory,
    pub ask: AskRule,
    pub resolve: Resolve,
    /// What the agent gets back: never secrets, never hidden hosts.
    pub present: Present,
    /// The answer is SSHCraft's own data: checked for key material before it goes out. Command
    /// output from a server (`host.exec`, `session.read`) is the agent's business.
    pub own_data: bool,
    /// Whether the request goes into the agent log (lists and reads do not, so a flood of them
    /// cannot push actions out of it).
    pub logged: bool,
    pub sessions: SessionEffect,
}

/// The agent's parameters as `T`; a field not allowed for agents is said as such.
fn params<T: DeserializeOwned>(value: Value) -> Result<T, Refusal> {
    serde_json::from_value(value).map_err(|e| {
        let text = e.to_string();
        if text.contains("unknown field") { invalid(format!("field not allowed for agents: {text}")) } else { invalid(text) }
    })
}

fn host_target(h: &Host) -> String {
    match &h.settings.address {
        Some(address) => format!("{} ({address})", h.name),
        None => h.name.clone(),
    }
}

fn plain(params: Value, target: String, detail: String) -> Resolved {
    Resolved { params, target, detail }
}

fn identity(_: &GateView, _: &Resolved, value: Value) -> Value {
    value
}

fn to_json<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

// ---- Lists ----

fn resolve_nothing(_: &GateView, _: &Connection, _: Value) -> Result<Resolved, Refusal> {
    Ok(plain(json!({}), String::new(), String::new()))
}

fn present_hosts(view: &GateView, _: &Resolved, _: Value) -> Value {
    to_json(&view.visible_hosts())
}

fn present_groups(view: &GateView, _: &Resolved, _: Value) -> Value {
    to_json(&view.visible_groups())
}

fn present_keys(view: &GateView, _: &Resolved, _: Value) -> Value {
    to_json(&view.keys)
}

#[derive(Deserialize)]
struct HostRef {
    host: String,
}

fn resolve_host_info(view: &GateView, _: &Connection, value: Value) -> Result<Resolved, Refusal> {
    let HostRef { host } = params(value)?;
    let h = view.host(&host).map_err(|r| r.on(&host))?;
    Ok(plain(json!({ "host": h.id.0.to_string() }), host_target(&h), String::new()))
}

/// The host a resolved request names, as an agent sees it.
fn present_host(view: &GateView, resolved: &Resolved, _: Value) -> Value {
    let id = resolved.params.get("host").and_then(Value::as_str).unwrap_or_default();
    view.hosts.iter().find(|h| h.id.0.to_string() == id).map_or(Value::Null, |h| to_json(&host_view(h)))
}

// ---- Connect ----

#[derive(Deserialize)]
struct ExecInput {
    host: String,
    command: String,
    #[serde(default)]
    timeout_secs: Option<u64>,
}

fn resolve_exec(view: &GateView, _: &Connection, value: Value) -> Result<Resolved, Refusal> {
    let ExecInput { host, command, timeout_secs } = params(value)?;
    let h = view.host(&host).map_err(|r| r.on(&host))?;
    let secs = timeout_secs.unwrap_or(DEFAULT_EXEC_SECS);
    if !(1..=MAX_EXEC_SECS).contains(&secs) {
        return Err(invalid(format!("timeout_secs must be 1 to {MAX_EXEC_SECS}")).on(&host));
    }
    if command.trim().is_empty() || command.chars().count() > MAX_DETAIL || command.contains('\0') {
        return Err(invalid(format!("command: 1 to {MAX_DETAIL} characters")).on(&host));
    }
    let params = json!({ "host": h.id.0.to_string(), "command": command, "timeout_secs": secs });
    Ok(plain(params, host_target(&h), command))
}

fn resolve_session_open(view: &GateView, _: &Connection, value: Value) -> Result<Resolved, Refusal> {
    let HostRef { host } = params(value)?;
    let h = view.host(&host).map_err(|r| r.on(&host))?;
    Ok(plain(json!({ "host": h.id.0.to_string() }), host_target(&h), String::new()))
}

/// A session this connection opened. Sessions whose host became hidden or went away were
/// closed before resolving.
fn own_session(conn: &Connection, id: SessionId) -> Result<(), Refusal> {
    if conn.owns_session(id) { Ok(()) } else { Err(not_found("session", false).on(&id.to_string())) }
}

#[derive(Deserialize)]
struct SendInput {
    id: SessionId,
    data: String,
}

fn resolve_session_send(_: &GateView, conn: &Connection, value: Value) -> Result<Resolved, Refusal> {
    let SendInput { id, data } = params(value)?;
    own_session(conn, id)?;
    if data.len() > MAX_OUTPUT {
        return Err(invalid("data too long").on(&id.to_string()));
    }
    let detail: String = data.chars().take(MAX_DETAIL).collect();
    Ok(plain(json!({ "id": id, "data": data }), id.to_string(), detail))
}

#[derive(Deserialize)]
struct ReadInput {
    id: SessionId,
    #[serde(default)]
    wait_ms: Option<u64>,
    #[serde(default)]
    limit: Option<usize>,
}

fn resolve_session_read(_: &GateView, conn: &Connection, value: Value) -> Result<Resolved, Refusal> {
    let ReadInput { id, wait_ms, limit } = params(value)?;
    own_session(conn, id)?;
    let wait_ms = wait_ms.unwrap_or(0).min(MAX_WAIT_MS);
    let limit = limit.unwrap_or(MAX_OUTPUT).clamp(1, MAX_OUTPUT);
    Ok(plain(json!({ "id": id, "wait_ms": wait_ms, "limit": limit }), id.to_string(), String::new()))
}

#[derive(Deserialize)]
struct SessionRef {
    id: SessionId,
}

fn resolve_session_close(_: &GateView, conn: &Connection, value: Value) -> Result<Resolved, Refusal> {
    let SessionRef { id } = params(value)?;
    own_session(conn, id)?;
    Ok(plain(json!({ "id": id }), id.to_string(), String::new()))
}

// ---- Edit ----

fn shown<T: std::fmt::Display>(value: &Option<T>) -> String {
    value.as_ref().map_or_else(|| "-".into(), ToString::to_string)
}

/// Applies a change to a host and describes it for the question: one line per changed field.
fn apply_host_change(view: &GateView, stored: &mut Host, input: HostInput) -> Result<Vec<String>, Refusal> {
    let mut changes = Vec::new();
    if let Some(name) = input.name {
        if !name_ok(&name) {
            return Err(invalid("name"));
        }
        if name != stored.name {
            changes.push(format!("name: {} -> {name}", stored.name));
            stored.name = name;
        }
    }
    if let Some(group) = input.group {
        let group = view.visible_group_id(group)?;
        if group != stored.group_id {
            let name =
                |g: Option<GroupId>| g.and_then(|g| view.groups.iter().find(|x| x.id == g).map(|x| x.name.clone())).unwrap_or_else(|| "-".into());
            changes.push(format!("group: {} -> {}", name(stored.group_id), name(group)));
            stored.group_id = group;
        }
    }
    if let Some(address) = input.address
        && address != stored.settings.address
    {
        changes.push(format!("address: {} -> {}", shown(&stored.settings.address), shown(&address)));
        stored.settings.address = address;
    }
    if let Some(port) = input.port
        && port != stored.settings.port
    {
        changes.push(format!("port: {} -> {}", shown(&stored.settings.port), shown(&port)));
        stored.settings.port = port;
    }
    if let Some(username) = input.username
        && username != stored.settings.username
    {
        changes.push(format!("user: {} -> {}", shown(&stored.settings.username), shown(&username)));
        stored.settings.username = username;
    }
    if let Some(key) = input.key {
        if let Some(id) = key
            && !view.keys.iter().any(|k| k.id == id)
        {
            return Err(not_found("key", false));
        }
        let new = key.map(|id| KeyRef::Stored { id: KeyId(id) });
        if new != stored.settings.key {
            let name = |k: &Option<KeyRef>| match k {
                Some(KeyRef::Stored { id }) => view.keys.iter().find(|x| x.id == id.0).map_or_else(|| id.0.to_string(), |x| x.name.clone()),
                Some(_) => "agent key".into(),
                None => "-".into(),
            };
            changes.push(format!("key: {} -> {}", name(&stored.settings.key), name(&new)));
            stored.settings.key = new;
        }
    }
    if let Some(tags) = input.tags
        && tags != stored.tags
    {
        changes.push(format!("tags: {} -> {}", stored.tags.join(", "), tags.join(", ")));
        stored.tags = tags;
    }
    Ok(changes)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HostSave {
    host: HostInput,
}

fn resolve_host_save(view: &GateView, _: &Connection, value: Value) -> Result<Resolved, Refusal> {
    let HostSave { host } = params(value)?;
    let (mut stored, new) = match host.id {
        Some(id) => (view.host(&id.to_string()).map_err(|r| r.on(&id.to_string()))?, false),
        None => (
            Host {
                id: HostId(Uuid::new_v4()),
                name: String::new(),
                group_id: None,
                settings: Default::default(),
                location: None,
                tags: Vec::new(),
                os: None,
                forwards: Vec::new(),
            },
            true,
        ),
    };
    if new && host.name.is_none() {
        return Err(invalid("name"));
    }
    let before = stored.clone();
    let target = host.name.clone().unwrap_or_else(|| stored.name.clone());
    let changes = apply_host_change(view, &mut stored, host).map_err(|r| r.on(&target))?;
    // Stored sign-in data (a password, an identity, agent forwarding) must not be pointed at
    // another machine by an agent.
    let moved = new
        || stored.settings.address != before.settings.address
        || stored.settings.port != before.settings.port
        || stored.group_id != before.group_id;
    if moved && view.carries_sign_in(&stored) {
        return Err(denied("this host signs in with stored data; agents cannot point it elsewhere").on(&target));
    }
    let mut detail = changes.join("\n");
    if new {
        detail = format!("new host\n{detail}");
    }
    Ok(plain(json!({ "host": stored }), target, detail))
}

fn present_saved_host(_: &GateView, resolved: &Resolved, _: Value) -> Value {
    resolved.params.get("host").and_then(|h| serde_json::from_value::<Host>(h.clone()).ok()).map_or(Value::Null, |h| to_json(&host_view(&h)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GroupSave {
    group: GroupInput,
}

fn resolve_group_save(view: &GateView, _: &Connection, value: Value) -> Result<Resolved, Refusal> {
    let GroupSave { group } = params(value)?;
    let target = group.name.clone();
    if !name_ok(&group.name) {
        return Err(invalid("name").on(&target));
    }
    let (mut stored, new) = match group.id {
        Some(id) => (view.group(&id.to_string()).map_err(|r| r.on(&target))?, false),
        None => (
            Group {
                id: GroupId(Uuid::new_v4()),
                name: String::new(),
                defaults: Default::default(),
                location: None,
                parent_id: None,
                tags: Vec::new(),
            },
            true,
        ),
    };
    let parent = view.visible_group_id(group.parent).map_err(|r| r.on(&target))?;
    let moved = !new && parent != stored.parent_id;
    // Moving a group moves its hosts: the same rule as for a host's address.
    if moved {
        let group_id = stored.id;
        let below_signs_in = signs_in_with_stored(&stored.defaults)
            || view.hosts.iter().any(|h| {
                h.group_id.is_some_and(|g| sshcraft_model::model::group_chain(&view.groups, g).iter().any(|c| c.id == group_id))
                    && signs_in_with_stored(&h.settings)
            });
        if below_signs_in {
            return Err(denied("hosts in this group sign in with stored data; agents cannot move it").on(&target));
        }
    }
    let detail = if new {
        "new group".to_owned()
    } else if moved {
        "moved to another parent group".to_owned()
    } else {
        String::new()
    };
    stored.parent_id = parent;
    stored.name = group.name;
    Ok(plain(json!({ "group": stored }), target, detail))
}

fn present_saved_group(_: &GateView, resolved: &Resolved, _: Value) -> Value {
    resolved.params.get("group").and_then(|g| serde_json::from_value::<Group>(g.clone()).ok()).map_or(Value::Null, |g| to_json(&group_view(&g)))
}

// ---- Delete ----

fn resolve_host_delete(view: &GateView, _: &Connection, value: Value) -> Result<Resolved, Refusal> {
    let HostRef { host } = params(value)?;
    let h = view.host(&host).map_err(|r| r.on(&host))?;
    Ok(plain(json!({ "id": h.id }), host_target(&h), String::new()))
}

#[derive(Deserialize)]
struct GroupRef {
    group: String,
}

fn resolve_group_delete(view: &GateView, _: &Connection, value: Value) -> Result<Resolved, Refusal> {
    let GroupRef { group } = params(value)?;
    let g = view.group(&group).map_err(|r| r.on(&group))?;
    if view.subtree_hidden(g.id) {
        return Err(denied("not allowed for agents").on(&g.name));
    }
    Ok(plain(json!({ "id": g.id }), g.name, String::new()))
}

#[derive(Deserialize)]
struct KeyRefInput {
    key: String,
}

fn resolve_key_delete(view: &GateView, _: &Connection, value: Value) -> Result<Resolved, Refusal> {
    let KeyRefInput { key } = params(value)?;
    let found: Vec<&KeyView> = view.keys.iter().filter(|k| k.id.to_string() == key || k.name == key).collect();
    let one = match found.as_slice() {
        [one] => *one,
        [] => return Err(not_found("key", false).on(&key)),
        _ => return Err(invalid("more than one key has this name, use its id").on(&key)),
    };
    // A key a hidden host or group uses stays: deleting it would break what the agent cannot see.
    let uses = |s: &sshcraft_model::model::ConnectionSettings| matches!(&s.key, Some(KeyRef::Stored { id }) if id.0 == one.id);
    let hidden_user = view.hosts.iter().any(|h| uses(&h.settings) && view.host_hidden(h))
        || view.groups.iter().any(|g| uses(&g.defaults) && view.group_hidden(g.id));
    if hidden_user {
        return Err(denied("not allowed for agents").on(&one.name));
    }
    Ok(plain(json!({ "id": one.id }), one.name.clone(), String::new()))
}

// ---- Keys ----

#[derive(Deserialize)]
struct GenerateInput {
    name: String,
    key_type: KeyType,
}

fn resolve_key_generate(_: &GateView, _: &Connection, value: Value) -> Result<Resolved, Refusal> {
    let GenerateInput { name, key_type } = params(value)?;
    if !name_ok(&name) {
        return Err(invalid("name").on(&name));
    }
    let spec = match key_type {
        KeyType::Ed25519 => json!({ "type": "ed25519" }),
        KeyType::Ecdsa => json!({ "type": "ecdsa", "curve": "nistp256" }),
        KeyType::Rsa => json!({ "type": "rsa", "bits": 4096 }),
    };
    Ok(plain(json!({ "name": name, "spec": spec }), name, String::new()))
}

#[derive(Deserialize)]
struct ImportInput {
    name: String,
    text: String,
    #[serde(default)]
    passphrase: Option<String>,
}

fn resolve_key_import(_: &GateView, _: &Connection, value: Value) -> Result<Resolved, Refusal> {
    let ImportInput { name, text, passphrase } = params(value)?;
    if !name_ok(&name) {
        return Err(invalid("name").on(&name));
    }
    // The key text never goes into the log or the question.
    Ok(plain(json!({ "name": name, "text": text, "passphrase": passphrase, "save_passphrase": true }), name, String::new()))
}

/// A stored key as an agent sees it: name, type and fingerprint, never the key.
fn present_key(_: &GateView, _: &Resolved, value: Value) -> Value {
    let field = |name: &str| value.get(name).cloned().unwrap_or(Value::Null);
    json!({ "id": field("id"), "name": field("name"), "kind": field("algorithm"), "fingerprint": field("fingerprint") })
}

fn present_nothing(_: &GateView, _: &Resolved, _: Value) -> Value {
    Value::Null
}

/// How long `host.exec` may take, from what was resolved.
pub fn exec_timeout(resolved: &Resolved) -> Duration {
    Duration::from_secs(resolved.params.get("timeout_secs").and_then(Value::as_u64).unwrap_or(DEFAULT_EXEC_SECS))
}

const fn access(category: AgentCategory, resolve: Resolve, present: Present) -> AgentAccess {
    AgentAccess { category, ask: AskRule::Level, resolve, present, own_data: true, logged: true, sessions: SessionEffect::None }
}

const fn list(resolve: Resolve, present: Present) -> AgentAccess {
    AgentAccess { logged: false, ..access(AgentCategory::List, resolve, present) }
}

pub static HOST_LIST: AgentAccess = list(resolve_nothing, present_hosts);
pub static GROUP_LIST: AgentAccess = list(resolve_nothing, present_groups);
pub static KEY_LIST: AgentAccess = list(resolve_nothing, present_keys);
pub static HOST_INFO: AgentAccess = list(resolve_host_info, present_host);

pub static HOST_EXEC: AgentAccess = AgentAccess { own_data: false, ..access(AgentCategory::Connect, resolve_exec, identity) };
pub static SESSION_OPEN: AgentAccess =
    AgentAccess { sessions: SessionEffect::Opens, ..access(AgentCategory::Connect, resolve_session_open, identity) };
pub static SESSION_SEND: AgentAccess = AgentAccess { ask: AskRule::OwnSession, ..access(AgentCategory::Connect, resolve_session_send, identity) };
pub static SESSION_READ: AgentAccess =
    AgentAccess { ask: AskRule::OwnSession, own_data: false, logged: false, ..access(AgentCategory::Connect, resolve_session_read, identity) };
pub static SESSION_CLOSE: AgentAccess =
    AgentAccess { ask: AskRule::Cleanup, sessions: SessionEffect::Closes, ..access(AgentCategory::Connect, resolve_session_close, present_nothing) };

pub static HOST_SAVE: AgentAccess = access(AgentCategory::Edit, resolve_host_save, present_saved_host);
pub static GROUP_SAVE: AgentAccess = access(AgentCategory::Edit, resolve_group_save, present_saved_group);

pub static HOST_DELETE: AgentAccess = access(AgentCategory::Delete, resolve_host_delete, present_nothing);
pub static GROUP_DELETE: AgentAccess = access(AgentCategory::Delete, resolve_group_delete, present_nothing);
pub static KEY_DELETE: AgentAccess = access(AgentCategory::Delete, resolve_key_delete, present_nothing);

pub static KEY_GENERATE: AgentAccess = access(AgentCategory::Keys, resolve_key_generate, present_key);
pub static KEY_IMPORT: AgentAccess = access(AgentCategory::Keys, resolve_key_import, present_key);

/// The commands agents and the control channel can reach, each with its access. Every other
/// command is never reachable for them. Only the actions of the Tauri version are here, so no
/// agent reaches more than before.
pub fn access_for(id: &str) -> Option<&'static AgentAccess> {
    Some(match id {
        "host.list" => &HOST_LIST,
        "group.list" => &GROUP_LIST,
        "key.list" => &KEY_LIST,
        "host.info" => &HOST_INFO,
        "host.exec" => &HOST_EXEC,
        "session.open" => &SESSION_OPEN,
        "session.send" => &SESSION_SEND,
        "session.read" => &SESSION_READ,
        "session.close" => &SESSION_CLOSE,
        "host.save" => &HOST_SAVE,
        "group.save" => &GROUP_SAVE,
        "host.delete" => &HOST_DELETE,
        "group.delete" => &GROUP_DELETE,
        "key.delete" => &KEY_DELETE,
        "key.generate" => &KEY_GENERATE,
        "key.import" => &KEY_IMPORT,
        "forward.start" => &FORWARD_START,
        "forward.stop" => &FORWARD_STOP,
        "forward.status" => &FORWARD_STATUS,
        "workspace.list" => &WORKSPACE_LIST,
        "workspace.save" => &WORKSPACE_SAVE,
        "workspace.delete" => &WORKSPACE_DELETE,
        "snippet.list" => &SNIPPET_LIST,
        "snippet.save" => &SNIPPET_SAVE,
        "snippet.delete" => &SNIPPET_DELETE,
        "log.list" => &LOG_LIST,
        "log.read" => &LOG_READ,
        "log.mark_add" => &LOG_MARK_ADD,
        "log.mark_remove" => &LOG_MARK_REMOVE,
        "log.delete" => &LOG_DELETE,
        "log.search" => &LOG_SEARCH,
        "sync.servers" => &SYNC_SERVERS,
        "sync.set_server" => &SYNC_SET_SERVER,
        "sync.server_logs" => &SYNC_SERVER_LOGS,
        "device.list" => &DEVICE_LIST,
        "share.find_recipient" => &SHARE_FIND_RECIPIENT,
        "share.items" => &SHARE_ITEMS,
        "share.list" => &SHARE_LIST,
        "share.set_grants" => &SHARE_SET_GRANTS,
        "share.end" => &SHARE_END,
        "share.of_items" => &SHARE_OF_ITEMS,
        _ => return None,
    })
}

// ---- Sync and Share ----

/// Parameters that name nothing an agent could not see: passed on as they are.
fn resolve_pass(_: &GateView, _: &Connection, value: Value) -> Result<Resolved, Refusal> {
    let target = ["id", "server"].iter().find_map(|k| value.get(*k).and_then(Value::as_str)).unwrap_or_default().to_owned();
    Ok(plain(value, target, String::new()))
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ShareWhatRef {
    GroupAll { group: Uuid },
    Selection { group: Uuid, hosts: Vec<Uuid> },
    Host { host: Uuid },
}

#[derive(Deserialize)]
struct ShareItemsInput {
    what: ShareWhatRef,
}

/// What is shared must be something agents see.
fn resolve_share_items(view: &GateView, _: &Connection, value: Value) -> Result<Resolved, Refusal> {
    let ShareItemsInput { what } = params(value.clone())?;
    let target = match what {
        ShareWhatRef::GroupAll { group } => view.group(&group.to_string())?.name,
        ShareWhatRef::Selection { group, hosts } => {
            for host in &hosts {
                view.host(&host.to_string())?;
            }
            view.group(&group.to_string())?.name
        }
        ShareWhatRef::Host { host } => host_target(&view.host(&host.to_string())?),
    };
    Ok(plain(value, target, String::new()))
}

#[derive(Deserialize)]
struct AreaInput {
    area: Uuid,
}

/// A share with something hidden in it is unknown to agents.
fn resolve_share_area(view: &GateView, _: &Connection, value: Value) -> Result<Resolved, Refusal> {
    let AreaInput { area } = params(value.clone())?;
    if view.shares.iter().any(|s| s.area == area && view.share_hidden(s.group.as_deref(), &s.hosts)) {
        return Err(not_found("share", true).on(&area.to_string()));
    }
    Ok(plain(value, area.to_string(), String::new()))
}

fn present_shares(view: &GateView, _: &Resolved, value: Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .filter(|s| {
                    let group = s.get("group").and_then(Value::as_str);
                    let hosts: Vec<String> = s
                        .get("hosts")
                        .and_then(Value::as_array)
                        .map(|h| h.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect())
                        .unwrap_or_default();
                    !view.share_hidden(group, &hosts)
                })
                .collect(),
        ),
        other => other,
    }
}

fn present_item_shares(view: &GateView, _: &Resolved, value: Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .filter(|i| !i.get("id").and_then(Value::as_str).is_some_and(|id| view.host_text_hidden(id) || view.group_text_hidden(id)))
                .collect(),
        ),
        other => other,
    }
}

pub static SYNC_SERVERS: AgentAccess = read(AgentCategory::Sync, resolve_nothing, identity);
pub static SYNC_SET_SERVER: AgentAccess = access(AgentCategory::Sync, resolve_pass, present_nothing);
pub static SYNC_SERVER_LOGS: AgentAccess = access(AgentCategory::Sync, resolve_pass, present_nothing);
pub static DEVICE_LIST: AgentAccess = read(AgentCategory::Sync, resolve_pass, identity);

pub static SHARE_FIND_RECIPIENT: AgentAccess = access(AgentCategory::Share, resolve_pass, identity);
pub static SHARE_ITEMS: AgentAccess = access(AgentCategory::Share, resolve_share_items, identity);
pub static SHARE_LIST: AgentAccess = read(AgentCategory::Share, resolve_pass, present_shares);
pub static SHARE_SET_GRANTS: AgentAccess = access(AgentCategory::Share, resolve_share_area, identity);
pub static SHARE_END: AgentAccess = access(AgentCategory::Share, resolve_share_area, identity);
pub static SHARE_OF_ITEMS: AgentAccess = read(AgentCategory::Share, resolve_nothing, present_item_shares);

// ---- Forward ----

#[derive(Deserialize)]
struct RuleInput {
    host_id: HostId,
    rule_id: sshcraft_model::forward::ForwardId,
}

/// A rule of a host agents see.
fn resolve_rule(view: &GateView, _: &Connection, value: Value) -> Result<Resolved, Refusal> {
    let RuleInput { host_id, rule_id } = params(value)?;
    let h = view.host(&host_id.0.to_string()).map_err(|r| r.on(&host_id.0.to_string()))?;
    let rule = h.forwards.iter().find(|r| r.id == rule_id).ok_or_else(|| not_found("rule", false).on(&h.name))?;
    let detail = rule.name.clone();
    Ok(plain(json!({ "host_id": host_id, "rule_id": rule_id }), host_target(&h), detail))
}

fn present_forwards(view: &GateView, _: &Resolved, value: Value) -> Value {
    without_hidden_hosts(view, value, "host_id")
}

// ---- Workspaces ----

fn present_templates(view: &GateView, _: &Resolved, _: Value) -> Value {
    to_json(&view.templates.iter().filter(|t| !view.template_hidden(t)).collect::<Vec<_>>())
}

#[derive(Deserialize)]
struct TemplateSave {
    template: crate::workspaces::Template,
}

fn resolve_template_save(view: &GateView, _: &Connection, value: Value) -> Result<Resolved, Refusal> {
    let TemplateSave { template } = params(value)?;
    let target = template.name.clone();
    // Its panes must be on hosts agents see, and a stored one it replaces must be visible too.
    for pane in &template.panes {
        if let crate::workspaces::TemplateTarget::Host { host_id } = pane.target {
            view.host(&host_id.0.to_string()).map_err(|r| r.on(&target))?;
        }
    }
    if view.templates.iter().any(|t| t.id == template.id && view.template_hidden(t)) {
        return Err(not_found("template", true).on(&target));
    }
    Ok(plain(json!({ "template": template }), target, String::new()))
}

#[derive(Deserialize)]
struct UuidRef {
    id: Uuid,
}

fn resolve_template_delete(view: &GateView, _: &Connection, value: Value) -> Result<Resolved, Refusal> {
    let UuidRef { id } = params(value)?;
    let stored = view.templates.iter().find(|t| t.id == id);
    if stored.is_some_and(|t| view.template_hidden(t)) {
        return Err(not_found("template", true).on(&id.to_string()));
    }
    Ok(plain(json!({ "id": id }), stored.map_or_else(|| id.to_string(), |t| t.name.clone()), String::new()))
}

// ---- Snippets ----

fn present_snippets(view: &GateView, _: &Resolved, _: Value) -> Value {
    to_json(&view.snippets.iter().filter(|s| !view.snippet_hidden(s)).collect::<Vec<_>>())
}

#[derive(Deserialize)]
struct SnippetSave {
    snippet: sshcraft_store::snippets::Snippet,
}

fn resolve_snippet_save(view: &GateView, _: &Connection, value: Value) -> Result<Resolved, Refusal> {
    let SnippetSave { snippet } = params(value)?;
    let target = snippet.name.clone();
    for host in &snippet.default_targets {
        view.host(&host.0.to_string()).map_err(|r| r.on(&target))?;
    }
    if view.snippets.iter().any(|s| s.id == snippet.id && view.snippet_hidden(s)) {
        return Err(not_found("snippet", true).on(&target));
    }
    Ok(plain(json!({ "snippet": snippet }), target, String::new()))
}

fn resolve_snippet_delete(view: &GateView, _: &Connection, value: Value) -> Result<Resolved, Refusal> {
    let UuidRef { id } = params(value)?;
    let stored = view.snippets.iter().find(|s| s.id.0 == id);
    if stored.is_some_and(|s| view.snippet_hidden(s)) {
        return Err(not_found("snippet", true).on(&id.to_string()));
    }
    Ok(plain(json!({ "id": id }), stored.map_or_else(|| id.to_string(), |s| s.name.clone()), String::new()))
}

// ---- Logs ----

/// A log agents may see; the rest of the parameters passes as it is.
fn visible_log(view: &GateView, value: &Value) -> Result<sshcraft_model::model::LogId, Refusal> {
    let id = value
        .get("id")
        .and_then(|v| serde_json::from_value::<sshcraft_model::model::LogId>(v.clone()).ok())
        .ok_or_else(|| invalid("id is missing"))?;
    if view.log_hidden(id) {
        return Err(not_found("log", true).on(&id.0.to_string()));
    }
    Ok(id)
}

fn resolve_log(view: &GateView, _: &Connection, value: Value) -> Result<Resolved, Refusal> {
    let id = visible_log(view, &value)?;
    Ok(plain(value, id.0.to_string(), String::new()))
}

#[derive(Deserialize)]
struct LogIds {
    ids: Vec<sshcraft_model::model::LogId>,
}

fn resolve_log_delete(view: &GateView, _: &Connection, value: Value) -> Result<Resolved, Refusal> {
    let LogIds { ids } = params(value)?;
    if let Some(hidden) = ids.iter().find(|id| view.log_hidden(**id)) {
        return Err(not_found("log", true).on(&hidden.0.to_string()));
    }
    let target = ids.iter().map(|id| id.0.to_string()).collect::<Vec<_>>().join(", ");
    Ok(plain(json!({ "ids": ids }), target, String::new()))
}

#[derive(Deserialize)]
struct SearchInput {
    query: String,
}

fn resolve_log_search(_: &GateView, _: &Connection, value: Value) -> Result<Resolved, Refusal> {
    let SearchInput { query } = params(value)?;
    Ok(plain(json!({ "query": query }), String::new(), query))
}

fn present_logs(view: &GateView, _: &Resolved, value: Value) -> Value {
    without_hidden_hosts(view, value, "host_id")
}

fn present_hits(view: &GateView, _: &Resolved, value: Value) -> Value {
    match value {
        Value::Array(hits) => Value::Array(
            hits.into_iter()
                .filter(|hit| {
                    !hit.get("log_id")
                        .and_then(|v| serde_json::from_value::<sshcraft_model::model::LogId>(v.clone()).ok())
                        .is_some_and(|id| view.log_hidden(id))
                })
                .collect(),
        ),
        other => other,
    }
}

const fn read(category: AgentCategory, resolve: Resolve, present: Present) -> AgentAccess {
    AgentAccess { ask: AskRule::Read, logged: false, ..access(category, resolve, present) }
}

pub static FORWARD_START: AgentAccess = access(AgentCategory::Forward, resolve_rule, present_nothing);
pub static FORWARD_STOP: AgentAccess = access(AgentCategory::Forward, resolve_rule, present_nothing);
pub static FORWARD_STATUS: AgentAccess = read(AgentCategory::Forward, resolve_nothing, present_forwards);

pub static WORKSPACE_LIST: AgentAccess = read(AgentCategory::Workspaces, resolve_nothing, present_templates);
pub static WORKSPACE_SAVE: AgentAccess = access(AgentCategory::Workspaces, resolve_template_save, identity);
pub static WORKSPACE_DELETE: AgentAccess = access(AgentCategory::Workspaces, resolve_template_delete, present_nothing);

pub static SNIPPET_LIST: AgentAccess = read(AgentCategory::Snippets, resolve_nothing, present_snippets);
pub static SNIPPET_SAVE: AgentAccess = access(AgentCategory::Snippets, resolve_snippet_save, identity);
pub static SNIPPET_DELETE: AgentAccess = access(AgentCategory::Snippets, resolve_snippet_delete, present_nothing);

pub static LOG_LIST: AgentAccess = read(AgentCategory::Logs, resolve_nothing, present_logs);
/// Log output is what a server printed: the agent's business, like `host.exec` output.
pub static LOG_READ: AgentAccess = AgentAccess { own_data: false, ..read(AgentCategory::Logs, resolve_log, identity) };
pub static LOG_MARK_ADD: AgentAccess = access(AgentCategory::Logs, resolve_log, identity);
pub static LOG_MARK_REMOVE: AgentAccess = access(AgentCategory::Logs, resolve_log, identity);
pub static LOG_DELETE: AgentAccess = access(AgentCategory::Logs, resolve_log_delete, present_nothing);
pub static LOG_SEARCH: AgentAccess = AgentAccess { own_data: false, ..read(AgentCategory::Logs, resolve_log_search, present_hits) };

/// Builds the resolve step of a later command that names a host in the field `host`: the host
/// must be one agents see.
pub fn visible_host_param(view: &GateView, value: &Value, field: &str) -> Result<HostId, Refusal> {
    let reference = value.get(field).and_then(Value::as_str).ok_or_else(|| invalid(format!("{field} is missing")))?;
    view.host(reference).map(|h| h.id).map_err(|r| r.on(reference))
}

/// Drops from a list every entry whose `field` names a host agents do not see.
pub fn without_hidden_hosts(view: &GateView, value: Value, field: &str) -> Value {
    let hidden: Vec<String> = view.hosts.iter().filter(|h| view.host_hidden(h)).map(|h| h.id.0.to_string()).collect();
    match value {
        Value::Array(items) => Value::Array(
            items.into_iter().filter(|item| !item.get(field).and_then(Value::as_str).is_some_and(|id| hidden.iter().any(|h| h == id))).collect(),
        ),
        other => other,
    }
}
