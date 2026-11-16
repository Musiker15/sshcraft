//! Launch actions: snippets a host types once when a session first connects (a `cd`, starting a
//! monitor, a greeting). They type into the session like the user, so they run only once allowed
//! on this device, for exactly these texts: a snippet changed through sync needs a new approval.

use crate::model::{Group, Host, SnippetId, group_chain};

/// Snippets one host or group may launch.
pub const MAX_LAUNCH: usize = 8;

/// The launch actions of `host`: its own, or the nearest group's.
pub fn launch_for(host: &Host, groups: &[Group]) -> Option<Vec<SnippetId>> {
    host.settings
        .launch
        .clone()
        .or_else(|| host.group_id.map(|g| group_chain(groups, g)).unwrap_or_default().iter().find_map(|g| g.defaults.launch.clone()))
}

/// What is allowed for launch actions with these texts.
pub fn approval_argv(texts: &[String]) -> Vec<String> {
    std::iter::once("launch".to_string()).chain(texts.iter().cloned()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changed_text_needs_a_new_approval() {
        assert_ne!(approval_argv(&["cd /srv".into()]), approval_argv(&["cd /tmp".into()]));
        assert_ne!(approval_argv(&["a".into(), "b".into()]), approval_argv(&["ab".into()]));
    }
}
