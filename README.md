# SSHCraft

Self-hosted SSH client with end-to-end encrypted sync and native Proton Pass support.

SSHCraft is an SSH client for Windows, macOS, Linux, FreeBSD, Android, iOS and iPadOS. Hosts,
groups and keys can be synced through your own server, encrypted on the device so the server
never sees them, and shared with other people on the same server. On the desktop, keys can come straight from the Proton Pass SSH agent. The app
speaks English and German and follows the system language unless you pick one. The sync server
`sshcraft-server` is a single program or Docker image with SQLite or PostgreSQL behind your own
reverse proxy.

**Status:** early development. Nothing here is ready for daily use yet.

## Quick start

```bash
cd app
npm install
npm run tauri dev
```

On Windows, SSHCraft uses the SSH agent of the Proton Pass desktop app directly, so keys stored
there work without importing them. `pass-cli` is only needed to import a key into SSHCraft's own
encrypted vault.

## Documentation

| Topic | Link |
|---|---|
| Design (German) | [docs/superpowers/specs/2026-11-11-sshcraft-design.md](docs/superpowers/specs/2026-11-11-sshcraft-design.md) |
| Roadmap (German) | [docs/superpowers/plans/2026-11-11-sshcraft-roadmap.md](docs/superpowers/plans/2026-11-11-sshcraft-roadmap.md) |
| Protocol and cryptography | [docs/protocol.md](docs/protocol.md) |
| Hosts, groups, import and export | [docs/hosts.md](docs/hosts.md) |
| Connections: passwords, identities, jump hosts, proxies, local and serial | [docs/connections.md](docs/connections.md) |
| Keys: types, passphrase, certificates, authorized_keys, FIDO2, Windows Hello | [docs/keys.md](docs/keys.md) |
| Sync on the device | [docs/sync.md](docs/sync.md) |
| Running the sync server | [docs/server.md](docs/server.md) |
| Sync server API | [docs/server-api.md](docs/server-api.md) |
| Workspaces, broadcast, templates and terminal settings | [docs/workspaces.md](docs/workspaces.md) |
| Keyboard shortcuts | [docs/shortcuts.md](docs/shortcuts.md) |
| Security notes | [docs/security.md](docs/security.md) |

## Help and contributing

Questions and bug reports go to the GitHub issues of this repository.

## License

[AGPL-3.0](LICENSE)
