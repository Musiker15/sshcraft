//! Sets up shell integration (OSC 133) on a server, and takes it away again, each with one
//! `sh -c` command. Only when the user asks for it for one host: nothing else in SSHCraft writes
//! to a server's start files.
//!
//! The script picks the start file of the login shell (`$SHELL`): `~/.bashrc`, `~/.zshrc` (or
//! `$ZDOTDIR/.zshrc`) or fish's `config.fish`. It adds one block between two marker lines, or
//! removes that block. The block is SSHCraft's own text, never data from the user or a server;
//! it travels Base64-encoded, so the script needs no quoting and none of its characters means
//! anything to the shell that runs it. A removal only deletes when both markers are there, so it
//! never takes the rest of a file with it.

use base64::Engine;
use serde::Serialize;

/// The first line of the block.
pub const BEGIN: &str = "# >>> SSHCraft shell integration >>>";
/// The last line of the block.
pub const END: &str = "# <<< SSHCraft shell integration <<<";

const ADDED: &str = "SSHCRAFT_ADDED";
const PRESENT: &str = "SSHCRAFT_PRESENT";
const REMOVED: &str = "SSHCRAFT_REMOVED";
const ABSENT: &str = "SSHCRAFT_ABSENT";
const OTHER_SHELL: &str = "SSHCRAFT_SHELL_OTHER";
const SHELL: &str = "SSHCRAFT_SHELL=";
const FAILED: &str = "SSHCRAFT_FAILED";

const NOTE: &str = "# Marks the prompt and each command for SSHCraft (OSC 133). SSHCraft removes this block\n\
# when you choose \"Remove shell integration\" for the host.";

const BASH: &str = r#"if [[ $- == *i* && -z $__sshcraft_on ]]; then
__sshcraft_on=1
__sshcraft_ran=1
__sshcraft_status() { __sshcraft_s=$?; }
__sshcraft_prompt() { printf '\e]133;D;%s\a\e]133;A\a' "$__sshcraft_s"; __sshcraft_ran=; }
PROMPT_COMMAND="__sshcraft_status;${PROMPT_COMMAND:+$PROMPT_COMMAND;}__sshcraft_prompt"
PS1="$PS1"'\[\e]133;B\a\]'
trap '[[ -z $__sshcraft_ran && $BASH_COMMAND != __sshcraft_* ]] && { __sshcraft_ran=1; printf "\e]133;C\a"; }' DEBUG
fi"#;

const ZSH: &str = r#"if [[ -o interactive && -z $__sshcraft_on ]]; then
__sshcraft_on=1
__sshcraft_precmd() { local s=$?; print -n "\e]133;D;$s\a\e]133;A\a"; }
__sshcraft_preexec() { print -n "\e]133;C\a"; }
precmd_functions+=(__sshcraft_precmd)
preexec_functions+=(__sshcraft_preexec)
PS1="$PS1"$'%{\e]133;B\a%}'
fi"#;

const FISH: &str = r#"if status is-interactive
    function __sshcraft_prompt --on-event fish_prompt
        printf '\e]133;D;%s\a\e]133;A\a' $status
    end
    function __sshcraft_preexec --on-event fish_preexec
        printf '\e]133;C\a'
    end
end"#;

/// A login shell the script knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
}

impl Shell {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "bash" => Some(Self::Bash),
            "zsh" => Some(Self::Zsh),
            "fish" => Some(Self::Fish),
            _ => None,
        }
    }

    /// The block as it goes into the start file.
    pub fn block(self) -> String {
        let body = match self {
            Self::Bash => BASH,
            Self::Zsh => ZSH,
            Self::Fish => FISH,
        };
        format!("{BEGIN}\n{NOTE}\n{body}\n{END}")
    }
}

/// What the script did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Outcome {
    /// The block went into the start file of `shell`.
    Added { shell: Shell },
    /// The start file of `shell` had the block already; nothing changed.
    AlreadyPresent { shell: Shell },
    /// The login shell is none of bash, zsh and fish; nothing changed.
    OtherShell,
    /// The block was taken out of every start file that had it.
    Removed,
    /// No start file had the block; nothing changed.
    Absent,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ShellIntegrationError {
    /// The server did not run the script (no `sh`, a Windows server, a forced command).
    #[error("the server did not run the shell integration script")]
    Unsupported,
    /// The script ran but could not write, or a block had lost its last line.
    #[error("the shell's start file could not be changed on the server")]
    Failed,
}

