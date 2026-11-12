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

## One instance

Only one SSHCraft runs at a time. A second start brings the open window to the front and exits,
so the local database and the key vault are never opened twice.

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
