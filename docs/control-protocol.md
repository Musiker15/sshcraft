# Control protocol

SSHCraft has two local connections, both on the same wire (the crate `sshcraft-link`):

- **The agent link**, while the user lets agents connect (Settings › AI agents › Allow agent
  control). `sshcraft-cli` and its MCP server use it. It only runs engine commands, each as the
  agent through the agent gate; it never reaches the window.
- **The control channel**, only with the launch option `--control <port>`. It is a developer
  tool: whoever holds its token clicks and types like the user, can confirm a question and take
  a picture of a terminal. Never on by default.

## The wire

- TCP on `127.0.0.1` only. A client takes only `127.0.0.1:<port>` and `[::1]:<port>` and never
  looks a name up.
- One JSON object per line. A request is `{"id": 1, "method": "…", "params": {…}}` (`params` may
  be left out); its answer is `{"id": 1, "ok": true, "result": …}` or
  `{"id": 1, "ok": false, "error": "…"}`.
- Limits: a request line of at most 1 MiB (a longer one closes the connection), an answer of at
  most 8 MiB (a larger one becomes the error `the answer is too large to send`), 16 connections
  per listener, 64 requests of one connection at once, 10 seconds to sign in.
- Requests run side by side. `{"method": "cancel", "params": {"request": <id>}}` drops a running
  one; a dropped request withdraws its question in SSHCraft and is logged as cancelled. When the
  connection ends, its running requests end with it.

## Signing in

The first lines of every connection sign in by challenge; nothing is run before. The token never
crosses the wire, and the server proves it holds the token before the client sends anything
derived from it, so a program that took the port of a stale connection file learns nothing.

```json
{"id": 1, "method": "auth.challenge", "params": {"clientNonce": "<64 hex: 32 fresh random bytes>"}}
{"id": 1, "ok": true, "result": {"serverNonce": "<64 hex>", "serverProof": "<64 hex>"}}
{"id": 2, "method": "auth", "params": {"proof": "<64 hex>"}}
{"id": 2, "ok": true, "result": {"authenticated": true}}
```

Both proofs are lowercase hex HMAC-SHA256, keyed by the token's 64 lowercase hex characters (as
ASCII), over `<label>\n<clientNonce>\n<serverNonce>`. The label is `sshcraft-link-v1 server` for
`serverProof` and `sshcraft-link-v1 client` for `proof`. A client checks `serverProof` and closes
the connection without sending `auth` when it does not match. A wrong proof gets
`authentication required` and the connection is closed. There is no sign-in with the token
itself.

## The agent link

While agents may connect, the app keeps its token in `agent-control.token` in its settings
folder (a fresh one at every start of the link, only the user may read it: mode 0600 on Unix, the user's profile on Windows)
and publishes the connection beside it in `agent-connection.json`:

```json
{ "version": 1, "pid": 41234, "port": 50731, "tokenFile": "…/agent-control.token", "app": "0.1.0" }
```

The file holds no secret. It goes when the link stops or the app quits. A file left by a crash
names a port nothing listens on, or one another program took: the challenge stops a client there
before it sends anything. The settings folder is `%APPDATA%\musiker15\SSHCraft\config` on
Windows; `SSHCRAFT_LINK_DIR` moves it (tests, portable use).

After signing in, a connection introduces itself once:

| Method | Params | Result |
|---|---|---|
| `hello` | `{agent: name, via: "mcp" \| "cli" \| "launcher"}` | `{app}`; the name is 1 to 64 characters without control or bidi characters |
| `engine.execute` | `{command, params}` | the command's result; it runs as that agent through the gate |
| `engine.commands` | none | the commands agents may use: `{id, label, params, enabled}` |
| `ping` | none | `"pong"` |

Anything else, `ui.*` and screenshots among it, is `unknown method`. When the connection ends,
the sessions it opened end.

**Consent.** Without the suite's `AI-MCP` flag (`craft-flags`), the switch *Allow agent control*
alone decides. With the flag, the ArtCraft Launcher's app list (`craft-ai-agents`) must name
`sshcraft` as well. The window looks every two seconds; turning either off stops the link, cuts
every agent off and removes the connection file. Nothing an agent sends changes the consent.

## The control channel

```sh
sshcraft --control 7878 --control-token-file /private/path/control.token
```

With `--control-token-file`, the token there is used (made when missing). Without it, a fresh
token for this start goes into `control.token` in the settings folder. Engine commands through
this channel go through the gate as the caller `control`, like an agent of that name.

| Method | Params |
|---|---|
| `engine.execute` | `{command, params}` |
| `engine.commands` | none: every command, with `agents` saying whether agents reach it |
| `engine.journal` | none: the commands run so far, without secrets |
| `window.list` | none |
| `ui.inspect` | none: the page, the settings section, the tabs with their panes, open questions, notices, the window size, whether a dialog is open, and `widgets`: what is on the screen with a name or a click, `{role, label, x, y, width, height, enabled, focused, checked?}` in screen points, from the frame shown last. Names only, never what is typed into a field |
| `ui.click` | `{x, y, button?: left\|right\|middle, count?: 1 to 8}` in screen points |
| `ui.move` | `{x, y}` |
| `ui.drag` | `{from: [x, y], to: [x, y]}` |
| `ui.key` | `{key, shift?, ctrl?, alt?, command?}` with egui's key names (`Enter`, `Tab`, `A`, `F5`) |
| `ui.text` | `{text}`, at most 4096 characters |
| `ui.screenshot` | none: `{width, height, png_base64}` |
| `ui.set` | `{page: hosts\|keys\|snippets\|forwarding\|logs\|settings\|tab:<n>}` |
| `ui.resize` | `{width, height}` |
| `ui.focus` | none |
| `terminal.read` | `{session?}`: the visible text of that session, or of the shown terminal |
| `app.quit` | none |

Every parameter is checked before anything happens: an unknown field, a wrong type, a number
outside ±1 000 000 or an unknown key or page is an error, and nothing is queued. Input goes into
the next frames through eframe's raw-input hook, so it reaches the widgets as a person's would;
the answer comes once the input went in. A request waits at most 60 seconds for the window.

`sshcraft-cli ui --bridge <port> --control-token-file <path> inspect|click|key|text|screenshot`
and `sshcraft-cli mcp --bridge …` work against such an app.
