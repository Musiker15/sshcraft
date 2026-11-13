//! Every agent request passes here: hidden hosts and groups are answered as unknown, the level
//! of the action's kind decides (allow, ask in SSHCraft, deny), every outcome is logged, and
//! no answer ever carries key material.

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use sshcraft_agent::{
    Action, AgentError, DEFAULT_EXEC_SECS, ErrorKind, GroupView, HostView, MAX_EXEC_SECS,
    MAX_OUTPUT, MAX_WAIT_MS, Request, Response, valid_agent_name,
};
use sshcraft_core::agent::{AgentCategory, AgentLevel, AgentLogEntry, AgentOutcome, AgentPolicy};
use sshcraft_core::model::{Group, GroupId, Host, HostId, KeyId, KeyRef};
use uuid::Uuid;

use super::backend::{AgentBackend, AgentPrompt, Confirmer};

/// How long a question waits for the user; then it counts as no.
pub const CONFIRM_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_NAME: usize = 100;
const MAX_COMMAND: usize = 10_000;

pub fn category(action: &Action) -> AgentCategory {
    match action {
        Action::ListHosts | Action::ListGroups | Action::ListKeys | Action::HostInfo { .. } => {
            AgentCategory::List
        }
        Action::Exec { .. }
        | Action::SessionOpen { .. }
        | Action::SessionWrite { .. }
        | Action::SessionRead { .. }
        | Action::SessionClose { .. } => AgentCategory::Connect,
        Action::SaveHost { .. } | Action::SaveGroup { .. } => AgentCategory::Edit,
        Action::DeleteHost { .. } | Action::DeleteGroup { .. } | Action::DeleteKey { .. } => {
            AgentCategory::Delete
        }
        Action::GenerateKey { .. } | Action::ImportKey { .. } => AgentCategory::Keys,
    }
}

/// A request resolved against what is stored, ready to run.
enum Op {
    ListHosts,
    ListGroups,
    ListKeys,
    HostInfo(Host),
    Exec(HostId, String, Duration),
    SessionOpen(HostId),
    SessionWrite(String, Vec<u8>),
    SessionRead(String, Duration),
    SessionClose(String),
    SaveHost(Box<Host>),
    SaveGroup(Group),
    DeleteHost(HostId),
    DeleteGroup(GroupId),
    DeleteKey(KeyId),
    GenerateKey(String, sshcraft_agent::KeyType),
    ImportKey(String, String, Option<String>),
}

/// Why a request did not run, and how the log calls it.
struct Refusal {
    error: AgentError,
    outcome: AgentOutcome,
}

fn refuse(kind: ErrorKind, message: impl Into<String>, outcome: AgentOutcome) -> Refusal {
    Refusal {
        error: AgentError::new(kind, message),
        outcome,
    }
}

fn not_found(what: &str, hidden: bool) -> Refusal {
    refuse(
        ErrorKind::NotFound,
        format!("no such {what}"),
        if hidden {
            AgentOutcome::Hidden
        } else {
            AgentOutcome::Failed
        },
    )
}

fn invalid(message: impl Into<String>) -> Refusal {
    refuse(ErrorKind::Invalid, message, AgentOutcome::Failed)
}

pub fn host_view(host: &Host) -> HostView {
    HostView {
        id: host.id.0,
        name: host.name.clone(),
        group: host.group_id.map(|g| g.0),
        address: host.settings.address.clone(),
        port: host.settings.port,
        username: host.settings.username.clone(),
        key: match &host.settings.key {
            Some(KeyRef::Stored { id }) => Some(id.0),
            _ => None,
        },
        tags: host.tags.clone(),
    }
}

fn group_view(group: &Group) -> GroupView {
    GroupView {
        id: group.id.0,
        name: group.name.clone(),
        parent: group.parent_id.map(|g| g.0),
    }
}

fn name_ok(name: &str) -> bool {
    !name.trim().is_empty()
        && name.chars().count() <= MAX_NAME
        && !name.chars().any(char::is_control)
}

/// What the gate knows while handling one request.
struct View {
    policy: AgentPolicy,
    hosts: Vec<Host>,
    groups: Vec<Group>,
}

impl View {
    fn host_hidden(&self, host: &Host) -> bool {
        self.policy.host_hidden(&self.groups, host)
    }

    fn group_hidden(&self, group: GroupId) -> bool {
        self.policy.group_hidden(&self.groups, group)
    }

    /// A visible host by id or by a name only one visible host has.
    fn host(&self, reference: &str) -> Result<Host, Refusal> {
        let matches: Vec<&Host> = self
            .hosts
            .iter()
            .filter(|h| h.id.0.to_string() == reference || h.name == reference)
            .collect();
        let visible: Vec<&Host> = matches
            .iter()
            .copied()
            .filter(|h| !self.host_hidden(h))
            .collect();
        match visible.as_slice() {
            [one] => Ok((*one).clone()),
            [] => Err(not_found("host", !matches.is_empty())),
            _ => Err(invalid("more than one host has this name, use its id")),
        }
    }

