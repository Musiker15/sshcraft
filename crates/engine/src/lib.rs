//! The SSHCraft engine. Everything SSHCraft does is a command behind [`Session::execute`]; the
//! interface, the CLI, the control channel and MCP are callers. Agents and the control channel
//! pass the agent gate, the user does not.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod agent_link;
pub mod agent_sessions;
#[cfg(test)]
mod bulk_tests;
pub mod caller;
pub mod cmd;
pub mod consent;
pub mod error;
pub mod events;
pub mod forwards;
pub mod gate;
mod hooks;
pub mod journal;
pub mod links;
pub mod listing;
#[cfg(test)]
mod login_approval_tests;
pub mod login_script;
pub mod passcli;
mod pool;
pub mod prompts;
#[cfg(test)]
mod reachable_tests;
pub mod recording;
pub mod secrets;
pub mod sessions;
pub mod settings;
pub mod sftp;
pub mod ssh_config;
pub mod sync;
pub mod terminal;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
#[cfg(test)]
mod tests;
pub mod waker;
pub mod workspaces;

use std::collections::HashMap;
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
pub use gate::{AgentConnection, AgentPrompt, Confirmer};
pub use journal::JournalEntry;
pub use sessions::{AgentConnector, EndReason, SessionId, SessionStatus, SessionTarget, WindowsAgent};
pub use terminal::{TermEvent, TerminalStream};

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
    /// Finds Proton Pass's `pass-cli` (`passcli::locate_installed` in the app).
    pub pass_cli: passcli::LocatePassCli,
    /// The SSH agent: the Proton Pass pipe ([`WindowsAgent`]) in the app.
    pub agent: Arc<dyn AgentConnector>,
    /// Answers questions about agents' actions in tests. `None` (the app): the question goes to
    /// the window through the event bus, and without a window the answer is no.
    pub confirmer: Option<Arc<dyn gate::Confirmer>>,
}

impl SessionConfig {
    /// The desktop app's engine on `data_dir`: the master key in the OS credential store,
    /// `config.toml` in the data folder, the Proton Pass agent, `pass-cli` found where it is
    /// installed. `presence` and `security` need the app's window and come from the app.
    pub fn for_app(
        data_dir: PathBuf,
        runtime: tokio::runtime::Handle,
        presence: Arc<dyn sshcraft_fido::presence::UserPresence>,
        security: Option<Arc<dyn sshcraft_fido::Authenticator>>,
    ) -> Self {
        Self {
            data_dir,
            runtime,
            master_key: Box::new(sshcraft_store::vault::KeyringMasterKey),
            presence,
            security,
            legacy_running: sshcraft_platform::legacy_instance_running,
            confirm_timeout: CONFIRM_TIMEOUT,
            event_capacity: EVENT_CAPACITY,
            settings: craft_settings::Location::Auto,
            pass_cli: passcli::locate_installed(),
            agent: Arc::new(WindowsAgent),
            confirmer: None,
        }
    }
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
    /// Running port forwarding rules.
    forwards: Arc<forwards::ForwardManager>,
    events: EventBus,
    journal: Mutex<journal::Journal>,
    settings: Arc<Mutex<craft_settings::SettingsStore<settings::EngineSettings>>>,
    runtime: tokio::runtime::Handle,
    security: Option<Arc<dyn sshcraft_fido::Authenticator>>,
    pass_cli: passcli::LocatePassCli,
    sessions: sessions::SessionManager,
    prompts: Arc<prompts::PromptRegistry<prompts::BusEmitter>>,
    agent_sessions: agent_sessions::AgentSessions,
    /// Open SFTP sessions (S1).
    sftp: sftp::SftpManager,
    /// Terminal streams of the user's sessions until the interface takes them.
    terminals: Mutex<HashMap<SessionId, TerminalStream>>,
    /// Called when a terminal stream gets output or a status (the window's repaint).
    terminal_waker: Mutex<Option<terminal::Waker>>,
    /// Work started by [`Session::start`]; stopped with the engine.
    background: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    /// Open connections of agents and the control channel, with the sessions each opened.
    connections: Mutex<HashMap<ConnId, gate::Connection>>,
    next_conn: std::sync::atomic::AtomicU64,
    confirmer: Option<Arc<dyn gate::Confirmer>>,
    confirm_timeout: Duration,
    /// The agent link while it runs.
    agent_link: Mutex<Option<agent_link::Running>>,
    /// Held for the life of the engine: no other process opens the same data.
    _lock: sshcraft_platform::DataLock,
}

impl Drop for Inner {
    fn drop(&mut self) {
        for task in self.background.get_mut().unwrap_or_else(PoisonError::into_inner).drain(..) {
            task.abort();
        }
        // Sessions end with the engine; their recordings are finished.
        for id in self.sessions.ids() {
            self.sessions.close(id);
        }
    }
}

