//! Sharing hosts and groups with other accounts of a sync server.

use futures::FutureExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sshcraft_model::model::{GroupId, HostId, ServerId};
use sshcraft_proto::area::Role;
use sshcraft_proto::identity::AccountPublic;
use sshcraft_store::store::{ItemKind, ShareRole};
use sshcraft_sync::contacts::{confirm, find};
use sshcraft_sync::engine::Engine;
use sshcraft_sync::shares::{Grant, ShareWhat, list, set_grants, share};
use uuid::Uuid;

use super::{CommandError, CommandSpec, command, needs_vault, parse, query, sync_run, to_value};
use crate::sync::{now_ms, sync_error};
use crate::{Ctx, Session};

type Out = futures::future::BoxFuture<'static, Result<Value, CommandError>>;

pub fn specs() -> Vec<CommandSpec> {
    let sealed = |spec: CommandSpec| CommandSpec { enabled: needs_vault, ..spec };
    vec![
        sealed(query("share.find_recipient", "Find an account to share with", "{ server, email }", run_find)),
        // Trusting a recipient's key is the user's decision alone.
        command("share.confirm_recipient", "Confirm a recipient", "{ server, email, account }", run_confirm),
        sealed(command(
            "share.items",
            "Share hosts or a group",
            "{ server, what: { kind: group_all, group } | { kind: selection, group, hosts } | { kind: host, host }, grants: [{ account, role }], with_keys }",
            run_share,
        )),
        query("share.list", "Shares of a server", "{ server }", run_list),
        sealed(command("share.set_grants", "Change who a share goes to", "{ server, area, grants: [{ account, role }] }", run_set_grants)),
        sealed(command("share.end", "End a share", "{ server, area }", run_end)),
        query("share.of_items", "Which hosts and groups are shared", "none", run_of_items),
    ]
}

/// What to share, as the interface sends it.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ShareWhatInput {
    GroupAll { group: GroupId },
    Selection { group: GroupId, hosts: Vec<HostId> },
    Host { host: HostId },
}

#[derive(Debug, Clone, Deserialize)]
pub struct GrantInput {
    pub account: AccountPublic,
    pub role: Role,
}

/// Whether an item is shared (own share) or came in a share (received).
#[derive(Debug, Clone, Serialize)]
pub struct ItemShare {
    pub kind: ItemKind,
    pub id: String,
    pub owned: bool,
    /// For a received item: the owner's name, when this device looked it up.
    pub owner_name: Option<String>,
    pub role: Option<ShareRole>,
}

async fn engine(session: &Session, server: ServerId) -> Result<Engine, CommandError> {
    let vault = session.vault()?;
    Engine::load(&session.shared_store(), &vault, server).await.map_err(sync_error)
}

fn grants(input: Vec<GrantInput>) -> Vec<Grant> {
    input.into_iter().map(|g| Grant { account: g.account, role: g.role }).collect()
}

#[derive(Deserialize)]
struct FindParams {
    server: ServerId,
    email: String,
}

fn run_find(ctx: Ctx, params: Value) -> Out {
    async move {
        let FindParams { server, email } = parse("share.find_recipient", params)?;
        let engine = engine(&ctx.session, server).await?;
        to_value(&find(&engine, &ctx.session.shared_store(), &email).await.map_err(sync_error)?)
    }
    .boxed()
}

#[derive(Deserialize)]
struct ConfirmParams {
    server: ServerId,
    email: String,
    account: AccountPublic,
}

/// The user compared the safety number (or accepted a changed key).
fn run_confirm(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let ConfirmParams { server, email, account } = parse("share.confirm_recipient", params)?;
        confirm(&ctx.session.shared_store(), server, &email, &account).map_err(sync_error)?;
        Ok(Value::Null)
    })
}

#[derive(Deserialize)]
struct ShareParams {
    server: ServerId,
    what: ShareWhatInput,
    grants: Vec<GrantInput>,
    with_keys: bool,
}

