//! The agent gate: what agents and the control channel may do. Every call that is not the user's
//! passes here: hidden hosts and groups (also behind a jump host) are answered as unknown, the
//! level of the command's category decides (allow, ask in SSHCraft, deny), the decision is logged
//! before anything runs, and SSHCraft's own data never goes out with key material in it.

pub mod access;
pub mod confirm;
pub mod scope;
pub mod view;
pub mod wire;

use serde_json::Value;
use sshcraft_store::agent::{AgentCategory, AgentLevel, AgentLogEntry, AgentOutcome, AgentPolicy, log_append};
use sshcraft_store::store::{Store, StoreError};
use uuid::Uuid;

pub use access::{AgentAccess, AskRule, Present, Refusal, Resolve, Resolved, SessionEffect};
pub use confirm::{AgentPrompt, Confirmer};
pub use scope::{AgentConnection, Connection};
pub use view::GateView;

use crate::caller::{Caller, ConnId};
use crate::cmd::CommandSpec;
use crate::error::{EngineError, ErrorCode};
use crate::events::EngineEvent;
use crate::sessions::SessionId;
use crate::{Ctx, Session};

/// Marks that `Share` and `Sync` were set back to their default once (R2.7).
const CATEGORIES_MIGRATION: &str = "migration.agent_categories";

/// Once per database: `Share` and `Sync` go back to their default (deny). Before R2 no agent
/// action used them, so a stored "allow" had no effect and must not start to have one silently.
/// A policy that cannot be read is left alone and tried again at the next start.
pub(crate) fn reset_categories_once(store: &Store) -> Result<(), StoreError> {
    if store.get_setting(CATEGORIES_MIGRATION)?.is_some() || AgentPolicy::load_checked(store).is_err() {
        return Ok(());
    }
    let mut policy = AgentPolicy::load(store);
    let reset = policy.levels.remove(&AgentCategory::Share).is_some() | policy.levels.remove(&AgentCategory::Sync).is_some();
    if reset {
        policy.save(store)?;
    }
    store.put_setting(CATEGORIES_MIGRATION, "1")
}

fn looks_like_key(text: &str) -> bool {
    (text.contains("-----BEGIN") && text.contains("PRIVATE KEY")) || text.contains("PuTTY-User-Key-File")
}

/// Last line of defence for SSHCraft's own data: an answer that looks like key material is not
/// sent.
fn guard(value: Value) -> Result<Value, EngineError> {
    if looks_like_key(&value.to_string()) {
        return Err(EngineError::failed(ErrorCode::Internal, "the answer looked like key material and was not sent"));
    }
    Ok(value)
}

/// The same for error messages, whatever produced them.
fn scrub(error: EngineError) -> EngineError {
    if looks_like_key(&error.to_string()) || error.detail_text().is_some_and(|d| looks_like_key(&d)) {
        EngineError::failed(ErrorCode::Internal, "the error message looked like key material and was not sent")
    } else {
        error
    }
}

/// Who asks and what, for the log and the question.
struct Who {
    agent: String,
    action: &'static str,
}

/// The time of a log entry: now, but always after the entry before, so the log keeps the order
/// in which things were decided (the decision before the failure).
fn log_time() -> i64 {
    static LAST: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
    let now = crate::now_ms();
    let previous =
        LAST.try_update(std::sync::atomic::Ordering::SeqCst, std::sync::atomic::Ordering::SeqCst, |last| Some(now.max(last.saturating_add(1))));
    let last = previous.unwrap_or(now);
    now.max(last.saturating_add(1))
}

impl Who {
    fn entry(&self, target: &str, detail: &str, outcome: AgentOutcome) -> AgentLogEntry {
        AgentLogEntry {
            id: Uuid::new_v4(),
            at: log_time(),
            agent: self.agent.clone(),
            action: self.action.to_string(),
            target: target.to_string(),
            detail: detail.to_string(),
            outcome,
        }
    }
}

/// Writes an entry into the sealed agent log and tells the interface.
fn write_log(session: &Session, entry: &AgentLogEntry) -> Result<(), EngineError> {
    let vault = session.vault()?;
    log_append(&vault, &session.store(), entry).map_err(|e| EngineError::failed(ErrorCode::Store, e))?;
    session.events().send(EngineEvent::AgentLogChanged);
    Ok(())
}

/// Writes its entry when dropped before [`LogGuard::done`]: an agent that went away while its
/// question was open leaves a trace.
struct LogGuard {
    session: Session,
    entry: Option<AgentLogEntry>,
}

impl LogGuard {
    fn done(mut self) {
        self.entry = None;
    }
}

impl Drop for LogGuard {
    fn drop(&mut self) {
        if let Some(entry) = self.entry.take() {
            let _ = write_log(&self.session, &entry);
        }
    }
}

/// The view, with the sessions of the connection whose host went away or became hidden closed.
fn load(session: &Session, conn: ConnId, category: AgentCategory) -> Result<(GateView, Connection), EngineError> {
    let vault = session.vault().ok();
    let mut view = GateView::load_for(&session.store(), vault.as_deref(), category)?;
    if category == AgentCategory::Share {
        // Listing shares takes the store's lock itself: done after the view released it.
        let servers = session.store().servers()?;
        for server in servers {
            view.shares.extend(sshcraft_sync::shares::list(&session.shared_store(), server.id).map_err(crate::sync::sync_error)?);
        }
    }
    let mut connection = session.connection(conn).ok_or_else(|| EngineError::Denied("unknown connection".to_string()))?;
    let gone: Vec<SessionId> = connection
        .sessions
        .iter()
        .filter(|(_, host)| view.hosts.iter().find(|h| h.id == **host).is_none_or(|h| view.host_hidden(h)))
        .map(|(id, _)| *id)
        .collect();
    for id in gone {
        session.agent_sessions().close(session.sessions(), id);
        session.forget_agent_session(conn, id);
        connection.sessions.remove(&id);
    }
    Ok((view, connection))
}

