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
  while the setting is on), never read it. A read request gets no answer.
- **Links:** only `http` and `https` links open, only on Ctrl+click, and Rust checks the link
  again (`open_link`) before the system opens it. `file:`, `javascript:` and other schemes are
  refused. The window has no opener permission of its own.
- **Broadcast:** answers the terminal gives to the server by itself (cursor position, device
  attributes, focus) go only to their own session, never to the other panes.

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

Only one SSHCraft runs at a time. A second start brings the open window to the front and exits,
so the local database and the key vault are never opened twice.

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

- From R6 on, agents reach SSHCraft only over loopback, with a token made at every start and kept
  in a file only the current user can read; it is off until the user allows agents. Until R6 there
  is no agent connection on `main`.
- No action returns key material, passwords or account keys; answers that look like key
  material are blocked. Rights per kind of action (allow, ask, block), questions only in
  SSHCraft's window, hidden hosts answered as unknown, a sealed log of every action.
- An agent with its own shell runs as the same user and can bypass SSHCraft (`pass-cli`, files).
  Details in [agents.md](agents.md).
