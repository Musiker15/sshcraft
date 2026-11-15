//! UI state (serde, so the control channel can read it from R6 on). Transient parts are skipped.

use serde::{Deserialize, Serialize};

use crate::palette::PaletteState;

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

/// What a tab shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TabKind {
    /// The new-tab page: recent hosts, local shell, serial port.
    New,
    /// A terminal of an engine session.
    Session { session: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabState {
    pub title: String,
    #[serde(flatten)]
    pub kind: TabKind,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UiState {
    pub page: Page,
    pub tabs: Vec<TabState>,
    #[serde(skip)]
    pub palette: PaletteState,
    /// Commands for the active terminal tab, taken by it in the frame.
    #[serde(skip)]
    pub pending: Vec<&'static str>,
}
