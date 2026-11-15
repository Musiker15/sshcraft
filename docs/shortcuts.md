# Keyboard shortcuts

> The desktop app is being rebuilt (R1 to R7); this page describes the behaviour the rebuild keeps.
> Every shortcut below works since R5.

Shortcuts that work everywhere in SSHCraft. Inside a terminal only combinations with
Ctrl+Shift (without Alt) or Ctrl+Tab are taken, so every key the shell needs (Ctrl+W, Ctrl+K,
Ctrl+R, Ctrl+C, Alt+..., AltGr) reaches the server unchanged.

| Shortcut | Action |
|---|---|
| Ctrl+Shift+P | Open the command palette |
| Ctrl+K | Open the command palette (not inside a terminal) |
| Ctrl+Shift+T | New tab |
| Ctrl+Shift+W | Close the current tab |
| Ctrl+Tab | Next tab |
| Ctrl+Shift+Tab | Previous tab |
| Ctrl+PageDown, Ctrl+PageUp | Next and previous tab (not inside a terminal) |
| Ctrl+Shift+C | Copy the selection in the terminal |
| Ctrl+Shift+V | Paste into the terminal |
| Ctrl+Shift+D | Split: add a pane to the tab |
| Ctrl+Shift+X | Close the focused pane |
| Ctrl+Shift+PageDown, Ctrl+Shift+PageUp | Next and previous pane |
| Ctrl+Shift+M | Switch between split and focus layout |
| Ctrl+Shift+B | Broadcast on or off |
| Ctrl+Shift+S | Snippets and history beside the terminal |
| Ctrl+Shift+L | Bookmark in the session log of the focused pane (if it records) |

Panes and broadcast are described in [workspaces.md](workspaces.md), the sidebar and the keys of the
completion list in [snippets.md](snippets.md), bookmarks in [session-logs.md](session-logs.md).

## Your own shortcuts

Settings → Shortcuts lists every action. **Change** waits for the next key
combination and puts it in place of the action's shortcuts, **Remove** leaves the action
without one, **Default** brings back the ones above.

- A shortcut needs Ctrl or Alt (or ⌘ on macOS), or it is a function key (F1 to F24). The
  Windows key cannot be used: the window does not see it.
- One combination belongs to one action; a combination already in use is refused with the
  name of the action that has it.
- A combination that is not Ctrl+Shift or Ctrl+Tab works outside the terminal only, and the
  list says so next to it.
- Shortcuts are kept on this device, in `config.toml` under the action's name (`newTab`,
  `palette`, ...), the same names the Tauri version used.

The menu bar shows each command with its current shortcut. Quit SSHCraft (Exit on Windows) and
About SSHCraft have none by default; macOS gives Quit ⌘Q. On Windows and Linux, Alt with a
menu's underlined letter opens it.

In the command palette, the arrow keys move the selection, Enter opens it and Escape closes the
palette. On the host list, Enter or a double click connects to the selected host, a right click
opens its menu.
