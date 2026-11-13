//! Agent rights, hidden hosts and groups, the log of agent actions, and answers to questions.
//! None of this is reachable for agents themselves.

use sshcraft_core::agent::{AgentLogEntry, AgentPolicy, log_clear, log_list};
use tauri::State;

use super::CommandResult;
use crate::state::AppState;

#[tauri::command]
pub fn agent_policy_get(state: State<'_, AppState>) -> AgentPolicy {
    AgentPolicy::load(&state.store.lock().expect("store lock"))
}

#[tauri::command]
pub fn agent_policy_set(
    state: State<'_, AppState>,
    policy: AgentPolicy,
) -> CommandResult<AgentPolicy> {
    let store = state.store.lock().expect("store lock");
    policy.save(&store)?;
    Ok(AgentPolicy::load(&store))
}

#[tauri::command]
pub fn agent_log_list(state: State<'_, AppState>) -> CommandResult<Vec<AgentLogEntry>> {
    let vault = state.vault.clone()?;
    log_list(&vault, &state.store.lock().expect("store lock"))
        .map_err(|e| crate::errors::CommandError::new(crate::errors::ErrorCode::Store, e))
}

#[tauri::command]
pub fn agent_log_clear(state: State<'_, AppState>) -> CommandResult<()> {
    log_clear(&state.store.lock().expect("store lock"))
        .map_err(|e| crate::errors::CommandError::new(crate::errors::ErrorCode::Store, e))
}

/// The user's answer to a question about an agent's action.
#[cfg(desktop)]
#[tauri::command]
pub fn answer_agent(state: State<'_, AppState>, prompt_id: u64, allow: bool) {
    state.agent_confirms.answer(prompt_id, allow);
}

#[cfg(not(desktop))]
#[tauri::command]
pub fn answer_agent(_prompt_id: u64, _allow: bool) {}
