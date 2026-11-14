//! Sync servers: set up, switch on and off, devices, moving items between places.

use futures::FutureExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sshcraft_model::model::ServerId;
use sshcraft_store::store::ItemKind;
use sshcraft_sync::account::{JoinAccount, NewAccount, Setup, account_key_qr_svg, create_account, join_account};
use sshcraft_sync::devices::{list_devices, revoke_device};
use sshcraft_sync::engine::Engine;
use uuid::Uuid;
use zeroize::Zeroizing;

use super::{CommandError, CommandSpec, command, needs_vault, parse, query, sync_run, to_value};
use crate::Ctx;
use crate::sync::{StatusView, note_change, now_ms, server_status, sync_error};

type Out = futures::future::BoxFuture<'static, Result<Value, CommandError>>;

pub fn specs() -> Vec<CommandSpec> {
    let sealed = |spec: CommandSpec| CommandSpec { enabled: needs_vault, ..spec };
    vec![
        query("sync.servers", "Sync servers", "none", run_servers),
        command("sync.set_server", "Switch sync", "{ id, sync_enabled, sync_keys }", run_set_server),
        command("sync.server_logs", "Switch log sync", "{ id, on }", run_server_logs),
        sealed(command(
            "sync.create_account",
            "Create a sync account",
            "{ input: { server_name, origin, api_key, name, email, device_name } }",
            run_create_account,
        )),
        sealed(command(
            "sync.join_account",
            "Join a sync account",
            "{ input: { server_name, origin, api_key, account_key, device_name } }",
            run_join_account,
        )),
        command("sync.remove_server", "Remove a sync server", "{ id }", run_remove_server),
        sealed(query("device.list", "Devices of an account", "{ id: server id }", run_devices)),
        sealed(command("device.revoke", "Revoke a device", "{ id: server id, device_id }", run_revoke)),
        command("item.move", "Move an item", "{ kind, item_id, location?: server id, none for this device only }", run_move),
    ]
}

#[derive(Debug, Clone, Serialize)]
pub struct ServerView {
    pub id: ServerId,
    pub name: String,
    pub origin: String,
    pub sync_enabled: bool,
    pub sync_keys: bool,
    /// Session logs sync too.
    pub sync_logs: bool,
    pub revoked: bool,
    pub status: StatusView,
}

#[derive(Deserialize)]
pub struct CreateInput {
    pub server_name: String,
    pub origin: String,
    pub api_key: Zeroizing<String>,
    pub name: String,
    pub email: String,
    pub device_name: String,
}

#[derive(Deserialize)]
pub struct JoinInput {
    pub server_name: String,
    pub origin: String,
    pub api_key: Zeroizing<String>,
    pub account_key: Zeroizing<String>,
    pub device_name: String,
}

impl std::fmt::Debug for CreateInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CreateInput").field("server_name", &self.server_name).field("origin", &self.origin).finish_non_exhaustive()
    }
}

impl std::fmt::Debug for JoinInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JoinInput").field("server_name", &self.server_name).field("origin", &self.origin).finish_non_exhaustive()
    }
}

/// Shown once after an account was created; the key is not stored anywhere.
#[derive(Serialize)]
pub struct AccountCreated {
    /// `None` when the account exists but setting it up here did not finish: the user keeps
    /// the key and finishes by joining with it.
    pub server_id: Option<ServerId>,
    pub account_key: Zeroizing<String>,
    pub qr_svg: String,
    /// Why the setup did not finish, if it did not.
    pub unfinished: Option<CommandError>,
}

fn platform() -> &'static str {
    std::env::consts::OS
}

fn run_servers(ctx: Ctx, _: Value) -> Out {
    sync_run(move || {
        let session = ctx.session;
        let rows = session.store().servers()?;
        let servers: Vec<ServerView> = rows
            .into_iter()
            .map(|row| ServerView {
                status: server_status(row.revoked, session.sync().status(row.id)),
                sync_logs: session.store().sync_logs(row.id).unwrap_or(false),
                id: row.id,
                name: row.name,
                origin: row.origin,
                sync_enabled: row.sync_enabled,
                sync_keys: row.sync_keys,
                revoked: row.revoked,
            })
            .collect();
        to_value(&servers)
    })
}

#[derive(Deserialize)]
struct InputParams<T> {
    input: T,
}

fn run_create_account(ctx: Ctx, params: Value) -> Out {
    async move {
        let InputParams { input } = parse::<InputParams<CreateInput>>("sync.create_account", params)?;
        let session = ctx.session;
        let vault = session.vault()?;
        let new = NewAccount {
            server_name: input.server_name,
            origin: input.origin,
            api_key: input.api_key.to_string(),
            name: input.name,
            email: input.email,
            device_name: input.device_name,
            platform: platform().to_owned(),
        };
        let (setup, export) = create_account(&session.shared_store(), &vault, new, now_ms() / 1000).await.map_err(sync_error)?;
        let (server_id, unfinished) = match setup {
            Setup::Ready(id) => {
                session.sync().start(id);
                (Some(id), None)
            }
            Setup::Unfinished(err) => (None, Some(sync_error(err))),
        };
        to_value(&AccountCreated { server_id, qr_svg: account_key_qr_svg(&export), account_key: export, unfinished })
    }
    .boxed()
}

