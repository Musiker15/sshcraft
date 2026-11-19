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
    ]
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