    fn group(&self, reference: &str) -> Result<Group, Refusal> {
        let matches: Vec<&Group> = self
            .groups
            .iter()
            .filter(|g| g.id.0.to_string() == reference || g.name == reference)
            .collect();
        let visible: Vec<&Group> = matches
            .iter()
            .copied()
            .filter(|g| !self.group_hidden(g.id))
            .collect();
        match visible.as_slice() {
            [one] => Ok((*one).clone()),
            [] => Err(not_found("group", !matches.is_empty())),
            _ => Err(invalid("more than one group has this name, use its id")),
        }
    }

    fn visible_group_id(&self, id: Option<Uuid>) -> Result<Option<GroupId>, Refusal> {
        match id {
            None => Ok(None),
            Some(id) => self.group(&id.to_string()).map(|g| Some(g.id)),
        }
    }
}

pub struct Gate<B: AgentBackend, C: Confirmer> {
    pub backend: Arc<B>,
    pub confirmer: Arc<C>,
    pub confirm_timeout: Duration,
}

impl<B: AgentBackend, C: Confirmer> Gate<B, C> {
    pub fn new(backend: Arc<B>, confirmer: Arc<C>) -> Self {
        Self {
            backend,
            confirmer,
            confirm_timeout: CONFIRM_TIMEOUT,
        }
    }

    pub async fn handle(&self, request: Request) -> Response {
        let id = request.id;
        if !valid_agent_name(&request.agent) {
            return Response::error(id, AgentError::new(ErrorKind::Invalid, "agent name"));
        }
        let action_name = request.action.name().to_owned();
        let logged = !matches!(request.action, Action::SessionRead { .. });
        let (result, target, detail) = self.run(&request.agent, request.action).await;
        let (response, outcome) = match result {
            Ok((value, outcome)) => match guard(value) {
                Ok(value) => (Response::ok(id, value), outcome),
                Err(error) => (Response::error(id, error), AgentOutcome::Failed),
            },
            Err(refusal) => (Response::error(id, refusal.error), refusal.outcome),
        };
        if logged {
            self.backend.log(AgentLogEntry {
                id: Uuid::new_v4(),
                at: crate::recording::now_ms(),
                agent: request.agent,
                action: action_name,
                target,
                detail,
                outcome,
            });
        }
        response
    }

    async fn run(
        &self,
        agent: &str,
        action: Action,
    ) -> (Result<(Value, AgentOutcome), Refusal>, String, String) {
        let category = category(&action);
        let action_name = action.name().to_owned();
        let view = match (self.backend.hosts(), self.backend.groups()) {
            (Ok(hosts), Ok(groups)) => View {
                policy: self.backend.policy(),
                hosts,
                groups,
            },
            (Err(e), _) | (_, Err(e)) => {
                return (
                    Err(Refusal {
                        error: e,
                        outcome: AgentOutcome::Failed,
                    }),
                    String::new(),
                    String::new(),
                );
            }
        };
        let (op, target, detail) = match self.resolve(&view, action) {
            Ok(resolved) => resolved,
            Err((refusal, target)) => return (Err(refusal), target, String::new()),
        };
        let level = view.policy.level(category);
        let outcome = match level {
            AgentLevel::Deny => {
                return (
                    Err(refuse(
                        ErrorKind::Denied,
                        "not allowed for agents",
                        AgentOutcome::Denied,
                    )),
                    target,
                    detail,
                );
            }
            AgentLevel::Allow => AgentOutcome::Allowed,
            // Writing to, reading from and closing a session an agent opened were allowed with
            // opening it.
            AgentLevel::Ask
                if matches!(
                    op,
                    Op::SessionWrite(..) | Op::SessionRead(..) | Op::SessionClose(_)
                ) =>
            {
                AgentOutcome::Allowed
            }
            AgentLevel::Ask => {
                let prompt = AgentPrompt {
                    agent: agent.to_owned(),
                    action: action_name,
                    target: target.clone(),
                    detail: detail.clone(),
                };
                let answer =
                    tokio::time::timeout(self.confirm_timeout, self.confirmer.confirm(prompt))
                        .await
                        .unwrap_or(false);
                if !answer {
                    return (
                        Err(refuse(
                            ErrorKind::Denied,
                            "declined in SSHCraft",
                            AgentOutcome::AskedDenied,
                        )),
                        target,
                        detail,
                    );
                }
                AgentOutcome::AskedAllowed
            }
        };
        let result = self
            .execute(&view, op)
            .await
            .map(|value| (value, outcome))
            .map_err(|error| Refusal {
                error,
                outcome: AgentOutcome::Failed,
            });
        (result, target, detail)
    }

