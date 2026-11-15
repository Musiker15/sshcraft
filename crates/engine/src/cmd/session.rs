//! Terminal sessions: open, type, read (agents), resize, close, list a directory for completion,
//! bookmarks in the recording. The user's output comes through the terminal stream
//! ([`crate::Session::attach_terminal`]), an agent's through `session.read`.

use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt;
use serde::Deserialize;
use serde_json::{Value, json};
use sshcraft_local::pty::ShellCommand;
use sshcraft_store::session_log::LogError;
use tokio::sync::mpsc;

use super::host::find_host;
use super::{CommandError, CommandSpec, command, parse, query, sync_run, to_value};
use crate::error::ErrorCode;
use crate::gate::wire::{MAX_OUTPUT, MAX_WAIT_MS, SessionOutput};
use crate::sessions::{SessionId, SessionTarget};
use crate::terminal::{StreamSink, TerminalStream};
use crate::{Ctx, Session};

type Out = futures::future::BoxFuture<'static, Result<Value, CommandError>>;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        command(
            "session.open",
            "Open a session",
            "{ target: { kind: host, host_id } | { kind: local, shell, cwd? } | { kind: serial, settings }, cols, rows }; agents: { host: id or unique name }",
            run_open,
        ),
        // Keystrokes never go into the journal.
        CommandSpec { journal: false, ..command("session.send", "Type into a session", "{ id, data: text } or { id, data_base64 }", run_send) },
        query("session.read", "Read an agent's session", "{ id, wait_ms?: at most 10000, limit?: bytes }", run_read),
        CommandSpec { journal: false, ..command("session.resize", "Resize a session", "{ id, cols, rows }", run_resize) },
        command("session.close", "Close a session", "{ id }", run_close),
        query("session.list_dir", "List a directory of a session", "{ id, dir, prefix }", run_list_dir),
        command("session.bookmark", "Bookmark the recording", "{ id, text }", run_bookmark),
        query("session.recording", "Whether a session is recorded", "{ id }", run_recording),
    ]
}

#[derive(Deserialize)]
struct UserOpen {
    target: SessionTarget,
    cols: u32,
    rows: u32,
}

#[derive(Deserialize)]
struct AgentOpen {
    host: String,
}

/// Whether `shell` is exactly one of the offered shells (program and arguments).
fn is_offered(shell: &ShellCommand, offered: &[ShellCommand]) -> bool {
    offered.iter().any(|o| o.program == shell.program && o.args == shell.args)
}

/// The shells this device offers, with the user's own.
pub(crate) async fn offered_shells(session: &Session) -> Vec<ShellCommand> {
    let custom = session.settings().get().terminal.local_shell.clone();
    // Asking wsl.exe for its distributions takes a moment.
    tokio::task::spawn_blocking(move || sshcraft_local::pty::available_shells(custom)).await.unwrap_or_default()
}

fn run_open(ctx: Ctx, params: Value) -> Out {
    async move {
        let session = ctx.session;
        if !ctx.caller.is_user() {
            let AgentOpen { host } = parse("session.open", params)?;
            let hosts = session.store().list_hosts()?;
            let host = find_host(&hosts, &host, "session.open")?;
            let id = session.agent_sessions().open(session.sessions(), host.id);
            return Ok(json!({ "session": id }));
        }
        let UserOpen { target, cols, rows } = parse("session.open", params)?;
        // A local shell starts only when it is one this device offers (or the user's own): a
        // caller cannot start any program it likes.
        if let SessionTarget::Local { shell, .. } = &target
            && !is_offered(shell, &offered_shells(&session).await)
        {
            return Err(CommandError::code(ErrorCode::LocalShellFailed));
        }
        let (sender, receiver) = mpsc::unbounded_channel();
        let id = session.sessions().open(target, cols, rows, Arc::new(StreamSink { sender, waker: session.terminal_waker() }));
        if let Some(flow) = session.sessions().flow(id) {
            session.park_terminal(TerminalStream::new(id, receiver, flow));
        }
        Ok(json!({ "session": id }))
    }
    .boxed()
}

#[derive(Deserialize)]
struct SendParams {
    id: SessionId,
    #[serde(default)]
    data: Option<String>,
    #[serde(default)]
    data_base64: Option<String>,
}

