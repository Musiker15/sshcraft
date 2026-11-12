# Stollen

Self-hosted SSH client with end-to-end encrypted sync and native Proton Pass support.

Stollen is an SSH client for Windows, Android and iOS. Hosts, groups and keys can be synced
through your own server, encrypted on the device so the server never sees them. On the desktop,
keys can come straight from the Proton Pass SSH agent.

**Status:** early development. Nothing here is ready for daily use yet.

## Quick start

```bash
cd app
npm install
npm run tauri dev
```

On Windows, Stollen uses the SSH agent of the Proton Pass desktop app directly, so keys stored
there work without importing them. `pass-cli` is only needed to import a key into Stollen's own
encrypted vault.

## Documentation

| Topic | Link |
|---|---|
| Design (German) | [docs/superpowers/specs/2026-11-11-stollen-design.md](docs/superpowers/specs/2026-11-11-stollen-design.md) |
| Roadmap (German) | [docs/superpowers/plans/2026-11-11-stollen-roadmap.md](docs/superpowers/plans/2026-11-11-stollen-roadmap.md) |

## Help and contributing

Questions and bug reports go to the GitHub issues of this repository.

## License

[AGPL-3.0](LICENSE)
