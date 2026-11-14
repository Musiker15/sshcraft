//! Running port forwarding rules. Each running rule has its own SSH connection, built like a
//! terminal's (jump hosts, proxy, keys, passphrase, Windows Hello), and reconnects the same way
//! after the connection is lost.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde::Serialize;
use sshcraft_model::forward::{ForwardId, ForwardKind, ForwardRule};
use sshcraft_model::model::HostId;
use sshcraft_model::policy::{Disconnect, next_reconnect_delay};
use sshcraft_ssh::{SshError, Tunnel};
use tokio::task::JoinHandle;

use crate::error::ErrorCode;
use crate::sessions::{Context, EndReason, Failure, OpenedKeys, connect_session, end_error, failure_code};

/// How often a running rule reports a changed number of connections.
const COUNT_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ForwardState {
    Starting,
    Running {
        bound_port: u16,
        connections: usize,
        /// Connections the server would not carry (for example `AllowTcpForwarding no`).
        refused: usize,
        /// Something worth knowing about the connection, such as an expired certificate.
        notice: Option<ErrorCode>,
    },
    Reconnecting {
        attempt: u32,
        delay_secs: u64,
    },
    Stopped,
    Failed {
        code: Option<ErrorCode>,
        detail: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ForwardStatus {
    pub rule_id: ForwardId,
    pub host_id: HostId,
    #[serde(flatten)]
    pub state: ForwardState,
}

pub trait ForwardSink: Send + Sync {
    fn status(&self, status: ForwardStatus);
}

/// Sends the state of forwarding rules on the engine's event bus.
pub struct BusSink(pub crate::events::EventBus);

impl ForwardSink for BusSink {
    fn status(&self, status: ForwardStatus) {
        self.0.send(crate::events::EngineEvent::ForwardStatus(status));
    }
}

/// A rule is known by its host and its id: ids are only unique within a host.
type RuleKey = (HostId, ForwardId);

struct Running {
    /// Which start of the rule this is; reports from an older one are dropped.
    generation: u64,
    task: JoinHandle<()>,
}

/// The running rules and their last status, under one lock: a status is only passed on while
/// its run is the current one, so a run that was just stopped cannot report after "stopped".
#[derive(Default)]
struct Table {
    running: HashMap<RuleKey, Running>,
    last: HashMap<RuleKey, ForwardStatus>,
}

struct Reporter {
    sink: Arc<dyn ForwardSink>,
    table: Mutex<Table>,
}

impl Reporter {
    fn status(key: RuleKey, state: ForwardState) -> ForwardStatus {
        ForwardStatus { host_id: key.0, rule_id: key.1, state }
    }

    /// From a run: passed on only while `generation` is the rule's current run.
    fn report(&self, key: RuleKey, generation: u64, state: ForwardState) {
        let mut table = self.table.lock().unwrap_or_else(PoisonError::into_inner);
        if table.running.get(&key).map(|r| r.generation) != Some(generation) {
            return;
        }
        let status = Self::status(key, state);
        if status.state == ForwardState::Stopped {
            table.last.remove(&key);
        } else {
            table.last.insert(key, status.clone());
        }
        // Sent under the lock, so statuses reach the window in order.
        self.sink.status(status);
    }

    /// Ends the rule's run (if any) and reports it stopped.
    fn stop(&self, key: RuleKey) {
        let mut table = self.table.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(running) = table.running.remove(&key) {
            running.task.abort();
        }
        table.last.remove(&key);
        self.sink.status(Self::status(key, ForwardState::Stopped));
    }
}

pub struct ForwardManager {
    ctx: Arc<Context>,
    reporter: Arc<Reporter>,
    generations: AtomicU64,
}

impl ForwardManager {
    pub(crate) fn new(ctx: Arc<Context>, sink: Arc<dyn ForwardSink>) -> Self {
        Self { ctx, reporter: Arc::new(Reporter { sink, table: Mutex::new(Table::default()) }), generations: AtomicU64::new(0) }
    }

    /// Starts the rule `rule_id` of host `host_id`; one that already runs is left alone.
    pub fn start(&self, host_id: HostId, rule_id: ForwardId) -> Result<(), ErrorCode> {
        let rule = {
            let store = self.ctx.store.lock().unwrap_or_else(PoisonError::into_inner);
            store
                .list_hosts()
                .map_err(|_| ErrorCode::Store)?
                .into_iter()
                .find(|h| h.id == host_id)
                .and_then(|h| h.forwards.into_iter().find(|r| r.id == rule_id))
                .ok_or(ErrorCode::NotFound)?
        };
        let key = (host_id, rule_id);
        let mut table = self.reporter.table.lock().unwrap_or_else(PoisonError::into_inner);
        if table.running.get(&key).is_some_and(|r| !r.task.is_finished()) {
            return Ok(());
        }
        let generation = self.generations.fetch_add(1, Ordering::SeqCst);
        let task = self.ctx.runtime().spawn(run(self.ctx.clone(), self.reporter.clone(), key, generation, rule));
        table.running.insert(key, Running { generation, task });
        Ok(())
    }

    /// Stops the rule: closes its port (or the server's), its connections and its SSH
    /// connection. A question it was waiting for (host key, password) is withdrawn. A rule that
    /// had failed loses its error.
    pub fn stop(&self, host_id: HostId, rule_id: ForwardId) {
        self.reporter.stop((host_id, rule_id));
    }

    /// Stops every rule of a host (the host is deleted).
    pub fn stop_host(&self, host_id: HostId) {
        self.keep_only(host_id, &[]);
    }

    /// Stops the host's rules that are not in `keep` (they were removed from the host).
    pub fn keep_only(&self, host_id: HostId, keep: &[ForwardId]) {
        for key in self.known() {
            if key.0 == host_id && !keep.contains(&key.1) {
                self.reporter.stop(key);
            }
        }
    }

    /// Stops every rule whose host or rule is no longer in the store (a deleted group, a sync,
    /// a share that was left).
    pub fn reconcile(&self) {
        let present: Vec<RuleKey> = {
            let store = self.ctx.store.lock().unwrap_or_else(PoisonError::into_inner);
            let Ok(hosts) = store.list_hosts() else {
                return;
            };
            hosts.iter().flat_map(|h| h.forwards.iter().map(move |r| (h.id, r.id))).collect()
        };
        for key in self.known() {
            if !present.contains(&key) {
                self.reporter.stop(key);
            }
        }
    }

    /// Rules that run or have a status.
    fn known(&self) -> Vec<RuleKey> {
        let table = self.reporter.table.lock().unwrap_or_else(PoisonError::into_inner);
        let mut keys: Vec<RuleKey> = table.running.keys().copied().collect();
        keys.extend(table.last.keys().filter(|k| !table.running.contains_key(k)));
        keys
    }

    /// The last status of every rule that runs or failed.
    pub fn statuses(&self) -> Vec<ForwardStatus> {
        self.reporter.table.lock().unwrap_or_else(PoisonError::into_inner).last.values().cloned().collect()
    }
}

impl Drop for ForwardManager {
    fn drop(&mut self) {
        for (_, running) in self.reporter.table.lock().unwrap_or_else(PoisonError::into_inner).running.drain() {
            running.task.abort();
        }
    }
}

/// Opens the tunnel the rule describes on an authenticated session.
async fn open(session: &sshcraft_ssh::Session, rule: &ForwardRule) -> Result<Tunnel, SshError> {
    match rule.kind {
        ForwardKind::Local => session.forward_local(&rule.bind_address, rule.bind_port, &rule.target_host, rule.target_port).await,
        ForwardKind::Remote => session.forward_remote(&rule.bind_address, rule.bind_port, &rule.target_host, rule.target_port).await,
        ForwardKind::Dynamic => session.forward_dynamic(&rule.bind_address, rule.bind_port).await,
    }
}

/// A tunnel that cannot open at the first start is not tried again. Once the rule ran, a taken
/// port or a refused forwarding is most likely the old connection the other side has not let go
/// of yet (a remote rule after a network change): then it is retried like a lost connection.
pub(crate) fn tunnel_failure(e: SshError, ever_running: bool) -> Failure {
    match e {
        SshError::PortInUse | SshError::ForwardRefused if ever_running => Failure::Lost(ErrorCode::Network, e.to_string()),
        SshError::PortInUse => end_error(ErrorCode::PortInUse, None),
        SshError::Bind(detail) => end_error(ErrorCode::BindFailed, Some(detail)),
        SshError::ForwardRefused => end_error(ErrorCode::ForwardRefused, None),
        other => Failure::Lost(ErrorCode::Network, other.to_string()),
    }
}

/// The life of one running rule: connect, forward, reconnect after a drop, end.
async fn run(ctx: Arc<Context>, reporter: Arc<Reporter>, key: RuleKey, generation: u64, rule: ForwardRule) {
    let report = |state| reporter.report(key, generation, state);
    let opened = OpenedKeys::default();
    let mut attempt = 0u32;
    let mut ever_running = false;
    loop {
        report(ForwardState::Starting);
        let started = async {
            let (session, notice) = connect_session(&ctx, key.0, &opened).await?;
            let tunnel = open(&session, &rule).await.map_err(|e| tunnel_failure(e, ever_running))?;
            Ok::<_, Failure>((session, tunnel, notice))
        }
        .await;
        match started {
            Ok((session, tunnel, notice)) => {
                ever_running = true;
                attempt = 0;
                let running = |tunnel: &Tunnel| ForwardState::Running {
                    bound_port: tunnel.bound_port(),
                    connections: tunnel.connections(),
                    refused: tunnel.refused(),
                    notice,
                };
                let mut shown = running(&tunnel);
                report(shown.clone());
                let closed = session.closed();
                tokio::pin!(closed);
                let mut tick = tokio::time::interval(COUNT_INTERVAL);
                loop {
                    tokio::select! {
                        () = &mut closed => break,
                        _ = tick.tick() => {
                            let now = running(&tunnel);
                            if now != shown {
                                shown = now;
                                report(shown.clone());
                            }
                        }
                    }
                }
                drop(tunnel);
            }
            // The user cancelled a sign-in question: that is a stop, not an error.
            Err(Failure::End(EndReason::AuthCancelled, ..)) => {
                report(ForwardState::Stopped);
                return;
            }
            Err(failure @ Failure::End(..)) => {
                let (code, detail) = failure_code(failure);
                if code == ErrorCode::SignInCancelled {
                    report(ForwardState::Stopped);
                    return;
                }
                report(ForwardState::Failed { code: Some(code), detail });
                return;
            }
            Err(Failure::Lost(code, detail)) if !ever_running => {
                report(ForwardState::Failed { code: Some(code), detail: Some(detail) });
                return;
            }
            Err(Failure::Lost(..)) => {}
        }
        attempt += 1;
        let lost = Disconnect::Lost("connection closed".into());
        match next_reconnect_delay(&lost, attempt) {
            Some(delay) => {
                report(ForwardState::Reconnecting { attempt, delay_secs: delay.as_secs() });
                tokio::time::sleep(delay.mul_f32(ctx.delay_scale)).await;
            }
            None => {
                report(ForwardState::Failed { code: Some(ErrorCode::Network), detail: None });
                return;
            }
        }
    }
}
