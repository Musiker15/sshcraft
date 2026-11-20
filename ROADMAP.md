# SSHCraft roadmap

> **Last updated:** 2026-11-20 · **Stage:** v0.4.0; S1 (SFTP) done

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
| R5 | Interface II: workspaces, forwarding, snippets, logs, sync, sharing, agent confirmation | done |
| R6 | Control channel, MCP crate, `sshcraft-cli` | done |
| R7 | macOS, Linux, FreeBSD, packaging, CI | done |
| F1 | Connection and configuration: ProxyCommand, reconnect, live `~/.ssh/config`, host keys | done |
| F2 | Sign-in and data safety: password manager references, TOTP, app lock, trash, backup | done |
| V1 | First release v0.1.0: packages for Windows, Linux and FreeBSD, server program, SBOM; unsigned and without macOS for now | done |
| M1 | Android tablets: the interface with a key bar, Keystore, sync, APK from `cargo xtask android` | done |
| M2, M3 | Android phones and GameActivity; iOS and iPadOS | later |
| S1 | SFTP: this computer and the host side by side, transfers, folders, permissions | done |
| F3 | Terminal comfort: shell integration, notifications, triggers, hints, compose, snippet placeholders, login scripts, program status | done |
| F4 | Connections and organisation: shared connections, Wake-on-LAN and knocking, X11, Unix sockets, `ssh://` links, quick connect, bulk edit, reachability, desktop shortcuts, server load | done |
| F5 | Persistent sessions: tmux and screen attached again on every connect, tmux windows as tabs, Telnet | done |
| F6 | Terminal extras: hex view, dropdown window on a global shortcut; no images and no ZMODEM or trzsz | done |
| F7 | Targets and automation: containers as targets, launch actions, macros, dynamic groups; no Kerberos and no PKCS#11 | done |

A database client is planned as a separate sister app after V1; it reaches server databases
through a tunnel that SSHCraft opens for it.
