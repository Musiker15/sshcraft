//! The host keys SSHCraft trusts: listed, removed one by one, written as a `known_hosts` file.
//! Reading one in is `import.known_hosts`.

use serde::Deserialize;
use serde_json::{Value, json};
use sshcraft_import::import::knownhosts;

use super::{CommandError, CommandSpec, command, parse, query, sync_run, to_value};
use crate::Ctx;
use crate::error::ErrorCode;

type Out = futures::future::BoxFuture<'static, Result<Value, CommandError>>;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        query("hostkey.list", "Known host keys", "{ address?: only this address }", run_list),
        command("hostkey.remove", "Remove a known host key", "{ address, port, fingerprint }", run_remove),
        command("hostkey.export", "Export known host keys", "{ path }: a known_hosts file", run_export),
    ]
}

#[derive(Deserialize)]
struct ListParams {
    #[serde(default)]
    address: Option<String>,
}

fn run_list(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let ListParams { address } = parse("hostkey.list", if params.is_null() { json!({}) } else { params })?;
        let mut keys = ctx.session.store().all_host_keys()?;
        if let Some(address) = address {
            keys.retain(|k| k.address == address);
        }
        to_value(&keys)
    })
}

#[derive(Deserialize)]
struct RemoveParams {
    address: String,
    port: u16,
    fingerprint: String,
}

fn run_remove(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let RemoveParams { address, port, fingerprint } = parse("hostkey.remove", params)?;
        if !ctx.session.store().remove_host_key(&address, port, &fingerprint)? {
            return Err(CommandError::NotFound(format!("host key {fingerprint} of {address}:{port}")));
        }
        ctx.session.announce(&[]);
        Ok(Value::Null)
    })
}

#[derive(Deserialize)]
struct ExportParams {
    path: String,
}

fn run_export(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let ExportParams { path } = parse("hostkey.export", params)?;
        let (text, skipped) = knownhosts::export(&ctx.session.store().all_host_keys()?);
        std::fs::write(&path, text).map_err(|e| CommandError::failed(ErrorCode::Io, e))?;
        Ok(json!({ "path": path, "skipped": skipped }))
    })
}
