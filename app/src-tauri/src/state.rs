//! Process-wide state: the store, the vault (if it unlocked) and the session manager.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use stollen_core::store::Store;
use stollen_core::vault::{KeyringMasterKey, Vault};
use tauri::{AppHandle, Emitter};
use tokio::sync::oneshot;

use crate::sessions::{HostKeyPrompter, SessionManager};

pub struct AppState {
    pub store: Arc<Mutex<Store>>,
    pub vault: Result<Arc<Vault>, String>,
    pub sessions: SessionManager,
    pub prompter: Arc<TauriPrompter>,
}

impl AppState {
    pub fn init(app: &AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
        let dirs = directories::ProjectDirs::from("de", "musiker15", "Stollen")
            .ok_or("no home directory")?;
        std::fs::create_dir_all(dirs.data_dir())?;
        let store = Store::open(&dirs.data_dir().join("stollen.db"))?;
        let vault = Vault::unlock(&store, Box::new(KeyringMasterKey))
            .map(Arc::new)
            .map_err(|e| e.to_string());
        let store = Arc::new(Mutex::new(store));
        let prompter = Arc::new(TauriPrompter::new(app.clone()));
        let sessions = SessionManager::new(store.clone(), vault.clone().ok(), prompter.clone());
        Ok(Self {
            store,
            vault,
            sessions,
            prompter,
        })
    }
}

#[derive(Clone, Serialize)]
struct HostKeyPrompt<'a> {
    prompt_id: u64,
    host_name: &'a str,
    address: &'a str,
    port: u16,
    fingerprint: &'a str,
}

/// Emits `host-key-prompt` and waits for `answer_host_key`. No answer counts as "no".
pub struct TauriPrompter {
    app: AppHandle,
    next_id: AtomicU64,
    pending: Mutex<HashMap<u64, oneshot::Sender<bool>>>,
}

impl TauriPrompter {
    fn new(app: AppHandle) -> Self {
        Self {
            app,
            next_id: AtomicU64::new(1),
            pending: Mutex::new(HashMap::new()),
        }
    }

    pub fn answer(&self, prompt_id: u64, accept: bool) {
        if let Some(tx) = self
            .pending
            .lock()
            .expect("pending lock")
            .remove(&prompt_id)
        {
            let _ = tx.send(accept);
        }
    }
}

#[async_trait::async_trait]
impl HostKeyPrompter for TauriPrompter {
    async fn ask(&self, host_name: &str, address: &str, port: u16, fingerprint: &str) -> bool {
        let prompt_id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .expect("pending lock")
            .insert(prompt_id, tx);
        let prompt = HostKeyPrompt {
            prompt_id,
            host_name,
            address,
            port,
            fingerprint,
        };
        if self.app.emit("host-key-prompt", prompt).is_err() {
            self.pending
                .lock()
                .expect("pending lock")
                .remove(&prompt_id);
            return false;
        }
        rx.await.unwrap_or(false)
    }
}
