//! Workspace templates: a layout of panes, each on a host, a local shell or a serial port.

use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use super::{CommandError, CommandSpec, command, parse, query, sync_run, to_value};
use crate::Ctx;
use crate::error::ErrorCode;
use crate::workspaces::{self, Template, TemplateError};

type Out = futures::future::BoxFuture<'static, Result<Value, CommandError>>;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        query("workspace.list", "Workspace templates", "none", run_list),
        command(
            "workspace.save",
            "Save a workspace template",
            "{ template: { id, name, layout, col_sizes, row_sizes, panes: [{ target, title, cwd?, command? }] } }",
            run_save,
        ),
        command("workspace.delete", "Delete a workspace template", "{ id }", run_delete),
    ]
}

fn template_error(e: TemplateError) -> CommandError {
    match e {
        TemplateError::Invalid(what) => CommandError::failed(ErrorCode::InvalidTemplate, what),
        TemplateError::TooMany => CommandError::code(ErrorCode::TooManyTemplates),
        TemplateError::Store(e) => CommandError::failed(ErrorCode::Store, e),
    }
}

fn run_list(ctx: Ctx, _: Value) -> Out {
    sync_run(move || to_value(&workspaces::list(&ctx.session.store())))
}

#[derive(Deserialize)]
struct SaveParams {
    template: Template,
}

fn run_save(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let SaveParams { template } = parse("workspace.save", params)?;
        to_value(&workspaces::save(&ctx.session.store(), template).map_err(template_error)?)
    })
}

#[derive(Deserialize)]
struct IdParams {
    id: Uuid,
}

fn run_delete(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let IdParams { id } = parse("workspace.delete", params)?;
        workspaces::delete(&ctx.session.store(), id).map_err(template_error)?;
        Ok(Value::Null)
    })
}
