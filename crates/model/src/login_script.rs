//! Login scripts: after connecting, wait for a text in the output, then send a line, step by step
//! (for example a menu on a jump server). A script runs commands with the user's credentials,
//! so it runs only once the user approved it on this device, like a proxy command; the digest of
//! [`approval_argv`] is what gets approved.

use serde::{Deserialize, Serialize};

use crate::model::{Group, Host, group_chain};

/// Steps one script may have.
pub const MAX_STEPS: usize = 16;
/// The longest text to wait for, in characters.
pub const MAX_EXPECT_CHARS: usize = 256;
/// The longest line to send, in characters.
pub const MAX_SEND_CHARS: usize = 1024;

/// Wait until the output shows `expect`, then send `send` and Enter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginStep {
    pub expect: String,
    pub send: String,
}

/// Why a script cannot be saved.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ScriptError {
    #[error("a login script has at most 16 steps")]
    TooManySteps,
    #[error("step {0}: the text to wait for has 1 to 256 characters")]
    Expect(usize),
    #[error("step {0}: the line to send has at most 1024 characters and no control characters")]
    Send(usize),
}

/// Whether `steps` keep the limits.
pub fn check(steps: &[LoginStep]) -> Result<(), ScriptError> {
    if steps.len() > MAX_STEPS {
        return Err(ScriptError::TooManySteps);
    }
    for (i, step) in steps.iter().enumerate() {
        let n = i + 1;
        if step.expect.is_empty() || step.expect.chars().count() > MAX_EXPECT_CHARS {
            return Err(ScriptError::Expect(n));
        }
        if step.send.chars().count() > MAX_SEND_CHARS || step.send.chars().any(char::is_control) {
            return Err(ScriptError::Send(n));
        }
    }
    Ok(())
}

/// What is approved for `steps`: a change to any step needs a new approval.
pub fn approval_argv(steps: &[LoginStep]) -> Vec<String> {
    std::iter::once("login-script".to_string()).chain(steps.iter().flat_map(|s| [s.expect.clone(), s.send.clone()])).collect()
}

/// The script of `host`: its own, else the nearest group's; `None` when no level sets one.
/// `Some(vec![])` on a level means "none here", not "inherit".
pub fn login_script_for(host: &Host, groups: &[Group]) -> Option<Vec<LoginStep>> {
    host.settings
        .login_script
        .clone()
        .or_else(|| host.group_id.map(|g| group_chain(groups, g)).unwrap_or_default().iter().find_map(|g| g.defaults.login_script.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(expect: &str, send: &str) -> LoginStep {
        LoginStep { expect: expect.into(), send: send.into() }
    }

    #[test]
    fn limits_are_checked() {
        assert_eq!(check(&[step("login:", "admin")]), Ok(()));
        assert_eq!(check(&vec![step("a", "b"); 17]), Err(ScriptError::TooManySteps));
        assert_eq!(check(&[step("", "x")]), Err(ScriptError::Expect(1)));
        assert_eq!(check(&[step("ok", "x"), step("ok", "a\x1b[31m")]), Err(ScriptError::Send(2)));
    }

    #[test]
    fn any_change_changes_what_is_approved() {
        let a = approval_argv(&[step("menu>", "2")]);
        assert_ne!(a, approval_argv(&[step("menu>", "3")]));
        assert_ne!(a, approval_argv(&[step("menu>", "2"), step("$", "id")]));
        assert_eq!(a, approval_argv(&[step("menu>", "2")]));
    }
}
