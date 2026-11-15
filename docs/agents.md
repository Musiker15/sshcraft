# AI agents

Claude Code, Codex and other AI agents can work with SSHCraft: list hosts, run commands, keep a
shell open, add and change hosts and groups, generate and import keys. They do this through the
`sshcraft-cli` program that comes with the app, either as an **MCP server** (`sshcraft-cli mcp`)
or as a **command line** for scripts. Agents never see a private key: SSHCraft holds the keys and opens
the SSH connections itself.

## Setting it up

1. In SSHCraft, turn on Settings › AI agents › **Allow agent control** (off on a fresh install).
   The page then says where agents connect.
2. `sshcraft-cli` is installed next to the app (`sshcraft`). Add that folder to `PATH`, or use
   the full path below.

**Claude Code:**

```bash
claude mcp add sshcraft -- sshcraft-cli mcp
```

**Codex** (`~/.codex/config.toml`):

```toml
[mcp_servers.sshcraft]
command = "sshcraft-cli"
args = ["mcp"]
```

Any other MCP client: command `sshcraft-cli`, argument `mcp`, transport stdio.

A registration of the Tauri version (`claude mcp add sshcraft -- sshcraft mcp`) must be made
again with `sshcraft-cli`. The SSHCraft app must be running and allow agent control. Without it
every tool answers *SSHCraft is not running*, and the command line exits with code 3;
`sshcraft-cli mcp --when-closed launch` starts the app instead and waits for it. With the ArtCraft
Launcher's AI-MCP flag on, the launcher's switch *Let AI agents use Craft apps* must be on too.
The tools in detail: [mcp.md](mcp.md).

## Command line

```bash
sshcraft-cli hosts
sshcraft-cli exec web -- systemctl status nginx
sshcraft-cli save-host '{"name": "db", "address": "10.0.0.5", "username": "root"}'
sshcraft-cli save-host '{"id": "<host id>", "username": "deploy"}'
sshcraft-cli generate-key deploy --type ed25519
sshcraft-cli commands
sshcraft-cli run --cmd 'forward.status'
```

`--agent <name>` sets the name SSHCraft shows in its questions and log (default `cli`; with
`mcp` it replaces the name the MCP client gives), `--json` prints results as JSON. A host is
named by its id or by a name only one host has.

- `exec` returns standard output and standard error together (at most 1 MiB) and the exit
  status, and waits up to `--timeout` seconds (1 to 600, default 60).
- `save-host` changes only the fields it names: a missing field stays, `null` clears it.
- Interactive sessions are an MCP feature: a session belongs to the connection that opened it
  and ends with it, so the command line, which connects once per call, has none.

## Rights

*Settings → AI agents* has a level for every kind of action:

| Kind | On a fresh install |
|---|---|
| View hosts, groups and key names | Allowed |
| Connect and run commands | Ask |
| Create and change hosts and groups | Ask |
| Delete hosts, groups and keys | Blocked |
| Import or generate keys | Ask |
| Share and change shares | Blocked |
| Manage devices and sync servers | Blocked |
| Port forwarding | Blocked |
| Session logs | Blocked |
| Workspace templates | Blocked |
| Snippets | Blocked |

The last four came with the rebuild and start blocked, as every kind added later does. On the
first start of the rebuilt SSHCraft, *Share* and *Sync* go back to *Blocked* once: the Tauri
version had no agent action for them, so an earlier "allowed" must not start to work silently.

These commands are reachable for agents; every other command (settings, agent rights, secrets,
imports, accounts, answers to questions) never is:

| Kind | Commands |
|---|---|
| View | `host.list`, `group.list`, `key.list`, `host.info` |
| Connect | `host.exec`, `session.open`, `session.send`, `session.read`, `session.close` |
| Create and change | `host.save`, `group.save` (only name, group, address, port, user, key, tags) |
| Delete | `host.delete`, `group.delete`, `key.delete` |
| Keys | `key.generate`, `key.import` |
| Port forwarding | `forward.start`, `forward.stop`, `forward.status` |
| Session logs | `log.list`, `log.read`, `log.search`, `log.mark_add`, `log.mark_remove`, `log.delete` |
| Workspace templates | `workspace.list`, `workspace.save`, `workspace.delete` |
| Snippets | `snippet.list`, `snippet.save`, `snippet.delete` |
| Sharing | `share.find_recipient`, `share.items`, `share.list`, `share.set_grants`, `share.end`, `share.of_items` |
| Sync | `sync.servers`, `sync.set_server`, `sync.server_logs`, `device.list` |

