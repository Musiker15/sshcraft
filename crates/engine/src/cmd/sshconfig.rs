//! `~/.ssh/config` kept in step and written back on request (see `crate::ssh_config`). Neither
//! command is open to agents: writing the file changes what `ssh` runs.

use serde::Deserialize;
use serde_json::{Value, json};
use sshcraft_model::model::HostId;

use super::{CommandError, CommandSpec, command, parse, to_value};
use crate::Ctx;
use crate::error::ErrorCode;

type Out = futures::future::BoxFuture<'static, Result<Value, CommandError>>;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        command("sshconfig.refresh", "Read ~/.ssh/config again", "none", run_refresh),
        command("sshconfig.write", "Write hosts to ~/.ssh/config", "{ host_ids: [id] }", run_write),
    ]
}

fn home() -> Result<std::path::PathBuf, CommandError> {
    sshcraft_platform::home_dir().ok_or_else(|| CommandError::failed(ErrorCode::Io, "no home directory"))
}

fn run_refresh(ctx: Ctx, _params: Value) -> Out {
    use futures::FutureExt;
    async move {
        let (outcome, _) = crate::ssh_config::refresh_from(&ctx.session, &home()?).await?;
        to_value(&outcome)
    }
    .boxed()
}

#[derive(Deserialize)]
struct WriteParams {
    host_ids: Vec<HostId>,
}

fn run_write(ctx: Ctx, params: Value) -> Out {
    use futures::FutureExt;
    async move {
        let WriteParams { host_ids } = parse("sshconfig.write", params)?;
        if host_ids.is_empty() || host_ids.len() > 1000 {
            return Err(CommandError::bad_params("sshconfig.write", "1 to 1000 hosts"));
        }
        let files = crate::ssh_config::write_back(&ctx.session, &host_ids, &home()?)?;
        ctx.session.announce(&[]);
        Ok(json!({ "files": files }))
    }
    .boxed()
}
