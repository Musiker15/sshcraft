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
    Snippets,
    Forwarding,
    Logs,
    Settings,
    /// The tab at this index of [`UiState::tabs`].
    Tab(usize),
}

/// One place in a tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PaneSlot {
    /// A page to pick a connection (hosts, local shell, serial port); `id` tells it apart.
    New { id: u64 },
    /// The terminal of an engine session.
    Session { session: sshcraft_engine::SessionId },
}

/// The panes of a tab: 1 to [`crate::panes::MAX_PANES`] in a near-square grid, or one large
/// with the others listed beside it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Panes {
    pub panes: Vec<PaneSlot>,
    pub focused: usize,
    /// One pane large, the others listed.
    pub focus_layout: bool,
    /// The share of the width of each grid column and the height of each row; sums to 1.
    pub col_sizes: Vec<f32>,
    pub row_sizes: Vec<f32>,
    /// Typing goes to every pane with a session.
    pub broadcast: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TabState {
    pub title: String,
    pub panes: Panes,
    /// The title was given (a template's or a snippet's name) and does not follow the panes.
    #[serde(default)]
    pub named: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UiState {
    pub page: Page,
    pub tabs: Vec<TabState>,
    /// The id the next new-pane page gets.
    pub next_pane: u64,
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
    /// Connections on their way: for a pane, or (without one) to a host.
    #[serde(skip)]
    pub connecting: Vec<(Option<PaneSlot>, Option<sshcraft_model::model::HostId>)>,
    /// The terminal shown in the last frame, to give the keyboard to the next one shown.
    #[serde(skip)]
    pub shown_terminal: Option<sshcraft_engine::SessionId>,
    #[serde(skip)]
    pub logs: crate::pages::logs::LogsView,
    #[serde(skip)]
    pub snippet_form: Option<crate::pages::snippets::SnippetForm>,
    #[serde(skip)]
    pub snippet_run: Option<crate::pages::snippets::SnippetRun>,
    #[serde(skip)]
    pub sidebar: crate::pages::snippets::Sidebar,
    #[serde(skip)]
    pub forward_form: Option<crate::pages::forwarding::ForwardForm>,
    #[serde(skip)]
    pub template_form: Option<crate::pages::templates::TemplateForm>,
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
