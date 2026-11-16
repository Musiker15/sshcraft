//! Runs a host's steps before connecting and after its session ended: a Wake-on-LAN packet, port
//! knocks on the host's address, a local program from its argument list (never a shell), only
//! once allowed on this device. A failed wake or knock does not stop the connection; a program
//! that is not allowed does.

use std::sync::{Arc, PoisonError};
use std::time::Duration;

use sshcraft_model::hooks::{ConnectStep, approval_argv, magic_packet, parse_mac};
use sshcraft_model::model::{Group, Host, HostId, group_chain};

use crate::error::ErrorCode;
use crate::sessions::{Context, Failure, allow_program, end_error, program_allowed};

/// How long one knock waits for its connection.
const KNOCK_TIMEOUT: Duration = Duration::from_millis(300);
/// How long a program may run.
const PROGRAM_TIMEOUT: Duration = Duration::from_secs(60);

/// Which list of steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum When {
    Before,
    After,
}

fn steps_for(host: &Host, groups: &[Group], when: When) -> Option<Vec<ConnectStep>> {
    let pick = |s: &sshcraft_model::model::ConnectionSettings| match when {
        When::Before => s.before_connect.clone(),
        When::After => s.after_disconnect.clone(),
    };
    pick(&host.settings).or_else(|| host.group_id.map(|g| group_chain(groups, g)).unwrap_or_default().iter().find_map(|g| pick(&g.defaults)))
}

/// The host's steps `when`, with its name and address; none when it has none.
fn load(ctx: &Context, host_id: HostId, when: When) -> Option<(Vec<ConnectStep>, String, String)> {
    let store = ctx.store.lock().unwrap_or_else(PoisonError::into_inner);
    let host = store.list_hosts().ok()?.into_iter().find(|h| h.id == host_id)?;
    let groups = store.list_groups().ok()?;
    let steps = steps_for(&host, &groups, when).filter(|s| !s.is_empty())?;
    sshcraft_model::hooks::check(&steps).ok()?;
    let chain = host.group_id.map(|g| group_chain(&groups, g)).unwrap_or_default();
    let refs: Vec<&Group> = chain.iter().collect();
    let address = sshcraft_model::model::effective_endpoint(&host, &refs).map(|(a, _)| a).unwrap_or_default();
    Some((steps, host.name, address))
}

/// Runs the host's steps after the session once dropped: when the last user of a connection let
/// go of it, or when connecting failed after the steps before had run.
pub(crate) struct AfterGuard {
    ctx: Arc<Context>,
    host: HostId,
}

impl AfterGuard {
    pub(crate) fn new(ctx: Arc<Context>, host: HostId) -> Self {
        Self { ctx, host }
    }
}

impl Drop for AfterGuard {
    fn drop(&mut self) {
        let (ctx, host) = (self.ctx.clone(), self.host);
        self.ctx.runtime.spawn(async move {
            let _done = run(&ctx, host, When::After).await;
        });
    }
}

/// Runs the host's steps `when`.
pub(crate) async fn run(ctx: &Context, host_id: HostId, when: When) -> Result<(), Failure> {
    let Some((steps, name, address)) = load(ctx, host_id, when) else { return Ok(()) };
    for step in &steps {
        match step {
            ConnectStep::WakeOnLan { mac, broadcast, port, wait_secs } => {
                if let Some(mac) = parse_mac(mac) {
                    // A computer that does not wake shows as a failed connection after.
                    let _sent = wake(mac, broadcast.as_deref().unwrap_or("255.255.255.255"), port.unwrap_or(9)).await;
                }
                pause(ctx, Duration::from_secs(u64::from(*wait_secs))).await;
            }
            ConnectStep::Knock { ports, delay_ms } => {
                // `[::1]` as written in the host, `::1` as the system wants it.
                let address = address.trim_start_matches('[').trim_end_matches(']');
                for port in ports {
                    // A knock is only a try: the port is meant to stay closed.
                    let _knocked = tokio::time::timeout(KNOCK_TIMEOUT, tokio::net::TcpStream::connect((address, *port))).await;
                    pause(ctx, Duration::from_millis(u64::from(*delay_ms))).await;
                }
            }
            ConnectStep::Command { argv, wait_secs } => {
                // After the session nobody is asked any more: a program not allowed here is
                // left out.
                if when == When::After {
                    if !program_allowed(ctx, &approval_argv(argv)) {
                        continue;
                    }
                } else {
                    allow_program(ctx, &name, &approval_argv(argv), argv).await?;
                }
                program(argv).await?;
                pause(ctx, Duration::from_secs(u64::from(*wait_secs))).await;
            }
        }
    }
    Ok(())
}

async fn pause(ctx: &Context, wait: Duration) {
    if !wait.is_zero() {
        tokio::time::sleep(wait.mul_f32(ctx.delay_scale)).await;
    }
}

async fn wake(mac: [u8; 6], broadcast: &str, port: u16) -> std::io::Result<()> {
    let socket = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
    socket.set_broadcast(true)?;
    socket.send_to(&magic_packet(mac), (broadcast, port)).await?;
    Ok(())
}

/// Runs `argv` without a shell and waits for it, at most [`PROGRAM_TIMEOUT`].
async fn program(argv: &[String]) -> Result<(), Failure> {
    let Some((program, args)) = argv.split_first() else { return Ok(()) };
    let mut child = tokio::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| end_error(ErrorCode::ConnectStepFailed, Some(format!("{program}: {e}"))))?;
    match tokio::time::timeout(PROGRAM_TIMEOUT, child.wait()).await {
        Ok(Ok(status)) if status.success() => Ok(()),
        Ok(Ok(status)) => Err(end_error(ErrorCode::ConnectStepFailed, Some(format!("{program}: {status}")))),
        Ok(Err(e)) => Err(end_error(ErrorCode::ConnectStepFailed, Some(format!("{program}: {e}")))),
        Err(_) => Err(end_error(ErrorCode::ConnectStepFailed, Some(format!("{program}: ran longer than a minute")))),
    }
}
