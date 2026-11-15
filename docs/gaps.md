# Gaps

> **Last updated:** 2026-11-15

The ranked list of open work. Pick from the top. When work lands, remove or update its row.

| Rank | Gap | Where it is planned |
|---|---|---|
| 1 | There is no desktop app on `main`: the engine (R2) and the terminal (R3) are there, the interface is rebuilt in R4 and R5; until then `cargo run -p sshcraft-ui-egui --example terminal_window` shows a local shell | [roadmap.md](roadmap.md) |
| 2 | Agents and the command line have no transport until the control channel and the MCP crate (R6) | R6 |
| 3 | Only Windows is built and tested; macOS, Linux and FreeBSD follow | R7 |
| 4 | The agent confirmation is a prompt on the event bus; the window that answers it comes in R5 | R5 |
| 4a | The window polls a session's output 60 times a second while a session is open; the terminal stream has no way yet to wake it when output arrives | R5 |
| 4c | The scrollback length is fixed at 10 000 lines; a setting for it | R5 |
| 4d | Ligatures come from the font alone (Cascadia Code has them, the default Cascadia Mono not); a switch to turn them off in a font that has them | R5 |
| 5 | No reconnect with backoff, no ProxyCommand, no host key management | F1 |
| 6 | No trash, no versions, no backup file | F2 |
| 7 | No SFTP | S1 |
