# SSHCraft

Self-hosted SSH client with end-to-end encrypted sync, written in Rust.

SSHCraft is an SSH client for Windows, macOS, Linux and FreeBSD, with Android and iOS to follow.
Hosts, groups and keys can be synced through your own server, encrypted on the device so the
server never sees them, and shared with other people on the same server. On the desktop, keys can
come straight from the Proton Pass SSH agent. The sync server `sshcraft-server` is a single program
or Docker image with SQLite or PostgreSQL behind your own reverse proxy.

It follows the architecture and conventions of the other Craft apps: layered crates, an engine
where everything is a command, an egui front end, a control channel and an MCP server for AI
agents, with rights per action and a confirmation in the app.

**Status:** rebuild in progress. The desktop app is being rebuilt on the Craft conventions (see
[ROADMAP.md](ROADMAP.md)); until then there is no desktop app on `main`. The sync server works.

## Quick start

The sync server, from the repository root:

```bash
cargo run -p sshcraft-server -- serve --public-origin https://sync.example.org
```

## Documentation

| Topic | Link |
|---|---|
| Roadmap | [ROADMAP.md](ROADMAP.md), [docs/roadmap.md](docs/roadmap.md) |
| Architecture | [docs/architecture.md](docs/architecture.md) |
| Decisions | [docs/decisions.md](docs/decisions.md) |
| Rules for contributors and agents | [AGENTS.md](AGENTS.md) |
| Running the sync server | [docs/server.md](docs/server.md) |
| Sync server API | [docs/server-api.md](docs/server-api.md) |
| Protocol and cryptography | [docs/protocol.md](docs/protocol.md) |
| Sync on the device | [docs/sync.md](docs/sync.md) |
| Hosts, groups, import and export | [docs/hosts.md](docs/hosts.md) |
| Connections: passwords, identities, jump hosts, proxies, local and serial | [docs/connections.md](docs/connections.md) |
| Keys: types, passphrase, certificates, authorized_keys, FIDO2, Windows Hello | [docs/keys.md](docs/keys.md) |
| Workspaces, broadcast, templates and terminal settings | [docs/workspaces.md](docs/workspaces.md) |
| Port forwarding: local, remote, SOCKS | [docs/forwarding.md](docs/forwarding.md) |
| Snippets, history and completion | [docs/snippets.md](docs/snippets.md) |
| Session logs | [docs/session-logs.md](docs/session-logs.md) |
| AI agents | [docs/agents.md](docs/agents.md) |
| Keyboard shortcuts | [docs/shortcuts.md](docs/shortcuts.md) |
| Security | [SECURITY.md](SECURITY.md), [docs/security.md](docs/security.md) |
| Glossary | [GLOSSARY.md](GLOSSARY.md) |

## Help and contributing

Questions and bug reports go to the GitHub issues of this repository. Before you contribute, read
[AGENTS.md](AGENTS.md); `cargo xtask ci` is the gate every change passes.

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
Versions up to the tag `tauri-final` were published under AGPL-3.0-only.
