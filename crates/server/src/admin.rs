//! Operator commands. They work on the store directly, not over HTTP, and write their output
//! to any `Write` so tests can read it. The server never knows names of areas (they are
//! encrypted), so commands name areas by id and member count.

use crate::store::{AccountRow, AreaFate, AreaOutcome, Store, StoreError};
use std::io::Write;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum AdminError {
    #[error("no user with the email {0}")]
    UnknownUser(String),
    #[error("the user has no device {0}")]
    UnknownDevice(Uuid),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

async fn user(store: &dyn Store, email: &str) -> Result<AccountRow, AdminError> {
    let norm = email.trim().to_lowercase();
    store
        .account_by_email(&norm)
        .await?
        .ok_or_else(|| AdminError::UnknownUser(email.trim().to_owned()))
}

/// `users list`
pub async fn users_list(store: &dyn Store, out: &mut impl Write) -> Result<(), AdminError> {
    writeln!(out, "EMAIL\tNAME\tCREATED\tACTIVE DEVICES\tLOCKED")?;
    for row in store.list_accounts().await? {
        let active = store
            .devices_of(&row.account)
            .await?
            .iter()
            .filter(|d| d.active)
            .count();
        writeln!(
            out,
            "{}\t{}\t{}\t{}\t{}",
            row.email,
            row.name,
            row.created_at,
            active,
            if row.locked { "locked" } else { "-" }
        )?;
    }
    Ok(())
}

/// `users lock <email>` and `users unlock <email>`. Locking ends the user's sessions.
pub async fn users_lock(
    store: &dyn Store,
    email: &str,
    locked: bool,
    out: &mut impl Write,
) -> Result<(), AdminError> {
    let row = user(store, email).await?;
    store.set_locked(&row.account, locked).await?;
    let what = if locked { "locked" } else { "unlocked" };
    writeln!(out, "{} {what}", row.email)?;
    Ok(())
}

async fn write_plan(
    store: &dyn Store,
    plan: &[AreaFate],
    out: &mut impl Write,
) -> Result<(), AdminError> {
    if plan.is_empty() {
        writeln!(out, "the user owns no areas")?;
    }
    for fate in plan {
        let outcome = match &fate.outcome {
            AreaOutcome::TransferTo(heir) => {
                let email = store
                    .account(heir)
                    .await?
                    .map(|r| r.email)
                    .unwrap_or_else(|| "an unknown account".into());
                format!("transfer to {email}")
            }
            AreaOutcome::Delete => "delete".to_owned(),
        };
        writeln!(
            out,
            "area {}\tmembers {}\t{outcome}",
            fate.area_id.0, fate.members
        )?;
    }
    Ok(())
}

/// `users delete <email> [--yes]`: without `yes` only shows what would happen to the user's areas.
pub async fn users_delete(
    store: &dyn Store,
    email: &str,
    yes: bool,
    out: &mut impl Write,
) -> Result<(), AdminError> {
    let row = user(store, email).await?;
    if !yes {
        let plan = store.plan_account_deletion(&row.account).await?;
        writeln!(out, "deleting {} would do this:", row.email)?;
        write_plan(store, &plan, out).await?;
        writeln!(out, "run again with --yes to delete")?;
        return Ok(());
    }
    let done = store.delete_account(&row.account).await?;
    writeln!(out, "deleted {}:", row.email)?;
    write_plan(store, &done, out).await
}

/// `devices list <email>`
pub async fn devices_list(
    store: &dyn Store,
    email: &str,
    out: &mut impl Write,
) -> Result<(), AdminError> {
    let row = user(store, email).await?;
    writeln!(out, "DEVICE\tNAME\tPLATFORM\tSTATE\tLAST SEEN")?;
    for d in store.devices_of(&row.account).await? {
        let state = match (d.active, d.admin_blocked) {
            (false, _) => "revoked",
            (true, true) => "blocked",
            (true, false) => "active",
        };
        let seen = d
            .last_seen
            .map_or_else(|| "-".to_owned(), |t| t.to_string());
        writeln!(
            out,
            "{}\t{}\t{}\t{state}\t{seen}",
            d.device_id, d.cert.name, d.cert.platform
        )?;
    }
    Ok(())
}

/// `devices block <email> <device-id>` and `devices unblock ...`. A block lives on the server
/// only: the server cannot sign a revocation, so the user's own revoke is still needed to rotate
/// the keys the device could read.
pub async fn devices_block(
    store: &dyn Store,
    email: &str,
    device_id: Uuid,
    blocked: bool,
    out: &mut impl Write,
) -> Result<(), AdminError> {
    let row = user(store, email).await?;
    let owns = store
        .device(device_id)
        .await?
        .is_some_and(|d| d.account == row.account);
    if !owns {
        return Err(AdminError::UnknownDevice(device_id));
    }
    store.set_admin_blocked(device_id, blocked).await?;
    let what = if blocked { "blocked" } else { "unblocked" };
    writeln!(out, "device {device_id} of {} {what}", row.email)?;
    Ok(())
}

/// `api-key show`
pub async fn api_key_show(store: &dyn Store, out: &mut impl Write) -> Result<(), AdminError> {
    let (key, _) = store.api_key().await?;
    writeln!(out, "{key}")?;
    Ok(())
}

/// `api-key rotate`: devices that have logged in before keep working.
pub async fn api_key_rotate(store: &dyn Store, out: &mut impl Write) -> Result<(), AdminError> {
    let key = store.rotate_api_key().await?;
    writeln!(out, "{key}")?;
    Ok(())
}
