//! The interface's own commands (`ui.*`): what the palette lists and shortcuts run. Actions that
//! change data run engine commands through the forms these commands open; a program reaches the
//! engine commands directly and never gets a dialog.

use crate::app::SshcraftApp;
use crate::state::Page;

/// Where a command's shortcuts work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Everywhere; inside a terminal only with Ctrl+Shift or Ctrl+Tab (docs/shortcuts.md).
    App,
    /// Only inside a terminal tab.
    Terminal,
}

pub struct UiCommand {
    pub id: &'static str,
    /// The name its own shortcuts are stored under (`shortcuts.bindings`, one of the engine's
    /// `SHORTCUT_ACTIONS`); the Tauri version's names where it had the action.
    pub action: &'static str,
    /// English; shown through `tr`.
    pub label: &'static str,
    pub defaults: &'static [&'static str],
    pub scope: Scope,
    pub enabled: fn(&SshcraftApp) -> bool,
    /// Whether it appears in the palette.
    pub in_palette: bool,
}

fn always(_: &SshcraftApp) -> bool {
    true
}

fn has_tabs(app: &SshcraftApp) -> bool {
    !app.ui.tabs.is_empty()
}

fn in_terminal(app: &SshcraftApp) -> bool {
    matches!(app.ui.page, Page::Tab(_))
}

const fn cmd(id: &'static str, action: &'static str, label: &'static str, defaults: &'static [&'static str]) -> UiCommand {
    UiCommand { id, action, label, defaults, scope: Scope::App, enabled: always, in_palette: true }
}

static COMMANDS: [UiCommand; 26] = [
    cmd("ui.palette", "palette", "Open the command palette", &["Ctrl+Shift+P", "Ctrl+K"]),
    cmd("ui.tab.new", "newTab", "New tab", &["Ctrl+Shift+T"]),
    UiCommand { enabled: has_tabs, ..cmd("ui.tab.close", "closeTab", "Close the current tab", &["Ctrl+Shift+W"]) },
    UiCommand { enabled: has_tabs, ..cmd("ui.tab.next", "nextTab", "Next tab", &["Ctrl+Tab", "Ctrl+PageDown"]) },
    UiCommand { enabled: has_tabs, ..cmd("ui.tab.previous", "prevTab", "Previous tab", &["Ctrl+Shift+Tab", "Ctrl+PageUp"]) },
    cmd("ui.page.hosts", "hostsPage", "Hosts", &[]),
    cmd("ui.page.keys", "keysPage", "Keys", &[]),
    UiCommand { enabled: has_tabs, ..cmd("ui.pane.split", "splitPane", "Split: add a pane", &["Ctrl+Shift+D"]) },
    UiCommand { enabled: has_tabs, ..cmd("ui.pane.close", "closePane", "Close the focused pane", &["Ctrl+Shift+X"]) },
    UiCommand { enabled: has_tabs, ..cmd("ui.pane.next", "nextPane", "Next pane", &["Ctrl+Shift+PageDown"]) },
    UiCommand { enabled: has_tabs, ..cmd("ui.pane.previous", "prevPane", "Previous pane", &["Ctrl+Shift+PageUp"]) },
    UiCommand { enabled: has_tabs, ..cmd("ui.pane.layout", "toggleLayout", "Switch between grid and one large pane", &["Ctrl+Shift+M"]) },
    UiCommand { enabled: has_tabs, ..cmd("ui.pane.broadcast", "toggleBroadcast", "Broadcast on or off", &["Ctrl+Shift+B"]) },
    UiCommand { enabled: has_tabs, ..cmd("ui.terminal.sidebar", "toggleSidebar", "Snippets and history beside the terminal", &["Ctrl+Shift+S"]) },
    UiCommand { enabled: has_tabs, ..cmd("ui.terminal.bookmark", "bookmark", "Bookmark in the session log", &["Ctrl+Shift+L"]) },
    cmd("ui.page.snippets", "snippetsPage", "Snippets", &[]),
    cmd("ui.page.forwarding", "forwardingPage", "Port forwarding", &[]),
    cmd("ui.page.logs", "logsPage", "Logs", &[]),
    cmd("ui.page.settings", "settingsPage", "Settings", &[]),
    UiCommand {
        scope: Scope::Terminal,
        enabled: in_terminal,
        in_palette: false,
        ..cmd("ui.terminal.copy", "copy", "Copy the selection", &["Ctrl+Shift+C"])
    },
    UiCommand { scope: Scope::Terminal, enabled: in_terminal, in_palette: false, ..cmd("ui.terminal.paste", "paste", "Paste", &["Ctrl+Shift+V"]) },
    UiCommand {
        scope: Scope::Terminal,
        enabled: in_terminal,
        ..cmd("ui.terminal.prev_command", "prevCommand", "Previous command", &["Ctrl+Shift+Up"])
    },
    UiCommand {
        scope: Scope::Terminal,
        enabled: in_terminal,
        ..cmd("ui.terminal.next_command", "nextCommand", "Next command", &["Ctrl+Shift+Down"])
    },
    UiCommand {
        scope: Scope::Terminal,
        enabled: in_terminal,
        ..cmd("ui.terminal.copy_output", "copyOutput", "Copy the output of the last command", &[])
    },
    // Asks first while sessions run (crate::quit); macOS gives it ⌘Q in the app menu.
    cmd("ui.app.quit", "quit", "Quit SSHCraft", &[]),
    cmd("ui.help.about", "about", "About SSHCraft", &[]),
];

pub fn ui_commands() -> &'static [UiCommand] {
    &COMMANDS
}

pub fn find(id: &str) -> Option<&'static UiCommand> {
    COMMANDS.iter().find(|c| c.id == id)
}

/// Runs a `ui.*` command; one that is not enabled does nothing.
pub fn run(app: &mut SshcraftApp, id: &str) {
    let Some(command) = find(id) else { return };
    if !(command.enabled)(app) {
        return;
    }
    match id {
        "ui.palette" => app.ui.palette.toggle(),
        "ui.tab.new" => app.open_new_tab(),
        "ui.tab.close" => app.close_tab(),
        "ui.tab.next" => app.cycle_tab(1),
        "ui.tab.previous" => app.cycle_tab(-1),
        "ui.pane.split" => app.split(),
        "ui.pane.close" => app.close_pane(),
        "ui.pane.next" => app.focus_pane(1),
        "ui.pane.previous" => app.focus_pane(-1),
        "ui.pane.layout" => app.toggle_layout(),
        "ui.pane.broadcast" => app.toggle_broadcast(),
        "ui.terminal.sidebar" => app.ui.sidebar.open = !app.ui.sidebar.open,
        "ui.terminal.bookmark" => crate::session_tab::bookmark(app),
        "ui.page.hosts" => app.ui.page = Page::Hosts,
        "ui.page.keys" => app.ui.page = Page::Keys,
        "ui.page.snippets" => app.ui.page = Page::Snippets,
        "ui.page.forwarding" => app.ui.page = Page::Forwarding,
        "ui.page.logs" => app.ui.page = Page::Logs,
        "ui.page.settings" => app.ui.page = Page::Settings,
        "ui.app.quit" => crate::quit::request(app),
        "ui.help.about" => app.ui.about = true,
        // The terminal tab handles these itself (it has the selection and the clipboard).
        _ => app.ui.pending.push(command.id),
    }
}
