//! Launch actions: snippets a host types once when a session first connects (a `cd`, starting a
//! monitor, a greeting). They type into the session like the user, so they run only once allowed
//! on this device, for exactly these texts: a snippet changed through sync needs a new approval.

use crate::model::{Group, Host, SnippetId, group_chain};

/// Snippets one host or group may launch.
pub const MAX_LAUNCH: usize = 8;

/// The launch actions of `host` and the id of the level that sets them: the host's own, or the
/// nearest group's. An empty list on a host means none, not the group's.
pub fn launch_for(host: &Host, groups: &[Group]) -> Option<(String, Vec<SnippetId>)> {
    if let Some(ids) = &host.settings.launch {
        return Some((host.id.0.to_string(), ids.clone()));
    }
    host.group_id
        .map(|g| group_chain(groups, g))
        .unwrap_or_default()
        .iter()
        .find_map(|g| g.defaults.launch.clone().map(|ids| (g.id.0.to_string(), ids)))
}

/// What is allowed for the launch actions with these texts that the level `level` (a host or
/// group id) sets: the same texts on another host need their own approval.
pub fn approval_argv(level: &str, texts: &[String]) -> Vec<String> {
    ["launch-actions:v1".to_string(), level.to_string()].into_iter().chain(texts.iter().cloned()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changed_text_needs_a_new_approval() {
        assert_ne!(approval_argv("h", &["cd /srv".into()]), approval_argv("h", &["cd /srv/x".into()]));
        assert_ne!(approval_argv("h", &["a".into(), "b".into()]), approval_argv("h", &["ab".into()]));
    }

    #[test]
    fn the_approval_belongs_to_one_level() {
        assert_ne!(approval_argv("host-a", &["cd /srv".into()]), approval_argv("host-b", &["cd /srv".into()]));
        // Not what a step program named `launch` would be approved as.
        assert_ne!(approval_argv("h", &["x".into()])[0], "launch");
    }
}
