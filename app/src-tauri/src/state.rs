//! Process-wide state: the store, the vault (if it unlocked) and the session manager.

use std::sync::{Arc, Mutex};

use serde::Serialize;
use sshcraft_core::store::Store;
use sshcraft_core::vault::{KeyringMasterKey, Vault};
use tauri::{AppHandle, Emitter, Manager};

use crate::forwards::{ForwardManager, ForwardSink, ForwardStatus};
use crate::hello::WindowsHello;
use crate::prompts::{AuthPrompt, HostKeyPrompt, PromptEmitter, PromptRegistry};
use crate::sessions::{SessionManager, WindowsAgent};
use crate::sync::{StatusView, SyncManager};
use sshcraft_core::model::ServerId;
use sshcraft_sync::runtime::{SyncObserver, SyncStatus};

pub struct AppState {
    pub store: Arc<Mutex<Store>>,
    pub vault: Result<Arc<Vault>, crate::errors::CommandError>,
    pub sessions: SessionManager,
    /// Running port forwarding rules.
    pub forwards: ForwardManager,
    pub prompter: Arc<PromptRegistry<TauriEmitter>>,
    pub sync: SyncManager,
    /// FIDO2 tokens and Windows Hello; `None` where there is no WebAuthn.
    pub security: Option<Arc<dyn sshcraft_fido::Authenticator>>,
    /// Sessions AI agents opened (only those are theirs).
    pub agent_sessions: crate::agent_sessions::AgentSessions,
    /// Questions to the user before an agent's action runs.
    #[cfg(desktop)]
    pub agent_confirms: Arc<crate::agents::tauri::TauriConfirmer>,
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
        let security = security_keys(app);
        let sessions = SessionManager::new(
            store.clone(),
            vault.clone().ok(),
            prompter.clone(),
            prompter.clone(),
            Arc::new(WindowsAgent),
            Arc::new(WindowsHello {
                hwnd: main_window_handle(app),
            }),
        );
        let sessions = match &security {
            Some(auth) => sessions.with_security_keys(auth.clone()),
            None => sessions,
        };
        let events = app.clone();
        sessions.on_hello_unavailable(Box::new(move || {
            let _ = events.emit("hello-unavailable", ());
        }));
        // An ended log moves to where its host lives and goes to that server.
        let logs_app = app.clone();
        let logs_store = store.clone();
        sessions.on_log_finished(Arc::new(move |_id| {
            let servers = {
                let store = logs_store.lock().expect("store lock");
                sshcraft_sync::logs::place_logs(&store, crate::recording::now_ms())
            };
            if let Ok(servers) = servers
                && let Some(state) = tauri::Manager::try_state::<AppState>(&logs_app)
            {
                state.sync.nudge(&servers);
            }
        }));
        let forwards =
            ForwardManager::new(sessions.context(), Arc::new(TauriForwardSink(app.clone())));
        let sync = SyncManager::new(
            store.clone(),
            vault.clone().ok(),
            Arc::new(TauriSyncObserver(app.clone())),
        );
        Ok(Self {
            store,
            vault,
            sessions,
            forwards,
            prompter,
            sync,
            security,
            agent_sessions: crate::agent_sessions::AgentSessions::default(),
            #[cfg(desktop)]
            agent_confirms: Arc::new(crate::agents::tauri::TauriConfirmer::new(app.clone())),
        })
    }
}

/// WebAuthn (FIDO2 tokens and Windows Hello), with its dialogs owned by the main window.
#[cfg(windows)]
fn security_keys(app: &AppHandle) -> Option<Arc<dyn sshcraft_fido::Authenticator>> {
    sshcraft_fido::webauthn::webauthn_available().then(|| {
        Arc::new(sshcraft_fido::webauthn::WebAuthn {
            hwnd: main_window_handle(app),
        }) as Arc<dyn sshcraft_fido::Authenticator>
    })
}

#[cfg(not(windows))]
fn security_keys(_app: &AppHandle) -> Option<Arc<dyn sshcraft_fido::Authenticator>> {
    None
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

/// Sends host key questions to the window as `host-key-prompt`, sign-in questions as
/// `auth-prompt`, and takes either back with `host-key-prompt-withdrawn` when their tab is
/// closed.
pub struct TauriEmitter(AppHandle);

#[derive(Clone, Serialize)]
struct Withdrawn {
    prompt_id: u64,
}

impl PromptEmitter for TauriEmitter {
    fn asked(&self, prompt: &HostKeyPrompt) -> bool {
        self.0.emit("host-key-prompt", prompt).is_ok()
    }

    fn asked_auth(&self, prompt: &AuthPrompt) -> bool {
        self.0.emit("auth-prompt", prompt).is_ok()
    }

    fn withdrawn(&self, prompt_id: u64) {
        let _ = self
            .0
            .emit("host-key-prompt-withdrawn", Withdrawn { prompt_id });
    }
}

/// Sends the state of port forwarding rules to the window as `forward-status`.
pub struct TauriForwardSink(AppHandle);

impl ForwardSink for TauriForwardSink {
    fn status(&self, status: ForwardStatus) {
        let _ = self.0.emit("forward-status", status);
    }
}

/// Sends sync news to the window: `sync-status`, `data-changed` and `sync-warning`.
pub struct TauriSyncObserver(AppHandle);

#[derive(Clone, Serialize)]
struct StatusEvent {
    server_id: ServerId,
    status: StatusView,
}

#[derive(Clone, Serialize)]
struct ServerEvent {
    server_id: ServerId,
}

#[derive(Clone, Serialize)]
struct WarningEvent {
    server_id: ServerId,
    detail: String,
}

impl SyncObserver for TauriSyncObserver {
    fn status(&self, server: ServerId, status: SyncStatus) {
        let _ = self.0.emit(
            "sync-status",
            StatusEvent {
                server_id: server,
                status: StatusView::from(&status),
            },
        );
    }

    fn data_changed(&self, server: ServerId) {
        // A host or rule that sync removed (or a share that ended) stops forwarding.
        if let Some(state) = self.0.try_state::<AppState>() {
            state.forwards.reconcile();
        }
        let _ = self
            .0
            .emit("data-changed", ServerEvent { server_id: server });
    }

    fn warning(&self, server: ServerId, message: String) {
        let _ = self.0.emit(
            "sync-warning",
            WarningEvent {
                server_id: server,
                detail: message,
            },
        );
    }
}
