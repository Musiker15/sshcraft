//! How a host's terminal looks: an accent colour for its tab, a short badge, a terminal theme.
//! Each field is inherited on its own from the nearest group that sets it.

use serde::{Deserialize, Serialize};

use crate::model::{Group, Host, group_chain};

/// The longest badge, in characters.
pub const MAX_BADGE_CHARS: usize = 16;
/// The terminal themes a host may pick; anything else is ignored.
pub const THEMES: [&str; 2] = ["sshcraft-dark", "sshcraft-light"];

/// The look of a host's or group's terminals. `None` means "not set here".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Appearance {
    /// The accent of the tab, red, green, blue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[u8; 3]>,
    /// A short word shown on the tab and in the terminal's corner, like "PROD".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<String>,
    /// One of [`THEMES`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
}

impl Appearance {
    pub fn is_empty(&self) -> bool {
        self.color.is_none() && self.badge.is_none() && self.theme.is_none()
    }
}

/// A badge as it may be shown: one plain line, at most [`MAX_BADGE_CHARS`]; `None` when nothing is
/// left.
pub fn clean_badge(badge: &str) -> Option<String> {
    let text: String = badge.chars().filter(|c| !c.is_control()).collect::<String>().trim().chars().take(MAX_BADGE_CHARS).collect();
    (!text.is_empty()).then_some(text)
}

/// Merges `levels`, nearest first: each field from the nearest level that sets it, the badge
/// cleaned, an unknown theme ignored.
pub fn merge<'a>(levels: impl IntoIterator<Item = Option<&'a Appearance>> + Clone) -> Appearance {
    let all = || levels.clone().into_iter().flatten();
    Appearance {
        color: all().find_map(|a| a.color),
        badge: all().find_map(|a| a.badge.as_deref().and_then(clean_badge)),
        theme: all().find_map(|a| a.theme.clone().filter(|t| THEMES.contains(&t.as_str()))),
    }
}

/// The look of `host`'s terminals with what its groups give.
pub fn appearance_for(host: &Host, groups: &[Group]) -> Appearance {
    let chain = host.group_id.map(|g| group_chain(groups, g)).unwrap_or_default();
    let levels: Vec<Option<&Appearance>> =
        std::iter::once(host.settings.appearance.as_ref()).chain(chain.iter().map(|g| g.defaults.appearance.as_ref())).collect();
    merge(levels)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look(color: Option<[u8; 3]>, badge: Option<&str>, theme: Option<&str>) -> Appearance {
        Appearance { color, badge: badge.map(str::to_string), theme: theme.map(str::to_string) }
    }

    #[test]
    fn each_field_comes_from_the_nearest_level_that_sets_it() {
        let host = look(None, Some("db1"), None);
        let group = look(Some([1, 2, 3]), Some("PROD"), Some("sshcraft-light"));
        let merged = merge([Some(&host), Some(&group)]);
        assert_eq!(merged, look(Some([1, 2, 3]), Some("db1"), Some("sshcraft-light")));
    }

    #[test]
    fn a_long_badge_is_cut_and_a_blank_one_is_none() {
        assert_eq!(clean_badge(&"x".repeat(40)).map(|b| b.chars().count()), Some(MAX_BADGE_CHARS));
        assert_eq!(clean_badge("  \n "), None);
        assert_eq!(clean_badge(" pr\u{7}od "), Some("prod".to_string()));
    }

    #[test]
    fn an_unknown_theme_is_ignored_and_the_group_s_counts() {
        let host = look(None, None, Some("matrix"));
        let group = look(None, None, Some("sshcraft-dark"));
        assert_eq!(merge([Some(&host), Some(&group)]).theme.as_deref(), Some("sshcraft-dark"));
        assert_eq!(merge([Some(&host)]).theme, None);
    }

    #[test]
    fn nothing_set_is_empty() {
        assert!(merge([None, None]).is_empty());
    }
}
