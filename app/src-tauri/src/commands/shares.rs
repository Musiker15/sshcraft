//! Sharing hosts and groups with other accounts of a sync server.

use serde::{Deserialize, Serialize};
use sshcraft_core::model::{GroupId, HostId, ServerId};
use sshcraft_core::store::{ItemKind, ShareRole};
use sshcraft_proto::area::Role;
use sshcraft_proto::identity::AccountPublic;
use sshcraft_sync::contacts::{Recipient, confirm, find};
use sshcraft_sync::engine::Engine;
use sshcraft_sync::shares::{
    Grant, RevokeOutcome, ShareOutcome, ShareView, ShareWhat, list, set_grants, share,
};
use tauri::State;
use uuid::Uuid;

use super::CommandResult;
use super::sync::vault;
use crate::state::AppState;
use crate::sync::{now_ms, sync_error};

/// What to share, as the window sends it.
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

async fn engine(state: &AppState, server: ServerId) -> CommandResult<Engine> {
    let vault = vault(state)?;
    Engine::load(&state.store, &vault, server)
        .await
        .map_err(sync_error)
}

fn grants(input: Vec<GrantInput>) -> Vec<Grant> {
    input
        .into_iter()
        .map(|g| Grant {
            account: g.account,
            role: g.role,
        })
        .collect()
}

#[tauri::command]
pub async fn find_recipient(
    state: State<'_, AppState>,
    server: ServerId,
    email: String,
) -> CommandResult<Recipient> {
    let engine = engine(&state, server).await?;
    find(&engine, &state.store, &email)
        .await
        .map_err(sync_error)
}

/// The user compared the safety number (or accepted a changed key).
#[tauri::command]
pub fn confirm_recipient(
    state: State<'_, AppState>,
    server: ServerId,
    email: String,
    account: AccountPublic,
) -> CommandResult<()> {
    confirm(&state.store, server, &email, &account).map_err(sync_error)
}

#[tauri::command]
pub async fn share_items(
    state: State<'_, AppState>,
    server: ServerId,
    what: ShareWhatInput,
    grants: Vec<GrantInput>,
    with_keys: bool,
) -> CommandResult<ShareOutcome> {
    let vault = vault(&state)?;
    let mut engine = engine(&state, server).await?;
    let what = match what {
        ShareWhatInput::GroupAll { group } => ShareWhat::GroupAll(group),
        ShareWhatInput::Selection { group, hosts } => ShareWhat::Selection { group, hosts },
        ShareWhatInput::Host { host } => ShareWhat::Host(host),
    };
    let outcome = share(
        &mut engine,
        &state.store,
        &vault,
        what,
        self::grants(grants),
        with_keys,
        now_ms(),
    )
    .await
    .map_err(sync_error)?;
    state.sync.nudge(&[server]);
    Ok(outcome)
}

#[tauri::command]
pub fn list_shares(state: State<'_, AppState>, server: ServerId) -> CommandResult<Vec<ShareView>> {
    list(&state.store, server).map_err(sync_error)
}

#[tauri::command]
pub async fn set_share_grants(
    state: State<'_, AppState>,
    server: ServerId,
    area: Uuid,
    grants: Vec<GrantInput>,
) -> CommandResult<RevokeOutcome> {
    let vault = vault(&state)?;
    let mut engine = engine(&state, server).await?;
    let outcome = set_grants(
        &mut engine,
        &state.store,
        &vault,
        area,
        self::grants(grants),
        now_ms(),
    )
    .await
    .map_err(sync_error)?;
    state.sync.nudge(&[server]);
    Ok(outcome)
}

#[tauri::command]
pub async fn unshare(
    state: State<'_, AppState>,
    server: ServerId,
    area: Uuid,
) -> CommandResult<RevokeOutcome> {
    let vault = vault(&state)?;
    let mut engine = engine(&state, server).await?;
    let outcome = sshcraft_sync::shares::unshare(&mut engine, &state.store, &vault, area, now_ms())
        .await
        .map_err(sync_error)?;
    state.sync.nudge(&[server]);
    Ok(outcome)
}

/// For the hosts page: which groups and hosts are shared or came in a share.
#[tauri::command]
pub fn item_shares(state: State<'_, AppState>) -> CommandResult<Vec<ItemShare>> {
    let store = state.store.lock().expect("store lock");
    let mut items: Vec<(ItemKind, String)> = store
        .list_groups()?
        .into_iter()
        .map(|g| (ItemKind::Group, g.id.0.to_string()))
        .collect();
    items.extend(
        store
            .list_hosts()?
            .into_iter()
            .map(|h| (ItemKind::Host, h.id.0.to_string())),
    );
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
            let owner_name = if share.owned {
                None
            } else {
                store
                    .contact_by_account(server, &share.owner)?
                    .map(|c| c.name)
            };
            out.push(ItemShare {
                kind,
                id,
                owned: share.owned,
                owner_name,
                role: share.role,
            });
        } else if !store.copies_of(server, kind, &id)?.is_empty() {
            out.push(ItemShare {
                kind,
                id,
                owned: true,
                owner_name: None,
                role: None,
            });
        }
    }
    Ok(out)
}