    /// Checks a request against what is stored and what agents may see.
    #[allow(clippy::result_large_err)]
    fn resolve(
        &self,
        view: &View,
        action: Action,
    ) -> Result<(Op, String, String), (Refusal, String)> {
        fn with(target: &str) -> impl FnOnce(Refusal) -> (Refusal, String) + '_ {
            move |r| (r, target.to_owned())
        }
        let host_target = |h: &Host| match &h.settings.address {
            Some(address) => format!("{} ({address})", h.name),
            None => h.name.clone(),
        };
        Ok(match action {
            Action::ListHosts => (Op::ListHosts, String::new(), String::new()),
            Action::ListGroups => (Op::ListGroups, String::new(), String::new()),
            Action::ListKeys => (Op::ListKeys, String::new(), String::new()),
            Action::HostInfo { host } => {
                let h = view.host(&host).map_err(with(&host))?;
                let target = host_target(&h);
                (Op::HostInfo(h), target, String::new())
            }
            Action::Exec {
                host,
                command,
                timeout_secs,
            } => {
                let h = view.host(&host).map_err(with(&host))?;
                let secs = timeout_secs.unwrap_or(DEFAULT_EXEC_SECS);
                if !(1..=MAX_EXEC_SECS).contains(&secs) {
                    return Err((invalid("timeout_secs must be 1 to 600"), host));
                }
                if command.trim().is_empty()
                    || command.chars().count() > MAX_COMMAND
                    || command.contains('\0')
                {
                    return Err((invalid("command"), host));
                }
                let target = host_target(&h);
                (
                    Op::Exec(h.id, command.clone(), Duration::from_secs(secs)),
                    target,
                    command,
                )
            }
            Action::SessionOpen { host } => {
                let h = view.host(&host).map_err(with(&host))?;
                let target = host_target(&h);
                (Op::SessionOpen(h.id), target, String::new())
            }
            Action::SessionWrite { session, data } => {
                if !self.backend.session_owned(&session) {
                    return Err((not_found("session", false), session));
                }
                if data.len() > MAX_OUTPUT {
                    return Err((invalid("data too long"), session));
                }
                (
                    Op::SessionWrite(session.clone(), data.clone().into_bytes()),
                    session,
                    data,
                )
            }
            Action::SessionRead { session, wait_ms } => {
                if !self.backend.session_owned(&session) {
                    return Err((not_found("session", false), session));
                }
                let wait = Duration::from_millis(wait_ms.unwrap_or(0).min(MAX_WAIT_MS));
                (
                    Op::SessionRead(session.clone(), wait),
                    session,
                    String::new(),
                )
            }
            Action::SessionClose { session } => {
                if !self.backend.session_owned(&session) {
                    return Err((not_found("session", false), session));
                }
                (Op::SessionClose(session.clone()), session, String::new())
            }
            Action::SaveHost { host } => {
                let target = host.name.clone();
                if !name_ok(&host.name) {
                    return Err((invalid("name"), target));
                }
                let mut stored = match host.id {
                    Some(id) => view.host(&id.to_string()).map_err(with(&target))?,
                    None => Host {
                        id: HostId(Uuid::new_v4()),
                        name: String::new(),
                        group_id: None,
                        settings: Default::default(),
                        location: None,
                        tags: Vec::new(),
                        os: None,
                        forwards: Vec::new(),
                    },
                };
                let group = view.visible_group_id(host.group).map_err(with(&target))?;
                if let Some(key) = host.key
                    && !self
                        .backend
                        .keys()
                        .map_err(|e| {
                            (
                                Refusal {
                                    error: e,
                                    outcome: AgentOutcome::Failed,
                                },
                                target.clone(),
                            )
                        })?
                        .iter()
                        .any(|k| k.id == key)
                {
                    return Err((not_found("key", false), target));
                }
                stored.name = host.name;
                stored.group_id = group;
                stored.settings.address = host.address;
                stored.settings.port = host.port;
                stored.settings.username = host.username;
                stored.settings.key = host.key.map(|id| KeyRef::Stored { id: KeyId(id) });
                stored.tags = host.tags;
                (Op::SaveHost(Box::new(stored)), target, String::new())
            }
            Action::SaveGroup { group } => {
                let target = group.name.clone();
                if !name_ok(&group.name) {
                    return Err((invalid("name"), target));
                }
                let mut stored = match group.id {
                    Some(id) => view.group(&id.to_string()).map_err(with(&target))?,
                    None => Group {
                        id: GroupId(Uuid::new_v4()),
                        name: String::new(),
                        defaults: Default::default(),
                        location: None,
                        parent_id: None,
                        tags: Vec::new(),
                    },
                };
                stored.parent_id = view.visible_group_id(group.parent).map_err(with(&target))?;
                stored.name = group.name;
                (Op::SaveGroup(stored), target, String::new())
            }
            Action::DeleteHost { host } => {
                let h = view.host(&host).map_err(with(&host))?;
                let target = host_target(&h);
                (Op::DeleteHost(h.id), target, String::new())
            }
            Action::DeleteGroup { group } => {
                let g = view.group(&group).map_err(with(&group))?;
                let name = g.name.clone();
                (Op::DeleteGroup(g.id), name, String::new())
            }
            Action::DeleteKey { key } => {
                let keys = self.backend.keys().map_err(|e| {
                    (
                        Refusal {
                            error: e,
                            outcome: AgentOutcome::Failed,
                        },
                        key.clone(),
                    )
                })?;
                let found: Vec<_> = keys
                    .iter()
                    .filter(|k| k.id.to_string() == key || k.name == key)
                    .collect();
                match found.as_slice() {
                    [one] => (
                        Op::DeleteKey(KeyId(one.id)),
                        one.name.clone(),
                        String::new(),
                    ),
                    [] => return Err((not_found("key", false), key)),
                    _ => return Err((invalid("more than one key has this name, use its id"), key)),
                }
            }
            Action::GenerateKey { name, key_type } => {
                if !name_ok(&name) {
                    return Err((invalid("name"), name));
                }
                (Op::GenerateKey(name.clone(), key_type), name, String::new())
            }
            Action::ImportKey {
                name,
                text,
                passphrase,
            } => {
                if !name_ok(&name) {
                    return Err((invalid("name"), name));
                }
                // The key text never goes into the log or the question.
                (
                    Op::ImportKey(name.clone(), text, passphrase),
                    name,
                    String::new(),
                )
            }
        })
    }

    async fn execute(&self, view: &View, op: Op) -> Result<Value, AgentError> {
        let b = &self.backend;
        match op {
            Op::ListHosts => Ok(json(
                &view
                    .hosts
                    .iter()
                    .filter(|h| !view.host_hidden(h))
                    .map(host_view)
                    .collect::<Vec<_>>(),
            )),
            Op::ListGroups => Ok(json(
                &view
                    .groups
                    .iter()
                    .filter(|g| !view.group_hidden(g.id))
                    .map(group_view)
                    .collect::<Vec<_>>(),
            )),
            Op::ListKeys => Ok(json(&b.keys()?)),
            Op::HostInfo(host) => Ok(json(&host_view(&host))),
            Op::Exec(host, command, timeout) => Ok(json(&b.exec(host, &command, timeout).await?)),
            Op::SessionOpen(host) => Ok(serde_json::json!({ "session": b.session_open(host)? })),
            Op::SessionWrite(session, data) => {
                b.session_write(&session, &data)?;
                Ok(Value::Null)
            }
            Op::SessionRead(session, wait) => Ok(json(&b.session_read(&session, wait).await?)),
            Op::SessionClose(session) => {
                b.session_close(&session)?;
                Ok(Value::Null)
            }
            Op::SaveHost(host) => {
                let view = host_view(&host);
                b.save_host(*host)?;
                Ok(json(&view))
            }
            Op::SaveGroup(group) => {
                let view = group_view(&group);
                b.save_group(group)?;
                Ok(json(&view))
            }
            Op::DeleteHost(host) => {
                b.delete_host(host)?;
                Ok(Value::Null)
            }
            Op::DeleteGroup(group) => {
                b.delete_group(group)?;
                Ok(Value::Null)
            }
            Op::DeleteKey(key) => {
                b.delete_key(key)?;
                Ok(Value::Null)
            }
            Op::GenerateKey(name, key_type) => Ok(json(&b.generate_key(&name, key_type).await?)),
            Op::ImportKey(name, text, passphrase) => Ok(json(
                &b.import_key(&name, &text, passphrase.as_deref()).await?,
            )),
        }
    }
}

fn json<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// Last line of defence: an answer that looks like it holds a private key is not sent.
fn guard(value: Value) -> Result<Value, AgentError> {
    let text = value.to_string();
    if text.contains("PRIVATE KEY") || text.contains("PuTTY-User-Key-File") {
        return Err(AgentError::new(
            ErrorKind::Failed,
            "the answer looked like key material and was not sent",
        ));
    }
    Ok(value)
}

#[cfg(test)]
#[path = "gate_tests.rs"]
mod tests;

#[cfg(test)]
pub(crate) mod tests_support {
    pub(crate) use super::tests::Fake;
}
