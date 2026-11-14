//! Opening links from terminal output: only `http` and `https`, and only the interface opens them.

use serde::Deserialize;
use serde_json::Value;

use super::{CommandError, CommandSpec, command, parse, sync_run};
use crate::Ctx;
use crate::error::ErrorCode;
use crate::events::{EngineEvent, UiRequest};

type Out = futures::future::BoxFuture<'static, Result<Value, CommandError>>;

pub fn specs() -> Vec<CommandSpec> {
    vec![command("link.open", "Open a link", "{ url: http or https }", run_open)]
}

#[derive(Deserialize)]
struct OpenParams {
    url: String,
}

/// A checked web link goes to the interface, which opens it in the browser; anything else is
/// refused.
fn run_open(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let OpenParams { url } = parse("link.open", params)?;
        let url = crate::links::validate_link(&url).ok_or_else(|| CommandError::code(ErrorCode::InvalidLink))?;
        ctx.session.events().send(EngineEvent::Ui(UiRequest::OpenUrl { url }));
        Ok(Value::Null)
    })
}
