# Decisions

The decisions that carry SSHCraft, newest first. The reasoning is in the German design documents
of the rebuild, which left the tree with R7 and are in the history at
[`docs/superpowers/specs/`](https://github.com/musiker15/sshcraft/tree/e04ce8846d23cd6e2f528c83a06c03248d5d41f5/docs/superpowers/specs): the design of the rebuild
(`2026-11-14-sshcraft-craft-neuaufbau-design.md`) and the comparison with other clients
(`2026-11-14-sshcraft-marktvergleich.md`).

| Date | Decision |
|---|---|
| 2026-11-17 | Android runs the egui interface in a NativeActivity, and the APK is put together by `cargo xtask android` from the SDK's tools, without Gradle or Kotlin. A NativeActivity loses text a keyboard commits at once (its clipboard, swiped words), so `MainActivity` adds a text connection in a small Java class that only hands the text to Rust (decided with Moritz after the first test on his tablet). GameActivity (screen readers) waits for M2. The master key is kept by keyring-core's Android store, encrypted with a Keystore key; app data is excluded from Android backup. HTTPS uses Mozilla's roots there, since the platform verifier needs Java. |
| 2026-11-15 | A password reference is used for an address only after it was allowed on this device, like a proxy command; copies in a share never carry one. |
| 2026-11-15 | A TOTP secret is a sealed password row and follows the rules of passwords; a stored password no longer answers a question for a one-time code. |
| 2026-11-15 | The trash keeps rows with their sealed parts as they were and restores them under their ids; what sync deletes goes there too. Versions come from saves in SSHCraft, at most 20 per item. |
| 2026-11-15 | The backup file is opened by a passphrase alone (Argon2id, XChaCha20-Poly1305), so it works without the keyring; restoring adds only what is missing. |
| 2026-11-15 | The app lock unlocks with Windows Hello and is not offered where there is none; Touch ID waits until it can be tried on a Mac. |
| 2026-11-15 | A proxy command is an argument list split by `sh` rules, never a shell; a line that needs one is refused. It runs only after it was allowed on this device, by its exact command line: entering it in a form or importing the own `~/.ssh/config` allows it; sync, shares, imports and agents never do, and saving a host for another reason does not. Host values that start with `-` never reach it. Agents cannot set one or aim one. |
| 2026-11-15 | Reconnect waits 1, 2, 4 … 32 seconds, then a minute; terminals up to a setting (10), forwards without a limit. Sleep and network changes are found by reading the clocks and the interface addresses every five seconds, not by system notifications. |
| 2026-11-15 | Schema 12: several host keys per server with the key itself; `UpdateHostKeys` keeps a new key only after the server proved it, and only from the target, not from jump hosts. A `known_hosts` import never adds a key to a server SSHCraft knows. |
| 2026-11-15 | `~/.ssh/config` is kept in step only when the user turns it on; a host that leaves the file stays in SSHCraft. Writing back happens only on request, edits only the managed lines of the host's own block and keeps a backup. |
| 2026-11-15 | The app draws its own title bar and menu bar (craft-window, craft-menu) unless the user picks the system's; while a terminal has the keyboard the menu bar takes no keys. |
| 2026-11-15 | Quitting with running sessions asks first, also from the menu and the system's close. |
| 2026-11-15 | Packages install into `Programs\SSHCraft` or `Program Files\SSHCraft` (not an ArtCraft suite folder), register no file types and no `ssh://` handler, and put `sshcraft-cli` on `PATH`. |
| 2026-11-15 | The German design documents leave the tree; the English docs carry the decisions, the history keeps the documents. |
| 2026-11-15 | Agents and the control channel share one wire (`sshcraft-link`) with challenge sign-in only; the token never crosses it. |
| 2026-11-15 | The agent link runs engine commands only; window input and screenshots exist only on `--control`. |
| 2026-11-15 | `sshcraft-cli` never opens the data; with the app closed, `mcp --when-closed launch` starts it. |
| 2026-11-15 | `ui.inspect` reports the app's state, not a widget list: the AccessKit tree is not reachable from the frame. |
| 2026-11-18 | `ui.inspect` lists the widgets after all: an egui plugin keeps the AccessKit tree of each finished frame, so the list is that of the frame on the screen. It gives names and places, never a field's value. |
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
