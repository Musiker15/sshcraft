//! Every agent request passes here: hidden hosts and groups (also behind a jump host) are
//! answered as unknown, the level of the action's kind decides (allow, ask in SSHCraft, deny),
//! the decision is logged before anything runs, and SSHCraft's own data never goes out with key
//! material in it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;
use sshcraft_agent::{
    Action, AgentError, DEFAULT_EXEC_SECS, ErrorKind, GroupView, HostInput, HostView,
    MAX_EXEC_SECS, MAX_OUTPUT, MAX_WAIT_MS, Request, Response, is_bidi_control, valid_agent_name,
};
use sshcraft_core::agent::{
    AgentCategory, AgentLevel, AgentLogEntry, AgentOutcome, AgentPolicy, MAX_DETAIL,
};
use sshcraft_core::model::{ConnectionSettings, Group, GroupId, Host, HostId, KeyId, KeyRef};
use uuid::Uuid;

use super::backend::{AgentBackend, AgentPrompt, Confirmer};

/// How long a question waits for the user; then it counts as no.
pub const CONFIRM_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_NAME: usize = 100;
/// A command is shown and logged in full: no longer than a log entry keeps.
pub const MAX_COMMAND: usize = MAX_DETAIL;
/// Jump hosts followed when checking a route.
const MAX_HOPS: usize = 16;

pub fn category(action: &Action) -> AgentCategory {
    match action {
        Action::ListHosts | Action::ListGroups | Action::ListKeys | Action::HostInfo { .. } => {
            AgentCategory::List
        }
        Action::Exec { .. }
        | Action::SessionOpen { .. }
        | Action::SessionWrite { .. }
        | Action::SessionRead { .. }
        | Action::SessionClose { .. }
        | Action::Cancel { .. } => AgentCategory::Connect,
        Action::SaveHost { .. } | Action::SaveGroup { .. } => AgentCategory::Edit,
        Action::DeleteHost { .. } | Action::DeleteGroup { .. } | Action::DeleteKey { .. } => {
            AgentCategory::Delete
        }
        Action::GenerateKey { .. } | Action::ImportKey { .. } => AgentCategory::Keys,
    }
}

/// One agent connection: the sessions opened over it (and on which host). They are closed when
/// the connection ends; no other connection reaches them.
#[derive(Default)]
pub struct Connection {
    sessions: Mutex<HashMap<String, HostId>>,
}

impl Connection {
    fn host_of(&self, session: &str) -> Option<HostId> {
        self.sessions
            .lock()
            .expect("sessions")
            .get(session)
            .copied()
    }

    /// The sessions still open, taken out (for closing).
    pub fn take_sessions(&self) -> Vec<String> {
        self.sessions
            .lock()
            .expect("sessions")
            .drain()
            .map(|(id, _)| id)
            .collect()
    }
}

/// A request resolved against what is stored, ready to run.
enum Op {
    ListHosts,
    ListGroups,
    ListKeys,
    HostInfo(Box<Host>),
    Exec(HostId, String, Duration),
    SessionOpen(HostId),
    SessionWrite(String, Vec<u8>),
    SessionRead(String, Duration),
    SessionClose(String),
    SaveHost(Box<Host>),
    SaveGroup(Box<Group>),
    DeleteHost(HostId),
    DeleteGroup(GroupId),
    DeleteKey(KeyId),
    GenerateKey(String, sshcraft_agent::KeyType),
    ImportKey(String, String, Option<String>),
}

