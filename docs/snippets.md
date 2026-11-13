# Snippets, history and completion

Snippets, the command history and completion stay on this device. They do not sync.

## Snippets

A snippet is a command or a short script with a name. The page **Snippets** in the sidebar lists
them, sorted into packages (folders); a snippet without a package shows under "Without package".

- **New snippet:** name, package (an existing one is offered while typing), the commands
  (several lines are fine), and the hosts it usually runs on (default hosts).
- Double click edits, the right-click menu also deletes.
- Limits: names and packages up to 100 characters, commands up to 10000 characters without
  control characters other than line breaks and tabs, 50 default hosts, 1000 snippets.

### Running a snippet

**Run** (on the page, in the terminal sidebar, or "Snippet: name" in the command palette) asks
where and how:

- **New connections:** one tab named "Snippet: name" with a pane per chosen host (up to 16 per
  tab, more hosts open more tabs). Each pane types the snippet once after it connects.
- **Open tabs:** the chosen panes that have a connection get the snippet right away.
- The default hosts are picked first, both as new connections and as open panes connected to
  them.
- **Run** presses Enter after every line, **Paste only** leaves the last line for you to check.
- A default host that was deleted is left out, and a notice says how many.

## Terminal sidebar

Ctrl+Shift+S (or the panel button at the right of the tab strip) shows snippets and the history
of the focused pane beside the terminal. A click pastes into the focused pane (with broadcast on,
into all panes of the tab); the play button next to a snippet runs it. The search covers both.

## History

SSHCraft keeps the commands typed in a terminal, per host and per local shell (not for serial
lines), up to 1000 per terminal, the most recent first.

- Encrypted with the vault, on this device only.
- Not kept:
  - lines starting with a space (as `HISTCONTROL=ignorespace` does);
  - input right after a password or passphrase question;
  - anything typed in a full-screen program (vim, less, top);
  - lines the shell changed in ways SSHCraft cannot follow (Tab completion, arrow keys, Ctrl+R),
    and pasted text with several lines;
  - input while broadcast is on.
- Settings → Terminal turns the history off and clears it.

## Completion

While you type at a prompt, SSHCraft suggests, in this order: commands from the history, snippets,
paths and known commands with their subcommands and options (git, docker, systemctl, apt,
kubectl and about fifty more). Nothing comes from an AI.

- The list shows up from two characters on and never takes a key by itself: Tab, Enter and the
  arrow keys go to the shell as usual.
- **Down arrow** selects the first suggestion; then Up/Down move, **Tab** or **Enter** takes it,
  **Escape** closes the list (Escape always closes it, and without a selection still reaches the
  shell).
- Taking a suggestion types only the missing rest. A snippet found by its name replaces the line
  (Ctrl+U first).
- Paths are suggested for a word that starts with `/`, `~`, `./` or `../` or contains `/`.
  - Over SSH SSHCraft asks the server with `ls -1Ap` on a separate channel of the same
    connection: the shell is not touched, and the name is quoted so it cannot run anything.
  - A local shell reads the folder directly.
  - Relative paths need the shell to report its directory (OSC 7, which many shells do or can
    be set to).
  - Listings are reused for 10 seconds.
- No suggestions in full-screen programs or after shell editing SSHCraft cannot follow.
- Settings → Terminal turns completion off.
