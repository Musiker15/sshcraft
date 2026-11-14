//! Port forwarding over an SSH session: local (`-L`), remote (`-R`) and dynamic (`-D`, SOCKS5).
//!
//! A [`Tunnel`] runs until it is dropped. Dropping it stops listening (or cancels the remote
//! forwarding) and ends every connection that runs through it.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use russh::client;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::{ClientHandler, SshError, protocol, socks};

/// Connections one tunnel carries at a time; more are closed right away.
pub const MAX_TUNNEL_CONNECTIONS: usize = 256;
const SOCKS_TIMEOUT: Duration = Duration::from_secs(10);
const OPEN_TIMEOUT: Duration = Duration::from_secs(15);
/// The pause after a failed accept, so a persistent error does not spin.
const ACCEPT_RETRY: Duration = Duration::from_millis(50);

type Handle = Arc<client::Handle<ClientHandler>>;
type Jumps = Arc<Vec<client::Handle<ClientHandler>>>;

/// Where connections of a remote forwarding go, by the address and port the server listens on.
pub(crate) type Remotes = Arc<Mutex<HashMap<(String, u32), RemoteTarget>>>;

#[derive(Clone)]
pub(crate) struct RemoteTarget {
    host: String,
    port: u16,
    count: Arc<AtomicUsize>,
    stop: watch::Receiver<()>,
}

pub struct Tunnel {
    bound_port: u16,
    count: Arc<AtomicUsize>,
    /// Connections the server would not carry.
    refused: Arc<AtomicUsize>,
    /// Dropped with the tunnel: every connection task sees it and ends.
    _stop: watch::Sender<()>,
    accept: Vec<JoinHandle<()>>,
    remote: Option<RemoteRegistration>,
}

struct RemoteRegistration {
    handle: Handle,
    remotes: Remotes,
    key: (String, u32),
}

impl Tunnel {
    /// The port that listens: the one asked for, or the one the system (or server) picked for 0.
    pub fn bound_port(&self) -> u16 {
        self.bound_port
    }

    /// Connections running through the tunnel right now.
    pub fn connections(&self) -> usize {
        self.count.load(Ordering::SeqCst)
    }

    /// Connections the server would not carry so far (for example `AllowTcpForwarding no`).
    pub fn refused(&self) -> usize {
        self.refused.load(Ordering::SeqCst)
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        for accept in self.accept.drain(..) {
            accept.abort();
        }
        if let Some(remote) = self.remote.take() {
            remote.remotes.lock().unwrap_or_else(PoisonError::into_inner).remove(&remote.key);
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                runtime.spawn(async move {
                    let (address, port) = remote.key;
                    let _ = remote.handle.cancel_tcpip_forward(address, port).await;
                });
            }
        }
    }
}

/// One place in a tunnel's connection count, given back when dropped.
struct Slot(Arc<AtomicUsize>);

