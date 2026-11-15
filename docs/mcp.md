# MCP

`sshcraft-cli mcp` serves the Model Context Protocol on stdin and stdout (version `2025-06-18`;
a client asking for `2025-03-26` or `2024-11-05` gets that one). It never opens SSHCraft's data
and listens on no port: it works in the open SSHCraft window through the agent link
([control-protocol.md](control-protocol.md)), and only while the user lets agents connect.
Setting it up and the rights agents have: [agents.md](agents.md).

## Tools

| Tool | Engine command |
|---|---|
| `execute_command {id, params?}` | any command agents may use (see `list_commands`) |
| `list_commands` | the commands agents may use, with their parameters |
| `list_windows` | the window |
| `sshcraft_list_hosts`, `sshcraft_list_groups`, `sshcraft_list_keys` | `host.list`, `group.list`, `key.list` |
| `sshcraft_host_info {host}` | `host.info` |
| `sshcraft_exec {host, command, timeout_secs?}` | `host.exec` |
| `sshcraft_session_open {host}` | `session.open` |
| `sshcraft_session_write {session, data}` | `session.send` |
| `sshcraft_session_read {session, wait_ms?}` | `session.read` |
| `sshcraft_session_close {session}` | `session.close` |
| `sshcraft_save_host {host}`, `sshcraft_save_group {group}` | `host.save`, `group.save` |
| `sshcraft_delete_host {host}`, `sshcraft_delete_group {group}`, `sshcraft_delete_key {key}` | `host.delete`, `group.delete`, `key.delete` |
| `sshcraft_generate_key {name, type}` | `key.generate` |
| `sshcraft_import_key {name, text, passphrase?}` | `key.import` |

The `sshcraft_*` tools keep the names of the Tauri version. A refused or failed call is a tool
result with `isError: true` and the reason; a denied action is the user's decision.

## Behaviour

- **Who asks.** The agent's name is the client's own (`clientInfo.name`); `--agent <name>` wins.
  SSHCraft shows it in its questions and log.
- **Side by side.** Calls run at once, at most 64; one waiting for the user's answer holds up
  neither `ping` nor other calls.
- **Cancelling.** `notifications/cancelled` drops the call; its question in SSHCraft goes and the
  log says cancelled. No answer follows for a cancelled call.
- **The end.** When stdin ends, the server stops, its connection to SSHCraft ends, and with it
  every session the agent opened and every question still open.
- **Bad input.** A line that is not JSON, not UTF-8 or longer than 1 MiB gets a parse error
  (`-32700`); a null id `-32600`; an unknown tool or arguments that are not an object `-32602`.
  The server goes on.
- **SSHCraft closed.** `--when-closed error` (the default) answers every tool with *SSHCraft is
  not running*. `--when-closed launch` starts the app as a double click would (the program beside
  `sshcraft-cli`, or `SSHCRAFT_APP_PATH`; never through a shell) and waits up to 45 seconds for it
  to let agents connect.
- **Bridge.** `--bridge <port> --control-token-file <path>` works against an app started with
  `--control`; engine commands then run as the caller `control`.

The command line (`sshcraft-cli hosts`, `exec`, …) uses the same connection, as the agent `cli`
unless `--agent` names another; it exits with code 3 while SSHCraft is not running.
