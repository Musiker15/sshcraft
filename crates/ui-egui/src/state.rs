//! UI state (serde, so the control channel can read it from R6 on). Transient parts are skipped.

use serde::{Deserialize, Serialize};

/// The page in the main area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Page {
    #[default]
    Hosts,
    Keys,
    Settings,
    /// The tab at this index of [`UiState::tabs`].
    Tab(usize),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UiState {
    pub page: Page,
}
