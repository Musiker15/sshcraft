//! Starting and stopping port forwarding rules; their state goes on the event bus.

use serde::Deserialize;
use serde_json::Value;
use sshcraft_model::forward::ForwardId;
use sshcraft_model::model::HostId;

use super::{CommandError, CommandSpec, command, parse, query, sync_run, to_value};
use crate::Ctx;

type Out = futures::future::BoxFuture<'static, Result<Value, CommandError>>;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        command("forward.start", "Start port forwarding", "{ host_id, rule_id }", run_start),
        command("forward.stop", "Stop port forwarding", "{ host_id, rule_id }", run_stop),
        query("forward.status", "Port forwarding status", "none", run_status),
    ]
}

#[derive(Deserialize)]
struct RuleParams {
    host_id: HostId,
    rule_id: ForwardId,
}

/// Starts the rule; one that already runs is left alone.
fn run_start(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let RuleParams { host_id, rule_id } = parse("forward.start", params)?;
        ctx.session.forwards().start(host_id, rule_id).map_err(CommandError::code)?;
        Ok(Value::Null)
    })
}

fn run_stop(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let RuleParams { host_id, rule_id } = parse("forward.stop", params)?;
        ctx.session.forwards().stop(host_id, rule_id);
        Ok(Value::Null)
    })
}

/// The last state of every rule that runs or failed.
fn run_status(ctx: Ctx, _: Value) -> Out {
    sync_run(move || to_value(&ctx.session.forwards().statuses()))
}