/// Runs command `spec` for an agent or the control channel.
pub(crate) async fn run_for_agent(session: &Session, spec: &'static CommandSpec, caller: Caller, params: Value) -> Result<Value, EngineError> {
    let (agent, conn) = match &caller {
        Caller::Agent { name, conn, .. } => (name.as_str().to_string(), *conn),
        Caller::Control { conn } => ("control".to_string(), *conn),
        Caller::User => return Err(EngineError::Denied("not an agent".to_string())),
    };
    let who = Who { agent, action: spec.id };
    if session.connection(conn).is_none() {
        return Err(EngineError::Denied("unknown connection".to_string()));
    }
    let Some(access) = spec.agent else {
        let _ = write_log(session, &who.entry("", "", AgentOutcome::Denied));
        return Err(EngineError::Denied("not allowed for agents".to_string()));
    };
    (spec.enabled)(session).map_err(|reason| EngineError::Disabled { id: spec.id.to_string(), reason })?;
    let log = |target: &str, detail: &str, outcome| {
        if access.logged {
            let _ = write_log(session, &who.entry(target, detail, outcome));
        }
    };

    let (mut view, connection) = load(session, conn, access.category)?;
    let mut resolved = match (access.resolve)(&view, &connection, params.clone()) {
        Ok(resolved) => resolved,
        Err(refusal) => {
            log(&refusal.target, "", refusal.outcome);
            return Err(refusal.error);
        }
    };
    let level = view.policy.level(access.category);
    let decided = match (access.ask, level) {
        // Closing one's own session is always allowed, so an agent can clean up.
        (AskRule::Cleanup, _) => AgentOutcome::Allowed,
        (_, AgentLevel::Deny) => {
            log(&resolved.target, &resolved.detail, AgentOutcome::Denied);
            return Err(EngineError::Denied("not allowed for agents".to_string()));
        }
        (_, AgentLevel::Allow) => AgentOutcome::Allowed,
        // Writing to and reading from a session this connection opened were allowed with opening
        // it; reading within a category needs no question either.
        (AskRule::OwnSession | AskRule::Read, AgentLevel::Ask) => AgentOutcome::Allowed,
        (AskRule::Level, AgentLevel::Ask) => {
            let guard = LogGuard { session: session.clone(), entry: Some(who.entry(&resolved.target, &resolved.detail, AgentOutcome::Cancelled)) };
            let prompt = AgentPrompt {
                prompt_id: 0,
                agent: who.agent.clone(),
                action: spec.id.to_string(),
                target: resolved.target.clone(),
                detail: resolved.detail.clone(),
            };
            let answer = match session.confirmer() {
                Some(confirmer) => tokio::time::timeout(session.confirm_timeout(), confirmer.confirm(prompt)).await.unwrap_or(false),
                None if !session.events().has_subscribers() => {
                    guard.done();
                    log(&resolved.target, &resolved.detail, AgentOutcome::AskedDenied);
                    return Err(EngineError::Denied("no SSHCraft window to ask in".to_string()));
                }
                None => tokio::time::timeout(session.confirm_timeout(), session.prompts().ask_agent(prompt)).await.unwrap_or(false),
            };
            guard.done();
            if !answer {
                log(&resolved.target, &resolved.detail, AgentOutcome::AskedDenied);
                return Err(EngineError::Denied("declined in SSHCraft".to_string()));
            }
            // The user may have changed or hidden something while the question was open.
            let (again, connection) = load(session, conn, access.category)?;
            view = again;
            match (access.resolve)(&view, &connection, params) {
                Ok(new) if new.target == resolved.target && new.detail == resolved.detail => resolved = new,
                Ok(_) => {
                    log(&resolved.target, &resolved.detail, AgentOutcome::Failed);
                    return Err(EngineError::failed(ErrorCode::Session, "changed while asking, ask again"));
                }
                Err(refusal) => {
                    log(&resolved.target, &resolved.detail, refusal.outcome);
                    return Err(refusal.error);
                }
            }
            AgentOutcome::AskedAllowed
        }
    };
    // The decision is on record before anything runs; without a log nothing runs.
    if access.logged
        && let Err(e) = write_log(session, &who.entry(&resolved.target, &resolved.detail, decided))
    {
        return Err(EngineError::failed(ErrorCode::Store, format!("the agent log cannot be written: {e}")));
    }
    let ctx = Ctx { session: session.clone(), caller };
    let result = session.run_spec(spec, ctx, resolved.params.clone()).await.and_then(|value| {
        match access.sessions {
            SessionEffect::Opens => {
                let id = value.get("session").and_then(|v| serde_json::from_value::<SessionId>(v.clone()).ok());
                let host = resolved.params.get("host").and_then(Value::as_str).and_then(|h| Uuid::parse_str(h).ok());
                if let (Some(id), Some(host)) = (id, host) {
                    session.own_agent_session(conn, id, sshcraft_model::model::HostId(host));
                }
            }
            SessionEffect::Closes => {
                if let Some(id) = resolved.params.get("id").and_then(|v| serde_json::from_value::<SessionId>(v.clone()).ok()) {
                    session.forget_agent_session(conn, id);
                }
            }
            SessionEffect::None => {}
        }
        let shown = (access.present)(&view, &resolved, value);
        if access.own_data { guard(shown) } else { Ok(shown) }
    });
    result.map_err(|error| {
        let error = scrub(error);
        log(&resolved.target, &error.to_string(), AgentOutcome::Failed);
        error
    })
}

#[cfg(test)]
#[path = "gate_tests.rs"]
mod tests;