impl Op {
    /// SSHCraft's own data: checked for key material before it goes out.
    fn own_data(&self) -> bool {
        !matches!(self, Op::Exec(..) | Op::SessionRead(..))
    }
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

fn denied(message: impl Into<String>) -> Refusal {
    refuse(ErrorKind::Denied, message, AgentOutcome::Denied)
}

fn failed(error: AgentError) -> Refusal {
    Refusal {
        error,
        outcome: AgentOutcome::Failed,
    }
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
        && !name.chars().any(|c| c.is_control() || is_bidi_control(c))
}

/// Whether these settings sign in with something stored: a password (also a proxy's), an
/// identity, or agent forwarding.
fn signs_in_with_stored(settings: &ConnectionSettings) -> bool {
    !settings.password_ids().is_empty()
        || settings.identity.is_some()
        || settings.agent_forwarding == Some(true)
}

/// What the gate knows while handling one request.
struct View {
    policy: AgentPolicy,
    hosts: Vec<Host>,
    groups: Vec<Group>,
}

impl View {
    fn group_chain(&self, group: Option<GroupId>) -> Vec<Group> {
        group
            .map(|g| sshcraft_core::model::group_chain(&self.groups, g))
            .unwrap_or_default()
    }

    /// The jump hosts a host uses: its own list, else the nearest group's.
    fn jumps(&self, host: &Host) -> Vec<HostId> {
        host.settings
            .jump_hosts
            .clone()
            .or_else(|| {
                self.group_chain(host.group_id)
                    .iter()
                    .find_map(|g| g.defaults.jump_hosts.clone())
            })
            .unwrap_or_default()
    }

    /// Hidden itself, through a group, or through a hidden host on its route.
    fn host_hidden(&self, host: &Host) -> bool {
        let mut seen = vec![host.id];
        let mut next = vec![host.clone()];
        while let Some(h) = next.pop() {
            if self.policy.host_hidden(&self.groups, &h) {
                return true;
            }
            for jump in self.jumps(&h) {
                if seen.contains(&jump) || seen.len() > MAX_HOPS {
                    continue;
                }
                seen.push(jump);
                if let Some(j) = self.hosts.iter().find(|x| x.id == jump) {
                    next.push(j.clone());
                }
            }
        }
        false
    }

    fn group_hidden(&self, group: GroupId) -> bool {
        self.policy.group_hidden(&self.groups, group)
    }

    /// Whether anything below `group` (groups at any depth, their hosts) is hidden.
    fn subtree_hidden(&self, group: GroupId) -> bool {
        let below: Vec<GroupId> = self
            .groups
            .iter()
            .filter(|g| {
                sshcraft_core::model::group_chain(&self.groups, g.id)
                    .iter()
                    .any(|c| c.id == group)
            })
            .map(|g| g.id)
            .collect();
        below.iter().any(|g| self.policy.hidden.contains(&g.0))
            || self
                .hosts
                .iter()
                .any(|h| h.group_id.is_some_and(|hg| below.contains(&hg)) && self.host_hidden(h))
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

    /// Whether a host here, with its groups, signs in with something stored.
    fn carries_sign_in(&self, host: &Host) -> bool {
        signs_in_with_stored(&host.settings)
            || self
                .group_chain(host.group_id)
                .iter()
                .any(|g| signs_in_with_stored(&g.defaults))
    }
}

fn shown<T: std::fmt::Display>(value: &Option<T>) -> String {
    value
        .as_ref()
        .map_or_else(|| "-".into(), ToString::to_string)
}

/// Applies a change to a host and describes it for the question: one line per changed field.
fn apply_host_change(
    view: &View,
    stored: &mut Host,
    input: HostInput,
    keys: &[sshcraft_agent::KeyView],
) -> Result<Vec<String>, Refusal> {
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
            let name = |g: Option<GroupId>| {
                g.and_then(|g| {
                    view.groups
                        .iter()
                        .find(|x| x.id == g)
                        .map(|x| x.name.clone())
                })
                .unwrap_or_else(|| "-".into())
            };
            changes.push(format!(
                "group: {} -> {}",
                name(stored.group_id),
                name(group)
            ));
            stored.group_id = group;
        }
    }
    if let Some(address) = input.address
        && address != stored.settings.address
    {
        changes.push(format!(
            "address: {} -> {}",
            shown(&stored.settings.address),
            shown(&address)
        ));
        stored.settings.address = address;
    }
    if let Some(port) = input.port
        && port != stored.settings.port
    {
        changes.push(format!(
            "port: {} -> {}",
            shown(&stored.settings.port),
            shown(&port)
        ));
        stored.settings.port = port;
    }
    if let Some(username) = input.username
        && username != stored.settings.username
    {
        changes.push(format!(
            "user: {} -> {}",
            shown(&stored.settings.username),
            shown(&username)
        ));
        stored.settings.username = username;
    }
    if let Some(key) = input.key {
        if let Some(id) = key
            && !keys.iter().any(|k| k.id == id)
        {
            return Err(not_found("key", false));
        }
        let new = key.map(|id| KeyRef::Stored { id: KeyId(id) });
        if new != stored.settings.key {
            let name = |k: &Option<KeyRef>| match k {
                Some(KeyRef::Stored { id }) => keys
                    .iter()
                    .find(|x| x.id == id.0)
                    .map_or_else(|| id.0.to_string(), |x| x.name.clone()),
                Some(_) => "agent key".into(),
                None => "-".into(),
            };
            changes.push(format!(
                "key: {} -> {}",
                name(&stored.settings.key),
                name(&new)
            ));
            stored.settings.key = new;
        }
    }
    if let Some(tags) = input.tags
        && tags != stored.tags
    {
        changes.push(format!(
            "tags: {} -> {}",
            stored.tags.join(", "),
            tags.join(", ")
        ));
        stored.tags = tags;
    }
    Ok(changes)
}

