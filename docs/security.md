# Security notes for the desktop app

The desktop app is being rebuilt (R1 to R7); this page describes the behaviour the rebuild keeps.
How to report a vulnerability is in [SECURITY.md](../SECURITY.md).

## Rules against the weaknesses of other clients

A comparison with other SSH clients (the German
[`superpowers/specs/2026-11-14-sshcraft-marktvergleich.md`](superpowers/specs/2026-11-14-sshcraft-marktvergleich.md),
section 4) found the weaknesses below; these rules keep them out of SSHCraft. Each gets a test
where it can have one.

1. **No shell string from foreign data.** File names, paths, host names and server output never go
   into a command line a shell evaluates; local programs start with argument lists. (Code
   execution bugs in Tabby and Electerm.)
2. **Links never run anything.** `ssh://` and other links at most open the connection dialog with
   checked fields. (`tabby://run`, Electerm's deep-link code execution.)
3. **Escape sequences are bounded.** OSC 52 may only write, never read, and only after consent per
   host; file transfer over the terminal (ZMODEM, trzsz) never starts by itself; queries whose
   answer could inject input are off. (Tabby's ZMODEM auto-start, Electerm's clipboard leak.)
4. **Paths from the network are normalized and bounded** before they touch anything local.
   (Path traversal in Tabby's SFTP.)
5. **Nothing is left on a server** unless the user asks for it. (iTerm2, CVE-2025-22275.)
6. **No telemetry.** From F2 on, every connection SSHCraft makes by itself is listed in the
   settings; an update check runs only after consent. (Warp, Wave.)
7. **Cryptography only from reviewed libraries**, signatures with deterministic nonces; F1 adds a
   strict-kex test. (PuTTY CVE-2024-31497, Terrapin CVE-2023-48795.)
8. **Agent interfaces without HTTP**, a token on every connection, rights per action. (Electerm's
   MCP server.)
9. **Performance budgets as tests:** 1 GB of output must not freeze the interface; a memory budget
   per session and for the idle app. (Electron clients.)
10. **Local data stays authoritative:** sync never deletes locally without a tombstone; from F2 on
    there is a trash and an encrypted backup file. (Termius vaults that emptied after updates.)
11. **Accessibility:** egui with AccessKit, the terminal readable as linear text by screen readers.
12. **Supply chain:** one official download address, signed packages through winget, Homebrew and
    Flathub, checksums, an SBOM; reproducible builds as a goal. (Xshell backdoor, fake PuTTY
    downloads.)
13. **Every security bug gets a test that failed before the fix.** (Tabby's incomplete fix.)

## Terminal output

What a server prints may not reach beyond the terminal on its own:

- **Clipboard (OSC 52):** a program may write text to the clipboard (at most 1 MiB, and only
  while the setting is on), never read it. A read request gets no answer. Each write asks first:
  "Once", "Always for this host" or "No". "Always" stores that one host in
  `terminal.clipboard_hosts` (at most 1000 hosts); a local shell or serial port always asks.
- **Links:** only `http` and `https` links open, only on Ctrl+click, and the engine checks the
  link again (`link.open`) before the window opens it; a refused link shows why and opens
  nothing. `file:`, `javascript:` and other schemes
  are refused. A link target longer than 2048 bytes is dropped.
- **Titles:** a window title loses control and bidi characters and is cut to 256 characters.
  Title reports (`CSI 20 t`, `CSI 21 t`) get no answer, so a server cannot make the terminal
  type a title it chose.
- **Long sequences:** an OSC sequence (title, clipboard, link) is held back until it ends; one
  longer than 1.5 MiB is dropped whole, so neither memory nor a cut-off clipboard follows from a
  sequence that never ends, and the output after it shows as usual.
- **Memory:** a cell keeps at most 16 joining characters, and what cells carry besides their
  character (joining characters, links, underline colours) has a budget of 32 MiB per session;
  past it the oldest lines lose links and decorations, never their text.
- **File transfer** over the terminal (ZMODEM, trzsz) never starts: the terminal does not speak
  either protocol.
- **Paste:** every `ESC` is removed from pasted text before it is sent, so it can neither end the
  bracketed paste early nor type an escape sequence of its own.
- **Broadcast:** answers the terminal gives to the server by itself (cursor position, device
  attributes, focus) go only to their own session, never to the other panes.

## Questions in the window

- **Host keys:** a server whose key is not stored yet shows its fingerprint and asks; only
  "Accept" stores it. A server whose key differs from the stored one is never asked about: the
  session ends with a warning and nothing to accept, and the stored key stays.
- **One click, one answer:** the buttons of a new question take a click only 400 ms after it
  appears, so a click meant for the question before it never answers the next one.
- **Sign-in questions** show what the server asks and hide the typed answer unless the server
  says it may be shown. A question takes the keyboard: while it is open, nothing typed reaches
  the terminal behind it, so a password typed out of habit never lands in a shell. Answer
  fields keep no undo history.
- **Late answers:** a dialog sends one command at a time, and an answer that comes after its
  dialog was closed never touches the dialog open now. A connection that opens late waits in
  its tab and does not take the page or the keyboard.

## Panes, history and agents in the window

- **Broadcast** sends only what is typed or pasted, and only to panes whose session is connected
  and that ask a password (or yes/no) question exactly when the pane typed in does; mouse, wheel
  and focus reports and the terminal's own answers go to their own session alone.
- **History** keeps only lines typed after a shell prompt sign and echoed back by the shell:
  never input nothing shows (a secret behind any question), never typeahead, never after a
  password, passphrase, PIN, token or code question (in five languages), never in a REPL, a
  full-screen program, a line starting with a space, an edited line, a multi-line paste or a
  broadcast. Clearing the history takes it out of the suggestions at once.
- **Templates** type the directory as one quoted word (`cd -- '<dir>'`) and refuse a directory or
  command with a control character, and a directory with a backslash (fish reads one inside
  single quotes): nothing in them runs as a second command. A snippet taken from the suggestions
  is pasted, never run.
- **An agent's question** names the agent, the action, the target and the command, warns about
  commands of several lines, over 300 characters or with hidden characters, and takes a click
  only 400 ms after it appears. Its buttons never take the keyboard focus, so no key (Tab,
  Enter, Space) answers it; Deny comes first. The same holds for an unknown host key.
- **The account key** of a new sync account is shown once, with a QR code, and the window keeps
  no copy after it is closed; the key and its QR code are wiped from memory then. Forms holding
  an API key or an account key never print them and wipe them when they close.
- **Deleting for good** (logs, a snippet, a forwarding rule, ending a share, removing a sync
  server) asks first and says what goes.
- **Sharing** asks to compare the recipient's safety number first, and stops with the old and the
  new number when the server names another key for that e-mail.

## Port forwarding

- New rules listen on loopback only (`127.0.0.1`, or `localhost` on the server for a remote
  rule). Another address makes the forwarding usable by anyone who reaches it; the wizard warns.
- The dynamic proxy has no authentication, like `ssh -D`. Keep it on loopback.
- A connection the server opens for a remote rule is only accepted for an address and port this
  session asked for; anything else is refused.
- Every rule carries at most 256 connections at a time, and a SOCKS handshake has 10 seconds.

## Command history and completion

- The history is sealed with the vault per terminal target, snippets per snippet; nothing of
  them is on disk in plain text, and they never sync. Only lines typed at a shell prompt are
  kept: not input after a password question (decided when the line starts, so asterisk feedback
  does not hide it), not lines starting with a space, not input to mysql, psql, python and the
  like, not input in full-screen programs. No suggestions and no path listing while such a line
  is typed.
- Path completion over SSH runs one fixed command on its own exec channel and sends the
  directory on standard input, so no login shell parses a typed name. Output is limited to
  64 KiB and 500 names, only what follows a marker line counts, names with control characters
  are left out.

## One instance

Only one SSHCraft runs at a time. The engine takes an exclusive lock on the data folder before it
opens the database or the vault, and refuses to open while the Tauri version of the same user
runs, so the local database, the key vault and the device's sync keys are never used by two
processes. (From R4 on, a second start brings the open window to the front.)

## Keys

Private keys are sealed in the vault with XChaCha20-Poly1305, bound to their id. A key whose
passphrase is not saved stays encrypted with that passphrase inside the vault too; the decrypted
key never reaches the disk, and the passphrase is asked at most three times per connection and
kept only in memory. Windows Hello guards only keys whose secret lies open in the vault: a key
with a passphrase asks for the passphrase instead, and a FIDO2 or Windows Hello key is checked by
its authenticator. Windows Hello keys never leave the device: they are refused as a sync item,
skipped in shares and cannot move to a server. *Add to authorized_keys* sends one fixed `sh`
command built from the key itself, never from text typed in the UI. See [keys.md](keys.md).

## Passwords and agent forwarding

Stored passwords are sealed with the vault's key (XChaCha20-Poly1305), bound to their id, and
never written in clear text to disk or a log. No command returns a stored password, nor the id it
is stored under, to the UI: forms only learn whether one is set. Sync carries them only inside the
end-to-end encrypted records and only with key sync on. Each hop of a route gets only its own
credentials, a proxy only its own, and a stored password answers a keyboard-interactive round only
when it is a single hidden question.

Agent forwarding is off by default, reaches the target only (never a jump host) and forwards the
system agent only, never SSHCraft's vault. An agent channel the server opens without forwarding
switched on is refused. See [connections.md](connections.md).

## Session logs

- A session log holds what the terminal showed, never the typed keys; a password a program
  prints on the screen is in it. Recording is off until switched on (setting, host or group).
- Every part and header is sealed with the vault, bound to its log and position. Plain in the
  database: ids, host id, start and end, size and storing time of each part, place.
- Log sync is a switch of its own per server and goes into the personal area only. The server
  learns how many log records there are and their sizes. Details in
  [session-logs.md](session-logs.md).

## AI agents

- Agents reach SSHCraft only over loopback (`127.0.0.1`), and only while the user allows agent
  control (off on a fresh install; with the suite's AI-MCP flag the ArtCraft Launcher must allow
  it too). The token stays in a file only the user can read; every connection signs in by
  challenge, so the token never crosses the wire and a program on a stale port learns nothing.
  The agent link runs engine commands only: no window input, no screenshots, no journal.
- The control channel (`--control`) is a developer tool, never on by default: whoever has its
  token clicks and types like the user. Its engine commands still pass the gate as `control`.
- A connection's sessions and open questions end with it; an agent cannot flood the log through
  the link (the newest 1000 entries stay).
- No action returns key material, passwords or account keys; answers that look like key
  material are blocked. Rights per kind of action (allow, ask, block), questions only in
  SSHCraft's window, hidden hosts answered as unknown, a sealed log of every action.
- The gate is part of the engine: every call that is not the user's (an agent or the control
  channel) passes it in `Session::execute`, so no transport can go around it. A command without
  an agent category is refused and logged.
- The engine's journal of commands never holds the value of a secret parameter, at any depth;
  typed keys and answers to sign-in questions are not journaled at all.
- An agent with its own shell runs as the same user and can bypass SSHCraft (`pass-cli`, files).
  Details in [agents.md](agents.md).
