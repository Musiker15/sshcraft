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
        }
    }
}
