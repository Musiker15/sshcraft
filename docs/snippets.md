# Snippets, history and completion

> The desktop app is being rebuilt (R1 to R7); this page describes the behaviour the rebuild keeps.

Snippets, the command history and completion stay on this device. They do not sync. Snippets
and the history are encrypted with the vault, so they need it unlocked.

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
- The default hosts are picked first, each once: its open pane where it has one, else a new
  connection.
- **Run** pastes the snippet and presses Enter; **Paste only** leaves it at the prompt to check.
  Where the shell supports bracketed paste (bash 5.1 and later, zsh, fish), several lines are
  pasted as one block and nothing runs before Enter. Without it each line runs as it arrives,
  even with "paste only", and tabs are typed as spaces so they do not start the shell's
  completion.
- A default host that was deleted is named in the dialog and left out.

### Placeholders

`{{name}}` in a snippet asks for a value when it runs; `{{name:default}}` offers one, which you
may change or clear. The run dialog lists each name once; **Run** stays off until every
placeholder without a default has a value. Values are typed exactly as you enter them, nothing
interprets them on your computer. Write `{{{{` for a literal `{{`. A snippet has at most 16
different placeholders; names use letters, digits, `-` and `_`.

### The snippet bar

Tick **On the bar** next to a snippet to get a button for it above every terminal (up to 12).
A click runs it in that pane at once, or opens the run dialog for that pane when the snippet has
placeholders.

## Terminal sidebar

Ctrl+Shift+S (or the panel button at the right of the tab strip) shows snippets and the history
of the focused pane beside the terminal. A click pastes into the focused pane (with broadcast on,
into all panes of the tab); the play button next to a snippet runs it. The search covers both,
and the history of the focused terminal can be cleared there.

## History

SSHCraft keeps the commands typed in a terminal, per host and per local shell (not for serial
lines), up to 1000 per terminal, the most recent first.

- Encrypted with the vault, on this device only.
- Only lines typed at a shell prompt (ending in `$`, `#`, `%`, `>`, `❯` or `»`, or starting
  with `➜`) are kept, and only when the shell echoed them: what a program does not show, such as
  a password, is never kept, whatever its question says. What kind of line it is is decided
  from the output when its first key is typed and kept until Enter, so a password stays secret
  even where the server prints an asterisk per key. A line typed before the shell showed its
  prompt (typeahead) is not kept either.
- Not kept:
  - lines starting with a space (as `HISTCONTROL=ignorespace` does);
  - input after a password, passphrase, PIN, token, secret or verification code question (also
    in German, French, Spanish and Portuguese, and with a hint after it, such as
    `Token (will be hidden):`);
  - input to programs with their own prompt: mysql, MariaDB, psql, sqlite, python, irb, a
    shell's continuation prompt `>`;
  - anything typed in a full-screen program (vim, less, top);
  - lines the shell changed in ways SSHCraft cannot follow (Tab completion, arrow keys, Ctrl+R),
    and pasted text with several lines;
  - input while broadcast is on.
- A nested shell (`ssh` or `su` inside the terminal) looks like a shell prompt: its commands go
  into the history of the outer host.
- Settings → Terminal turns the history off and clears all of it; the sidebar clears the focused
  terminal's. A cleared line is gone from the suggestions and the sidebar at once. Deleting a
  host deletes its history.

## Completion

While you type at a prompt, SSHCraft suggests, in this order: commands from the history, snippets,
and paths (relative ones start at the shell's directory when it reports it with OSC 7). Nothing
comes from an AI.

- The list shows up from two characters on, only at a shell prompt (never while a password is
  typed, nor in mysql, python and the like), and never takes a key by itself: Tab, Enter and the
  arrow keys go to the shell as usual.
- **Down arrow** selects the first suggestion; then Up/Down move, **Tab** or **Enter** takes it
  (and only takes it: nothing runs), **Escape** closes the list (without a selection Escape still
  reaches the shell).
- Taking a suggestion types only the missing rest. A snippet found by its name replaces the line:
  SSHCraft erases what you typed with Backspace and pastes the snippet.
- Paths are suggested for a word that starts with `/`, `~`, `./` or `../` or contains `/`.
  - Over SSH SSHCraft runs one fixed command, `sh -c '… ls -1ApL -- "$d"'`, on a separate
    channel of the same connection and sends the directory on its standard input. No shell ever
    parses the typed name, whatever the login shell is (bash, fish, csh, cmd). It needs `sh` on
    the server and runs as the user that logged in (not as the user of an `su` or `sudo -i`
    inside the terminal).
  - A local shell reads the folder directly.
  - Relative paths need the shell to report its directory (OSC 7, which many shells do or can
    be set to).
  - Listings are reused for 10 seconds.
- No suggestions in full-screen programs or after shell editing SSHCraft cannot follow.
- Settings → Terminal turns completion off.
