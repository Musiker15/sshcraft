//! Running port forwarding rules. Each running rule has its own SSH connection, built like a
//! terminal's (jump hosts, proxy, keys, passphrase, Windows Hello), and reconnects the same way
//! after the connection is lost.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use sshcraft_core::forward::{ForwardId, ForwardKind, ForwardRule};
use sshcraft_core::model::HostId;
use sshcraft_core::policy::{Disconnect, next_reconnect_delay};
use sshcraft_ssh::{SshError, Tunnel};
use tokio::task::JoinHandle;

use crate::errors::ErrorCode;
use crate::sessions::{Context, Failure, OpenedKeys, connect_session, failure_code};

/// How often a running rule reports a changed number of connections.
const COUNT_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ForwardState {
    Starting,
    Running {
        bound_port: u16,
        connections: usize,
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

/// Passes every status on and keeps the last one per rule for [`ForwardManager::statuses`].
struct Reporter {
    sink: Arc<dyn ForwardSink>,
    last: Mutex<HashMap<ForwardId, ForwardStatus>>,
}

impl Reporter {
    fn report(&self, host_id: HostId, rule_id: ForwardId, state: ForwardState) {
        let status = ForwardStatus {
            rule_id,
            host_id,
            state,
        };
        {
            let mut last = self.last.lock().expect("last lock");
            if status.state == ForwardState::Stopped {
                last.remove(&rule_id);
            } else {
                last.insert(rule_id, status.clone());
            }
        }
        self.sink.status(status);
    }
}

struct Running {
    host_id: HostId,
    task: JoinHandle<()>,
}

pub struct ForwardManager {
    ctx: Arc<Context>,
    reporter: Arc<Reporter>,
    running: Mutex<HashMap<ForwardId, Running>>,
}

impl ForwardManager {
    pub(crate) fn new(ctx: Arc<Context>, sink: Arc<dyn ForwardSink>) -> Self {
        Self {
            ctx,
            reporter: Arc::new(Reporter {
                sink,
                last: Mutex::new(HashMap::new()),
            }),
            running: Mutex::new(HashMap::new()),
        }
    }

    /// Starts the rule `rule_id` of host `host_id`; one that already runs is left alone.
    pub fn start(&self, host_id: HostId, rule_id: ForwardId) -> Result<(), ErrorCode> {
        let rule = {
            let store = self.ctx.store.lock().expect("store lock");
            store
                .list_hosts()
                .map_err(|_| ErrorCode::Store)?
                .into_iter()
                .find(|h| h.id == host_id)
                .and_then(|h| h.forwards.into_iter().find(|r| r.id == rule_id))
                .ok_or(ErrorCode::NotFound)?
        };
        let mut running = self.running.lock().expect("running lock");
        if running.get(&rule_id).is_some_and(|r| !r.task.is_finished()) {
            return Ok(());
        }
        let task = tokio::spawn(run(self.ctx.clone(), self.reporter.clone(), host_id, rule));
        running.insert(rule_id, Running { host_id, task });
        Ok(())
    }

    /// Stops the rule: closes its port (or the server's), its connections and its SSH
    /// connection. A question it was waiting for (host key, password) is withdrawn.
    pub fn stop(&self, rule_id: ForwardId) {
        let stopped = self.running.lock().expect("running lock").remove(&rule_id);
        if let Some(running) = stopped {
            running.task.abort();
            self.reporter
                .report(running.host_id, rule_id, ForwardState::Stopped);
        } else {
            // A rule that failed on its own: forget its error.
            self.reporter
                .last
                .lock()
                .expect("last lock")
                .remove(&rule_id);
        }
    }

    /// Stops every rule of a host (the host is deleted).
    pub fn stop_host(&self, host_id: HostId) {
        self.keep_only(host_id, &[]);
    }

    /// Stops the host's rules that are not in `keep` (they were removed from the host).
    pub fn keep_only(&self, host_id: HostId, keep: &[ForwardId]) {
        let rules: Vec<ForwardId> = self
            .running
            .lock()
            .expect("running lock")
            .iter()
            .filter(|(id, r)| r.host_id == host_id && !keep.contains(id))
            .map(|(id, _)| *id)
            .collect();
        for rule in rules {
            self.stop(rule);
        }
    }

    /// The last status of every rule that runs or failed.
    pub fn statuses(&self) -> Vec<ForwardStatus> {
        self.reporter
            .last
            .lock()
            .expect("last lock")
            .values()
            .cloned()
            .collect()
    }
}

impl Drop for ForwardManager {
    fn drop(&mut self) {
        for (_, running) in self.running.lock().expect("running lock").drain() {
            running.task.abort();
        }
    }
}

/// Opens the tunnel the rule describes on an authenticated session.
async fn open(session: &sshcraft_ssh::Session, rule: &ForwardRule) -> Result<Tunnel, SshError> {
    match rule.kind {
        ForwardKind::Local => {
            session
                .forward_local(
                    &rule.bind_address,
                    rule.bind_port,
                    &rule.target_host,
                    rule.target_port,
                )
                .await
        }
        ForwardKind::Remote => {
            session
                .forward_remote(
                    &rule.bind_address,
                    rule.bind_port,
                    &rule.target_host,
                    rule.target_port,
                )
                .await
        }
        ForwardKind::Dynamic => {
            session
                .forward_dynamic(&rule.bind_address, rule.bind_port)
                .await
        }
    }
}

/// A tunnel that cannot open is not tried again; anything else is the network.
fn tunnel_failure(e: SshError) -> Failure {
    match e {
        SshError::PortInUse => crate::sessions::end_error(ErrorCode::PortInUse, None),
        SshError::Bind(detail) => crate::sessions::end_error(ErrorCode::BindFailed, Some(detail)),
        SshError::ForwardRefused => crate::sessions::end_error(ErrorCode::ForwardRefused, None),
        other => Failure::Lost(ErrorCode::Network, other.to_string()),
    }
}

/// The life of one running rule: connect, forward, reconnect after a drop, end.
async fn run(ctx: Arc<Context>, reporter: Arc<Reporter>, host_id: HostId, rule: ForwardRule) {
    let report = |state| reporter.report(host_id, rule.id, state);
    let opened = OpenedKeys::default();
    let mut attempt = 0u32;
    let mut ever_running = false;
    loop {
        report(ForwardState::Starting);
        let started = async {
            let (session, _notice) = connect_session(&ctx, host_id, &opened).await?;
            let tunnel = open(&session, &rule).await.map_err(tunnel_failure)?;
            Ok::<_, Failure>((session, tunnel))
        }
        .await;
        match started {
            Ok((session, tunnel)) => {
                ever_running = true;
                attempt = 0;
                let mut shown = tunnel.connections();
                report(ForwardState::Running {
                    bound_port: tunnel.bound_port(),
                    connections: shown,
                });
                let closed = session.closed();
                tokio::pin!(closed);
                let mut tick = tokio::time::interval(COUNT_INTERVAL);
                loop {
                    tokio::select! {
                        () = &mut closed => break,
                        _ = tick.tick() => {
                            let now = tunnel.connections();
                            if now != shown {
                                shown = now;
                                report(ForwardState::Running {
                                    bound_port: tunnel.bound_port(),
                                    connections: now,
                                });
                            }
                        }
                    }
                }
                drop(tunnel);
            }
            Err(failure @ Failure::End(..)) => {
                let (code, detail) = failure_code(failure);
                report(ForwardState::Failed {
                    code: Some(code),
                    detail,
                });
                return;
            }
            Err(Failure::Lost(code, detail)) if !ever_running => {
                report(ForwardState::Failed {
                    code: Some(code),
                    detail: Some(detail),
                });
                return;
            }
            Err(Failure::Lost(..)) => {}
        }
        attempt += 1;
        let lost = Disconnect::Lost("connection closed".into());
        match next_reconnect_delay(&lost, attempt) {
            Some(delay) => {
                report(ForwardState::Reconnecting {
                    attempt,
                    delay_secs: delay.as_secs(),
                });
                tokio::time::sleep(delay.mul_f32(ctx.delay_scale)).await;
            }
            None => {
                report(ForwardState::Failed {
                    code: Some(ErrorCode::Network),
                    detail: None,
                });
                return;
            }
        }
    }
}
