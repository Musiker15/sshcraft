//! The encrypted backup file (see `sshcraft_store::backup`): written and read only by the user
//! in the window, never by an agent. The passphrase is never kept or logged.

use serde::Deserialize;
use serde_json::{Value, json};
use sshcraft_store::backup::{self, BackupError};
use zeroize::Zeroizing;

use super::{CommandError, CommandSpec, command, needs_vault, parse, sync_run, to_value};
use crate::Ctx;
use crate::error::ErrorCode;

type Out = futures::future::BoxFuture<'static, Result<Value, CommandError>>;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            enabled: needs_vault,
            ..command("backup.create", "Write a backup file", "{ path, passphrase: at least 12 characters }", run_create)
        },
        CommandSpec {
            enabled: needs_vault,
            ..command("backup.restore", "Restore from a backup file", "{ path, passphrase }: adds what is missing", run_restore)
        },
    ]
}

#[derive(Deserialize)]
struct FileParams {
    path: String,
    passphrase: Zeroizing<String>,
}

fn backup_error(e: BackupError) -> CommandError {
    match e {
        BackupError::WeakPassphrase => CommandError::bad_params("backup", e.to_string()),
        BackupError::NotABackup | BackupError::WrongPassphraseOrChanged | BackupError::TooLarge => CommandError::Denied(e.to_string()),
        BackupError::Store(e) => CommandError::failed(ErrorCode::Store, e),
        BackupError::Vault(e) => CommandError::failed(ErrorCode::Store, e),
    }
}

fn run_create(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let FileParams { path, passphrase } = parse("backup.create", params)?;
        let vault = ctx.session.vault()?;
        let (text, counts) = backup::create(&ctx.session.store(), &vault, &passphrase, crate::now_ms() / 1000).map_err(backup_error)?;
        std::fs::write(&path, text).map_err(|e| CommandError::failed(ErrorCode::Io, e))?;
        Ok(json!({ "path": path, "counts": counts }))
    })
}

fn run_restore(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let FileParams { path, passphrase } = parse("backup.restore", params)?;
        let vault = ctx.session.vault()?;
        let bytes = super::import::read_limited(std::path::Path::new(&path), u64::try_from(backup::MAX_FILE).unwrap_or(u64::MAX))?;
        let text = String::from_utf8(bytes).map_err(|_| backup_error(BackupError::NotABackup))?;
        let (counts, at_servers) = backup::restore_missing(&ctx.session.store(), &vault, &text, &passphrase).map_err(backup_error)?;
        // What lives at a sync server goes there again (the server and other devices lost it).
        let mut servers = Vec::new();
        {
            let store = ctx.session.store();
            for (kind, id) in at_servers {
                for server in crate::sync::note_change(&store, kind, &id, None, crate::sync::now_ms())? {
                    if !servers.contains(&server) {
                        servers.push(server);
                    }
                }
            }
        }
        ctx.session.sync().nudge(&servers);
        ctx.session.announce(&servers);
        to_value(&counts)
    })
}
