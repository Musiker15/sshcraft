# Architecture

> **Last updated:** 2026-11-14 · **Change:** R2, the engine

How SSHCraft is built today. Rules for contributors are in [`AGENTS.md`](../AGENTS.md); the
design of the rebuild is
[`superpowers/specs/2026-11-14-sshcraft-craft-neuaufbau-design.md`](superpowers/specs/2026-11-14-sshcraft-craft-neuaufbau-design.md).

## Workspace and layering

A Cargo workspace with `crates/*`, `apps/*` and `xtask`. `cargo xtask layers` enforces the
layering: a crate depends only on lower layers, and nothing below L6 depends on a UI toolkit.

| Layer | Crate | Job |
|---|---|---|
| L0 | `proto` | Account and device keys, device log, area keys, signed and encrypted records, rollback protection |
| L0 | `platform` | Platform folders: the data folder of the app |
| L1 | `model` | Pure data: hosts, groups, identities, key metadata, forwarding rules, agent policy, shell events |
| L2 | `store` | SQLite database (schema 11), vault with the master key in the OS credential store, session logs, history, snippets, agent audit log |
| L3 | `import` | Import from ssh config, known_hosts, key files, CSV, PuTTY, MobaXterm, SecureCRT; export as CSV and JSON |
| L3 | `ssh` | SSH sessions on russh: authentication, jump hosts, proxies, agent forwarding, tunnels, authorized_keys |
| L3 | `local` | Local terminal (PTY) and serial port (serial2) |
| L3 | `sync` | Client side of sync: accounts, devices, areas, shares, live updates |
| L3 | `server-lib` | The sync server: routes, state, SQLite and PostgreSQL storage, operator commands |
| L4 | `fido` | FIDO2 security keys and Windows Hello; the one crate that may use `unsafe` |
| L5 | `engine` | The session: every action as a command, the agent gate, terminal sessions, forwarding, recording, settings |
| exempt | `apps/sshcraft-server` | The sync server program |
| exempt | `xtask` | `ci`, `fmt`, `clippy`, `test`, `layers`, `assets`, `deny`, `wasm` |

Coming with the rebuild: `term` (L2, terminal emulation), `ui-egui` and `mcp` (L6),
`apps/sshcraft` and `apps/sshcraft-cli`.

## Engine

`sshcraft-engine` is SSHCraft without a window. `Session::open` takes the data folder (an
exclusive lock on it, refused while the Tauri version runs), opens the database and the vault and
reads `config.toml`.

- **Commands.** Every action is a command in `crates/engine/src/cmd/*`: a stable id (`host.save`,
  `session.open`; the list is pinned in `tests/it/command_ids.txt`), a label, parameter
  documentation, `enabled`, `run`, and whether agents may reach it. The interface, the CLI, the
  control channel and MCP all call `Session::execute(caller, id, params)`. It is async; a command
  that panics becomes an `Internal` error and the engine goes on. A command called by a program
  never opens a dialog: without a path it fails, only the user gets a dialog.
- **Callers.** `Caller::User`, an agent (`AgentConnection` from `Session::agent_connection`) or
  the control channel (`Session::control_connection`). Everything but the user passes the agent
  gate (see [agents.md](agents.md)); a connection's sessions end with it.
- **Journal.** Successful commands go into an in-memory journal (at most 1000 entries and 1 MiB),
  never with the value of a secret parameter (`password`, `passphrase`, `secret`, `api_key`,
  `account_key`, `answers`, `text`, `data`). Keystrokes and sign-in answers are not journaled.
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

## Data

- The database lives in the folder `sshcraft_platform::data_dir()` returns (on Windows
  `%APPDATA%\musiker15\SSHCraft\data\sshcraft.db`), the one the Tauri version used.
- Private keys and passwords are stored only as XChaCha20-Poly1305 ciphertext; the master key is
  in the operating system's credential store under `de.musiker15.sshcraft`. When it is missing
  while keys exist, no new one is created.
- Tests pin the folder, the service name and schema 11.

## Sync

The client encrypts and signs every record before it leaves the device; the server stores and
relays them and can read none of them. See [protocol.md](protocol.md) and [sync.md](sync.md).

## Platforms and builds

Windows first; macOS, Linux and FreeBSD follow in R7. The sync server builds as a program and as a
Docker image (`packaging/docker/Dockerfile`).
