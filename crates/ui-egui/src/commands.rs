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

const fn cmd(id: &'static str, label: &'static str, defaults: &'static [&'static str]) -> UiCommand {
    UiCommand { id, label, defaults, scope: Scope::App, enabled: always, in_palette: true }
}

static COMMANDS: [UiCommand; 10] = [
    cmd("ui.palette", "Open the command palette", &["Ctrl+Shift+P", "Ctrl+K"]),
    cmd("ui.tab.new", "New tab", &["Ctrl+Shift+T"]),
    UiCommand { enabled: has_tabs, ..cmd("ui.tab.close", "Close the current tab", &["Ctrl+Shift+W"]) },
    UiCommand { enabled: has_tabs, ..cmd("ui.tab.next", "Next tab", &["Ctrl+Tab", "Ctrl+PageDown"]) },
    UiCommand { enabled: has_tabs, ..cmd("ui.tab.previous", "Previous tab", &["Ctrl+Shift+Tab", "Ctrl+PageUp"]) },
    cmd("ui.page.hosts", "Hosts", &[]),
    cmd("ui.page.keys", "Keys", &[]),
    cmd("ui.page.settings", "Settings", &[]),
    UiCommand { scope: Scope::Terminal, enabled: in_terminal, in_palette: false, ..cmd("ui.terminal.copy", "Copy the selection", &["Ctrl+Shift+C"]) },
    UiCommand { scope: Scope::Terminal, enabled: in_terminal, in_palette: false, ..cmd("ui.terminal.paste", "Paste", &["Ctrl+Shift+V"]) },
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
        "ui.page.hosts" => app.ui.page = Page::Hosts,
        "ui.page.keys" => app.ui.page = Page::Keys,
        "ui.page.settings" => app.ui.page = Page::Settings,
        // The terminal tab handles these itself (it has the selection and the clipboard).
        _ => app.ui.pending.push(command.id),
    }
}
