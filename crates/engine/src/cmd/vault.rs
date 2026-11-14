//! The state of the vault and of Windows Hello.

use serde::Serialize;
use serde_json::Value;

use super::{CommandError, CommandSpec, query, sync_run, to_value};
use crate::Ctx;
use crate::error::ErrorCode;

type Out = futures::future::BoxFuture<'static, Result<Value, CommandError>>;

pub fn specs() -> Vec<CommandSpec> {
    vec![query("vault.status", "Vault status", "none", run_status), query("vault.hello", "Windows Hello status", "none", run_hello)]
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
