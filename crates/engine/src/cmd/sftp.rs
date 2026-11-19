//! SFTP (S1): open a host's files, list, create folders, rename, change permissions, delete; the
//! local side's listing for the pane beside it. Agents have no access (not in
//! `gate::access::access_for`).

use futures::FutureExt;
use serde::Deserialize;
use serde_json::{Value, json};
use sshcraft_model::model::HostId;

use super::{CommandError, CommandSpec, command, parse, query, to_value};
use crate::Ctx;
use crate::error::ErrorCode;
use crate::sftp::SftpId;
use crate::sftp::manager::SftpFailure;
use crate::sftp::transfers::Conflict;

type Out = futures::future::BoxFuture<'static, Result<Value, CommandError>>;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        command("sftp.open", "Open a host's files", "{ host_id } -> { id, home }", run_open),
        command("sftp.close", "Close a host's files", "{ id }", run_close),
        query(
            "sftp.list",
            "List a remote folder",
            "{ id, path } -> { path, entries: [{ name, kind, size, modified, permissions }], truncated }",
            run_list,
        ),
        query(
            "sftp.local_list",
            "List a local folder",
            "{ path?: absolute, default the home folder } -> { path, entries, truncated }",
            run_local_list,
        ),
        command("sftp.mkdir", "Create a remote folder", "{ id, dir, name } -> { path }", run_mkdir),
        command("sftp.rename", "Rename a remote file", "{ id, path, name } -> { path }", run_rename),
        command("sftp.chmod", "Change remote permissions", "{ id, path, mode: 0..=0o7777 }", run_chmod),
        query("sftp.count", "Count remote files", "{ id, paths } -> { files, dirs, bytes }", run_count),
        command("sftp.delete", "Delete remote files", "{ id, paths } -> { deleted }", run_delete),
        command(
            "sftp.upload",
            "Upload files",
            "{ id, local: [absolute paths], remote_dir, conflict?: skip | overwrite | rename, name?: for one file } -> { transfer }",
            run_upload,
        ),
        command(
            "sftp.download",
            "Download files",
            "{ id, remote: [paths], local_dir: absolute, conflict?: skip | overwrite | rename } -> { transfer }",
            run_download,
        ),
        command(
            "sftp.copy",
            "Copy files between hosts",
            "{ from: id, remote: [paths], to: id, remote_dir, conflict?: skip | overwrite | rename } -> { transfer }",
            run_copy,
        ),
        query(
            "sftp.transfers",
            "List transfers",
            "-> [{ id, sftp, source, direction, label, state, files_done, files_total, bytes_done, bytes_total, skipped, errors, resumed }]",
            run_transfers,
        ),
        command("sftp.cancel", "Cancel a transfer", "{ transfer }", run_cancel),
        command("sftp.clear_transfers", "Clear ended transfers", "{}", run_clear),
        command(
            "sftp.download_file",
            "Download into a file",
            "{ id, remote: path of a file, local_file: an existing file, overwritten } -> { transfer }",
            run_download_file,
        ),
        command("sftp.retry", "Try a transfer again", "{ transfer } -> { transfer }", run_retry),
        command("sftp.reconnect", "Connect a host's files again", "{ id } -> { home }", run_reconnect),
    ]
}

fn run_download_file(ctx: Ctx, params: Value) -> Out {
    async move {
        let DownloadFileParams { id, remote, local_file } = parse("sftp.download_file", params)?;
        let transfer = ctx.session.sftp().download_file(id, remote, local_file).map_err(failed)?;
        Ok(json!({ "transfer": transfer }))
    }
    .boxed()
}

fn run_retry(ctx: Ctx, params: Value) -> Out {
    async move {
        let TransferParams { transfer } = parse("sftp.retry", params)?;
        let transfer = ctx.session.sftp().retry(transfer).map_err(failed)?;
        Ok(json!({ "transfer": transfer }))
    }
    .boxed()
}

