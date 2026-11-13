# Keyboard shortcuts

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
| Ctrl+Shift+D | Split: add a pane to the tab |
| Ctrl+Shift+X | Close the focused pane |
| Ctrl+Shift+PageDown, Ctrl+Shift+PageUp | Next and previous pane |
| Ctrl+Shift+M | Switch between split and focus layout |
| Ctrl+Shift+B | Broadcast on or off |
| Ctrl+Shift+S | Snippets and history beside the terminal |

Panes and broadcast are described in [workspaces.md](workspaces.md), the sidebar and the keys of the
completion list in [snippets.md](snippets.md).

## Your own shortcuts

Settings → Keyboard shortcuts lists every action. **Change** waits for the next key
combination and puts it in place of the action's shortcuts, **Remove** leaves the action
without one, **Default** brings back the ones above.

- A shortcut needs Ctrl, Alt or the Windows key, or it is a function key (F1 to F24).
- One combination belongs to one action; a combination already in use is refused with the
  name of the action that has it.
- A combination that is not Ctrl+Shift or Ctrl+Tab works outside the terminal only, and the
  list says so next to it.
- Shortcuts are kept on this device.

In the command palette, the arrow keys move the selection, Enter opens it and Escape closes the
palette. On the host list, Enter or a double click connects to the selected host, a right click
opens its menu.
