# Connections

How SSHCraft signs in and how it reaches a host: passwords, keyboard-interactive, identities,
jump hosts, proxies, agent forwarding and environment variables, plus the two terminals that are
not SSH (a local shell and a serial port).

## Signing in

A host signs in with what it has, in this order:

1. its **key** (`publickey`), from the vault or from the SSH agent (Proton Pass);
2. its **stored password** (`password`), if the server offers that method;
3. **keyboard-interactive**, if offered. A stored password answers at most one round, and only a
   round with exactly one hidden question. Every other round (a visible question, a code, several
   questions at once) goes to you in a dialog;
4. with no stored password and a server that only offers `password`, SSHCraft asks you once.

A server that accepts the key or the password but wants a second factor (partial success) gets
that first and then the questions; a stored password never answers the second factor.

**First contact.** A stored password never goes to a server whose host key you trust for the
first time in that very connection: the address may have been changed (by a member of a share,
or a group it inherits from), so SSHCraft asks for the password instead. From the second
connection on, the stored password is used. The same holds for each jump host. A stored proxy
password stays only while the proxy keeps its kind, address, port and user; when someone (you, or
a member of a share) points the proxy elsewhere, the password is dropped and has to be entered
again. What you type is not kept: after a reconnect you are asked again. Cancel ends
the tab without retrying.

Passwords are sealed in the vault like keys (see [security.md](security.md)). The forms show only
whether one is stored, with *Change* and *Remove*; no part of the app ever shows a stored
password again. Before the first key or password from the vault is used, Windows Hello asks once
per run.

## Identities

An identity bundles a user, a password and a key under a name (*Keys → Identities*). Hosts and
groups pick one. On each level (the host, then each group above it), the identity fills what that
level does not set itself; then the next level up follows. So a host with its own user and an
identity takes the user from the host and the key from the identity, and an identity on a host
beats a user set on its group.

## Jump hosts

A host or group lists jump hosts in order (*Connection → Jump hosts*). SSHCraft connects to the
first, opens a `direct-tcpip` channel through it to the next, and so on to the target. Each hop
checks its own host key and signs in with its **own** user, key and password; a jump host never
gets the target's credentials and the other way round. A jump host's own jump hosts and proxy do
not apply when it is used as a hop: what counts is set on the target. At most 8 jump hosts; a list
with the target itself, a host twice or a deleted host is refused before anything connects.

When something fails at a jump host (a wrong password, a missing key, no connection), the status
line says what failed and names that jump host.

*From the group* inherits, *None* stops the inheritance.

## Proxy

HTTP (`CONNECT`) or SOCKS5, with an optional user and password, set on the host or a group. The
proxy carries the first connection only (to the first jump host, or the target). A SOCKS5 proxy
resolves the host name itself. An HTTP proxy receives its user and password unencrypted (Basic
authentication). *Direct* switches off a proxy inherited from a group. Reaching a hop takes at
most 15 seconds.

## Agent forwarding

Off unless switched on for a host or group. When on, the target (never a jump host) may use the
SSH agent of this device, Proton Pass on Windows, while the session is open. Where no agent can be
forwarded yet (macOS, Linux and FreeBSD for now), the status line under the terminal says so. Keys from SSHCraft's
own vault are never forwarded. Anyone with root on the server can use the forwarded agent in that
time, so switch it on only where you need it. A server that opens an agent channel without
forwarding switched on is refused.

## Environment variables

Name and value pairs on hosts and groups; a name set lower in the tree wins. SSHCraft sends them
before the shell starts, but the server only takes what its `AcceptEnv` allows (OpenSSH accepts
`LANG` and `LC_*` by default). Names are letters, digits and underscores, not starting with a
digit, at most 64 per level.

## Local terminal and serial port

Desktop only, from the new tab page or the command palette:

- **Local terminal:** Windows PowerShell, PowerShell 7 (if installed), cmd and each WSL
  distribution; elsewhere the login shell. An own shell (program and arguments) can be added under
  *Settings → Own shell*. It starts in the home directory; when it exits, the tab ends.
- **Serial:** port, baud rate (50 to 4 000 000), data bits (5 to 8), stop bits (1 or 2), parity
  and flow control (software XON/XOFF or hardware RTS/CTS). The last settings are remembered on
  this device. A port that disappears (an adapter pulled out) is retried like a lost connection.
  The port list shows the port names only (`COM3`, `/dev/ttyUSB0`), without a description of
  the USB device: the serial library (serial2) lists paths, not device details.

## Post-quantum key exchange

SSHCraft prefers `mlkem768x25519-sha256`, the hybrid of ML-KEM and X25519 that `russh` offers
first; servers without it fall back to Curve25519. Post-quantum host keys (ML-DSA) follow once
`russh` supports them.

## Importing from `~/.ssh/config`

`ProxyJump` through hosts of the same file (or hosts that already exist at the same place with
that name) becomes jump hosts; other forms (`user@host:port`, a host not in the file) and
`ProxyCommand` stay a warning. `ForwardAgent yes|no` and `SetEnv` are taken over.