fn run_reconnect(ctx: Ctx, params: Value) -> Out {
    async move {
        let IdParams { id } = parse("sftp.reconnect", params)?;
        let home = ctx.session.sftp().reconnect(id).await.map_err(failed)?;
        Ok(json!({ "home": home }))
    }
    .boxed()
}

fn failed((code, detail): SftpFailure) -> CommandError {
    CommandError::Failed { code, detail }
}

#[derive(Deserialize)]
struct OpenParams {
    host_id: HostId,
}

#[derive(Deserialize)]
struct IdParams {
    id: SftpId,
}

#[derive(Deserialize)]
struct PathParams {
    id: SftpId,
    path: String,
}

#[derive(Deserialize)]
struct LocalParams {
    #[serde(default)]
    path: Option<String>,
}

#[derive(Deserialize)]
struct MkdirParams {
    id: SftpId,
    dir: String,
    name: String,
}

#[derive(Deserialize)]
struct RenameParams {
    id: SftpId,
    path: String,
    name: String,
}

#[derive(Deserialize)]
struct ChmodParams {
    id: SftpId,
    path: String,
    mode: u32,
}

/// At most this many paths in one call.
const MAX_PATHS: usize = 10_000;

#[derive(Deserialize)]
struct PathsParams {
    id: SftpId,
    paths: Vec<String>,
}

fn paths(id: &str, params: Value) -> Result<PathsParams, CommandError> {
    let p: PathsParams = parse(id, params)?;
    if p.paths.is_empty() || p.paths.len() > MAX_PATHS {
        return Err(CommandError::bad_params(id, format!("paths: 1 to {MAX_PATHS}")));
    }
    Ok(p)
}

fn run_open(ctx: Ctx, params: Value) -> Out {
    async move {
        let OpenParams { host_id } = parse("sftp.open", params)?;
        let (id, home) = ctx.session.sftp().open(host_id).await.map_err(failed)?;
        Ok(json!({ "id": id, "home": home }))
    }
    .boxed()
}

fn run_close(ctx: Ctx, params: Value) -> Out {
    async move {
        let IdParams { id } = parse("sftp.close", params)?;
        if !ctx.session.sftp().close(id).await {
            return Err(CommandError::NotFound(format!("SFTP session {id}")));
        }
        Ok(Value::Null)
    }
    .boxed()
}

fn run_list(ctx: Ctx, params: Value) -> Out {
    async move {
        let PathParams { id, path } = parse("sftp.list", params)?;
        to_value(&ctx.session.sftp().list(id, &path).await.map_err(failed)?)
    }
    .boxed()
}

/// The user's home folder, the local pane's start.
pub(crate) fn local_home() -> Option<std::path::PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(std::path::PathBuf::from).filter(|p| p.is_dir())
}

fn run_local_list(_ctx: Ctx, params: Value) -> Out {
    async move {
        let LocalParams { path } = parse("sftp.local_list", params)?;
        let path = match path {
            Some(p) if !p.is_empty() => std::path::PathBuf::from(p),
            _ => local_home().ok_or_else(|| CommandError::failed(ErrorCode::NotFound, "no home folder"))?,
        };
        to_value(&crate::sftp::manager::local_list(path).await.map_err(failed)?)
    }
    .boxed()
}

fn run_mkdir(ctx: Ctx, params: Value) -> Out {
    async move {
        let MkdirParams { id, dir, name } = parse("sftp.mkdir", params)?;
        let path = ctx.session.sftp().mkdir(id, &dir, &name).await.map_err(failed)?;
        Ok(json!({ "path": path }))
    }
    .boxed()
}

fn run_rename(ctx: Ctx, params: Value) -> Out {
    async move {
        let RenameParams { id, path, name } = parse("sftp.rename", params)?;
        let path = ctx.session.sftp().rename(id, &path, &name).await.map_err(failed)?;
        Ok(json!({ "path": path }))
    }
    .boxed()
}

fn run_chmod(ctx: Ctx, params: Value) -> Out {
    async move {
        let ChmodParams { id, path, mode } = parse("sftp.chmod", params)?;
        ctx.session.sftp().chmod(id, &path, mode).await.map_err(failed)?;
        Ok(Value::Null)
    }
    .boxed()
}