fn encoded(shell: Shell) -> String {
    base64::engine::general_purpose::STANDARD.encode(shell.block())
}

/// The command that adds the block to the login shell's start file, unless it is there already.
pub fn install_command() -> String {
    let (bash, zsh, fish) = (encoded(Shell::Bash), encoded(Shell::Zsh), encoded(Shell::Fish));
    format!(
        "sh -c 'umask 077; s=\"${{SHELL##*/}}\"; \
         case \"$s\" in \
         bash) f=\"$HOME/.bashrc\"; b=\"{bash}\";; \
         zsh) f=\"${{ZDOTDIR:-$HOME}}/.zshrc\"; b=\"{zsh}\";; \
         fish) f=\"${{XDG_CONFIG_HOME:-$HOME/.config}}/fish/config.fish\"; b=\"{fish}\"; mkdir -p \"${{f%/*}}\" || {{ echo {FAILED}; exit 1; }};; \
         *) echo {OTHER_SHELL}; exit 0;; \
         esac; \
         echo \"{SHELL}$s\"; \
         touch \"$f\" || {{ echo {FAILED}; exit 1; }}; \
         if grep -qF \"{BEGIN}\" \"$f\"; then echo {PRESENT}; exit 0; fi; \
         t=$(printf %s \"$b\" | {{ base64 -d 2>/dev/null || base64 -D 2>/dev/null || openssl base64 -d -A 2>/dev/null; }}); \
         [ -n \"$t\" ] || {{ echo {FAILED}; exit 1; }}; \
         if [ -s \"$f\" ] && [ -n \"$(tail -c 1 \"$f\")\" ]; then echo >> \"$f\"; fi; \
         if printf \"%s\\n\" \"$t\" >> \"$f\"; then echo {ADDED}; else echo {FAILED}; fi'"
    )
}

/// The command that takes the block out of every start file the install script may have used.
/// The file is rewritten in place (`cat >`), so its owner, rights and links stay as they were.
pub fn remove_command() -> String {
    format!(
        "sh -c 'r=; \
         for f in \"$HOME/.bashrc\" \"${{ZDOTDIR:-$HOME}}/.zshrc\" \"${{XDG_CONFIG_HOME:-$HOME/.config}}/fish/config.fish\"; do \
         [ -f \"$f\" ] && grep -qF \"{BEGIN}\" \"$f\" || continue; \
         grep -qF \"{END}\" \"$f\" || {{ echo {FAILED}; exit 1; }}; \
         p=\"$f.sshcraft-part\"; \
         if sed \"/^{BEGIN}/,/^{END}/d\" \"$f\" > \"$p\" && cat \"$p\" > \"$f\"; then rm -f \"$p\"; r=1; else rm -f \"$p\"; echo {FAILED}; exit 1; fi; \
         done; \
         if [ -n \"$r\" ]; then echo {REMOVED}; else echo {ABSENT}; fi'"
    )
}

/// Reads what the script printed. Anything without one of its marks means it did not run.
pub fn parse_outcome(output: &[u8]) -> Result<Outcome, ShellIntegrationError> {
    let text = String::from_utf8_lossy(output);
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let shell = lines.iter().rev().find_map(|l| l.strip_prefix(SHELL)).and_then(Shell::parse);
    let mark = lines.iter().rev().find_map(|line| match *line {
        ADDED | PRESENT | REMOVED | ABSENT | OTHER_SHELL | FAILED => Some(*line),
        _ => None,
    });
    match (mark, shell) {
        (Some(ADDED), Some(shell)) => Ok(Outcome::Added { shell }),
        (Some(PRESENT), Some(shell)) => Ok(Outcome::AlreadyPresent { shell }),
        (Some(OTHER_SHELL), _) => Ok(Outcome::OtherShell),
        (Some(REMOVED), _) => Ok(Outcome::Removed),
        (Some(ABSENT), _) => Ok(Outcome::Absent),
        (Some(FAILED), _) => Err(ShellIntegrationError::Failed),
        _ => Err(ShellIntegrationError::Unsupported),
    }
}

#[cfg(test)]
#[path = "shell_integration_tests.rs"]
mod tests;