- **Allowed** runs at once, **Blocked** is refused.
- A list or status within a kind (`forward.status`, `log.list`, `workspace.list`, `share.list`,
  `share.of_items`, `sync.servers`, `device.list`) runs at *Ask* without a question and without a
  log entry; *Blocked* still blocks it. Reading a session log, searching the logs and listing
  snippets carry content that is sealed in the vault: they follow the level and are logged.
- The question names what changes: for sync the server and whether keys go to it, for a share
  what is shared, to whom and whether keys go with it, for a workspace every pane and the command
  it types, for a snippet its content. An agent cannot put a command into a local or serial pane
  of a workspace, and cannot share a group with anything hidden below it.
- **No window, no yes.** A question needs the SSHCraft window. While no window listens, *Ask*
  is answered no at once ("no SSHCraft window to ask in"); there is no headless mode.
- **Ask** opens a window in SSHCraft with the agent, the action, the target and, for commands, the
  command. Only that window answers, never the agent's chat. Without an answer within two
  minutes it counts as *Deny*; an agent that disconnects takes its question back.
- Opening a session asks; writing to it and reading from it then follow without asking again.
  A session belongs to the connection that opened it (one `sshcraft-cli mcp` process) and is closed
  when that connection ends, or when its host gets hidden. Closing one's own session is always
  allowed. Agents never reach the tabs of the user.
- The question shows what changes: for a host every changed field with its old and new value.
  Characters that hide or reorder text are shown as `⟨U+…⟩`, and a command with several lines,
  over 300 characters or with such characters carries a warning. The buttons answer only after
  a short moment, so a click meant for an earlier question cannot answer a new one, and only to
  a click: no key answers, so Tab and Enter typed for a terminal never allow anything. Deny
  comes first.
- An agent cannot point stored sign-in data elsewhere: a host (or the groups above it) with a
  stored password, an identity or agent forwarding cannot get another address, port or group
  through an agent, and no host can be created in such a group. A group with such hosts cannot
  be moved by an agent.
- After the user's answer SSHCraft checks the request again; if the host was hidden or changed
  meanwhile, it does not run.
- Settings, the agents' rights included, are out of reach for agents. A session log, snippet,
  workspace template or share that belongs to a hidden host or group is unknown to agents.

## Hiding hosts and groups

Right-click a host or group on the hosts page and choose *Hide from agents* (production servers,
say). Hidden hosts, every host in a hidden group at any depth, and every host whose jump hosts
lead through a hidden one, appear in no list, and every action on them answers like for a host
that does not exist. A group with anything hidden below it cannot be deleted by an agent, nor a
key a hidden host uses. Key names stay visible. *Settings → AI agents* lists what is
hidden and shows it again. Rights and hiding are settings of this device.

## Log

Every agent action that changes or runs something is logged before it runs: time, agent, action,
target, the command for commands (at most 2000 characters, the longest command an agent may
send), and how it was decided (allowed, asked and allowed, asked and denied, blocked, hidden,
cancelled when the agent went away during the question); a failure adds a second entry.
Listing and reading session output are not logged, so they cannot push actions out of the log.
The log keeps the last 1000 entries, sealed with the vault, and can be cleared in the settings.
If the log cannot be written, or the agent settings cannot be read, agents get nothing.

## What agents never get

- No private key, passphrase or password, and no account or device key. Keys appear with id,
  name, type and fingerprint only; hosts without passwords or identities.
- Importing a key sends key material **into** SSHCraft; it never comes out again, and the key
  text is neither shown in the question nor logged.
- An answer that looks like key material is not sent at all.

## The limit SSHCraft cannot close

An agent with a shell of its own runs as the same Windows user as SSHCraft. It could start
`pass-cli` or read files of that user without going through SSHCraft. SSHCraft makes its own
store harder to reach: keys from the vault are unlocked with Windows Hello on first use after
start. Give agents a shell only where you would let them act as you.

## The connection

`sshcraft-cli` talks to the app over loopback (`127.0.0.1`), never over the network. While agents
may connect, SSHCraft listens on a port and names it in `agent-connection.json` in its settings
folder, beside a token file only your user can read. Every connection signs in by challenge: the
token never crosses the wire, and SSHCraft proves it holds the token before `sshcraft-cli` sends
anything derived from it, so another program on a stale port learns nothing. Requests run side
by side; an MCP client's cancellation, or the client going away, withdraws an open question and
ends the agent's sessions. The protocol: [control-protocol.md](control-protocol.md).
