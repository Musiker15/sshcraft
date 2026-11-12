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
