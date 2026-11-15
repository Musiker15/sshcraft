# Architecture

> **Last updated:** 2026-11-15 · **Change:** R7, platforms and packages

How SSHCraft is built today. Rules for contributors are in [`AGENTS.md`](../AGENTS.md), the
decisions behind it in [`decisions.md`](decisions.md). The German design of the rebuild left the
tree with R7; it is in the history at [`docs/superpowers/specs/`](https://github.com/musiker15/sshcraft/tree/e04ce8846d23cd6e2f528c83a06c03248d5d41f5/docs/superpowers/specs).

## Workspace and layering

A Cargo workspace with `crates/*`, `apps/*` and `xtask`. `cargo xtask layers` enforces the
layering: a crate depends only on lower layers, and nothing below L6 depends on a UI toolkit.

| Layer | Crate | Job |
|---|---|---|
| L0 | `proto` | Account and device keys, device log, area keys, signed and encrypted records, rollback protection |
| L0 | `platform` | Platform folders: the data folder of the app |
| L1 | `model` | Pure data: hosts, groups, identities, key metadata, forwarding rules, agent policy, shell events |
| L2 | `store` | SQLite database (schema 13), vault with the master key in the OS credential store, session logs, history, snippets, agent audit log |
| L2 | `term` | Terminal emulation on `alacritty_terminal`: screen, scrollback, selection, regex search, marks, the escape sequence rules, keys and mouse as bytes |
| L3 | `import` | Import from ssh config, known_hosts, key files, CSV, PuTTY, MobaXterm, SecureCRT; export as CSV and JSON |
| L3 | `ssh` | SSH sessions on russh: authentication, jump hosts, proxies, agent forwarding, tunnels, authorized_keys |
| L3 | `local` | Local terminal (PTY) and serial port (serial2) |
| L3 | `sync` | Client side of sync: accounts, devices, areas, shares, live updates |
| L3 | `server-lib` | The sync server: routes, state, SQLite and PostgreSQL storage, operator commands |
| L4 | `fido` | FIDO2 security keys and Windows Hello; the one crate that may use `unsafe` |
| L5 | `engine` | The session: every action as a command, the agent gate, terminal sessions, forwarding, recording, settings |
| L6 | `ui-egui` | The egui interface; so far the terminal: layout and painting, the widget, the output pump |
| exempt | `apps/sshcraft-server` | The sync server program |
| exempt | `xtask` | `ci`, `fmt`, `clippy`, `test`, `layers`, `assets`, `deny`, `wasm`, `perf` |

Coming with the rebuild: `mcp` (L6), `apps/sshcraft` and `apps/sshcraft-cli`.

## Engine

`sshcraft-engine` is SSHCraft without a window. `Session::open` takes the data folder (an
exclusive lock on it, refused while the Tauri version runs), opens the database and the vault and
reads `config.toml`.

- **Commands.** Every action is a command in `crates/engine/src/cmd/*`: a stable id (`host.save`,
  `session.open`; the list is pinned in `tests/it/command_ids.txt`), a label, parameter
  documentation, `enabled`, `run`, and whether agents may reach it. The interface, the CLI, the
  control channel and MCP all call `Session::execute(caller, id, params)`. It is async and runs
  the command on the engine's own runtime, so any executor may await it; dropping the call
  cancels the command. A command that panics becomes an `Internal` error and the engine goes on. A command called by a program
  never opens a dialog: without a path it fails, only the user gets a dialog.
- **Callers.** `Caller::User`, an agent (`AgentConnection` from `Session::agent_connection`) or
  the control channel (`Session::control_connection`). Everything but the user passes the agent
  gate (see [agents.md](agents.md)); a connection's sessions end with it.
- **Journal.** Successful commands go into an in-memory journal (at most 1000 entries and 1 MiB),
  never with the value of a secret parameter (`password`, `passphrase`, `secret`, `api_key`,
  `account_key`, `answers`, `text`, `data`) or of text the journal should not hold in the clear
  (`content`, `command`). Keystrokes and sign-in answers are not journaled.
- **Event bus.** Data changes, prompts (host key, sign-in, agent confirmation), forwarding and
  sync status, and requests to the interface (a dialog, a link to open) go out on a broadcast
  channel. A subscriber that falls behind loses old events; open prompts can always be listed
  again with `prompt.list`. Without a subscriber no question is asked, and the answer is no.
- **Terminal stream.** `session.open` returns an id; the interface takes the output once with
  `Session::attach_terminal` and acknowledges what it showed. Output not acknowledged counts
  against a high watermark of 512 KiB, so a tab that is never read stops the session from reading
  the server.
- **Settings.** Device settings (language, appearance, terminal, shortcuts, log defaults, serial)
  are in `config.toml` through craft-settings; the settings of the Tauri version were taken over
  once. Data (workspace templates, agent rights and log, history, snippets) stays in the database.

## Terminal

- **`term`** wraps `alacritty_terminal` completely: the renderer and the interface see only
  SSHCraft's own types (`Screen`, `CellView`, `Notice`, `Key`), so the emulator can change
  inside the crate. A size is clamped (2 to 1000 columns, 1 to 500 rows); screen and
  scrollback together hold at most 4 million cells, so a session's grid stays under 128 MiB.
  A prefilter holds an OSC sequence back until it ends and drops one longer than 1.5 MiB. A
  guard between the parser and the emulator keeps at most 16 joining characters per cell, drops
  link targets over 2048 bytes, and holds the cells' extras (joining characters, links,
  underline colours) to 32 MiB by clearing them from the oldest lines first.
