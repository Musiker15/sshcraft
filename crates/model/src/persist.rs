//! Sessions that outlive the connection: the shell runs inside tmux or screen on the server, and
//! every connect (also after a drop or sleep) attaches to the same session again. The command is
//! fixed; only the session name goes into it, and only after [`check`] let it through.

use serde::{Deserialize, Serialize};

/// The longest session name.
pub const MAX_NAME: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tool {
    Tmux,
    Screen,
}

/// Which multiplexer and which of its sessions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Persistence {
    pub tool: Tool,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PersistError {
    #[error("a session name has 1 to 64 letters, digits, dots, dashes or underscores and does not start with a dash")]
    Name,
}

/// Whether `name` is safe inside the fixed command: ASCII letters, digits, `.`, `-` and `_`,
/// not starting with `-` (an option to tmux or screen).
pub fn check(name: &str) -> Result<(), PersistError> {
    let ok = !name.is_empty()
        && name.len() <= MAX_NAME
        && !name.starts_with('-')
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'));
    if ok { Ok(()) } else { Err(PersistError::Name) }
}

impl Persistence {
    /// The command that attaches to the session, creating it when it is not there; `None` for a
    /// name [`check`] refuses.
    pub fn command(&self) -> Option<String> {
        check(&self.name).ok()?;
        Some(match self.tool {
            Tool::Tmux => format!("tmux new-session -A -s {}", self.name),
            Tool::Screen => format!("screen -D -RR {}", self.name),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_checked() {
        for good in ["work", "a.b-c_d", "0", &"x".repeat(64)] {
            assert_eq!(check(good), Ok(()), "{good}");
        }
        for bad in ["", "-t", "a b", "a;rm -rf ~", "$(id)", "x`id`", "ä", "a\nb", &"x".repeat(65)] {
            assert_eq!(check(bad), Err(PersistError::Name), "{bad}");
        }
    }

    #[test]
    fn the_command_is_fixed_around_the_name() {
        let tmux = Persistence { tool: Tool::Tmux, name: "work".into() };
        assert_eq!(tmux.command().as_deref(), Some("tmux new-session -A -s work"));
        let screen = Persistence { tool: Tool::Screen, name: "work".into() };
        assert_eq!(screen.command().as_deref(), Some("screen -D -RR work"));
        assert_eq!(Persistence { tool: Tool::Tmux, name: "a;b".into() }.command(), None);
    }
}