fn run_count(ctx: Ctx, params: Value) -> Out {
    async move {
        let PathsParams { id, paths } = paths("sftp.count", params)?;
        to_value(&ctx.session.sftp().count(id, &paths).await.map_err(failed)?)
    }
    .boxed()
}

fn run_delete(ctx: Ctx, params: Value) -> Out {
    async move {
        let PathsParams { id, paths } = paths("sftp.delete", params)?;
        let deleted = ctx.session.sftp().delete(id, &paths).await.map_err(failed)?;
        Ok(json!({ "deleted": deleted }))
    }
    .boxed()
}

#[derive(Deserialize)]
struct UploadParams {
    id: SftpId,
    local: Vec<std::path::PathBuf>,
    remote_dir: String,
    #[serde(default)]
    conflict: Conflict,
    /// The name on the server, for one file whose own name says nothing (Android's picker).
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
struct DownloadFileParams {
    id: SftpId,
    remote: String,
    local_file: std::path::PathBuf,
}

#[derive(Deserialize)]
struct DownloadParams {
    id: SftpId,
    remote: Vec<String>,
    local_dir: std::path::PathBuf,
    #[serde(default)]
    conflict: Conflict,
}

#[derive(Deserialize)]
struct CopyParams {
    from: SftpId,
    remote: Vec<String>,
    to: SftpId,
    remote_dir: String,
    #[serde(default)]
    conflict: Conflict,
}

fn run_copy(ctx: Ctx, params: Value) -> Out {
    async move {
        let CopyParams { from, remote, to, remote_dir, conflict } = parse("sftp.copy", params)?;
        if remote.is_empty() || remote.len() > MAX_PATHS {
            return Err(CommandError::bad_params("sftp.copy", format!("remote: 1 to {MAX_PATHS}")));
        }
        let transfer = ctx.session.sftp().copy(from, remote, to, remote_dir, conflict).map_err(failed)?;
        Ok(json!({ "transfer": transfer }))
    }
    .boxed()
}

#[derive(Deserialize)]
struct TransferParams {
    transfer: crate::sftp::transfers::TransferId,
}

fn run_upload(ctx: Ctx, params: Value) -> Out {
    async move {
        let UploadParams { id, local, remote_dir, conflict, name } = parse("sftp.upload", params)?;
        if local.is_empty() || local.len() > MAX_PATHS {
            return Err(CommandError::bad_params("sftp.upload", format!("local: 1 to {MAX_PATHS}")));
        }
        let transfer = ctx.session.sftp().upload(id, local, remote_dir, conflict, name).map_err(failed)?;
        Ok(json!({ "transfer": transfer }))
    }
    .boxed()
}

fn run_download(ctx: Ctx, params: Value) -> Out {
    async move {
        let DownloadParams { id, remote, local_dir, conflict } = parse("sftp.download", params)?;
        if remote.is_empty() || remote.len() > MAX_PATHS {
            return Err(CommandError::bad_params("sftp.download", format!("remote: 1 to {MAX_PATHS}")));
        }
        let transfer = ctx.session.sftp().download(id, remote, local_dir, conflict).map_err(failed)?;
        Ok(json!({ "transfer": transfer }))
    }
    .boxed()
}

fn run_transfers(ctx: Ctx, _params: Value) -> Out {
    async move { to_value(&ctx.session.sftp().transfers().list()) }.boxed()
}

fn run_cancel(ctx: Ctx, params: Value) -> Out {
    async move {
        let TransferParams { transfer } = parse("sftp.cancel", params)?;
        if !ctx.session.sftp().transfers().cancel(transfer) {
            return Err(CommandError::NotFound(format!("transfer {transfer}")));
        }
        Ok(Value::Null)
    }
    .boxed()
}

fn run_clear(ctx: Ctx, _params: Value) -> Out {
    async move {
        ctx.session.sftp().transfers().clear_ended();
        Ok(Value::Null)
    }
    .boxed()
}
