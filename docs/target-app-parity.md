# Target app parity: Termius

> **Last updated:** 2026-11-17 · **Target:** Termius (desktop)

Where SSHCraft stands against Termius, area by area, after the rebuild (R1 to R7). The full comparison
with other clients, with sources, is the German
[`2026-11-14-sshcraft-marktvergleich.md`](https://github.com/musiker15/sshcraft/blob/e04ce8846d23cd6e2f528c83a06c03248d5d41f5/docs/superpowers/specs/2026-11-14-sshcraft-marktvergleich.md) in the history.

| Area | Termius | SSHCraft |
|---|---|---|
| Connections: key, password, keyboard-interactive, identities, jump hosts, proxies, agent forwarding, environment | yes | yes |
| ProxyCommand, reconnect with backoff after sleep and network changes | no ProxyCommand (ignored on import), reconnect | yes, without a shell and only after it was allowed on the device |
| `~/.ssh/config` kept in step (with `Include`) and written back; host key management with `UpdateHostKeys` | one-time import | yes |
| Passwords from a password manager, TOTP | no | yes (Proton Pass, 1Password, Bitwarden, KeePassXC) |
| Trash, earlier versions, encrypted backup file | no | yes |
| App lock | yes (PIN, biometrics) | Windows Hello; Touch ID later |
| Local terminal and serial | yes | yes |
| Terminal, tabs, command palette, new tab page, menus | yes | yes |
| Workspaces: split, focus, broadcast, templates | yes | yes |
| Hosts and groups: nesting, tags, icons, import, export | yes | yes |
| Keys: generate, import, passphrase, authorized_keys, certificates, FIDO2, device-bound keys | yes | yes (FIDO2 and device-bound keys on Windows) |
| Port forwarding: local, remote, dynamic | yes | yes, and to Unix sockets on the server |
| Shared connection per host, Wake-on-LAN, port knocking, programs before and after connecting, X11 forwarding | not compared yet | yes (F4); programs only after they were allowed on the device |
| Quick connect, `ssh://` links, desktop shortcuts for hosts | yes | yes (F4); links only fill the dialog, not on macOS yet |
| Changing many hosts at once, reachability in the host list, server load under the terminal | yes | yes (F4); reachability and load off by default |
| Shared hosts that ask each member for their own sign-in | team plans | yes |
| Sessions that stay (tmux, screen), tmux windows as tabs, Telnet | Mosh, Telnet | yes (F5); no Mosh and no Eternal Terminal |
| Dropdown window on a global shortcut, hex view of received bytes | not compared yet | yes (F6); no images and no ZMODEM or trzsz |
| Containers on a host as targets (Docker, Podman, Kubernetes), launch actions, macros, dynamic groups in the host list | not compared yet | yes (F7); launch actions only after they were allowed on the device; no Kerberos and no PKCS#11 |
| Snippets, history, completion | yes | yes, with placeholders and a snippet bar |
| Shell integration (command marks and exit codes), notification when a command ends, triggers and highlights, hint mode, compose field, login scripts, program status of agents | not compared yet | yes (F3); login scripts only after they were allowed on the device |
| Colour, badge and theme per host or group | not compared yet | yes |
| Session logs | yes | yes |
| Sync | Termius cloud, subscription | own server, end-to-end encrypted, free |
| Sharing | team plans | free on the own server |
| AI agents | AI features in the app | MCP and command line with rights per action |
| Platforms | Windows, macOS, Linux | Windows, Linux, FreeBSD since v0.1.0 (unsigned); macOS builds, not released yet |
| SFTP | yes | S1, after the first release |
| Mobile | yes | Android tablets since M1 (sideloaded APK); phones M2, iOS and iPadOS M3 |
