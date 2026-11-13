//! Terminal sessions. Output goes to the UI as raw bytes, status as JSON.

use std::sync::Arc;

use tauri::State;
use tauri::ipc::{Channel, InvokeResponseBody};

use super::{CommandError, CommandResult, ErrorCode};
use crate::sessions::{SessionId, SessionSink, SessionStatus, SessionTarget};
use crate::state::AppState;

struct ChannelSink {
    data: Channel<InvokeResponseBody>,
    status: Channel<SessionStatus>,
}

impl SessionSink for ChannelSink {
    fn data(&self, bytes: Vec<u8>) {
        let _ = self.data.send(InvokeResponseBody::Raw(bytes));
    }

    fn status(&self, status: SessionStatus) {
        let _ = self.status.send(status);
    }
}

#[tauri::command]
pub async fn session_open(
    state: State<'_, AppState>,
    target: SessionTarget,
    cols: u32,
    rows: u32,
    on_data: Channel<InvokeResponseBody>,
    on_status: Channel<SessionStatus>,
) -> CommandResult<SessionId> {
    // A local shell starts only when it is one this device offers (or the user's own): the
    // window cannot start any program it likes.
    #[cfg(desktop)]
    if let SessionTarget::Local { shell, .. } = &target {
        let custom = state
            .store
            .lock()
            .expect("store lock")
            .get_setting("terminal.local_shell")
            .ok()
            .flatten()
            .and_then(|json| serde_json::from_str(&json).ok());
        let offered =
            tokio::task::spawn_blocking(move || sshcraft_local::pty::available_shells(custom))
                .await
                .unwrap_or_default();
        if !is_offered(shell, &offered) {
            return Err(CommandError::code(ErrorCode::LocalShellFailed));
        }
    }
    let sink = Arc::new(ChannelSink {
        data: on_data,
        status: on_status,
    });
    Ok(state.sessions.open(target, cols, rows, sink))
}

/// Whether `shell` is exactly one of the offered shells (program and arguments).
#[cfg(desktop)]
fn is_offered(
    shell: &sshcraft_local::pty::ShellCommand,
    offered: &[sshcraft_local::pty::ShellCommand],
) -> bool {
    offered
        .iter()
        .any(|o| o.program == shell.program && o.args == shell.args)
}

/// Synchronous on purpose: sync commands run in call order, so keystrokes stay in order.
#[tauri::command]
pub fn session_write(
    state: State<'_, AppState>,
    id: SessionId,
    data: Vec<u8>,
) -> CommandResult<()> {
    state
        .sessions
        .write(id, &data)
        .map_err(|e| CommandError::new(ErrorCode::Session, e))
}

/// The UI has rendered `bytes` output bytes of this session.
#[tauri::command]
pub fn session_ack(state: State<'_, AppState>, id: SessionId, bytes: usize) {
    state.sessions.ack(id, bytes);
}

#[tauri::command]
pub async fn session_resize(
    state: State<'_, AppState>,
    id: SessionId,
    cols: u32,
    rows: u32,
) -> CommandResult<()> {
    state
        .sessions
        .resize(id, cols, rows)
        .await
        .map_err(|e| CommandError::new(ErrorCode::Session, e))
}

#[tauri::command]
pub fn session_close(state: State<'_, AppState>, id: SessionId) {
    state.sessions.close(id);
}

#[tauri::command]
pub fn answer_host_key(state: State<'_, AppState>, prompt_id: u64, accept: bool) {
    state.prompter.answer(prompt_id, accept);
}

/// The user's answers to a sign-in question; `null` cancels the sign-in.
#[tauri::command]
pub fn answer_auth(state: State<'_, AppState>, prompt_id: u64, answers: Option<Vec<String>>) {
    state.prompter.answer_auth(prompt_id, answers);
}

#[cfg(all(test, desktop))]
mod tests {
    use super::*;
    use sshcraft_local::pty::ShellCommand;

    fn shell(program: &str) -> ShellCommand {
        ShellCommand {
            program: program.into(),
            args: Vec::new(),
            label: program.into(),
        }
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
