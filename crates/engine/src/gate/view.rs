//! A snapshot of what the gate decides on: the agent policy and the hosts, groups and keys.

use sshcraft_model::model::{Group, Host};
use sshcraft_store::agent::AgentPolicy;

use super::wire::KeyView;

#[derive(Debug, Clone, Default)]
pub struct GateView {
    pub policy: AgentPolicy,
    pub hosts: Vec<Host>,
    pub groups: Vec<Group>,
    pub keys: Vec<KeyView>,
}
