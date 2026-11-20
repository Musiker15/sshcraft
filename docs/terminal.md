# Terminal comfort

The terminal can mark where commands start and end, notify you when a long one finishes, colour
words you choose and label addresses on the screen for copying. Everything a server sends for
these features is bounded; [security.md](security.md#terminal-output) has the limits.

## Shell integration

With shell integration your shell marks its prompt and each command (OSC 133). SSHCraft then
shows a thin bar left of every prompt line: green when the command exited with 0, red for any
other end, grey while it runs. Hover it to see the exit code, how long the command took and when
it started.

- **Previous and next command:** Ctrl+Shift+Up and Ctrl+Shift+Down scroll to the prompt above or
  below.
- **Copy the output of the last command:** from the command palette.
- **Fold the output of a command:** click the bar left of its prompt. The output becomes one row
  that says how many lines it hides; click that row to show them again. *Fold or show the output
  of the last command* and *Show all folded output* are in the command palette. A fold only
  changes what the window shows: selecting across it copies the hidden lines too, a search that
  finds a match in folded output opens it, and new output does not touch it. Only a finished
  command that printed something folds, and a full-screen program is never folded.

SSHCraft never installs anything on a server. Add the lines for your shell to its start file
yourself:

**bash** (`~/.bashrc`):

```bash
__sshcraft_ran=1
__sshcraft_status() { __sshcraft_s=$?; }
__sshcraft_prompt() { printf '\e]133;D;%s\a\e]133;A\a' "$__sshcraft_s"; __sshcraft_ran=; }
PROMPT_COMMAND="__sshcraft_status;${PROMPT_COMMAND:+$PROMPT_COMMAND;}__sshcraft_prompt"
PS1="$PS1"'\[\e]133;B\a\]'
trap '[[ -z $__sshcraft_ran && $BASH_COMMAND != __sshcraft_* ]] && { __sshcraft_ran=1; printf "\e]133;C\a"; }' DEBUG
```

The exit code is taken first and the prompt marks come last, so other `PROMPT_COMMAND` entries
(direnv, `history -a`) neither change the code nor start a command.

**zsh** (`~/.zshrc`):

```zsh
__sshcraft_precmd() { local s=$?; print -n "\e]133;D;$s\a\e]133;A\a"; }
__sshcraft_preexec() { print -n "\e]133;C\a"; }
precmd_functions+=(__sshcraft_precmd)
preexec_functions+=(__sshcraft_preexec)
PS1="$PS1"$'%{\e]133;B\a%}'
```

**fish** (`~/.config/fish/config.fish`):

```fish
function __sshcraft_prompt --on-event fish_prompt
    printf '\e]133;D;%s\a\e]133;A\a' $status
end
function __sshcraft_preexec --on-event fish_preexec
    printf '\e]133;C\a'
end
```

fish has no hook at the end of its prompt, so very long commands can lose the mark of their
prompt line; the bar then shifts by one.

## Notifications

- **A long command finished:** when a command ran longer than the setting *Notifications*
  (10 seconds by default, 0 turns it off) and ends while the SSHCraft window is in the
  background, a desktop notification says on which host and with what exit code. It needs
  shell integration.
- **Messages from the server:** programs can ask for a desktop notification (OSC 9 and
  OSC 777). The title always starts with the host's name, so a server cannot pretend to be
  SSHCraft, and each tab shows at most one every 5 seconds. While you look at the tab it is a
  message in the window instead. *Servers may show desktop notifications* in the settings turns
  them off.

## Highlights and triggers

In *Settings, Terminal*:

- **Highlights** colour every match of a regular expression on the screen, ignoring case.
- **Triggers** check each finished output line once. A trigger either sends a notification
  (with the host's name, limited like a server's message) or marks the tab like the bell.
  Full-screen programs such as `vim` are left out.

Each list holds up to 32 patterns of up to 256 characters. A pattern that is not valid, or that
would need too much memory to run, is marked red and not used.

## Hint mode

Ctrl+Shift+H puts a short label on every URL, path, IP address and hash on the screen. Type a
label to copy its text; type it in capitals to type the text into the terminal instead. Escape
leaves. Nothing found is ever opened or run.

## Pastes and the compose field

- **Recent pastes:** Ctrl+Shift+Y lists the last 20 pastes to paste one again. They are kept in
  memory only and are gone when SSHCraft closes, because a paste may hold a password.
- **Compose field:** Ctrl+Shift+E opens a text field under the terminal for a long command or a
  block of lines. Ctrl+Enter sends it to the terminal, or to every pane while broadcast is on.

## Program status

Programs can report what they are doing (OSC 7501): working, waiting for you, done or failed.
Coding agents in a terminal use it, for example. The tab's dot shows the most urgent state:
amber when a program waits for you, red when one failed, blue when one finished, a blue ring
while one works. Hover the dot for the program's own message. A program that starts waiting
for you also sends one notification. Typing in the terminal clears finished and failed
programs; a new prompt clears the others.

## Hex view

*Hex view of what the session received* (command palette, or a shortcut of your own) opens a
panel over the lower half of the terminal. It lists every byte that arrived since it opened as
offset, hex and printable text, which helps when a program draws odd characters or a device
sends something unexpected. Bytes that are not printable ASCII show as `.`. The panel keeps at
most the last 64 KiB and nothing while it is closed; closing it drops what it held.
