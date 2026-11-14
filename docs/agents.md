# AI agents

> The desktop app is being rebuilt (R1 to R7); this page describes the behaviour the rebuild keeps. The connection between agents and the app described below changes in R6: a loopback connection with a token per start replaces the named pipe, as the rebuild design (section 9) says.

Claude Code, Codex and other AI agents can work with SSHCraft: list hosts, run commands, keep a
shell open, add and change hosts and groups, generate and import keys. They do this through the
`sshcraft` program that comes with the app, either as an **MCP server** (`sshcraft mcp`) or as a
**command line** for scripts. Agents never see a private key: SSHCraft holds the keys and opens
the SSH connections itself.

## Setting it up

`sshcraft.exe` is installed next to the app (`sshcraft-app.exe`): with the setup program in
`%LOCALAPPDATA%\SSHCraft`, with the MSI package in `C:\Program Files\SSHCraft`. Add that folder
to `PATH`, or use the full path below.

**Claude Code:**

```bash
claude mcp add sshcraft -- sshcraft mcp
```

**Codex** (`~/.codex/config.toml`):

```toml
[mcp_servers.sshcraft]
command = "sshcraft"
args = ["mcp"]
```

Any other MCP client: command `sshcraft`, argument `mcp`, transport stdio.

The SSHCraft app must be running. Without it every tool answers *SSHCraft is not running*, and
the command line exits with code 3.

## Command line

```bash
sshcraft hosts
sshcraft exec web -- systemctl status nginx
sshcraft save-host '{"name": "db", "address": "10.0.0.5", "username": "root"}'
sshcraft save-host '{"id": "<host id>", "username": "deploy"}'
sshcraft generate-key deploy --type ed25519
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

- **Allowed** runs at once, **Blocked** is refused.
- **Ask** opens a window in SSHCraft with the agent, the action, the target and, for commands, the
  command. Only that window answers, never the agent's chat. Without an answer within two
  minutes it counts as *Deny*; an agent that disconnects takes its question back.
- Opening a session asks; writing to it and reading from it then follow without asking again.
  A session belongs to the connection that opened it (one `sshcraft mcp` process) and is closed
  when that connection ends, or when its host gets hidden. Closing one's own session is always
  allowed. Agents never reach the tabs of the user.
- The question shows what changes: for a host every changed field with its old and new value.
  Characters that hide or reorder text are shown as `⟨U+…⟩`, and a command with several lines,
  over 300 characters or with such characters carries a warning. The buttons answer only after
  a short moment, so a click meant for an earlier question cannot answer a new one.
- An agent cannot point stored sign-in data elsewhere: a host (or the groups above it) with a
  stored password, an identity or agent forwarding cannot get another address, port or group
  through an agent, and no host can be created in such a group. A group with such hosts cannot
  be moved by an agent.
- After the user's answer SSHCraft checks the request again; if the host was hidden or changed
  meanwhile, it does not run.
- Settings, the agents' rights included, are out of reach for agents. Sharing and sync have no
  agent actions in this version.

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

`sshcraft` talks to the app over a named pipe `\\.\pipe\sshcraft-agent-<your SID>`. Its access
list admits only your Windows user, remote clients are refused, and SSHCraft creates it as the
first instance. `sshcraft` checks that the program serving the pipe runs as your user before it
sends anything. Another account that takes the name first can keep agents from reaching
SSHCraft, but cannot pose as it. Requests run side by side; an MCP client's cancellation, or the
client going away, withdraws an open question.
