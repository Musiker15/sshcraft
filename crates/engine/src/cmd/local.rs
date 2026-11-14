//! What this device offers for local terminals: its shells and its serial ports.

use futures::FutureExt;
use serde_json::Value;

use super::session::offered_shells;
use super::{CommandError, CommandSpec, query, to_value};
use crate::Ctx;

type Out = futures::future::BoxFuture<'static, Result<Value, CommandError>>;

pub fn specs() -> Vec<CommandSpec> {
    vec![query("local.shells", "Local shells", "none", run_shells), query("serial.ports", "Serial ports", "none", run_ports)]
}

fn run_shells(ctx: Ctx, _: Value) -> Out {
    async move { to_value(&offered_shells(&ctx.session).await) }.boxed()
}

fn run_ports(_: Ctx, _: Value) -> Out {
    async move {
        let ports = tokio::task::spawn_blocking(sshcraft_local::serial::list_ports).await.unwrap_or_default();
        to_value(&ports)
    }
    .boxed()
}
