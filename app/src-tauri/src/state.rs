//! Process-wide state: the store, the vault (if it unlocked) and the session manager.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use sshcraft_core::store::Store;
use sshcraft_core::vault::{KeyringMasterKey, Vault};
use tauri::{AppHandle, Emitter};
use tokio::sync::oneshot;

use crate::hello::WindowsHello;
use crate::sessions::{HostKeyPrompter, SessionManager, WindowsAgent};

pub struct AppState {
    pub store: Arc<Mutex<Store>>,
    pub vault: Result<Arc<Vault>, crate::errors::CommandError>,
    pub sessions: SessionManager,
    pub prompter: Arc<TauriPrompter>,
}

impl AppState {
    pub fn init(app: &AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
        let dirs = directories::ProjectDirs::from("de", "musiker15", "SSHCraft")
            .ok_or("no home directory")?;
        std::fs::create_dir_all(dirs.data_dir())?;
        let store = Store::open(&dirs.data_dir().join("sshcraft.db"))?;
        let vault = Vault::unlock(&store, Box::new(KeyringMasterKey))
            .map(Arc::new)
            .map_err(crate::errors::CommandError::from);
        let store = Arc::new(Mutex::new(store));
        let prompter = Arc::new(TauriPrompter::new(app.clone()));
        let sessions = SessionManager::new(
            store.clone(),
            vault.clone().ok(),
            prompter.clone(),
            Arc::new(WindowsAgent),
            Arc::new(WindowsHello {
                hwnd: main_window_handle(app),
            }),
        );
        Ok(Self {
            store,
            vault,
            sessions,
            prompter,
        })
    }
}

/// Raw handle of the main window, so the Windows Hello dialog belongs to it.
#[cfg(windows)]
fn main_window_handle(app: &AppHandle) -> isize {
    use tauri::Manager;
    app.get_webview_window("main")
        .and_then(|w| w.hwnd().ok())
        .map_or(0, |h| h.0 as isize)
}

#[cfg(not(windows))]
fn main_window_handle(_app: &AppHandle) -> isize {
    0
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