/// Queues the input before the first await, so calls keep their order.
fn run_send(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let SendParams { id, data, data_base64 } = parse("session.send", params)?;
        let bytes = match (data, data_base64) {
            (Some(text), None) => text.into_bytes(),
            (None, Some(encoded)) => {
                data_encoding::BASE64.decode(encoded.as_bytes()).map_err(|e| CommandError::bad_params("session.send", format!("data_base64: {e}")))?
            }
            _ => return Err(CommandError::bad_params("session.send", "give either data or data_base64")),
        };
        let session = ctx.session;
        let written =
            if ctx.caller.is_user() { session.sessions().write(id, &bytes) } else { session.agent_sessions().write(session.sessions(), id, &bytes) };
        written.map_err(|e| CommandError::new(ErrorCode::Session, e))?;
        Ok(Value::Null)
    })
}

#[derive(Deserialize)]
struct ReadParams {
    id: SessionId,
    #[serde(default)]
    wait_ms: Option<u64>,
    #[serde(default)]
    limit: Option<usize>,
}

fn run_read(ctx: Ctx, params: Value) -> Out {
    async move {
        let ReadParams { id, wait_ms, limit } = parse("session.read", params)?;
        if ctx.caller.is_user() {
            return Err(CommandError::bad_params("session.read", "attach the terminal stream"));
        }
        let wait = Duration::from_millis(wait_ms.unwrap_or(0).min(MAX_WAIT_MS));
        let limit = limit.unwrap_or(MAX_OUTPUT).clamp(1, MAX_OUTPUT);
        let session = ctx.session;
        let read =
            session.agent_sessions().read(session.sessions(), id, limit, wait).await.map_err(|_| CommandError::NotFound("session".to_string()))?;
        to_value(&SessionOutput { output: String::from_utf8_lossy(&read.output).into_owned(), ended: read.ended })
    }
    .boxed()
}

#[derive(Deserialize)]
struct ResizeParams {
    id: SessionId,
    cols: u32,
    rows: u32,
}

fn run_resize(ctx: Ctx, params: Value) -> Out {
    async move {
        let ResizeParams { id, cols, rows } = parse("session.resize", params)?;
        ctx.session.sessions().resize(id, cols, rows).await.map_err(|e| CommandError::new(ErrorCode::Session, e))?;
        Ok(Value::Null)
    }
    .boxed()
}

#[derive(Deserialize)]
struct IdParams {
    id: SessionId,
}

fn run_close(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let IdParams { id } = parse("session.close", params)?;
        let session = ctx.session;
        let agents = session.agent_sessions();
        if ctx.caller.is_user() {
            // The user may close any session, an agent's too.
            if !agents.close(session.sessions(), id) {
                session.close_session(id);
            }
        } else if !agents.close(session.sessions(), id) {
            return Err(CommandError::NotFound("session".to_string()));
        }
        Ok(Value::Null)
    })
}

#[derive(Deserialize)]
struct ListDirParams {
    id: SessionId,
    dir: String,
    #[serde(default)]
    prefix: String,
}

fn run_list_dir(ctx: Ctx, params: Value) -> Out {
    async move {
        let ListDirParams { id, dir, prefix } = parse("session.list_dir", params)?;
        let names = ctx.session.sessions().list_dir(id, &dir, &prefix).await.map_err(|e| CommandError::new(ErrorCode::Session, e))?;
        to_value(&names)
    }
    .boxed()
}

#[derive(Deserialize)]
struct BookmarkParams {
    id: SessionId,
    #[serde(default)]
    text: String,
}

fn run_bookmark(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let BookmarkParams { id, text } = parse("session.bookmark", params)?;
        let mark = ctx.session.sessions().bookmark(id, &text).map_err(|e| match e {
            LogError::NotFound => CommandError::code(ErrorCode::NotRecording),
            e => e.into(),
        })?;
        to_value(&mark)
    })
}

fn run_recording(ctx: Ctx, params: Value) -> Out {
    sync_run(move || {
        let IdParams { id } = parse("session.recording", params)?;
        Ok(Value::Bool(ctx.session.sessions().is_recording(id)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(program: &str) -> ShellCommand {
        ShellCommand { program: program.into(), args: Vec::new(), label: program.into() }
    }

    #[test]
    fn only_offered_shells_start() {
        let offered = vec![shell("cmd.exe"), shell("pwsh.exe")];
        assert!(is_offered(&shell("cmd.exe"), &offered));
        assert!(!is_offered(&shell("evil.exe"), &offered));
        let mut changed = shell("cmd.exe");
        changed.args = vec!["/c".into(), "del".into()];
        assert!(!is_offered(&changed, &offered));
    }
}