impl Slot {
    fn take(count: &Arc<AtomicUsize>) -> Option<Self> {
        if count.fetch_add(1, Ordering::SeqCst) >= MAX_TUNNEL_CONNECTIONS {
            count.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        Some(Self(count.clone()))
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// `[::1]` as ssh writes it, `::1` as the system wants it.
pub(crate) fn unbracket(address: &str) -> &str {
    address.strip_prefix('[').and_then(|a| a.strip_suffix(']')).unwrap_or(address)
}

fn bind_error(e: std::io::Error) -> SshError {
    if e.kind() == std::io::ErrorKind::AddrInUse { SshError::PortInUse } else { SshError::Bind(e.to_string()) }
}

/// The listeners for `address:port`. `localhost` listens on IPv4 and, where there is one, IPv6
/// loopback on the same port, so clients reach it whichever they resolve it to.
async fn bind(address: &str, port: u16) -> Result<Vec<TcpListener>, SshError> {
    let address = unbracket(address);
    if !address.eq_ignore_ascii_case("localhost") {
        return Ok(vec![TcpListener::bind((address, port)).await.map_err(bind_error)?]);
    }
    let first = TcpListener::bind(("127.0.0.1", port)).await.map_err(bind_error)?;
    let port = first.local_addr().map_err(bind_error)?.port();
    let mut listeners = vec![first];
    if let Ok(second) = TcpListener::bind(("::1", port)).await {
        listeners.push(second);
    }
    Ok(listeners)
}

/// Accepts until the tunnel stops. A failed accept (a client that reset before it was taken,
/// too many open files) does not end the listener.
async fn accept_loop(listener: TcpListener, on_connection: impl Fn(TcpStream, SocketAddr)) {
    loop {
        match listener.accept().await {
            Ok((stream, peer)) => on_connection(stream, peer),
            Err(_) => tokio::time::sleep(ACCEPT_RETRY).await,
        }
    }
}

/// Runs `work` until it ends or the tunnel goes away.
async fn until_stopped(work: impl Future<Output = ()>, mut stop: watch::Receiver<()>) {
    tokio::select! {
        () = work => {}
        _ = stop.changed() => {}
    }
}

/// Listens on `bind_address:bind_port`; each connection goes to `target`, or with `None` to
/// where its SOCKS5 request says.
pub(crate) async fn listen(
    handle: Handle,
    jumps: Jumps,
    bind_address: &str,
    bind_port: u16,
    target: Option<(String, u16)>,
) -> Result<Tunnel, SshError> {
    let listeners = bind(bind_address, bind_port).await?;
    let bound_port = listeners[0].local_addr().map_err(|e| SshError::Bind(e.to_string()))?.port();
    let target = target.map(|(host, port)| (unbracket(&host).to_owned(), port));
    let (stop_tx, stop) = watch::channel(());
    let count = Arc::new(AtomicUsize::new(0));
    let refused = Arc::new(AtomicUsize::new(0));
    let accept = listeners
        .into_iter()
        .map(|listener| {
            let (handle, jumps, target) = (handle.clone(), jumps.clone(), target.clone());
            let (count, refused, stop) = (count.clone(), refused.clone(), stop.clone());
            tokio::spawn(async move {
                // The jump hosts' connections stay open while the tunnel listens.
                let _jumps = jumps;
                accept_loop(listener, |stream, peer| {
                    let Some(slot) = Slot::take(&count) else {
                        return;
                    };
                    let work = connection(handle.clone(), stream, peer, target.clone(), slot, refused.clone());
                    tokio::spawn(until_stopped(work, stop.clone()));
                })
                .await;
            })
        })
        .collect();
    Ok(Tunnel { bound_port, count, refused, _stop: stop_tx, accept, remote: None })
}

async fn connection(handle: Handle, mut stream: TcpStream, peer: SocketAddr, target: Option<(String, u16)>, _slot: Slot, refused: Arc<AtomicUsize>) {
    let socks = target.is_none();
    let (host, port) = match target {
        Some(target) => target,
        None => match tokio::time::timeout(SOCKS_TIMEOUT, socks::read_request(&mut stream)).await {
            Ok(Ok(target)) => target,
            _ => return,
        },
    };
    let opened =
        tokio::time::timeout(OPEN_TIMEOUT, handle.channel_open_direct_tcpip(host, u32::from(port), peer.ip().to_string(), u32::from(peer.port())))
            .await;
    let Ok(Ok(channel)) = opened else {
        refused.fetch_add(1, Ordering::SeqCst);
        if socks {
            let _ = socks::reply_failure(&mut stream, socks::HOST_UNREACHABLE).await;
        }
        return;
    };
    if socks && socks::reply_success(&mut stream, SocketAddr::from(([0, 0, 0, 0], 0))).await.is_err() {
        return;
    }
    let mut channel = channel.into_stream();
    let _ = tokio::io::copy_bidirectional(&mut stream, &mut channel).await;
}

/// Asks the server to listen on `bind_address:bind_port`; its connections go to
/// `target_host:target_port` from here.
pub(crate) async fn remote(
    handle: Handle,
    jumps: Jumps,
    remotes: Remotes,
    bind_address: &str,
    bind_port: u16,
    target_host: &str,
    target_port: u16,
) -> Result<Tunnel, SshError> {
    let bind_address = unbracket(bind_address);
    let target_host = unbracket(target_host);
    let (stop_tx, stop) = watch::channel(());
    let count = Arc::new(AtomicUsize::new(0));
    let entry = RemoteTarget { host: target_host.to_owned(), port: target_port, count: count.clone(), stop };
    let mut key = (bind_address.to_owned(), u32::from(bind_port));
    // Registered first: the server may send a connection right after it agreed.
    remotes.lock().unwrap_or_else(PoisonError::into_inner).insert(key.clone(), entry.clone());
    let port = match handle.tcpip_forward(bind_address, u32::from(bind_port)).await {
        Ok(port) => port,
        Err(e) => {
            remotes.lock().unwrap_or_else(PoisonError::into_inner).remove(&key);
            return Err(match e {
                russh::Error::RequestDenied => SshError::ForwardRefused,
                other => protocol(other),
            });
        }
    };
    if port != 0 && port != key.1 {
        let mut map = remotes.lock().unwrap_or_else(PoisonError::into_inner);
        map.remove(&key);
        key.1 = port;
        map.insert(key.clone(), entry);
    }
    let accept = tokio::spawn(async move {
        // Keeps the jump hosts' connections open while the server listens.
        let _jumps = jumps;
        std::future::pending::<()>().await;
    });
    Ok(Tunnel {
        bound_port: u16::try_from(key.1).unwrap_or(bind_port),
        count,
        refused: Arc::new(AtomicUsize::new(0)),
        _stop: stop_tx,
        accept: vec![accept],
        remote: Some(RemoteRegistration { handle, remotes, key }),
    })
}

/// A `forwarded-tcpip` channel from the server: the target registered for its address and port
/// (or, when the server names the address differently, the only one on that port).
pub(crate) fn remote_target(remotes: &Remotes, address: &str, port: u32) -> Option<RemoteTarget> {
    let map = remotes.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(target) = map.get(&(address.to_owned(), port)) {
        return Some(target.clone());
    }
    let mut on_port = map.iter().filter(|((_, p), _)| *p == port);
    match (on_port.next(), on_port.next()) {
        (Some((_, target)), None) => Some(target.clone()),
        _ => None,
    }
}

/// Carries one `forwarded-tcpip` channel to its local target.
pub(crate) fn serve_remote(channel: russh::Channel<client::Msg>, target: RemoteTarget, slot_count: Arc<AtomicUsize>) -> bool {
    let Some(slot) = Slot::take(&slot_count) else {
        return false;
    };
    let stop = target.stop.clone();
    let work = async move {
        let _slot = slot;
        let connect = TcpStream::connect((target.host.as_str(), target.port));
        let Ok(Ok(mut tcp)) = tokio::time::timeout(OPEN_TIMEOUT, connect).await else {
            return;
        };
        let mut channel = channel.into_stream();
        let _ = tokio::io::copy_bidirectional(&mut tcp, &mut channel).await;
    };
    tokio::spawn(until_stopped(work, stop));
    true
}

impl RemoteTarget {
    pub(crate) fn count(&self) -> Arc<AtomicUsize> {
        self.count.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::{Remotes, remote_target, unbracket};

    /// A task that panicked while holding the table of remote forwardings must not take the
    /// next lookup down: the lookup goes on with the table as it is.
    #[test]
    fn poisoned_remotes_lock_does_not_panic() {
        let remotes: Remotes = Remotes::default();
        let held = remotes.clone();
        let _ = std::thread::spawn(move || {
            let _guard = held.lock();
            panic!("a task dies while it holds the remotes");
        })
        .join();
        assert!(remotes.is_poisoned());
        assert!(remote_target(&remotes, "127.0.0.1", 2222).is_none());
    }

    #[test]
    fn brackets_come_off() {
        assert_eq!(unbracket("[::1]"), "::1");
        assert_eq!(unbracket("::1"), "::1");
        assert_eq!(unbracket("127.0.0.1"), "127.0.0.1");
        assert_eq!(unbracket("[x"), "[x");
    }
}
