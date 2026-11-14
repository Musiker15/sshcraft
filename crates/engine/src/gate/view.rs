//! A snapshot of what the gate decides on: the agent policy and the hosts, groups and keys. Hidden
//! hosts and groups (also behind a jump host) are unknown to agents.

use sshcraft_model::model::{ConnectionSettings, Group, GroupId, Host, HostId, KeyRef, LogId, group_chain};
use sshcraft_store::agent::{AgentCategory, AgentPolicy};
use sshcraft_store::snippets::Snippet;
use sshcraft_store::store::Store;
use sshcraft_store::vault::Vault;
use uuid::Uuid;

use crate::workspaces::{Template, TemplateTarget};

use super::access::{Refusal, invalid, not_found};
use super::wire::{GroupView, HostView, KeyView, is_bidi_control};
use crate::error::{EngineError, ErrorCode};

/// Jump hosts followed when checking a route.
pub const MAX_HOPS: usize = 16;
/// Longest host, group or key name an agent may give.
pub const MAX_NAME: usize = 100;

#[derive(Debug, Clone, Default)]
pub struct GateView {
    pub policy: AgentPolicy,
    pub hosts: Vec<Host>,
    pub groups: Vec<Group>,
    pub keys: Vec<KeyView>,
    /// Workspace templates, loaded for the `Workspaces` category.
    pub templates: Vec<Template>,
    /// Snippets, loaded for the `Snippets` category (they need the vault).
    pub snippets: Vec<Snippet>,
    /// Each session log and the host it recorded, loaded for the `Logs` category.
    pub logs: Vec<(LogId, Option<HostId>)>,
    /// The shares of every server, loaded for the `Share` category.
    pub shares: Vec<sshcraft_sync::shares::ShareView>,
    /// Sync servers by id and name, loaded for the `Sync` and `Share` categories.
    pub servers: Vec<(sshcraft_model::model::ServerId, String)>,
}

impl GateView {
    /// Whether the group with this id (written as text) is hidden; an unknown one is not.
    pub fn group_text_hidden(&self, id: &str) -> bool {
        Uuid::parse_str(id).is_ok_and(|id| self.groups.iter().any(|g| g.id.0 == id) && self.group_hidden(GroupId(id)))
    }

    /// Whether the host with this id (written as text) is hidden; an unknown one is not.
    pub fn host_text_hidden(&self, id: &str) -> bool {
        Uuid::parse_str(id).is_ok_and(|id| self.host_id_hidden(HostId(id)))
    }

    /// A share of a hidden group or with a hidden host is hidden as a whole.
    pub fn share_hidden(&self, group: Option<&str>, hosts: &[String]) -> bool {
        group.is_some_and(|g| self.group_text_hidden(g)) || hosts.iter().any(|h| self.host_text_hidden(h))
    }

    /// [`GateView::load`], plus what the category's rules look at.
    pub fn load_for(store: &Store, vault: Option<&Vault>, category: AgentCategory) -> Result<Self, EngineError> {
        let mut view = Self::load(store)?;
        match (category, vault) {
            (AgentCategory::Workspaces, _) => view.templates = crate::workspaces::list(store),
            (AgentCategory::Snippets, Some(vault)) => view.snippets = sshcraft_store::snippets::list(vault, store)?,
            (AgentCategory::Logs, Some(vault)) => {
                view.logs = sshcraft_store::session_log::list(vault, store)?.into_iter().map(|s| (s.meta.id, s.meta.host_id)).collect();
            }
            _ => {}
        }
        Ok(view)
    }

    /// Whether the host with this id is hidden from agents; an unknown host is not.
    pub fn host_id_hidden(&self, id: HostId) -> bool {
        self.hosts.iter().find(|h| h.id == id).is_some_and(|h| self.host_hidden(h))
    }

    /// A template with a pane on a hidden host is hidden as a whole.
    pub fn template_hidden(&self, template: &Template) -> bool {
        template.panes.iter().any(|p| matches!(p.target, TemplateTarget::Host { host_id } if self.host_id_hidden(host_id)))
    }

    /// A snippet that runs on a hidden host is hidden as a whole.
    pub fn snippet_hidden(&self, snippet: &Snippet) -> bool {
        snippet.default_targets.iter().any(|h| self.host_id_hidden(*h))
    }

    /// A session log recorded on a hidden host is hidden.
    pub fn log_known(&self, id: LogId) -> bool {
        self.logs.iter().any(|(log, _)| *log == id)
    }

    pub fn log_hidden(&self, id: LogId) -> bool {
        self.logs.iter().any(|(log, host)| *log == id && host.is_some_and(|h| self.host_id_hidden(h)))
    }

    /// Reads the policy, hosts, groups and keys. Without a readable policy nothing runs: hidden
    /// hosts must not show because the list broke.
    pub fn load(store: &Store) -> Result<Self, EngineError> {
        let policy =
            AgentPolicy::load_checked(store).map_err(|e| EngineError::failed(ErrorCode::Store, format!("the agent settings cannot be read: {e}")))?;
        let keys = sshcraft_store::vault::list_keys(store)?
            .into_iter()
            .map(|info| KeyView { id: info.id.0, name: info.name, kind: info.algorithm, fingerprint: info.fingerprint })
            .collect();
        Ok(Self { policy, hosts: store.list_hosts()?, groups: store.list_groups()?, keys, ..Self::default() })
    }

