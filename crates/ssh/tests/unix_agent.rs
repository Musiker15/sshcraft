//! The SSH agent on Linux, FreeBSD and macOS: the socket `SSH_AUTH_SOCK` names.
#![cfg(unix)]

use std::path::Path;

use russh::keys::agent::client::AgentClient;
use russh::keys::{Algorithm, PrivateKey};
use sshcraft_ssh::agent::{UnixAgentOpener, open_unix_agent, socket_from};
use sshcraft_ssh::{AgentOpener, SshError};

#[derive(Clone)]
struct Plain;
impl russh::keys::agent::server::Agent for Plain {}

/// An agent listening on `path`, as ssh-agent does.
fn serve_at(path: &Path) {
    let listener = tokio::net::UnixListener::bind(path).expect("bind");
    let incoming = Box::pin(futures::stream::unfold(listener, |listener| async move {
        let next = listener.accept().await.map(|(stream, _)| stream);
        Some((next, listener))
    }));
    tokio::spawn(russh::keys::agent::server::serve(incoming, Plain));
}

fn key() -> PrivateKey {
    PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).expect("key")
}

#[tokio::test]
async fn keys_come_from_the_socket() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("agent.sock");
    serve_at(&path);
    let mut loader = AgentClient::connect_uds(&path).await.expect("connect");
    loader.add_identity(&key(), &[]).await.expect("add");

    let mut agent = open_unix_agent(&path).await.expect("open");
    assert_eq!(agent.request_identities().await.expect("list").len(), 1);

    // Forwarding opens its own stream per channel.
    let stream = UnixAgentOpener::new(path.clone()).open().await.expect("forward");
    let mut forwarded = AgentClient::connect(stream);
    assert_eq!(forwarded.request_identities().await.expect("list").len(), 1);
}

#[tokio::test]
async fn a_missing_socket_is_an_unavailable_agent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("gone.sock");
    assert!(matches!(open_unix_agent(&path).await, Err(SshError::AgentUnavailable(_))));
    assert!(matches!(UnixAgentOpener::new(path).open().await, Err(SshError::AgentUnavailable(_))));
}

#[test]
fn an_unset_or_empty_variable_names_no_socket() {
    assert_eq!(socket_from(None), None);
    assert_eq!(socket_from(Some("".into())), None);
    assert_eq!(socket_from(Some("/run/user/1000/ssh-agent.sock".into())), Some("/run/user/1000/ssh-agent.sock".into()));
}
