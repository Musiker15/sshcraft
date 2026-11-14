//! The SSHCraft engine. Everything SSHCraft does is a command behind [`Session::execute`]; the
//! interface, the CLI, the control channel and MCP are callers. Agents and the control channel
//! pass the agent gate, the user does not.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod caller;
pub mod cmd;
pub mod error;
pub mod events;
pub mod gate;
pub mod journal;
pub mod settings;
pub mod sync;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
#[cfg(test)]
mod tests;

use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use futures::FutureExt;
use serde_json::Value;
use sshcraft_store::store::Store;
use sshcraft_store::vault::{MasterKeyProvider, Vault};

pub use caller::{AgentName, Caller, ConnId, Via};
pub use cmd::{CommandInfo, CommandSpec};
pub use error::{EngineError, ErrorCode, ErrorKind};
pub use events::{EngineEvent, EventBus, UiRequest};
pub use journal::JournalEntry;

/// Deepest nesting accepted in the parameters of a command.
pub const MAX_PARAM_DEPTH: usize = 32;
/// How long an agent's question waits for the user.
pub const CONFIRM_TIMEOUT: Duration = Duration::from_secs(120);
/// Events kept for a slow subscriber before it lags.
pub const EVENT_CAPACITY: usize = 1024;

/// What the engine needs from the platform.
pub struct SessionConfig {
    pub data_dir: PathBuf,
    pub runtime: tokio::runtime::Handle,
    pub master_key: Box<dyn MasterKeyProvider>,
    pub presence: Arc<dyn sshcraft_fido::presence::UserPresence>,
    pub security: Option<Arc<dyn sshcraft_fido::Authenticator>>,
    /// Whether a Tauri version of SSHCraft runs (`sshcraft_platform::legacy_instance_running`).
    pub legacy_running: fn() -> bool,
    pub confirm_timeout: Duration,
    pub event_capacity: usize,
    /// Where `config.toml` lives (`Location::Auto` in the app, `Memory` in tests).
    pub settings: craft_settings::Location,
}

/// What happened while opening.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenReport {
    /// Whether the vault opened; without it the commands that need it are disabled.
    pub vault: Result<(), ErrorCode>,
    /// What happened to the settings of the Tauri version.
    pub takeover: settings::Takeover,
    /// `config.toml` had damaged sections, was moved aside or could not be read.
    pub settings_problems: bool,
    /// `config.toml` cannot be written (newer SSHCraft, folder not writable).
    pub settings_read_only: bool,
}

/// Passed to every command run.
#[derive(Clone)]
pub struct Ctx {
    pub session: Session,
    pub caller: Caller,
}

struct Inner {
    store: Arc<Mutex<Store>>,
    vault: Result<Arc<Vault>, ErrorCode>,
    sync: sync::SyncManager,
    events: EventBus,
    journal: Mutex<journal::Journal>,
    settings: Mutex<craft_settings::SettingsStore<settings::EngineSettings>>,
    #[allow(dead_code)]
    runtime: tokio::runtime::Handle,
    #[allow(dead_code)]
    presence: Arc<dyn sshcraft_fido::presence::UserPresence>,
    #[allow(dead_code)]
    security: Option<Arc<dyn sshcraft_fido::Authenticator>>,
    #[allow(dead_code)]
    confirm_timeout: Duration,
    /// Held for the life of the engine: no other process opens the same data.
    _lock: sshcraft_platform::DataLock,
}

/// One open SSHCraft: the store, the vault, the running sessions. Cheap to clone.
#[derive(Clone)]
pub struct Session(Arc<Inner>);

impl Session {
    /// Opens the data in `config.data_dir`. Refuses while a Tauri version runs or another engine
    /// holds the folder, before the database or the vault are touched.
    pub fn open(config: SessionConfig) -> Result<(Session, OpenReport), EngineError> {
        if (config.legacy_running)() {
            return Err(EngineError::LegacyRunning);
        }
        let lock = sshcraft_platform::lock_data_dir(&config.data_dir).map_err(|e| match e {
            sshcraft_platform::PlatformError::DataInUse(_) => EngineError::DataInUse,
            other => EngineError::failed(ErrorCode::Io, other),
        })?;
        let store = Store::open(&config.data_dir.join("sshcraft.db"))?;
        let vault = Vault::unlock(&store, config.master_key).map(Arc::new).map_err(|e| error::vault_code(&e));
        let (mut prefs, load) = craft_settings::SettingsStore::<settings::EngineSettings>::open(config.settings);
        let takeover = settings::take_over(&store, &mut prefs, &load, now_ms());
        let report = OpenReport {
            vault: vault.as_ref().map(|_| ()).map_err(|code| *code),
            takeover,
            settings_problems: load.has_problems(),
            settings_read_only: prefs.is_read_only(),
        };
        let events = EventBus::new(config.event_capacity);
        let store = Arc::new(Mutex::new(store));
        let sync = sync::SyncManager::new(store.clone(), vault.as_ref().ok().cloned(), Arc::new(sync::BusObserver(events.clone())));
        let inner = Inner {
            store,
            vault,
            sync,
            events,
            journal: Mutex::new(journal::Journal::default()),
            settings: Mutex::new(prefs),
            runtime: config.runtime,
            presence: config.presence,
            security: config.security,
            confirm_timeout: config.confirm_timeout,
            _lock: lock,
        };
        Ok((Session(Arc::new(inner)), report))
    }

