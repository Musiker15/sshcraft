# Decisions

The decisions that carry SSHCraft, newest first. The reasoning is in the design documents under
[`superpowers/specs/`](superpowers/specs/) (German).

| Date | Decision |
|---|---|
| 2026-11-15 | Agents and the control channel share one wire (`sshcraft-link`) with challenge sign-in only; the token never crosses it. |
| 2026-11-15 | The agent link runs engine commands only; window input and screenshots exist only on `--control`. |
| 2026-11-15 | `sshcraft-cli` never opens the data; with the app closed, `mcp --when-closed launch` starts it. |
| 2026-11-15 | `ui.inspect` reports the app's state, not a widget list: the AccessKit tree is not reachable from the frame. |
| 2026-11-15 | A tab is a set of up to 16 panes; a connection lands in the pane it was asked from. Questions name their tab through a task-local session id. |
| 2026-11-15 | The terminal stream wakes the window; nothing polls for output. |
| 2026-11-15 | History and suggestions follow the bytes the window sends and the cursor's line at the first key; any edit key makes a line no history. |
| 2026-11-15 | The session-log viewer plays a log back in a read-only `term` terminal; marks sit on term's own line marks. |
| 2026-11-15 | The account key's QR code is drawn from the key with `qrcode`, the crate the sync crate already uses. |
| 2026-11-15 | The window asks the engine through a runner: a command runs on the engine's runtime and its answer comes back on a later frame; no frame waits for the engine. |
| 2026-11-15 | A new question's answer buttons take a click only 400 ms after it appears; a changed host key is never asked about, the tab shows a warning and nothing to accept. |
| 2026-11-15 | OSC 52 asks per write ("Once", "Always for this host", "No"); "Always" stores the host in `terminal.clipboard_hosts` (at most 1000). Local shells and serial ports always ask. |
| 2026-11-15 | Shortcuts are stored under the Tauri version's action names; actions new in the egui window get new names in `SHORTCUT_ACTIONS`. |
| 2026-11-15 | File dialogs go through craft-dialogs: asked in a frame, shown after it, answered on a later frame, so a native dialog never blocks the event loop. |
| 2026-11-15 | Long lists draw only the rows in view and cache their filtering per data version: the hosts page with 5000 hosts takes 0.2 ms a frame (budget 8 ms, `cargo xtask perf`). |
| 2026-11-15 | The terminal stays on `alacritty_terminal` (Apache-2.0): a trial measured 130 to 170 MiB/s on every kind of output. `term` hides it behind its own types. |
| 2026-11-15 | Terminal text is drawn in runs of narrow cells (ligatures) and wide cells and clusters on their own (the grid stays exact); a full redraw of 200 by 50 cells took 4.8 ms. |
| 2026-11-15 | A frame feeds terminal output in 32 KiB slices until 4 MiB or 6 ms and acknowledges only what it fed; budgets are release-build tests (`cargo xtask perf`). |
| 2026-11-15 | `Session::execute` runs the command on the engine's runtime: the window's frame loop may await it, and dropping the call cancels the command. |
| 2026-11-15 | Agents get the same answer for an unknown and a hidden session log, before the level is looked at. |
| 2026-11-15 | Pasted text loses every `ESC` before it is sent; a paste cannot end the bracket or type a sequence. |
| 2026-11-15 | Per-cell extras of the emulator have a budget of 32 MiB per session; past it the oldest lines lose links and decorations, never their text. |
| 2026-11-15 | Dependabot's `windows-future` 0.100 is not taken alone: it must move with `windows` 0.100, which is not released. |
| 2026-11-14 | For agents, "Ask" exempts only lists and status from the question and the log; reading a session log, searching logs and listing snippets ask and are logged like every other action. |
| 2026-11-14 | A `config.toml` that cannot be read is moved aside by craft-settings; the settings of the Tauri version are then taken over, since nothing of the user's is overwritten. Only a newer `config.toml` blocks the takeover. |
| 2026-11-14 | `Share` and `Sync` are set back to "deny" once (marker `migration.agent_categories`): before R2 no agent action used them, so a stored "allow" must not start to work silently. Categories added after R2 start at "deny". |
| 2026-11-14 | The engine refuses to open while the Tauri version runs; it is recognized by its agent pipe `sshcraft-agent-<SID>` of the same user. No new service may use that prefix. |
| 2026-11-14 | Device settings go to `config.toml` (craft-settings); data that belongs to the account or must stay sealed stays in the database. |
| 2026-11-14 | `Caller` names the user, an agent or the control channel, each agent and control call with the id of its connection; sessions belong to their connection and end with it. |
| 2026-11-14 | `Session::execute` is async: commands that talk to servers or wait for the user do not block a thread. |
| 2026-11-14 | Licences: `deny.toml` allows exactly the list of the house rules, which now includes Unicode-3.0 (the ICU crates of url and idna); four crates outside the list have a named exception (foldhash Zlib, option-ext MPL-2.0, webpki-roots and webpki-root-certs CDLA-Permissive-2.0). |
| 2026-11-14 | Database client as a separate sister app after V1; it reaches server databases through a tunnel SSHCraft opens for it. |
| 2026-11-14 | `fido` is the one crate that may use `unsafe` (WebAuthn and Windows Hello through the Windows API). |
| 2026-11-14 | Agents get no headless mode: the confirmation and Windows Hello need the window, and only one process opens the database. |
| 2026-11-14 | New agent categories start at "deny"; secrets, imports from agent-given paths and the agent rights themselves are never reachable for agents. |
| 2026-11-14 | wasm is checked for `proto` and `model` only; there is no web build. |
| 2026-11-14 | Licence MIT OR Apache-2.0 instead of AGPL-3.0-only; versions up to `tauri-final` stay under AGPL. |
| 2026-11-14 | Rebuild on the Craft conventions: Rust and egui instead of Tauri and React, a new workspace, the core crates moved in. The Tauri version is kept at the tag `tauri-final`. |
| 2026-11-14 | SFTP joins the plan, after the first release. |
| 2026-11-13 | Session logs record output only, never keystrokes; recording is off by default; log sync has its own switch and stays in the personal area. |
| 2026-11-13 | AI agents through MCP and a command line: rights per category, confirmation only in the SSHCraft window, hidden hosts, a sealed audit log written before the action, never key material. |
| 2026-11-12 | The feature set of Termius, improved, with SFTP later; English and German with AUTO. |
| 2026-11-11 | The sync server never reads user data, not even after a takeover: end-to-end encryption, signed records, fingerprint comparison of account keys, rollback protection. |
| 2026-11-11 | The account key is created by the client and kept in Proton Pass; every device has its own key and can be revoked on its own. |
| 2026-11-11 | A changed server fingerprint always aborts the connection, also on reconnect. |
