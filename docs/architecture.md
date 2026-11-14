# Architecture

> **Last updated:** 2026-11-14 · **Change:** R1, the workspace on the Craft conventions

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
| exempt | `apps/sshcraft-server` | The sync server program |
| exempt | `xtask` | `ci`, `fmt`, `clippy`, `test`, `layers`, `assets`, `deny`, `wasm` |

Coming with the rebuild: `term` (L2, terminal emulation), `engine` (L5, session and command
registry), `ui-egui` and `mcp` (L6), `apps/sshcraft` and `apps/sshcraft-cli`.

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