fn run_share(ctx: Ctx, params: Value) -> Out {
    async move {
        let ShareParams { server, what, grants: wanted, with_keys } = parse("share.items", params)?;
        let session = ctx.session;
        let _turn = sshcraft_sync::engine::server_lock(server).lock_owned().await;
        let vault = session.vault()?;
        let mut engine = engine(&session, server).await?;
        let what = match what {
            ShareWhatInput::GroupAll { group } => ShareWhat::GroupAll(group),
            ShareWhatInput::Selection { group, hosts } => ShareWhat::Selection { group, hosts },
            ShareWhatInput::Host { host } => ShareWhat::Host(host),
        };
        let outcome = share(&mut engine, &session.shared_store(), &vault, what, grants(wanted), with_keys, now_ms()).await.map_err(sync_error)?;
        session.sync().nudge(&[server]);
        session.announce(&[server]);
        to_value(&outcome)
    }
    .boxed()
}

#[derive(Deserialize)]
struct ServerParams {
    server: ServerId,
}

fn run_list(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let ServerParams { server } = parse("share.list", params)?;
        to_value(&list(&ctx.session.shared_store(), server).map_err(sync_error)?)
    })
}

#[derive(Deserialize)]
struct GrantsParams {
    server: ServerId,
    area: Uuid,
    grants: Vec<GrantInput>,
}

fn run_set_grants(ctx: Ctx, params: Value) -> Out {
    async move {
        let GrantsParams { server, area, grants: wanted } = parse("share.set_grants", params)?;
        let session = ctx.session;
        let _turn = sshcraft_sync::engine::server_lock(server).lock_owned().await;
        let vault = session.vault()?;
        let mut engine = engine(&session, server).await?;
        let outcome = set_grants(&mut engine, &session.shared_store(), &vault, area, grants(wanted), now_ms()).await.map_err(sync_error)?;
        session.sync().nudge(&[server]);
        to_value(&outcome)
    }
    .boxed()
}

#[derive(Deserialize)]
struct AreaParams {
    server: ServerId,
    area: Uuid,
}

fn run_end(ctx: Ctx, params: Value) -> Out {
    async move {
        let AreaParams { server, area } = parse("share.end", params)?;
        let session = ctx.session;
        let _turn = sshcraft_sync::engine::server_lock(server).lock_owned().await;
        let vault = session.vault()?;
        let mut engine = engine(&session, server).await?;
        let outcome = sshcraft_sync::shares::unshare(&mut engine, &session.shared_store(), &vault, area, now_ms()).await.map_err(sync_error)?;
        session.sync().nudge(&[server]);
        session.announce(&[server]);
        to_value(&outcome)
    }
    .boxed()
}

/// For the hosts page: which groups and hosts are shared or came in a share.
fn run_of_items(ctx: Ctx, _: Value) -> Out {
    sync_run(move || {
        let store = ctx.session.store();
        let mut items: Vec<(ItemKind, String)> = store.list_groups()?.into_iter().map(|g| (ItemKind::Group, g.id.0.to_string())).collect();
        items.extend(store.list_hosts()?.into_iter().map(|h| (ItemKind::Host, h.id.0.to_string())));
        let mut out = Vec::new();
        for (kind, id) in items {
            let Some(server) = store.location_of(kind, &id)? else {
                continue;
            };
            let share = match store.effective_area(server, kind, &id)? {
                Some(area) => store.share(server, area)?,
                None => None,
            };
            if let Some(share) = share {
                let owner_name = if share.owned { None } else { store.contact_by_account(server, &share.owner)?.map(|c| c.name) };
                out.push(ItemShare { kind, id, owned: share.owned, owner_name, role: share.role });
            } else if !store.copies_of(server, kind, &id)?.is_empty() {
                out.push(ItemShare { kind, id, owned: true, owner_name: None, role: None });
            }
        }
        to_value(&out)
    })
}
