//! Process-wide state: the store, the vault (if it unlocked) and the session manager.

use std::sync::{Arc, Mutex};

use serde::Serialize;
use sshcraft_core::store::Store;
use sshcraft_core::vault::{KeyringMasterKey, Vault};
use tauri::{AppHandle, Emitter};

use crate::hello::WindowsHello;
use crate::prompts::{HostKeyPrompt, PromptEmitter, PromptRegistry};
use crate::sessions::{SessionManager, WindowsAgent};

pub struct AppState {
    pub store: Arc<Mutex<Store>>,
    pub vault: Result<Arc<Vault>, crate::errors::CommandError>,
    pub sessions: SessionManager,
    pub prompter: Arc<PromptRegistry<TauriEmitter>>,
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
        let prompter = Arc::new(PromptRegistry::new(TauriEmitter(app.clone())));
        let sessions = SessionManager::new(
            store.clone(),
            vault.clone().ok(),
            prompter.clone(),
            Arc::new(WindowsAgent),
            Arc::new(WindowsHello {
                hwnd: main_window_handle(app),
            }),
        );
        let events = app.clone();
        sessions.on_hello_unavailable(Box::new(move || {
            let _ = events.emit("hello-unavailable", ());
        }));
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

/// Sends host key questions to the window as `host-key-prompt` and takes them back with
/// `host-key-prompt-withdrawn` when their tab is closed.
pub struct TauriEmitter(AppHandle);

#[derive(Clone, Serialize)]
struct Withdrawn {
    prompt_id: u64,
}

impl PromptEmitter for TauriEmitter {
    fn asked(&self, prompt: &HostKeyPrompt) -> bool {
        self.0.emit("host-key-prompt", prompt).is_ok()
    }

    fn withdrawn(&self, prompt_id: u64) {
        let _ = self
            .0
            .emit("host-key-prompt-withdrawn", Withdrawn { prompt_id });
    }
}
