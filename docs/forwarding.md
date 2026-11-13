# Port forwarding

Port forwarding rules belong to a host. They sync and are shared together with the host, and
they go when the host is deleted. The page **Port forwarding** in the sidebar lists the rules of
all hosts.

## Kinds

| Kind | ssh option | What it does |
|---|---|---|
| Local | `-L 127.0.0.1:8080:db:5432` | A port on this device reaches `db:5432` as seen from the server, for example a database that only listens there. |
| Remote | `-R localhost:9000:127.0.0.1:3000` | A port on the server reaches `127.0.0.1:3000` as seen from this device, for example to show a local web server. |
| Dynamic | `-D 127.0.0.1:1080` | A SOCKS5 proxy on this device. Programs that use it reach anything the server can reach. |

The dynamic proxy speaks SOCKS5 without authentication and only `CONNECT` (no `BIND`, no UDP,
no SOCKS4). Targets can be IPv4, IPv6 or names; names are resolved on the server.

## Creating and editing

**New rule** opens a short wizard: the kind, the host, the addresses and ports, then a summary
with the equivalent `ssh` option. New rules listen on `127.0.0.1` (local, dynamic) or
`localhost` on the server (remote). Another address, such as `0.0.0.0`, makes the rule reachable
from other machines; the wizard warns about it. For a remote rule the server decides whether it
listens beyond itself (`GatewayPorts` in OpenSSH).

A running rule cannot be edited; stop it first. A rule removed from its host stops.

Limits: ports 1 to 65535, addresses without spaces (at most 253 characters), names up to 100
characters, 50 rules per host.

## Starting and stopping

- A double click or Enter starts a stopped rule and stops a running one. The right-click menu
  has the same, plus edit and delete.
- Each running rule opens its own SSH connection to its host, the same way a terminal does:
  jump hosts, proxy, keys, saved or asked passphrase, Windows Hello. A question (host key,
  password) shows up like for a terminal; stopping the rule takes it back.
- After the connection is lost, the rule reconnects like a terminal (up to 5 attempts) and
  listens again.
- A local port that is already taken, an address that cannot be used, or a server that refuses
  the remote forwarding (`AllowTcpForwarding no`) ends the rule with that error; it is not
  tried again.
- A rule carries at most 256 connections at a time; more are closed right away.
- Rules do not start by themselves when SSHCraft starts, and they stop when it ends.
