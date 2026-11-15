//! The state of the vault and of Windows Hello.

use serde::Serialize;
use serde_json::Value;

use super::{CommandError, CommandSpec, command, query, sync_run, to_value};
use crate::Ctx;
use crate::error::ErrorCode;

type Out = futures::future::BoxFuture<'static, Result<Value, CommandError>>;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        command("app.lock", "Lock the window", "none", run_lock),
        command("app.unlock", "Unlock the window", "none; asks Windows Hello", run_unlock),
        query("vault.status", "Vault status", "none", run_status),
        query("vault.hello", "Windows Hello status", "none", run_hello),
    ]
}

#[derive(Serialize)]
struct VaultStatus {
    ok: bool,
    code: Option<ErrorCode>,
    detail: Option<String>,
}

fn run_status(ctx: Ctx, _: Value) -> Out {
    sync_run(move || {
        let status = match ctx.session.vault_state() {
            Ok(()) => VaultStatus { ok: true, code: None, detail: None },
            Err(code) => VaultStatus { ok: false, code: Some(code), detail: None },
        };
        to_value(&status)
    })
}

#[derive(Serialize)]
struct HelloStatus {
    missing: bool,
}

fn run_hello(ctx: Ctx, _: Value) -> Out {
    sync_run(move || to_value(&HelloStatus { missing: ctx.session.hello_missing() }))
}

/// Locks the window: keys and passwords need the user's presence again.
fn run_lock(ctx: Ctx, _params: Value) -> Out {
    use futures::FutureExt;
    async move {
        ctx.session.sessions().relock().await;
        Ok(Value::Null)
    }
    .boxed()
}

/// Unlocks the window after Windows Hello; only from the window, never by an agent.
fn run_unlock(ctx: Ctx, _params: Value) -> Out {
    use futures::FutureExt;
    async move {
        if !ctx.caller.is_user() {
            return Err(CommandError::Denied("only in the window".into()));
        }
        ctx.session.sessions().verify_user().await.map_err(CommandError::code)?;
        Ok(Value::Null)
    }
    .boxed()
}
