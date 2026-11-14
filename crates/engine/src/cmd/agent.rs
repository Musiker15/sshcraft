//! Agent rights, hidden hosts and groups, the log of agent actions. None of this is reachable
//! for agents themselves.

use serde::Deserialize;
use serde_json::Value;
use sshcraft_store::agent::{AgentCategory, AgentLevel, AgentPolicy, log_clear, log_list};
use uuid::Uuid;

use super::{CommandError, CommandSpec, command, needs_vault, parse, query, sync_run, to_value};
use crate::Ctx;
use crate::error::ErrorCode;
use crate::events::EngineEvent;

type Out = futures::future::BoxFuture<'static, Result<Value, CommandError>>;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        query("agent.policy", "Agent rights", "none", run_policy),
        command("agent.set_hidden", "Hide from agents", "{ id: host or group id, hidden }", run_set_hidden),
        command(
            "agent.set_level",
            "Set an agent right",
            "{ category: list|connect|edit|delete|keys|share|sync|forward|logs|workspaces|snippets, level: allow|ask|deny }",
            run_set_level,
        ),
        CommandSpec { enabled: needs_vault, ..query("agent.log", "Agent log", "none", run_log) },
        command("agent.log_clear", "Clear the agent log", "none", run_log_clear),
    ]
}

fn run_policy(ctx: Ctx, _: Value) -> Out {
    sync_run(move || to_value(&AgentPolicy::load(&ctx.session.store())))
}

#[derive(Deserialize)]
struct HiddenParams {
    id: Uuid,
    hidden: bool,
}

/// One change on the stored policy, so a window that has not loaded the policy yet cannot
/// overwrite it.
fn run_set_hidden(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let HiddenParams { id, hidden } = parse("agent.set_hidden", params)?;
        to_value(&AgentPolicy::set_hidden(&ctx.session.store(), id, hidden)?)
    })
}

#[derive(Deserialize)]
struct LevelParams {
    category: AgentCategory,
    level: AgentLevel,
}

fn run_set_level(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let LevelParams { category, level } = parse("agent.set_level", params)?;
        to_value(&AgentPolicy::set_level(&ctx.session.store(), category, level)?)
    })
}

fn run_log(ctx: Ctx, _: Value) -> Out {
    sync_run(move || {
        let vault = ctx.session.vault()?;
        let entries = log_list(&vault, &ctx.session.store()).map_err(|e| CommandError::new(ErrorCode::Store, e))?;
        to_value(&entries)
    })
}

fn run_log_clear(ctx: Ctx, _: Value) -> Out {
    sync_run(move || {
        log_clear(&ctx.session.store()).map_err(|e| CommandError::new(ErrorCode::Store, e))?;
        ctx.session.events().send(EngineEvent::AgentLogChanged);
        Ok(Value::Null)
    })
}