/// A [`Session`] that is gone once everything else let go of it.
pub(crate) struct Weak(std::sync::Weak<Inner>);

impl Weak {
    pub(crate) fn upgrade(&self) -> Option<Session> {
        self.0.upgrade().map(Session)
    }
}

/// One open SSHCraft: the store, the vault, the running sessions. Cheap to clone.
#[derive(Clone)]
pub struct Session(Arc<Inner>);

impl Session {
    /// The runtime the engine runs its work on; the interface spawns its command tasks there.
    pub fn runtime(&self) -> tokio::runtime::Handle {
        self.0.runtime.clone()
    }

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
        gate::reset_categories_once(&store)?;
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
        let settings = Arc::new(Mutex::new(prefs));
        let prompts = Arc::new(prompts::PromptRegistry::new(prompts::BusEmitter(events.clone())));
        let sessions = sessions::SessionManager::new(sessions::ManagerParts {
            store: store.clone(),
            vault: vault.as_ref().ok().cloned(),
            prompter: prompts.clone(),
            auth: prompts.clone(),
            agent: config.agent,
            presence: config.presence,
            security: config.security.clone(),
            settings: settings_reader(&settings),
            runtime: config.runtime.clone(),
            delay_scale: 1.0,
        });
        waker::spawn(&config.runtime, Arc::downgrade(&sessions.context()));
        let forwards = Arc::new(forwards::ForwardManager::new(sessions.context(), Arc::new(forwards::BusSink(events.clone()))));
        // A host or rule that sync removed (or a share that ended) stops forwarding.
        let observer = sync::BusObserver { events: events.clone(), forwards: forwards.clone() };
        let sync = sync::SyncManager::new(store.clone(), vault.as_ref().ok().cloned(), Arc::new(observer));
        let sftp = sftp::SftpManager::new(sessions.context());
        let inner = Inner {
            store,
            vault,
            sync,
            forwards,
            events,
            journal: Mutex::new(journal::Journal::default()),
            settings,
            runtime: config.runtime,
            security: config.security,
            pass_cli: config.pass_cli,
            sessions,
            prompts,
            agent_sessions: agent_sessions::AgentSessions::default(),
            sftp,
            terminals: Mutex::new(HashMap::new()),
            terminal_waker: Mutex::new(None),
            background: Mutex::new(Vec::new()),
            connections: Mutex::new(HashMap::new()),
            next_conn: std::sync::atomic::AtomicU64::new(1),
            agent_link: Mutex::new(None),
            confirmer: config.confirmer,
            confirm_timeout: config.confirm_timeout,
            _lock: lock,
        };
        let session = Session(Arc::new(inner));
        session.bind_session_hooks();
        let days = session.settings().get().general.trash_days;
        let _ = session.store().purge_old_trash(days);
        ssh_config::spawn_watch(session.downgrade());
        Ok((session, report))
    }

    /// A handle that does not keep the engine open, for tasks that run beside it.
    pub(crate) fn downgrade(&self) -> Weak {
        Weak(Arc::downgrade(&self.0))
    }

    /// Windows Hello missing goes on the bus; an ended session log moves to where its host lives
    /// and goes to that server.
    fn bind_session_hooks(&self) {
        let events = self.0.events.clone();
        self.0.sessions.on_hello_unavailable(Box::new(move || events.send(EngineEvent::HelloUnavailable)));
        let weak = Arc::downgrade(&self.0);
        self.0.sessions.on_log_finished(Arc::new(move |_id| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let placed = {
                let store = inner.store.lock().unwrap_or_else(PoisonError::into_inner);
                sshcraft_sync::logs::place_logs(&store, now_ms())
            };
            if let Ok(servers) = placed {
                inner.sync.nudge(&servers);
            }
        }));
    }

    /// Starts sync with every server that has it on, cleans up what a stopped SSHCraft left
    /// (unfinished session logs, logs past their retention, here and at their sync server) and
    /// from then on deletes expired logs every hour. Call once after [`Session::open`], inside
    /// the runtime or not.
    pub fn start(&self) {
        let _runtime = self.0.runtime.enter();
        let Ok(vault) = self.0.vault.clone() else {
            return;
        };
        self.0.sync.start_all();
        let days = self.settings().get().logs.log_retention_days;
        if let Ok(servers) = recording::housekeeping(&self.0.store, &vault, days, now_ms(), true) {
            self.0.sync.nudge(&servers);
        }
        let weak = Arc::downgrade(&self.0);
        let tidy = recording::tidy_regularly(self.0.store.clone(), vault, settings_reader(&self.0.settings), move |servers| {
            if let Some(inner) = weak.upgrade() {
                inner.sync.nudge(servers);
            }
        });
        let task = self.0.runtime.spawn(tidy);
        self.0.background.lock().unwrap_or_else(PoisonError::into_inner).push(task);
    }

    /// Before quitting: running recordings are finished and stored, the settings written.
    pub async fn shutdown(&self) {
        self.0.sessions.finish_recordings();
        let mut settings = self.settings();
        if settings.is_dirty() && !settings.is_read_only() {
            let _ = settings.flush();
        }
    }

    /// The output of the user's session `id`: once per session, and never for a session an agent
    /// opened.
    pub fn attach_terminal(&self, id: SessionId) -> Option<TerminalStream> {
        self.0.terminals.lock().unwrap_or_else(PoisonError::into_inner).remove(&id)
    }

    /// Keeps the stream of a session the user opened until the interface attaches it.
    /// Wakes the window whenever a terminal stream gets output or a status, so it need not look
    /// for output on a timer. Applies to the sessions opened from now on.
    pub fn set_terminal_waker(&self, waker: terminal::Waker) {
        *self.0.terminal_waker.lock().unwrap_or_else(PoisonError::into_inner) = Some(waker);
    }

    pub(crate) fn terminal_waker(&self) -> Option<terminal::Waker> {
        self.0.terminal_waker.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub(crate) fn park_terminal(&self, stream: TerminalStream) {
        self.0.terminals.lock().unwrap_or_else(PoisonError::into_inner).insert(stream.id(), stream);
    }

    /// The output of session `id` its terminal has not acknowledged yet; `None` for an unknown
    /// session.
    pub fn unacknowledged(&self, id: SessionId) -> Option<usize> {
        self.0.sessions.unacknowledged(id)
    }

    pub(crate) fn sessions(&self) -> &sessions::SessionManager {
        &self.0.sessions
    }

    pub(crate) fn prompts(&self) -> &prompts::PromptRegistry<prompts::BusEmitter> {
        &self.0.prompts
    }

    pub(crate) fn forwards(&self) -> &forwards::ForwardManager {
        &self.0.forwards
    }

    pub(crate) fn agent_sessions(&self) -> &agent_sessions::AgentSessions {
        &self.0.agent_sessions
    }

    pub(crate) fn sftp(&self) -> &sftp::SftpManager {
        &self.0.sftp
    }

    /// Closes a session of any caller and drops a stream nobody attached.
    pub(crate) fn close_session(&self, id: SessionId) {
        self.0.sessions.close(id);
        self.0.terminals.lock().unwrap_or_else(PoisonError::into_inner).remove(&id);
    }

    /// Runs command `id` for `caller`. A command that panics becomes [`EngineError::Internal`];
    /// the engine goes on. The command runs on the engine's runtime, so any executor may await
    /// this (the window's frame loop is not tokio); dropping the future cancels the command, as
    /// an agent that goes away takes its question back.
    pub async fn execute(&self, caller: Caller, id: &str, params: Value) -> Result<Value, EngineError> {
        let session = self.clone();
        let name = id.to_string();
        let task = AbortOnDrop(self.0.runtime.spawn(async move { session.execute_here(caller, &name, params).await }));
        match task.await {
            Ok(result) => result,
            Err(e) => {
                tracing::error!(command = id, "a command task ended without a result: {e}");
                Err(EngineError::Internal { id: id.to_string() })
            }
        }
    }

    async fn execute_here(&self, caller: Caller, id: &str, params: Value) -> Result<Value, EngineError> {
        let spec = cmd::find_command(id).ok_or_else(|| EngineError::UnknownCommand(id.to_string()))?;
        let params = if params.is_null() { Value::Object(serde_json::Map::new()) } else { params };
        if journal::depth(&params, MAX_PARAM_DEPTH) > MAX_PARAM_DEPTH {
            return Err(EngineError::bad_params(id, format!("parameters nest deeper than {MAX_PARAM_DEPTH} levels")));
        }
        let journal_params = spec.journal.then(|| params.clone());
        let result = if caller.is_user() {
            (spec.enabled)(self).map_err(|reason| EngineError::Disabled { id: id.to_string(), reason })?;
            let ctx = Ctx { session: self.clone(), caller: caller.clone() };
            self.run_spec(spec, ctx, params).await
        } else {
            // The gate parses hostile input: a panic there is caught like one in a command body.
            match AssertUnwindSafe(gate::run_for_agent(self, spec, caller.clone(), params)).catch_unwind().await {
                Ok(result) => result,
                Err(_) => {
                    tracing::error!(command = id, "the agent gate panicked; the engine goes on");
                    Err(EngineError::Internal { id: id.to_string() })
                }
            }
        };
        if let (Ok(_), Some(params)) = (&result, journal_params) {
            self.lock_journal().push(caller.label(), id, &params);
        }
        result
    }

    /// Runs a command body; a panic becomes [`EngineError::Internal`].
    pub(crate) async fn run_spec(&self, spec: &'static CommandSpec, ctx: Ctx, params: Value) -> Result<Value, EngineError> {
        match AssertUnwindSafe((spec.run)(ctx, params)).catch_unwind().await {
            Ok(result) => result,
            Err(_) => {
                tracing::error!(command = spec.id, "command panicked; the engine goes on");
                Err(EngineError::Internal { id: spec.id.to_string() })
            }
        }
    }

    /// A new connection of the agent `name`, reached `via` MCP, the launcher or the CLI. Its calls
    /// go through the agent gate; dropping it closes the sessions it opened.
    pub fn agent_connection(&self, name: &str, via: Via) -> Result<gate::AgentConnection, EngineError> {
        let name =
            AgentName::new(name).ok_or_else(|| EngineError::bad_params("agent", "agent name: 1 to 64 characters, no control or bidi characters"))?;
        Ok(self.new_connection(Some((name, via))))
    }

    /// A new connection of the control channel, checked like an agent named `control`.
    pub fn control_connection(&self) -> gate::AgentConnection {
        self.new_connection(None)
    }

    fn new_connection(&self, name: Option<(AgentName, Via)>) -> gate::AgentConnection {
        let conn = ConnId(self.0.next_conn.fetch_add(1, std::sync::atomic::Ordering::SeqCst));
        self.lock_connections().insert(conn, gate::Connection { conn: Some(conn), sessions: Default::default() });
        gate::AgentConnection::new(conn, name, Arc::downgrade(&self.0))
    }

    fn lock_connections(&self) -> MutexGuard<'_, HashMap<ConnId, gate::Connection>> {
        self.0.connections.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// What a connection holds right now; `None` for one that is not open.
    pub(crate) fn connection(&self, conn: ConnId) -> Option<gate::Connection> {
        self.lock_connections().get(&conn).cloned()
    }

    /// Gives session `id` to connection `conn`; when the connection went away meanwhile, the
    /// session is closed at once instead of staying open until the engine stops.
    pub(crate) fn own_agent_session(&self, conn: ConnId, id: SessionId, host: sshcraft_model::model::HostId) {
        let owned = match self.lock_connections().get_mut(&conn) {
            Some(connection) => {
                connection.sessions.insert(id, host);
                true
            }
            None => false,
        };
        if !owned && !self.agent_sessions().close(self.sessions(), id) {
            self.sessions().close(id);
        }
    }

    pub(crate) fn forget_agent_session(&self, conn: ConnId, id: SessionId) {
        if let Some(connection) = self.lock_connections().get_mut(&conn) {
            connection.sessions.remove(&id);
        }
    }

    pub(crate) fn confirmer(&self) -> Option<Arc<dyn gate::Confirmer>> {
        self.0.confirmer.clone()
    }

    pub(crate) fn confirm_timeout(&self) -> Duration {
        self.0.confirm_timeout
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

    pub(crate) fn security(&self) -> Option<Arc<dyn sshcraft_fido::Authenticator>> {
        self.0.security.clone()
    }

    pub(crate) fn pass_cli(&self) -> Option<Arc<dyn passcli::PassCli>> {
        (self.0.pass_cli)()
    }

    /// Whether Windows Hello turned out not to be set up on this PC.
    pub(crate) fn hello_missing(&self) -> bool {
        self.0.sessions.hello_missing()
    }

    /// The store behind its lock, for work that runs off the async threads.
    pub(crate) fn shared_store(&self) -> Arc<Mutex<Store>> {
        self.0.store.clone()
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

/// Reads the settings as they are at the moment of the call.
fn settings_reader(settings: &Arc<Mutex<craft_settings::SettingsStore<settings::EngineSettings>>>) -> sessions::ReadSettings {
    let settings = settings.clone();
    Arc::new(move || settings.lock().unwrap_or_else(PoisonError::into_inner).get().clone())
}

/// Milliseconds since 1970; 0 if the clock is before that.
pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX)).unwrap_or(0)
}

/// A task that is cancelled when the future awaiting it is dropped.
struct AbortOnDrop<T>(tokio::task::JoinHandle<T>);

impl<T> std::future::Future for AbortOnDrop<T> {
    type Output = Result<T, tokio::task::JoinError>;

    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        std::pin::Pin::new(&mut self.0).poll(cx)
    }
}

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}
