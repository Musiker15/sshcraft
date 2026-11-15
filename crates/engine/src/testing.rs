//! An engine for tests: in-memory master key, a presence check that always passes, no Tauri
//! version running. Built with the `testing` feature (the interface and MCP tests use it too).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use sshcraft_fido::presence::{Presence, UserPresence};
use sshcraft_store::vault::MemoryMasterKey;

use zeroize::Zeroizing;

use crate::passcli::{PassCli, PassCliError, PassCliStatus, PassKey, ensure_ready, parse_private_key};
use crate::sessions::AgentConnector;
use crate::{EVENT_CAPACITY, SessionConfig};

/// The JSON `pass-cli item view --output json` prints for an SSH key item.
pub fn view_json(private_key: &str) -> String {
    serde_json::json!({
        "item": {
            "id": "i1",
            "share_id": "s1",
            "content": {
                "title": "server-root",
                "content": { "SshKey": { "private_key": private_key, "public_key": "ssh-ed25519 AAAA" } }
            }
        }
    })
    .to_string()
}

/// A `pass-cli` that answers from memory.
pub struct FakePassCli {
    pub status: PassCliStatus,
    pub keys: Vec<PassKey>,
    /// The private key text of every item, as Proton Pass would hand it out.
    pub private_key: String,
}

impl FakePassCli {
    pub fn with_key(key: &str) -> Self {
        Self { status: PassCliStatus::Ready, keys: Vec::new(), private_key: key.to_owned() }
    }
}

impl PassCli for FakePassCli {
    fn status(&self) -> PassCliStatus {
        self.status
    }

    fn list_ssh_keys(&self) -> Result<Vec<PassKey>, PassCliError> {
        ensure_ready(self.status)?;
        Ok(self.keys.clone())
    }

    fn private_key(&self, _share_id: &str, _item_id: &str) -> Result<Zeroizing<String>, PassCliError> {
        ensure_ready(self.status)?;
        parse_private_key(view_json(&self.private_key).as_bytes())
    }
}

/// Hands out in-memory agents holding `keys`, or fails like a missing Proton Pass.
pub struct TestAgents {
    pub keys: Vec<russh::keys::PrivateKey>,
    pub reachable: bool,
}

#[async_trait::async_trait]
impl AgentConnector for TestAgents {
    async fn connect(&self) -> Result<sshcraft_ssh::DynAgent, sshcraft_ssh::SshError> {
        if !self.reachable {
            return Err(sshcraft_ssh::SshError::AgentUnavailable("pipe not found".into()));
        }
        Ok(sshcraft_ssh::testing::spawn_agent(&self.keys, true).await)
    }
}

/// Windows Hello that always says yes.
pub struct AlwaysPresent;

#[async_trait::async_trait]
impl UserPresence for AlwaysPresent {
    async fn verify(&self, _reason: &str) -> Presence {
        Presence::Verified
    }
}

impl SessionConfig {
    /// An engine on the data folder `dir`, for tests. Must be called inside a tokio runtime.
    pub fn for_tests(dir: &Path) -> SessionConfig {
        SessionConfig {
            data_dir: dir.to_path_buf(),
            runtime: tokio::runtime::Handle::current(),
            master_key: Box::new(MemoryMasterKey::default()),
            presence: Arc::new(AlwaysPresent),
            security: None,
            legacy_running: || false,
            confirm_timeout: Duration::from_secs(2),
            event_capacity: EVENT_CAPACITY,
            settings: craft_settings::Location::Memory,
            pass_cli: Arc::new(|| None),
            agent: Arc::new(TestAgents { keys: Vec::new(), reachable: false }),
            confirmer: None,
        }
    }
}

/// A test SSH server with a host for it in `session`: the host's key is stored in the vault and
/// the server's host key is trusted, so `session.open` connects without a question. The server's
/// shell answers `flood\n` with [`sshcraft_ssh::testing::flood_pattern`].
pub async fn trusted_test_host(session: &crate::Session) -> (sshcraft_ssh::testing::TestServer, sshcraft_model::model::HostId) {
    use russh::keys::ssh_key::LineEnding;
    use russh::keys::{Algorithm, PrivateKey};
    use sshcraft_model::model::{ConnectionSettings, Host, HostId, KeyRef};
    use sshcraft_ssh::testing::{TestAuth, TestServer};

    let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
    let server = TestServer::start_with(TestAuth { key: Some(key.public_key().clone()), quiet: true, ..Default::default() }).await;
    let store = session.store();
    let info = session.vault().unwrap().import_key(&store, "test", &key.to_openssh(LineEnding::LF).unwrap(), None).unwrap();
    let host = Host {
        forwards: Vec::new(),
        location: None,
        id: HostId::new(),
        name: "test host".into(),
        group_id: None,
        settings: ConnectionSettings {
            address: Some("127.0.0.1".into()),
            port: Some(server.port),
            username: Some("tester".into()),
            key: Some(KeyRef::Stored { id: info.id }),
            ..Default::default()
        },
        tags: Vec::new(),
        os: None,
    };
    store.upsert_host(&host).unwrap();
    store.set_known_host("127.0.0.1", server.port, &server.host_fingerprint).unwrap();
    (server, host.id)
}
