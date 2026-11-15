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
    Session { session: sshcraft_engine::SessionId },
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
    pub hosts: crate::pages::hosts::HostsView,
    pub settings: crate::pages::settings::SettingsView,
    #[serde(skip)]
    pub palette: PaletteState,
    /// Commands for the active terminal tab, taken by it in the frame.
    #[serde(skip)]
    pub pending: Vec<&'static str>,
    /// A connection asked for on a page, opened by the frame loop.
    #[serde(skip)]
    pub connect_request: Option<ConnectRequest>,
    #[serde(skip)]
    pub new_tab_query: String,
    #[serde(skip)]
    pub host_form: Option<crate::pages::host_form::HostForm>,
    #[serde(skip)]
    pub group_form: Option<crate::pages::host_form::GroupForm>,
    #[serde(skip)]
    pub confirm: Option<crate::pages::hosts::Confirm>,
    #[serde(skip)]
    pub key_dialog: Option<crate::pages::keys::KeyDialog>,
    #[serde(skip)]
    pub identity_form: Option<crate::pages::keys::IdentityForm>,
    /// A key waiting for its deletion to be confirmed: id and name.
    #[serde(skip)]
    pub key_confirm: Option<(String, String)>,
    #[serde(skip)]
    pub agent_keys: crate::pages::key_sources::AgentKeys,
    #[serde(skip)]
    pub passcli: Option<crate::pages::key_sources::PassCliView>,
    #[serde(skip)]
    pub picker: Option<crate::session_tab::Picker>,
    #[serde(skip)]
    pub import: Option<crate::pages::import_export::ImportState>,
    #[serde(skip)]
    pub export: Option<crate::pages::import_export::ExportState>,
}

/// What a page asked to connect to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectRequest {
    Host(sshcraft_model::model::HostId),
    Local,
    Serial,
}
