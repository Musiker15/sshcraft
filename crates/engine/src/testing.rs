//! An engine for tests: in-memory master key, a presence check that always passes, no Tauri
//! version running. Built with the `testing` feature (the interface and MCP tests use it too).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use sshcraft_fido::presence::{Presence, UserPresence};
use sshcraft_store::vault::MemoryMasterKey;

use crate::{EVENT_CAPACITY, SessionConfig};

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
        }
    }
}
