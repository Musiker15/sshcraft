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

static REGISTRY: OnceLock<Vec<CommandSpec>> = OnceLock::new();

/// Every command, built once.
pub fn command_specs() -> &'static [CommandSpec] {
    REGISTRY.get_or_init(|| {
        #[allow(unused_mut)]
        let mut specs: Vec<CommandSpec> = Vec::new();
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
