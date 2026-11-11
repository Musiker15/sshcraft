//! Terminal sessions. Output goes to the UI as raw bytes, status as JSON.

use std::sync::Arc;

use stollen_core::model::HostId;
use tauri::State;
use tauri::ipc::{Channel, InvokeResponseBody};

use super::{CommandError, CommandResult};
use crate::sessions::{SessionId, SessionSink, SessionStatus};
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
    host_id: HostId,
    cols: u32,
    rows: u32,
    on_data: Channel<InvokeResponseBody>,
    on_status: Channel<SessionStatus>,
) -> CommandResult<SessionId> {
    let sink = Arc::new(ChannelSink {
        data: on_data,
        status: on_status,
    });
    Ok(state.sessions.open(host_id, cols, rows, sink))
}

#[tauri::command]
pub async fn session_write(
    state: State<'_, AppState>,
    id: SessionId,
    data: Vec<u8>,
) -> CommandResult<()> {
    state
        .sessions
        .write(id, &data)
        .await
        .map_err(|e| CommandError::new("session", e))
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
        .map_err(|e| CommandError::new("session", e))
}

#[tauri::command]
pub fn session_close(state: State<'_, AppState>, id: SessionId) {
    state.sessions.close(id);
}

#[tauri::command]
pub fn answer_host_key(state: State<'_, AppState>, prompt_id: u64, accept: bool) {
    state.prompter.answer(prompt_id, accept);
}
