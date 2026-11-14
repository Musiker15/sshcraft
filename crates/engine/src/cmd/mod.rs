//! The command registry. Every user-visible action is a command here: id, label, menu path,
//! shortcut, parameter documentation, `enabled`, `run`. The interface, the CLI, the control
//! channel and MCP all call them through [`crate::Session::execute`].

use std::sync::OnceLock;

use futures::future::BoxFuture;
use serde::Serialize;
use serde_json::Value;

use crate::error::EngineError;
use crate::gate::AgentAccess;
use crate::{Ctx, Session};

pub type Enabled = fn(&Session) -> Result<(), String>;
pub type Run = fn(Ctx, Value) -> BoxFuture<'static, Result<Value, EngineError>>;

pub struct CommandSpec {
    /// Stable id: `area.action` (`host.save`, `session.open`). Never changes once shipped.
    pub id: &'static str,
    pub label: &'static str,
    /// Where the command sits in the menus of the interface.
    pub menu: &'static [&'static str],
    pub shortcut: Option<&'static str>,
    /// What the parameters are, for `engine.commands` and the MCP tool list.
    pub params: &'static str,
    pub enabled: Enabled,
    pub run: Run,
    /// Whether a successful run goes into the journal (queries do not).
    pub journal: bool,
    /// Always false: SSHCraft has no undo.
    pub undoable: bool,
    /// `None`: never reachable for agents or the control channel.
    pub agent: Option<&'static AgentAccess>,
}

/// What `engine.commands` lists about a command.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CommandInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub menu: Vec<&'static str>,
    pub shortcut: Option<&'static str>,
    pub params: &'static str,
    pub enabled: bool,
    pub agents: bool,
}

/// Always available.
pub fn always(_: &Session) -> Result<(), String> {
    Ok(())
}

/// Available while the vault is unlocked.
pub fn needs_vault(session: &Session) -> Result<(), String> {
    session.vault_state().map_err(|code| format!("the vault is not available ({})", <&'static str>::from(code)))
}

pub mod group;
pub mod host;
pub mod identity;
pub mod key;
pub mod password;
pub mod settings;
pub mod vault;

/// The error type of the commands moved from the Tauri version.
pub(crate) type CommandError = EngineError;
pub(crate) type CommandResult<T> = Result<T, EngineError>;

/// Runs a synchronous command body as the future a [`Run`] returns.
pub(crate) fn sync_run(f: impl FnOnce() -> Result<Value, EngineError> + Send + 'static) -> BoxFuture<'static, Result<Value, EngineError>> {
    use futures::FutureExt;
    async move { f() }.boxed()
}

/// Runs blocking work (bcrypt, key generation, a child process) on tokio's blocking threads, so
/// the async threads stay free. A panic there becomes [`EngineError::Internal`].
pub(crate) fn blocking_run(
    id: &'static str,
    f: impl FnOnce() -> Result<Value, EngineError> + Send + 'static,
) -> BoxFuture<'static, Result<Value, EngineError>> {
    use futures::FutureExt;
    async move { tokio::task::spawn_blocking(f).await.map_err(|_| EngineError::Internal { id: id.to_string() })? }.boxed()
}

/// A command that changes something: it goes into the journal.
pub(crate) fn command(id: &'static str, label: &'static str, params: &'static str, run: Run) -> CommandSpec {
    CommandSpec { id, label, menu: &[], shortcut: None, params, enabled: always, run, journal: true, undoable: false, agent: None }
}

/// A command that only reads: it stays out of the journal.
pub(crate) fn query(id: &'static str, label: &'static str, params: &'static str, run: Run) -> CommandSpec {
    CommandSpec { journal: false, ..command(id, label, params, run) }
}

/// The parameters of command `id` as `T`; anything that does not fit is `BadParams`.
pub(crate) fn parse<T: serde::de::DeserializeOwned>(id: &str, params: Value) -> Result<T, EngineError> {
    serde_json::from_value(params).map_err(|e| EngineError::bad_params(id, e))
}

pub(crate) fn to_value<T: Serialize>(value: &T) -> Result<Value, EngineError> {
    serde_json::to_value(value).map_err(|e| EngineError::failed(crate::error::ErrorCode::Internal, e))
}

static REGISTRY: OnceLock<Vec<CommandSpec>> = OnceLock::new();

/// Every command, built once.
pub fn command_specs() -> &'static [CommandSpec] {
    REGISTRY.get_or_init(|| {
        let mut specs: Vec<CommandSpec> = Vec::new();
        specs.extend(settings::specs());
        specs.extend(group::specs());
        specs.extend(host::specs());
        specs.extend(identity::specs());
        specs.extend(password::specs());
        specs.extend(key::specs());
        specs.extend(vault::specs());
        #[cfg(test)]
        specs.extend(test_commands());
        specs
    })
}

pub fn find_command(id: &str) -> Option<&'static CommandSpec> {
    command_specs().iter().find(|spec| spec.id == id)
}

/// Commands that exist only in the engine's own tests.
#[cfg(test)]
fn test_commands() -> Vec<CommandSpec> {
    use futures::FutureExt;
    fn echo(_: Ctx, params: Value) -> BoxFuture<'static, Result<Value, EngineError>> {
        async move { Ok(params) }.boxed()
    }
    #[allow(clippy::panic)]
    fn panics(_: Ctx, _: Value) -> BoxFuture<'static, Result<Value, EngineError>> {
        async move { panic!("a command that panics") }.boxed()
    }
    fn vault(_: Ctx, _: Value) -> BoxFuture<'static, Result<Value, EngineError>> {
        async move { Ok(Value::Null) }.boxed()
    }
    let spec = |id, label, params, run: Run, enabled: Enabled| CommandSpec {
        id,
        label,
        menu: &[],
        shortcut: None,
        params,
        enabled,
        run,
        journal: true,
        undoable: false,
        agent: None,
    };
    vec![
        spec("test.echo", "Echo", "any value, returned as it is", echo, always),
        spec("test.panic", "Panic", "nothing", panics, always),
        spec("test.vault", "Needs the vault", "nothing", vault, needs_vault),
    ]
}