/// Writes its entry when dropped without [`LogGuard::done`]: an agent that went away while its
/// question was open leaves a trace.
struct LogGuard<'a, B: AgentBackend> {
    backend: &'a B,
    entry: Option<AgentLogEntry>,
}

impl<B: AgentBackend> LogGuard<'_, B> {
    fn done(mut self) {
        self.entry = None;
    }
}

impl<B: AgentBackend> Drop for LogGuard<'_, B> {
    fn drop(&mut self) {
        if let Some(entry) = self.entry.take() {
            let _ = self.backend.log(entry);
        }
    }
}

pub struct Gate<B: AgentBackend, C: Confirmer> {
    pub backend: Arc<B>,
    pub confirmer: Arc<C>,
    pub confirm_timeout: Duration,
}

struct Ask<'a> {
    agent: &'a str,
    action: &'a str,
}

impl<B: AgentBackend, C: Confirmer> Gate<B, C> {
    pub fn new(backend: Arc<B>, confirmer: Arc<C>) -> Self {
        Self {
            backend,
            confirmer,
            confirm_timeout: CONFIRM_TIMEOUT,
        }
    }

    fn view(&self) -> Result<View, Refusal> {
        let policy = self.backend.policy().map_err(failed)?;
        Ok(View {
            policy,
            hosts: self.backend.hosts().map_err(failed)?,
            groups: self.backend.groups().map_err(failed)?,
        })
    }

    fn entry(
        &self,
        who: &Ask<'_>,
        target: &str,
        detail: &str,
        outcome: AgentOutcome,
    ) -> AgentLogEntry {
        AgentLogEntry {
            id: Uuid::new_v4(),
            at: crate::recording::now_ms(),
            agent: who.agent.to_owned(),
            action: who.action.to_owned(),
            target: target.to_owned(),
            detail: detail.to_owned(),
            outcome,
        }
    }

    /// Closes the sessions of a connection that ended.
    pub fn connection_closed(&self, conn: &Connection) {
        for session in conn.take_sessions() {
            let _ = self.backend.session_close(&session);
        }
    }

