//! Local shells and serial ports (desktop only; on a phone the lists are empty). What they
//! remember stays on this device and is not synced.

use tauri::State;

use super::CommandResult;
use crate::state::AppState;

/// The user's own shell, besides the ones the system offers.
#[cfg(desktop)]
const LOCAL_SHELL: &str = "terminal.local_shell";
/// The serial settings used last.
#[cfg(desktop)]
const SERIAL_LAST: &str = "serial.last";

#[cfg(desktop)]
fn read<T: serde::de::DeserializeOwned>(state: &AppState, key: &str) -> Option<T> {
    state
        .store
        .lock()
        .expect("store lock")
        .get_setting(key)
        .ok()
        .flatten()
        .and_then(|json| serde_json::from_str(&json).ok())
}

#[cfg(desktop)]
fn write<T: serde::Serialize>(state: &AppState, key: &str, value: &T) -> CommandResult<()> {
    let json = serde_json::to_string(value)
        .map_err(|e| super::CommandError::new(super::ErrorCode::Internal, e))?;
    Ok(state
        .store
        .lock()
        .expect("store lock")
        .put_setting(key, &json)?)
}

#[cfg(desktop)]
#[tauri::command]
pub async fn local_shells(
    state: State<'_, AppState>,
) -> CommandResult<Vec<sshcraft_local::pty::ShellCommand>> {
    let custom = read(&state, LOCAL_SHELL);
    // Asking wsl.exe for its distributions takes a moment.
    Ok(
        tokio::task::spawn_blocking(move || sshcraft_local::pty::available_shells(custom))
            .await
            .unwrap_or_default(),
    )
}

#[cfg(desktop)]
#[tauri::command]
pub fn local_shell_get(state: State<'_, AppState>) -> Option<sshcraft_local::pty::ShellCommand> {
    read(&state, LOCAL_SHELL)
}

/// Sets (or with `null` removes) the user's own shell.
#[cfg(desktop)]
#[tauri::command]
pub fn local_shell_set(
    state: State<'_, AppState>,
    shell: Option<sshcraft_local::pty::ShellCommand>,
) -> CommandResult<()> {
    let shell = shell.filter(|s| !s.program.trim().is_empty());
    write(&state, LOCAL_SHELL, &shell)
}

#[cfg(desktop)]
#[tauri::command]
pub async fn serial_ports() -> Vec<sshcraft_local::serial::SerialPortInfo> {
    tokio::task::spawn_blocking(sshcraft_local::serial::list_ports)
        .await
        .unwrap_or_default()
}

#[cfg(desktop)]
#[tauri::command]
pub fn serial_last(state: State<'_, AppState>) -> sshcraft_local::serial::SerialSettings {
    read(&state, SERIAL_LAST).unwrap_or_default()
}

#[cfg(desktop)]
#[tauri::command]
pub fn serial_remember(
    state: State<'_, AppState>,
    settings: sshcraft_local::serial::SerialSettings,
) -> CommandResult<()> {
    write(&state, SERIAL_LAST, &settings)
}

#[cfg(mobile)]
#[tauri::command]
pub fn local_shells(_state: State<'_, AppState>) -> Vec<serde_json::Value> {
    Vec::new()
}

#[cfg(mobile)]
#[tauri::command]
pub fn serial_ports() -> Vec<serde_json::Value> {
    Vec::new()
}

#[cfg(mobile)]
#[tauri::command]
pub fn local_shell_get(_state: State<'_, AppState>) -> Option<serde_json::Value> {
    None
}

#[cfg(mobile)]
#[tauri::command]
pub fn local_shell_set(
    _state: State<'_, AppState>,
    _shell: Option<serde_json::Value>,
) -> CommandResult<()> {
    Err(super::CommandError::code(super::ErrorCode::NotSupported))
}

#[cfg(mobile)]
#[tauri::command]
pub fn serial_last(_state: State<'_, AppState>) -> Option<serde_json::Value> {
    None
}

#[cfg(mobile)]
#[tauri::command]
pub fn serial_remember(
    _state: State<'_, AppState>,
    _settings: serde_json::Value,
) -> CommandResult<()> {
    Err(super::CommandError::code(super::ErrorCode::NotSupported))
}