    /// Runs command `id` for `caller`. A command that panics becomes [`EngineError::Internal`];
    /// the engine goes on.
    pub async fn execute(&self, caller: Caller, id: &str, params: Value) -> Result<Value, EngineError> {
        let spec = cmd::find_command(id).ok_or_else(|| EngineError::UnknownCommand(id.to_string()))?;
        let params = if params.is_null() { Value::Object(serde_json::Map::new()) } else { params };
        if journal::depth(&params, MAX_PARAM_DEPTH) > MAX_PARAM_DEPTH {
            return Err(EngineError::bad_params(id, format!("parameters nest deeper than {MAX_PARAM_DEPTH} levels")));
        }
        if !caller.is_user() {
            return Err(EngineError::Denied("agents are not served yet".to_string()));
        }
        (spec.enabled)(self).map_err(|reason| EngineError::Disabled { id: id.to_string(), reason })?;
        let journal_params = spec.journal.then(|| params.clone());
        let ctx = Ctx { session: self.clone(), caller: caller.clone() };
        let result = match AssertUnwindSafe((spec.run)(ctx, params)).catch_unwind().await {
            Ok(result) => result,
            Err(_) => {
                tracing::error!(command = id, "command panicked; the engine goes on");
                Err(EngineError::Internal { id: id.to_string() })
            }
        };
        if let (Ok(_), Some(params)) = (&result, journal_params) {
            self.lock_journal().push(caller.label(), id, &params);
        }
        result
    }

    /// Every command with its documentation.
    pub fn commands(&self) -> Vec<CommandInfo> {
        cmd::command_specs()
            .iter()
            .map(|spec| CommandInfo {
                id: spec.id,
                label: spec.label,
                menu: spec.menu.to_vec(),
                shortcut: spec.shortcut,
                params: spec.params,
                enabled: (spec.enabled)(self).is_ok(),
                agents: spec.agent.is_some(),
            })
            .collect()
    }

    /// The commands run so far, without secrets.
    pub fn journal(&self) -> Vec<JournalEntry> {
        self.lock_journal().entries()
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<EngineEvent> {
        self.0.events.subscribe()
    }

    #[allow(dead_code)] // used by the commands from R2.4 on
    pub(crate) fn events(&self) -> &EventBus {
        &self.0.events
    }

    #[allow(dead_code)] // used by the commands from R2.4 on
    /// The store; a thread that panicked holding it left no half-done write (writes run in
    /// transactions), so the store stays usable.
    pub(crate) fn store(&self) -> MutexGuard<'_, Store> {
        self.0.store.lock().unwrap_or_else(PoisonError::into_inner)
    }

    #[allow(dead_code)] // used by the commands from R2.4 on
    pub(crate) fn vault(&self) -> Result<Arc<Vault>, EngineError> {
        self.0.vault.clone().map_err(EngineError::code)
    }

    pub(crate) fn sync(&self) -> &sync::SyncManager {
        &self.0.sync
    }

    /// Tells the interface that data changed, once per sync server it belongs to.
    pub(crate) fn announce(&self, servers: &[sshcraft_model::model::ServerId]) {
        if servers.is_empty() {
            self.0.events.send(EngineEvent::DataChanged { server: None });
        }
        for server in servers {
            self.0.events.send(EngineEvent::DataChanged { server: Some(*server) });
        }
    }

    pub(crate) fn vault_state(&self) -> Result<(), ErrorCode> {
        self.0.vault.as_ref().map(|_| ()).map_err(|code| *code)
    }

    pub(crate) fn settings(&self) -> MutexGuard<'_, craft_settings::SettingsStore<settings::EngineSettings>> {
        self.0.settings.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn lock_journal(&self) -> MutexGuard<'_, journal::Journal> {
        self.0.journal.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Milliseconds since 1970; 0 if the clock is before that.
pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX)).unwrap_or(0)
}