- **Drawing** (`ui-egui/src/terminal/`). `layout` turns a screen into rectangles, text and
  lines and is tested without egui; `paint` draws them. Narrow cells of one look become one
  text run, so a font with ligatures joins them; a wide character or a cluster (accents,
  emoji sequences, skin tones) is drawn on its own cells. A character no font has is a box,
  never a panic. The font is the first installed family of `appearance.font_family`, then
  egui's monospace; the system's fallback fonts fill in CJK and emoji.
- **Widget.** `TerminalView` takes keys, IME composition and commit, paste, the mouse
  (selection, wheel, reports to programs that ask, Ctrl+click on links) and returns the bytes
  for the session; it never sends anything itself. Screen readers read the visible text.
- **Pump.** `Pump::frame` feeds 32 KiB slices of a session's output until 4 MiB or 6 ms per
  frame and acknowledges exactly what it fed, so a flood keeps the window responsive and the
  engine reads the server on only as the terminal catches up.
- **Budgets.** `cargo xtask perf` runs `crates/term/tests/budgets.rs` in a release build: a
  feed slice well inside a frame, 1 GiB of `cat`, the memory of a full scrollback.

## Interface

- **App.** `apps/sshcraft` opens the engine (`SessionConfig::for_app`, the real data folder),
  takes the window handle for Windows Hello and security keys, and runs `SshcraftApp` in eframe.
  A second start, or a running Tauri version, shows why and stops.
- **Frame.** `SshcraftApp::frame` takes the answers of earlier commands, the engine's events
  (data changed, questions, links to open) and the answers of file dialogs; fetches what is
  stale; applies tokens, theme and language; runs the shortcuts; draws the shell (sidebar, tabs,
  page), the palette, the questions and the messages. File dialogs show after the frame
  (`show_dialogs`).
- **Window frame and menus.** craft-window decides who draws the title bar
  (`policy::decide_for` with the prefix `SSHCRAFT`): the app's own bar by default, the system's
  with Settings, General, Use system title bar (read at start) and on tiling window managers.
  `window.rs` draws the strip: the menu bar (craft-menu, `menus.rs`, built from the `ui.*`
  commands with their current shortcuts) and, where the app draws the frame, the window buttons.
  On macOS the menu bar is the native one (`craft_menu::mac`). Nothing runs from a menu while a
  dialog, a question or the palette is open.
