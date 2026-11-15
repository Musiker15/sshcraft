//! Whether agents may connect to the running app (spec 9.2). Without the suite's `AI-MCP` flag,
//! the user's own switch "Allow agent control" decides alone; with the flag, the ArtCraft
//! Launcher's switch must name SSHCraft as well. Nothing an agent sends changes either.

/// SSHCraft's id in the ArtCraft Launcher's app list.
pub const APP_ID: &str = "sshcraft";

/// What decides whether agents may connect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Consent {
    /// Settings › AI agents › Allow agent control.
    pub allow_setting: bool,
    /// The suite's `AI-MCP` flag (`craft-flags`).
    pub mcp_flag: bool,
    /// The launcher's app list names SSHCraft (`craft-ai-agents`).
    pub launcher_allows: bool,
}

impl Consent {
    pub fn wanted(&self) -> bool {
        agents_wanted(self.allow_setting, self.mcp_flag, self.launcher_allows)
    }
}

/// Without the flag the setting decides; with it, the setting and the launcher both.
pub fn agents_wanted(allow_setting: bool, mcp_flag: bool, launcher_allows: bool) -> bool {
    allow_setting && (!mcp_flag || launcher_allows)
}

/// The flags file is read only up to this size.
const MAX_FLAGS_FILE: u64 = 64 * 1024;

/// Whether the suite's `AI-MCP` flag is on (its file, then `CRAFT_FLAGS`).
pub fn mcp_flag() -> bool {
    let file = craft_flags::default_path()
        .filter(|p| std::fs::metadata(p).is_ok_and(|m| m.len() <= MAX_FLAGS_FILE))
        .and_then(|p| std::fs::read_to_string(p).ok());
    let env = std::env::var(craft_flags::ENV).ok();
    craft_flags::Flags::from_parts(file.as_deref(), env.as_deref()).enabled(&craft_flags::AI_MCP)
}

/// The consent now, with the user's own switch `allow_setting`. Reads two small files: ask
/// from a timer, not every frame.
pub fn read(allow_setting: bool) -> Consent {
    Consent { allow_setting, mcp_flag: mcp_flag(), launcher_allows: craft_ai_agents::allowed(APP_ID) }
}

#[cfg(test)]
mod tests {
    use super::agents_wanted;

    #[test]
    fn consent_table() {
        // (setting, flag, launcher) -> wanted
        let table = [
            (false, false, false, false),
            (false, false, true, false),
            (true, false, false, true),
            (true, false, true, true),
            (false, true, false, false),
            (false, true, true, false),
            (true, true, false, false),
            (true, true, true, true),
        ];
        for (setting, flag, launcher, wanted) in table {
            assert_eq!(agents_wanted(setting, flag, launcher), wanted, "setting {setting}, flag {flag}, launcher {launcher}");
        }
    }
}
