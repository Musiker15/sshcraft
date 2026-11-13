# AI agents

Claude Code, Codex and other AI agents can work with SSHCraft: list hosts, run commands, keep a
shell open, add and change hosts and groups, generate and import keys. They do this through the
`sshcraft` program that comes with the app, either as an **MCP server** (`sshcraft mcp`) or as a
**command line** for scripts. Agents never see a private key: SSHCraft holds the keys and opens
the SSH connections itself.

## Setting it up

`sshcraft.exe` is installed next to `SSHCraft.exe`: with the setup program in
`%LOCALAPPDATA%\SSHCraft`, with the MSI package in `C:\Program Files\SSHCraft`. Add that folder to
`PATH`, or use the full path below.

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
sshcraft session open web
sshcraft session write <session> "tail -n 50 /var/log/syslog" --enter
sshcraft session read <session> --wait 2000
sshcraft session close <session>
sshcraft save-host '{"name": "db", "address": "10.0.0.5", "username": "root"}'
sshcraft generate-key deploy --type ed25519
```

`--agent <name>` sets the name SSHCraft shows in its questions and log (default `cli`), `--json`
prints results as JSON. A host is named by its id or by a name only one host has.

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
- Opening a session asks; writing to it, reading from it and closing it then follow without
  asking again. Agents only reach sessions they opened, never the tabs of the user.
- Settings, the agents' rights included, are out of reach for agents. Sharing and sync have no
  agent actions in this version.

## Hiding hosts and groups

Right-click a host or group on the hosts page and choose *Hide from agents* (production servers,
say). Hidden hosts, and every host in a hidden group at any depth, appear in no list, and every
action on them answers like for a host that does not exist. *Settings → AI agents* lists what is
hidden and shows it again. Rights and hiding are settings of this device.

## Log

Every agent action except reading session output is logged: time, agent, action, target, the
command for commands, and how it ended (allowed, asked and allowed, asked and denied, blocked,
hidden, failed). The log keeps the last 1000 entries, sealed with the vault, and can be cleared
in the settings.

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
first instance, so no other program holds the name. `sshcraft` checks that the program serving
the pipe runs as your user before it sends anything.
