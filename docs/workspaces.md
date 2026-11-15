# Workspaces and terminal

> The desktop app is being rebuilt (R1 to R7); this page describes the behaviour the rebuild keeps.

A session tab can hold up to 16 sessions (panes). With one pane it looks like a plain tab;
the tools at the right end of the tab strip turn it into a workspace.

## Split and focus

- **Split** (Ctrl+Shift+D) adds a pane. It starts as a small new-tab page: pick a host, a
  local shell or a serial port there.
- The panes sit in a near-square grid: 2 side by side, 3 and 4 in two rows, up to 4 × 4. The
  last pane reaches the right edge.
- Drag the thin line between columns or rows to change their size. Drag a pane's title bar
  onto another pane to swap the two.
- **Focus** (Ctrl+Shift+M) shows one pane large and lists the others on the left with their
  status dot; a click brings another one to the front. The maximize button in a pane's title
  bar does the same for that pane.
- Ctrl+Shift+PageDown and Ctrl+Shift+PageUp move to the next and previous pane.
- **Move to its own tab** (button in the title bar) takes a pane out into a tab next to it.
  Drag a tab onto another tab to put its panes into that one (up to 16; the rest stays).
- None of this reconnects: a pane keeps its connection wherever it moves.
- Ctrl+Shift+X closes the focused pane, the last pane closes the tab.

## Broadcast

Ctrl+Shift+B (or the antenna button) sends what you type in one pane to every pane of the
tab that has a session. While it is on, all panes of the tab have a red border and the tab
shows the antenna. It is off for every new tab and for every opened template.

Answers the terminal gives to the server on its own (cursor position, device attributes,
focus reports) are never broadcast; they only go to their own session.

Typing goes only to panes in the same state as the one you type in: while it asks for a
password (or a yes/no), only to the panes that ask too; at a prompt, only to the panes at a
prompt. So a command never lands in another server's password question, and a password never
at another server's prompt, where it would show and run.

## Status dots

Every pane has a dot in its title bar, the tab shows the most urgent one of its panes:

| Dot | Meaning |
|---|---|
| red | the session ended with an error |
| amber ring | the pane waits for input: the bell rang, or the last line asks for a password, a passphrase, yes or no |
| blue | new output you have not looked at |
| green, yellow, grey | connected, connecting, ended |

Looking at a pane (its tab active and the pane focused) clears blue and amber, red stays.

## Workspace templates

The save button in the tab strip stores the tab as a template: its name, the layout and sizes,
and for each pane the connection, a title, a working directory and a command to run after
connecting. The working directory is taken from the shell if it reports it (OSC 7).

- Templates stay on this device. They are not synced.
- Open one from the new-tab page (Ctrl+Shift+T) or the command palette ("Workspace: name").
  Delete one with the bin next to it on the new-tab page.
- Over SSH the directory is changed with `cd -- '<dir>'` and the command follows with `&&`,
  typed once after the first connect. That needs a POSIX shell (or fish) on the server. A
  directory with a backslash types nothing, since fish would read it inside the quotes. A local
  shell starts in the directory directly, a serial line gets only the command.
- A pane closed while it still connects takes its connection with it: nothing is typed.
- A pane whose host was deleted in the meantime is left out, and a notice says how many.
- Limits: 100 templates, 16 panes, 100 characters for the name, 1000 for directory and command.
  Directory and command are single lines.
- A stored template this version cannot read (damaged, or written by a newer version) is not
  shown, and it is dropped the next time a template is saved or deleted.
- A local shell starts only in a local, absolute directory. A network path (`\\server\share`)
  or a relative one is ignored and the shell starts in your home directory.

## Terminal type

Settings → Terminal → Terminal type sets `TERM` for new connections: `xterm-256color`
(default), `xterm`, `screen-256color`, `tmux-256color`, `linux`, `vt220` or `vt100`. It applies
to SSH and local shells, not to serial lines.

## Clipboard from the server (OSC 52)

Programs such as tmux, vim or neovim can copy to the local clipboard with OSC 52. SSHCraft lets
them write (up to 1 MiB) and never answers a request to read the clipboard. Settings →
Terminal turns writing off.

## Links

Links in the output, plain ones and OSC 8 hyperlinks, open in the browser with Ctrl+click.
Only `http` and `https` links open; SSHCraft checks the link again before handing it to the
system.