fn run_join_account(ctx: Ctx, params: Value) -> Out {
    async move {
        let InputParams { input } = parse::<InputParams<JoinInput>>("sync.join_account", params)?;
        let session = ctx.session;
        let vault = session.vault()?;
        let join = JoinAccount {
            server_name: input.server_name,
            origin: input.origin,
            api_key: input.api_key.to_string(),
            account_key: input.account_key.to_string(),
            device_name: input.device_name,
            platform: platform().to_owned(),
        };
        let id = join_account(&session.shared_store(), &vault, join, now_ms() / 1000).await.map_err(sync_error)?;
        session.sync().start(id);
        to_value(&id)
    }
    .boxed()
}

#[derive(Deserialize)]
struct SetServerParams {
    id: ServerId,
    sync_enabled: bool,
    sync_keys: bool,
}

/// Switches sync and key sync. With key sync off, keys at this server become local keys.
fn run_set_server(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let SetServerParams { id, sync_enabled, sync_keys } = parse("sync.set_server", params)?;
        {
            let store = ctx.session.store();
            store.update_server_flags(id, sync_enabled, sync_keys)?;
            if !sync_keys {
                for key in store.list_key_records()? {
                    if key.location == Some(id) {
                        store.set_location(ItemKind::Key, &key.id.0.to_string(), None)?;
                    }
                }
            }
        }
        if sync_enabled {
            ctx.session.sync().start(id);
        } else {
            ctx.session.sync().stop(id);
        }
        Ok(Value::Null)
    })
}

#[derive(Deserialize)]
struct LogsParams {
    id: ServerId,
    on: bool,
}

/// Switches log sync for a server; turning it on sends the logs already there.
fn run_server_logs(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let LogsParams { id, on } = parse("sync.server_logs", params)?;
        sshcraft_sync::logs::set_sync_logs(&ctx.session.store(), id, on, now_ms()).map_err(sync_error)?;
        ctx.session.sync().nudge(&[id]);
        Ok(Value::Null)
    })
}

#[derive(Deserialize)]
struct IdParams {
    id: ServerId,
}

/// Forgets a server on this device; its items stay here as local items.
fn run_remove_server(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let IdParams { id } = parse("sync.remove_server", params)?;
        ctx.session.sync().stop(id);
        ctx.session.store().remove_server(id)?;
        ctx.session.forwards().reconcile();
        ctx.session.announce(&[]);
        Ok(Value::Null)
    })
}

fn run_devices(ctx: Ctx, params: Value) -> Out {
    async move {
        let IdParams { id } = parse("device.list", params)?;
        let vault = ctx.session.vault()?;
        let store = ctx.session.shared_store();
        let mut engine = Engine::load(&store, &vault, id).await.map_err(sync_error)?;
        to_value(&list_devices(&mut engine, &store).await.map_err(sync_error)?)
    }
    .boxed()
}

#[derive(Deserialize)]
struct RevokeParams {
    id: ServerId,
    device_id: Uuid,
}

fn run_revoke(ctx: Ctx, params: Value) -> Out {
    async move {
        let RevokeParams { id, device_id } = parse("device.revoke", params)?;
        let _turn = sshcraft_sync::engine::server_lock(id).lock_owned().await;
        let vault = ctx.session.vault()?;
        let store = ctx.session.shared_store();
        let mut engine = Engine::load(&store, &vault, id).await.map_err(sync_error)?;
        revoke_device(&mut engine, &store, &vault, device_id, now_ms() / 1000).await.map_err(sync_error)?;
        ctx.session.sync().nudge(&[id]);
        Ok(Value::Null)
    }
    .boxed()
}

#[derive(Deserialize)]
struct MoveParams {
    kind: ItemKind,
    item_id: String,
    #[serde(default)]
    location: Option<ServerId>,
}

/// Moves an item to a server or back to this device only.
fn run_move(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let MoveParams { kind, item_id, location } = parse("item.move", params)?;
        let servers = {
            let store = ctx.session.store();
            crate::sync::share_rights(&store, crate::sync::Change::Move(kind, &item_id))?;
            let before = store.location_of(kind, &item_id)?;
            store.set_location(kind, &item_id, location)?;
            note_change(&store, kind, &item_id, before, now_ms())?
        };
        ctx.session.sync().nudge(&servers);
        ctx.session.announce(&servers);
        Ok(Value::Null)
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn inputs_never_show_their_secrets() {
        let input: super::JoinInput = serde_json::from_value(serde_json::json!({
            "server_name": "home",
            "origin": "https://sync.example.org",
            "api_key": "api-secret",
            "account_key": "sshcraft-account-1:SECRET",
            "device_name": "laptop",
        }))
        .unwrap();
        let shown = format!("{input:?}");
        assert!(!shown.contains("api-secret") && !shown.contains("SECRET"), "{shown}");
    }
}
