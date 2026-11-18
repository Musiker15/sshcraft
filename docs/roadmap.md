# Roadmap

> **Last updated:** 2026-11-16

The one-page summary is [ROADMAP.md](../ROADMAP.md). The sub-plans of the rebuild and what comes
after, with the source of each (the German design of the rebuild, sections 13 and 16, and the
comparison with other clients, both in the history at [`docs/superpowers/specs/`](https://github.com/musiker15/sshcraft/tree/e04ce8846d23cd6e2f528c83a06c03248d5d41f5/docs/superpowers/specs)).

## Current focus

The rebuild (R1 to R7) is done: SSHCraft builds and tests on Windows, Linux and macOS in CI and
on FreeBSD in a VM, has the integrated title bar and menus of the Craft apps, the fonts from
craft-fonts, and packages for every platform ([releasing.md](releasing.md)). F1 (connection and
configuration), F2 (sign-in and data safety) and the first release (V1, v0.1.0 on 2026-11-16)
are done. v0.1.0 has packages for Windows, Linux and FreeBSD and the sync server, unsigned and
without macOS ([releasing.md](releasing.md#what-gets-built), gap 7a); winget and Flathub
submissions follow. Next is S1 (SFTP).

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
| F3 | Shell integration (OSC 133) with gutter, jumps and output copy; desktop notifications; highlights and triggers; colour, badge and theme per host; hint mode; paste history and compose field; snippet placeholders and bar; login scripts with per-device approval; program status (OSC 7501) |
| F4 | One shared connection per host; Wake-on-LAN, port knocking and programs before and after connecting; X11 forwarding; forwarding to Unix sockets; `ssh://` links and quick connect; changing many hosts at once; reachability in the host list; desktop shortcuts; server load under the terminal; shared hosts that ask for the member's own sign-in |
| F5 | tmux and screen sessions attached again on every connect; tmux control mode with windows as tabs; Telnet hosts; Mosh and Eternal Terminal left out |
| F6 | Hex view of what a session received; dropdown window on a global shortcut; images and file transfer through the terminal left out |
| F7 | Containers on a host as targets (Docker, Podman, Kubernetes); launch actions typed once when a session starts, allowed per device; macros from steps, run from the palette; dynamic groups from saved filters; Kerberos and PKCS#11 left out |

## After the first release

| Step | Result |
|---|---|
| M1 | Android tablets (13+, arm64): the egui interface in a NativeActivity with a key bar, touch scrolling and Android's clipboard; the master key in the Android Keystore; sync; the APK built and signed by `cargo xtask android` ([android.md](android.md)) |

M2 (GameActivity, phones, notifications), M3 (iOS and iPadOS), S1 (SFTP). A database client follows as a separate sister app.
