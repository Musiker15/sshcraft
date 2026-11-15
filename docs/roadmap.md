# Roadmap

> **Last updated:** 2026-11-15

The one-page summary is [ROADMAP.md](../ROADMAP.md). The sub-plans of the rebuild and what comes
after, with the source of each (the German design of the rebuild, sections 13 and 16, and the
comparison with other clients, both in the history at [`docs/superpowers/specs/`](https://github.com/musiker15/sshcraft/tree/e04ce8846d23cd6e2f528c83a06c03248d5d41f5/docs/superpowers/specs)).

## Current focus

The rebuild (R1 to R7) is done: SSHCraft builds and tests on Windows, Linux and macOS in CI and
on FreeBSD in a VM, has the integrated title bar and menus of the Craft apps, the fonts from
craft-fonts, and packages for every platform ([releasing.md](releasing.md)). F1 (connection and
configuration) and F2 (sign-in and data safety) are done. The first release (V1) is being
prepared: the release workflow also packs the sync server, `cargo xtask sbom` makes the bill of
materials and `cargo xtask stores` the winget and Homebrew entries. Moritz installs and tries
the packages before anything is published.

## The rebuild

| Step | Result |
|---|---|
| R1 | Workspace, MIT OR Apache-2.0, AGENTS.md with the house rules, xtask, lints; the Tauri app removed; the core crates moved, without panics |
| R2 | Engine: `Session`, command registry for every action of the Tauri backend, agent gate, crash guard and fuzz tests, settings through craft-settings |
| R3 | Terminal: a short throwaway trial first, then `term` on `alacritty_terminal` and the egui renderer |
| R4 | Interface I: window, sidebar, tabs, command palette, tokens and themes, languages, shortcuts; hosts, groups, keys, identities, import and export, connecting |
| R5 | Interface II: workspaces, broadcast, port forwarding, snippets and completion, session logs, sync, sharing, devices, agent settings and confirmation |
| R6 | Control channel, MCP crate, `sshcraft-cli`, ArtCraft Launcher connection |
| R7 | macOS, Linux, FreeBSD; packaging; CI; `docs/superpowers/` leaves the tree, its links point to the English docs and the history |

## Before the first release

| Step | Result |
|---|---|
| F1 | ProxyCommand; reconnect with backoff and rebuilt tunnels; `~/.ssh/config` read live and written on request, with `Include`; host key management with `UpdateHostKeys`; a strict-kex test |
| F2 | Passwords as references to a password manager; TOTP; desktop app lock; trash and versions; encrypted backup file; the list of connections SSHCraft makes |
| V1 | Signed packages for every desktop target, checksums, SBOM, winget, Homebrew, Flathub, server program, Docker image |

## After the first release

M1 to M3 (Android, iOS and iPadOS), S1 (SFTP), F3 (terminal comfort), F4 (connections and
organisation), F5 (persistent sessions, tmux, Mosh, Eternal Terminal, Telnet), F6 (terminal
extras), F7 (targets and automation). A database client follows as a separate sister app.