- **Quitting.** The close button, the system's close and File, Quit end in `quit.rs`, which runs
  in `logic` (also while the window is hidden): with sessions running it asks first, and
  craft-quit then quits the platform's way (on macOS through AppKit).
- **Fonts.** `build.rs` embeds craft-fonts' interface faces when `CRAFT_FONTS_DIR` names a
  checkout (`fonts.rs`); they come after egui's faces and before the system's. Without a checkout
  the table is empty.
- **Runner.** Pages never wait for the engine: `Runner::run` spawns `Session::execute` on the
  engine's runtime and hands the result to a continuation on a later frame.
- **Data.** `Data` caches hosts, groups, keys, identities and settings; an engine event marks a
  part stale, and one fetch per part runs at a time. Its `version` lets pages cache what they
  derive (the hosts page's filtered lists).
- **Pages** (`ui-egui/src/pages/`): hosts and groups with inherited settings, keys and identities
  with the agent's keys and Proton Pass, import with a preview and export, settings. Forms are
  modal windows of fixed height: a window that changes its height loses a click.
- **Sessions** (`session_tab.rs`, `prompts.rs`): one tab per session with its `Terminal` and
  `Pump`; the terminal's answers go back to the session, titles become the tab's, clipboard
  writes need consent per host, links go through `link.open`. Host key and sign-in questions
  show one at a time over the window.
- **Panes** (`panes.rs`, `state::Panes`): every tab holds 1 to 16 panes in a near-square grid or
  one large with the others listed; dividers resize, title bars swap and detach, tabs merge.
  Broadcast and status dots (`status.rs`) sit on the panes.
- **More pages:** port forwarding, snippets (with the sidebar beside the terminal), session logs
  (`log_viewer.rs`), Settings > Sync (accounts, devices, shares) and Settings > AI agents. Typed
  lines go through `history.rs` and `completion.rs`; templates through `pages/templates.rs`.
- **Control from outside** (R6): `sshcraft-link` (L1) is the wire (JSON lines on loopback,
  challenge sign-in, token and connection files). The engine serves agents on it
  (`agent_link.rs`, started by the window as the consent says, `link.rs`); `crates/mcp` (L6) is
  the MCP server over stdio, `apps/sshcraft-cli` the command line; both talk to the running app and
  never open its data. `--control` adds the developer's control channel (`apps/sshcraft/src/control.rs`,
  answered in the window by `ui-egui/src/control.rs`).
- **Words and keys.** `tr` and `trf` translate English texts to German (`i18n/de.rs`; a test
  fails for a text without German). Shortcuts are craft-keymap chords; inside a terminal only
  Ctrl+Shift and Ctrl+Tab are taken.
- **Tests and budgets.** `sshcraft_ui_egui::testing` runs the app in egui_kittest on a test
  engine; `crates/ui-egui/tests/budgets.rs` holds the memory of the empty app and the frame time
  of 5000 hosts (release, `cargo xtask perf`).

## Data

- The database lives in the folder `sshcraft_platform::data_dir()` returns (on Windows
  `%APPDATA%\musiker15\SSHCraft\data\sshcraft.db`), the one the Tauri version used.
- Private keys and passwords are stored only as XChaCha20-Poly1305 ciphertext; the master key is
  in the operating system's credential store under `de.musiker15.sshcraft`. When it is missing
  while keys exist, no new one is created.
- Tests pin the folder, the service name and schema 13; older databases migrate on open.

## Sync

The client encrypts and signs every record before it leaves the device; the server stores and
relays them and can read none of them. See [protocol.md](protocol.md) and [sync.md](sync.md).

## Platforms and builds

Windows, macOS, Linux and FreeBSD. CI runs the gate on Ubuntu and Windows for every pull
request, on macOS on `main` and for pull requests with the label `full-ci`, FreeBSD in a VM when
its files change. `cargo xtask linux` runs clippy and the tests in a Linux container from any
machine. The packages (`packaging/`, adapted from gridcraft) and how a release is made are in
[releasing.md](releasing.md). The sync server builds as a program and as a Docker image
(`packaging/docker/Dockerfile`).
