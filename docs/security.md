# Security notes for the desktop app

## Content Security Policy

The window runs under this policy (see `app/src-tauri/tauri.conf.json`):

```
default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:;
font-src 'self'; connect-src ipc: http://ipc.localhost; object-src 'none'; base-uri 'none';
form-action 'none'; frame-ancestors 'none'
```

`style-src` allows inline styles because the terminal library xterm.js creates `<style>`
elements at run time for its colour theme and character sizes. A nonce cannot cover elements
created after the page has loaded. Scripts stay limited to the app's own files.

## Terminal output

What a server prints may not reach beyond the terminal on its own:

- **Clipboard (OSC 52):** a program may write text to the clipboard (at most 1 MiB, and only
  while the setting is on), never read it. A read request gets no answer.
- **Links:** only `http` and `https` links open, only on Ctrl+click, and Rust checks the link
  again (`open_link`) before the system opens it. `file:`, `javascript:` and other schemes are
  refused. The window has no opener permission of its own.
- **Broadcast:** answers xterm.js gives to the server by itself (cursor position, device
  attributes, focus) go only to their own session, never to the other panes.

## Port forwarding

- New rules listen on loopback only (`127.0.0.1`, or `localhost` on the server for a remote
  rule). Another address makes the forwarding usable by anyone who reaches it; the wizard warns.
- The dynamic proxy has no authentication, like `ssh -D`. Keep it on loopback.
- A connection the server opens for a remote rule is only accepted for an address and port this
  session asked for; anything else is refused.
- Every rule carries at most 256 connections at a time, and a SOCKS handshake has 10 seconds.

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
