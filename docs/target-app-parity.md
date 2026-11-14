# Target app parity: Termius

> **Last updated:** 2026-11-14 · **Target:** Termius (desktop)

Where SSHCraft stands against Termius, area by area. "Tauri version only" means the feature exists
at the tag `tauri-final` and returns with the rebuilt interface (R4 and R5). The full comparison
with other clients, with sources, is
[`superpowers/specs/2026-11-14-sshcraft-marktvergleich.md`](superpowers/specs/2026-11-14-sshcraft-marktvergleich.md).

| Area | Termius | SSHCraft |
|---|---|---|
| Connections: key, password, keyboard-interactive, identities, jump hosts, proxies, agent forwarding, environment | yes | core crates on `main`; interface: Tauri version only |
| Local terminal and serial | yes | core crate on `main`; interface: Tauri version only |
| Terminal, tabs, command palette, new tab page | yes | Tauri version only (R3, R4) |
| Workspaces: split, focus, broadcast, templates | yes | Tauri version only (R5) |
| Hosts and groups: nesting, tags, icons, import, export | yes | core crates on `main`; interface: Tauri version only |
| Keys: generate, import, passphrase, authorized_keys, certificates, FIDO2, device-bound keys | yes | core crates on `main`; interface: Tauri version only |
| Port forwarding: local, remote, dynamic | yes | core crate on `main`; interface: Tauri version only |
| Snippets, history, completion | yes | core crate on `main`; interface: Tauri version only |
| Session logs | yes | core crate on `main`; interface: Tauri version only |
| Sync | Termius cloud, subscription | own server, end-to-end encrypted, free; server and client crates on `main` |
| Sharing | team plans | free on the own server; client crate on `main` |
| AI agents | AI features in the app | MCP and command line with rights per action (R6) |
| SFTP | yes | S1, after the first release |
| Mobile | yes | M1 to M3, after the first release |