    fn group_chain(&self, group: Option<GroupId>) -> Vec<Group> {
        group.map(|g| group_chain(&self.groups, g)).unwrap_or_default()
    }

    /// The jump hosts a host uses: its own list, else the nearest group's.
    fn jumps(&self, host: &Host) -> Vec<HostId> {
        host.settings
            .jump_hosts
            .clone()
            .or_else(|| self.group_chain(host.group_id).iter().find_map(|g| g.defaults.jump_hosts.clone()))
            .unwrap_or_default()
    }

    /// Hidden itself, through a group, or through a hidden host on its route.
    pub fn host_hidden(&self, host: &Host) -> bool {
        let mut seen = vec![host.id];
        let mut next = vec![host.clone()];
        while let Some(h) = next.pop() {
            if self.policy.host_hidden(&self.groups, &h) {
                return true;
            }
            for jump in self.jumps(&h) {
                if seen.contains(&jump) || seen.len() > MAX_HOPS {
                    continue;
                }
                seen.push(jump);
                if let Some(j) = self.hosts.iter().find(|x| x.id == jump) {
                    next.push(j.clone());
                }
            }
        }
        false
    }

    pub fn group_hidden(&self, group: GroupId) -> bool {
        self.policy.group_hidden(&self.groups, group)
    }

    /// Whether anything below `group` (groups at any depth, their hosts) is hidden.
    pub fn subtree_hidden(&self, group: GroupId) -> bool {
        let below: Vec<GroupId> =
            self.groups.iter().filter(|g| group_chain(&self.groups, g.id).iter().any(|c| c.id == group)).map(|g| g.id).collect();
        below.iter().any(|g| self.policy.hidden.contains(&g.0))
            || self.hosts.iter().any(|h| h.group_id.is_some_and(|hg| below.contains(&hg)) && self.host_hidden(h))
    }

    /// A visible host by id or by a name only one visible host has.
    pub fn host(&self, reference: &str) -> Result<Host, Refusal> {
        let matches: Vec<&Host> = self.hosts.iter().filter(|h| h.id.0.to_string() == reference || h.name == reference).collect();
        let visible: Vec<&Host> = matches.iter().copied().filter(|h| !self.host_hidden(h)).collect();
        match visible.as_slice() {
            [one] => Ok((*one).clone()),
            [] => Err(not_found("host", !matches.is_empty())),
            _ => Err(invalid("more than one host has this name, use its id")),
        }
    }

    /// A visible group by id or by a name only one visible group has.
    pub fn group(&self, reference: &str) -> Result<Group, Refusal> {
        let matches: Vec<&Group> = self.groups.iter().filter(|g| g.id.0.to_string() == reference || g.name == reference).collect();
        let visible: Vec<&Group> = matches.iter().copied().filter(|g| !self.group_hidden(g.id)).collect();
        match visible.as_slice() {
            [one] => Ok((*one).clone()),
            [] => Err(not_found("group", !matches.is_empty())),
            _ => Err(invalid("more than one group has this name, use its id")),
        }
    }

    pub fn visible_group_id(&self, id: Option<Uuid>) -> Result<Option<GroupId>, Refusal> {
        match id {
            None => Ok(None),
            Some(id) => self.group(&id.to_string()).map(|g| Some(g.id)),
        }
    }

    /// Whether a host here, with its groups, signs in with something stored.
    pub fn carries_sign_in(&self, host: &Host) -> bool {
        signs_in_with_stored(&host.settings) || self.group_chain(host.group_id).iter().any(|g| signs_in_with_stored(&g.defaults))
    }

    /// The hosts agents may see.
    pub fn visible_hosts(&self) -> Vec<HostView> {
        self.hosts.iter().filter(|h| !self.host_hidden(h)).map(host_view).collect()
    }

    /// The groups agents may see.
    pub fn visible_groups(&self) -> Vec<GroupView> {
        self.groups.iter().filter(|g| !self.group_hidden(g.id)).map(group_view).collect()
    }
}

/// Whether these settings sign in with something stored: a password (also a proxy's), an
/// identity, or agent forwarding.
pub fn signs_in_with_stored(settings: &ConnectionSettings) -> bool {
    !settings.password_ids().is_empty() || settings.identity.is_some() || settings.agent_forwarding == Some(true)
}

/// A name an agent may give: not blank, at most [`MAX_NAME`] characters, no control or bidi
/// characters.
pub fn name_ok(name: &str) -> bool {
    !name.trim().is_empty() && name.chars().count() <= MAX_NAME && !name.chars().any(|c| c.is_control() || is_bidi_control(c))
}

pub fn host_view(host: &Host) -> HostView {
    HostView {
        id: host.id.0,
        name: host.name.clone(),
        group: host.group_id.map(|g| g.0),
        address: host.settings.address.clone(),
        port: host.settings.port,
        username: host.settings.username.clone(),
        key: match &host.settings.key {
            Some(KeyRef::Stored { id }) => Some(id.0),
            _ => None,
        },
        tags: host.tags.clone(),
    }
}

pub fn group_view(group: &Group) -> GroupView {
    GroupView { id: group.id.0, name: group.name.clone(), parent: group.parent_id.map(|g| g.0) }
}
