//! Answers to the questions sessions ask: trust a host key, sign in. `prompt.list` shows every
//! open question again, for an interface that missed the event.

use serde::Deserialize;
use serde_json::Value;

use super::{CommandError, CommandSpec, command, parse, query, sync_run, to_value};
use crate::Ctx;

type Out = futures::future::BoxFuture<'static, Result<Value, CommandError>>;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        command("prompt.host_key", "Answer a host key question", "{ prompt_id, accept }", run_host_key),
        // Answers can be passwords and codes: never in the journal.
        CommandSpec {
            journal: false,
            ..command("prompt.auth", "Answer a sign-in question", "{ prompt_id, answers: list of text, or null to cancel }", run_auth)
        },
        query("prompt.list", "Open questions", "none", run_list),
    ]
}

#[derive(Deserialize)]
struct HostKeyParams {
    prompt_id: u64,
    accept: bool,
}

fn run_host_key(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let HostKeyParams { prompt_id, accept } = parse("prompt.host_key", params)?;
        if !ctx.session.prompts().answer(prompt_id, accept) {
            return Err(CommandError::NotFound(format!("host key question {prompt_id}")));
        }
        Ok(Value::Null)
    })
}

#[derive(Deserialize)]
struct AuthParams {
    prompt_id: u64,
    answers: Option<Vec<String>>,
}

fn run_auth(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let AuthParams { prompt_id, answers } = parse("prompt.auth", params)?;
        if !ctx.session.prompts().answer_auth(prompt_id, answers) {
            return Err(CommandError::NotFound(format!("sign-in question {prompt_id}")));
        }
        Ok(Value::Null)
    })
}

fn run_list(ctx: Ctx, _: Value) -> Out {
    sync_run(move || to_value(&ctx.session.prompts().open_prompts()))
}