    pub async fn handle(&self, request: Request, conn: &Connection) -> Response {
        let id = request.id;
        if !valid_agent_name(&request.agent) {
            return Response::error(id, AgentError::new(ErrorKind::Invalid, "agent name"));
        }
        let action = request.action;
        let name = action.name().to_owned();
        let who = Ask {
            agent: &request.agent,
            action: &name,
        };
        let category = category(&action);
        // Looking around is not logged: a flood of lists must not push actions out of the log.
        let logged =
            category != AgentCategory::List && !matches!(action, Action::SessionRead { .. });
        let log = |target: &str, detail: &str, outcome| {
            if logged {
                let _ = self.backend.log(self.entry(&who, target, detail, outcome));
            }
        };

        let mut view = match self.view() {
            Ok(view) => view,
            Err(r) => return Response::error(id, r.error),
        };
        let (mut op, mut target, mut detail) = match self.resolve(&view, conn, action.clone()) {
            Ok(resolved) => resolved,
            Err((refusal, target)) => {
                log(&target, "", refusal.outcome);
                return Response::error(id, refusal.error);
            }
        };
        let level = view.policy.level(category);
        let decided = match (&op, level) {
            // Closing one's own session is always allowed, so an agent can clean up.
            (Op::SessionClose(_), _) => AgentOutcome::Allowed,
            (_, AgentLevel::Deny) => {
                log(&target, &detail, AgentOutcome::Denied);
                return Response::error(id, denied("not allowed for agents").error);
            }
            (_, AgentLevel::Allow) => AgentOutcome::Allowed,
            // Writing to and reading from a session opened over this connection were allowed
            // with opening it.
            (Op::SessionWrite(..) | Op::SessionRead(..), AgentLevel::Ask) => AgentOutcome::Allowed,
            (_, AgentLevel::Ask) => {
                let guard = LogGuard {
                    backend: self.backend.as_ref(),
                    entry: Some(self.entry(&who, &target, &detail, AgentOutcome::Cancelled)),
                };
                let prompt = AgentPrompt {
                    agent: who.agent.to_owned(),
                    action: name.clone(),
                    target: target.clone(),
                    detail: detail.clone(),
                };
                let answer =
                    tokio::time::timeout(self.confirm_timeout, self.confirmer.confirm(prompt))
                        .await
                        .unwrap_or(false);
                guard.done();
                if !answer {
                    log(&target, &detail, AgentOutcome::AskedDenied);
                    return Response::error(
                        id,
                        AgentError::new(ErrorKind::Denied, "declined in SSHCraft"),
                    );
                }
                // The user may have changed or hidden something while the question was open.
                view = match self.view() {
                    Ok(view) => view,
                    Err(r) => return Response::error(id, r.error),
                };
                match self.resolve(&view, conn, action) {
                    Ok((new_op, new_target, new_detail)) => {
                        if new_detail != detail || new_target != target {
                            log(&target, &detail, AgentOutcome::Failed);
                            return Response::error(
                                id,
                                AgentError::new(
                                    ErrorKind::Failed,
                                    "changed while asking, ask again",
                                ),
                            );
                        }
                        op = new_op;
                        target = new_target;
                        detail = new_detail;
                    }
                    Err((refusal, _)) => {
                        log(&target, &detail, refusal.outcome);
                        return Response::error(id, refusal.error);
                    }
                }
                AgentOutcome::AskedAllowed
            }
        };
        // The decision is on record before anything runs; without a log nothing runs.
        if logged
            && let Err(e) = self
                .backend
                .log(self.entry(&who, &target, &detail, decided))
        {
            return Response::error(
                id,
                AgentError::new(
                    ErrorKind::Failed,
                    format!("the agent log cannot be written: {}", e.message),
                ),
            );
        }
        let checked = op.own_data();
        let result = match self.execute(&view, conn, op).await {
            Ok(value) if checked => guard(value),
            other => other,
        }
        .map_err(scrub);
        match result {
            Ok(value) => Response::ok(id, value),
            Err(error) => {
                log(&target, &error.message, AgentOutcome::Failed);
                Response::error(id, error)
            }
        }
    }

