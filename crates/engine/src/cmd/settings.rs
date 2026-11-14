//! Settings of this device: `settings.get`, `settings.save`, the default local shell and the
//! last serial settings. None of them is reachable for agents.

use futures::FutureExt;
use futures::future::BoxFuture;
use serde::Deserialize;
use serde_json::Value;
use sshcraft_local::pty::ShellCommand;
use sshcraft_local::serial::SerialSettings;

use super::{CommandSpec, command, parse, query, to_value};
use crate::error::EngineError;
use crate::events::EngineEvent;
use crate::settings::EngineSettings;
use crate::{Ctx, Session};

type Out = BoxFuture<'static, Result<Value, EngineError>>;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        query("settings.get", "Settings", "none", get),
        command("settings.save", "Save settings", "{ settings: the settings as settings.get returns them }", save),
        query("local.default_shell", "Default local shell", "none", default_shell),
        command("local.set_default_shell", "Set the default local shell", "{ shell: { program, args, label } | null }", set_default_shell),
        query("serial.last", "Last serial settings", "none", serial_last),
        command("serial.remember", "Remember serial settings", "{ settings: { port, baud, data_bits, stop_bits, parity, flow } }", serial_remember),
    ]
}

/// Changes the settings and saves them at once; a read-only `config.toml` is an error.
fn change(session: &Session, f: impl FnOnce(&mut EngineSettings)) -> Result<EngineSettings, EngineError> {
    let mut store = session.settings();
    if store.is_read_only() {
        return Err(EngineError::Settings("config.toml is read-only (written by a newer SSHCraft, or not writable)".to_string()));
    }
    store.update(f);
    store.flush().map_err(|e| EngineError::Settings(e.to_string()))?;
    let saved = store.get().clone();
    drop(store);
    session.events().send(EngineEvent::SettingsChanged);
    Ok(saved)
}

fn get(ctx: Ctx, _: Value) -> Out {
    async move { to_value(ctx.session.settings().get()) }.boxed()
}

#[derive(Deserialize)]
struct SaveParams {
    settings: EngineSettings,
}

fn save(ctx: Ctx, params: Value) -> Out {
    async move {
        let p: SaveParams = parse("settings.save", params)?;
        let wanted = p.settings.sanitized();
        let saved = change(&ctx.session, |s| *s = wanted)?;
        to_value(&saved)
    }
    .boxed()
}

fn default_shell(ctx: Ctx, _: Value) -> Out {
    async move { to_value(&ctx.session.settings().get().terminal.local_shell) }.boxed()
}

#[derive(Deserialize)]
struct ShellParams {
    shell: Option<ShellCommand>,
}

fn set_default_shell(ctx: Ctx, params: Value) -> Out {
    async move {
        let p: ShellParams = parse("local.set_default_shell", params)?;
        let saved = change(&ctx.session, |s| s.terminal.local_shell = p.shell)?;
        to_value(&saved.terminal.local_shell)
    }
    .boxed()
}

fn serial_last(ctx: Ctx, _: Value) -> Out {
    async move { to_value(&ctx.session.settings().get().serial.last.clone().unwrap_or_default()) }.boxed()
}

#[derive(Deserialize)]
struct SerialParams {
    settings: SerialSettings,
}

fn serial_remember(ctx: Ctx, params: Value) -> Out {
    async move {
        let p: SerialParams = parse("serial.remember", params)?;
        p.settings.validate().map_err(|e| EngineError::bad_params("serial.remember", e))?;
        change(&ctx.session, |s| s.serial.last = Some(p.settings))?;
        Ok(Value::Null)
    }
    .boxed()
}
