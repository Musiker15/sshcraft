//! What the engine tells the interface without being asked: changed data, prompts, sync status.
//! The interface subscribes from R4 on; without subscribers nothing waits for an answer.

use serde::Serialize;
use sshcraft_model::model::ServerId;
use tokio::sync::broadcast;

/// A request to the interface that only it can carry out.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "request", rename_all = "snake_case")]
pub enum UiRequest {
    /// Open a dialog, filled with `params`.
    Dialog { name: String, params: serde_json::Value },
    /// Open a checked `http` or `https` link in the browser.
    OpenUrl { url: String },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum EngineEvent {
    /// Hosts, groups, keys or other data changed; `server` is the sync server it belongs to.
    DataChanged {
        server: Option<ServerId>,
    },
    /// Whether to trust a server's host key; answer with `prompt.host_key`.
    HostKeyPrompt(crate::prompts::HostKeyPrompt),
    /// A sign-in question (password, code, passphrase); answer with `prompt.auth`.
    AuthPrompt(crate::prompts::AuthPrompt),
    /// Whether an agent's action may run; answer with `prompt.agent`.
    AgentPrompt(crate::gate::confirm::AgentPrompt),
    ProxyCommandPrompt(crate::prompts::ProxyCommandPrompt),
    /// A prompt was answered elsewhere, timed out or its session went away.
    PromptWithdrawn {
        prompt_id: u64,
    },
    /// The sync state of a server changed.
    SyncStatus {
        server: ServerId,
        status: crate::sync::StatusView,
    },
    /// A sync server sent something this device could not use.
    SyncWarning {
        server: ServerId,
        detail: String,
    },
    AgentLogChanged,
    /// The agent link started or stopped.
    AgentLinkChanged,
    /// A port forwarding rule started, runs, reconnects, stopped or failed.
    ForwardStatus(crate::forwards::ForwardStatus),
    /// Windows Hello is not set up: stored keys are used without the check.
    HelloUnavailable,
    SettingsChanged,
    Ui(UiRequest),
}

/// The broadcast channel of the engine. A slow subscriber loses old events (`Lagged`); state
/// that matters (open prompts) can always be asked for again.
#[derive(Debug, Clone)]
pub struct EventBus {
    sender: broadcast::Sender<EngineEvent>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity.max(1));
        Self { sender }
    }

    /// Sends to every subscriber; without subscribers the event goes nowhere.
    pub fn send(&self, event: EngineEvent) {
        let _ = self.sender.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<EngineEvent> {
        self.sender.subscribe()
    }

    pub fn has_subscribers(&self) -> bool {
        self.sender.receiver_count() > 0
    }
}