    /// Checks a request against what is stored and what agents may see.
    #[allow(clippy::result_large_err)]
    fn resolve(
        &self,
        view: &View,
        conn: &Connection,
        action: Action,
    ) -> Result<(Op, String, String), (Refusal, String)> {
        fn with(target: &str) -> impl FnOnce(Refusal) -> (Refusal, String) + '_ {
            move |r| (r, target.to_owned())
        }
        let host_target = |h: &Host| match &h.settings.address {
            Some(address) => format!("{} ({address})", h.name),
            None => h.name.clone(),
        };
        // A session of this connection whose host is still visible.
        let session = |session: &str| -> Result<(), (Refusal, String)> {
            let Some(host) = conn.host_of(session) else {
                return Err((not_found("session", false), session.to_owned()));
            };
            match view.hosts.iter().find(|h| h.id == host) {
                Some(h) if !view.host_hidden(h) => Ok(()),
                _ => {
                    // Hidden or gone meanwhile: the session ends.
                    conn.sessions.lock().expect("sessions").remove(session);
                    let _ = self.backend.session_close(session);
                    Err((not_found("session", true), session.to_owned()))
                }
            }
        };
        Ok(match action {
            Action::ListHosts => (Op::ListHosts, String::new(), String::new()),
            Action::ListGroups => (Op::ListGroups, String::new(), String::new()),
            Action::ListKeys => (Op::ListKeys, String::new(), String::new()),
            Action::Cancel { .. } => {
                return Err((invalid("cancel is not an action"), String::new()));
            }
            Action::HostInfo { host } => {
                let h = view.host(&host).map_err(with(&host))?;
                let target = host_target(&h);
                (Op::HostInfo(Box::new(h)), target, String::new())
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
                    return Err((
                        invalid(format!("command: 1 to {MAX_COMMAND} characters")),
                        host,
                    ));
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
            Action::SessionWrite { session: s, data } => {
                session(&s)?;
                if data.len() > MAX_OUTPUT {
                    return Err((invalid("data too long"), s));
                }
                let detail: String = data.chars().take(MAX_DETAIL).collect();
                (Op::SessionWrite(s.clone(), data.into_bytes()), s, detail)
            }
            Action::SessionRead {
                session: s,
                wait_ms,
            } => {
                session(&s)?;
                let wait = Duration::from_millis(wait_ms.unwrap_or(0).min(MAX_WAIT_MS));
                (Op::SessionRead(s.clone(), wait), s, String::new())
            }
            Action::SessionClose { session: s } => {
                if conn.host_of(&s).is_none() {
                    return Err((not_found("session", false), s));
                }
                (Op::SessionClose(s.clone()), s, String::new())
            }
            Action::SaveHost { host } => {
                let keys = self
                    .backend
                    .keys()
                    .map_err(|e| (failed(e), String::new()))?;
                let (mut stored, new) = match host.id {
                    Some(id) => (
                        view.host(&id.to_string()).map_err(with(&id.to_string()))?,
                        false,
                    ),
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
                    return Err((invalid("name"), String::new()));
                }
                let before = stored.clone();
                let target = host.name.clone().unwrap_or_else(|| stored.name.clone());
                let changes =
                    apply_host_change(view, &mut stored, host, &keys).map_err(with(&target))?;
                // Stored sign-in data (a password, an identity, agent forwarding) must not be
                // pointed at another machine by an agent.
                let moved = new
                    || stored.settings.address != before.settings.address
                    || stored.settings.port != before.settings.port
                    || stored.group_id != before.group_id;
                if moved && view.carries_sign_in(&stored) {
                    return Err((
                        denied(
                            "this host signs in with stored data; agents cannot point it elsewhere",
                        ),
                        target,
                    ));
                }
                let mut detail = changes.join("\n");
                if new {
                    detail = format!("new host\n{detail}");
                }
                (Op::SaveHost(Box::new(stored)), target, detail)
            }
            Action::SaveGroup { group } => {
                let target = group.name.clone();
                if !name_ok(&group.name) {
                    return Err((invalid("name"), target));
                }
                let (mut stored, new) = match group.id {
                    Some(id) => (view.group(&id.to_string()).map_err(with(&target))?, false),
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
                let parent = view.visible_group_id(group.parent).map_err(with(&target))?;
                let moved = !new && parent != stored.parent_id;
                // Moving a group moves its hosts: the same rule as for a host's address.
                if moved {
                    let group_id = stored.id;
                    let below_signs_in = signs_in_with_stored(&stored.defaults)
                        || view.hosts.iter().any(|h| {
                            h.group_id.is_some_and(|g| {
                                sshcraft_core::model::group_chain(&view.groups, g)
                                    .iter()
                                    .any(|c| c.id == group_id)
                            }) && signs_in_with_stored(&h.settings)
                        });
                    if below_signs_in {
                        return Err((
                            denied(
                                "hosts in this group sign in with stored data; agents cannot move it",
                            ),
                            target,
                        ));
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
                (Op::SaveGroup(Box::new(stored)), target, detail)
            }
            Action::DeleteHost { host } => {
                let h = view.host(&host).map_err(with(&host))?;
                let target = host_target(&h);
                (Op::DeleteHost(h.id), target, String::new())
            }
            Action::DeleteGroup { group } => {
                let g = view.group(&group).map_err(with(&group))?;
                if view.subtree_hidden(g.id) {
                    return Err((denied("not allowed for agents"), g.name));
                }
                let name = g.name.clone();
                (Op::DeleteGroup(g.id), name, String::new())
            }
            Action::DeleteKey { key } => {
                let keys = self.backend.keys().map_err(|e| (failed(e), key.clone()))?;
                let found: Vec<_> = keys
                    .iter()
                    .filter(|k| k.id.to_string() == key || k.name == key)
                    .collect();
                let one = match found.as_slice() {
                    [one] => *one,
                    [] => return Err((not_found("key", false), key)),
                    _ => {
                        return Err((invalid("more than one key has this name, use its id"), key));
                    }
                };
                let uses = |s: &ConnectionSettings| matches!(&s.key, Some(KeyRef::Stored { id }) if id.0 == one.id);
                let hidden_user = view
                    .hosts
                    .iter()
                    .any(|h| uses(&h.settings) && view.host_hidden(h))
                    || view
                        .groups
                        .iter()
                        .any(|g| uses(&g.defaults) && view.group_hidden(g.id));
                if hidden_user {
                    return Err((denied("not allowed for agents"), one.name.clone()));
                }
                (
                    Op::DeleteKey(KeyId(one.id)),
                    one.name.clone(),
                    String::new(),
                )
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

    async fn execute(&self, view: &View, conn: &Connection, op: Op) -> Result<Value, AgentError> {
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
            Op::SessionOpen(host) => {
                let session = b.session_open(host)?;
                conn.sessions
                    .lock()
                    .expect("sessions")
                    .insert(session.clone(), host);
                Ok(serde_json::json!({ "session": session }))
            }
            Op::SessionWrite(session, data) => {
                b.session_write(&session, &data)?;
                Ok(Value::Null)
            }
            Op::SessionRead(session, wait) => Ok(json(&b.session_read(&session, wait).await?)),
            Op::SessionClose(session) => {
                conn.sessions.lock().expect("sessions").remove(&session);
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
                b.save_group(*group)?;
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

fn looks_like_key(text: &str) -> bool {
    (text.contains("-----BEGIN") && text.contains("PRIVATE KEY"))
        || text.contains("PuTTY-User-Key-File")
}

/// Last line of defence for SSHCraft's own data: an answer that looks like key material is not
/// sent.
fn guard(value: Value) -> Result<Value, AgentError> {
    if looks_like_key(&value.to_string()) {
        return Err(AgentError::new(
            ErrorKind::Failed,
            "the answer looked like key material and was not sent",
        ));
    }
    Ok(value)
}

/// The same for error messages, whatever produced them.
fn scrub(error: AgentError) -> AgentError {
    if looks_like_key(&error.message) {
        AgentError::new(
            error.kind,
            "the error message looked like key material and was not sent",
        )
    } else {
        error
    }
}

#[cfg(test)]
#[path = "gate_tests.rs"]
mod tests;

#[cfg(test)]
pub(crate) mod tests_support {
    pub(crate) use super::tests::Fake;
}
