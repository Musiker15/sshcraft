# SSHCraft roadmap

> **Last updated:** 2026-11-15 · **Stage:** rebuild on the Craft conventions, R5 of R7 next

SSHCraft reached its first feature set (sub-plans 1 to 15: hosts, keys, connections, workspaces,
port forwarding, snippets, session logs, sync, sharing, AI agents) as a Tauri app, kept at the tag
`tauri-final`. It is now being rebuilt in Rust and egui on the conventions of the Craft apps. The
detail is in [docs/roadmap.md](docs/roadmap.md), the open work in [docs/gaps.md](docs/gaps.md).

| Step | What | State |
|---|---|---|
| R1 | Scaffold and rules: workspace, licence, AGENTS.md, xtask, lints, core crates moved | done |
| R2 | Engine: session, command registry, agent gate, crash guard | done |
| R3 | Terminal: emulation crate and egui renderer | done |
| R4 | Interface I: window, sidebar, tabs, palette, tokens, languages, hosts and keys | done |
| R5 | Interface II: workspaces, forwarding, snippets, logs, sync, sharing, agent confirmation | open |
| R6 | Control channel, MCP crate, `sshcraft-cli` | open |
| R7 | macOS, Linux, FreeBSD, packaging, CI | open |
| F1 | Connection and configuration: ProxyCommand, reconnect, live `~/.ssh/config`, host keys | open |
| F2 | Sign-in and data safety: password manager references, TOTP, app lock, trash, backup | open |
| V1 | First release: signed packages, winget, Homebrew, Flathub, server program and image | open |
| M1 to M3 | Android, iOS and iPadOS | later |
| S1 | SFTP | after V1 |
| F3 to F7 | Terminal comfort, connections, persistent sessions, terminal extras, targets and automation | after V1 |

A database client is planned as a separate sister app after V1; it reaches server databases
through a tunnel that SSHCraft opens for it.
