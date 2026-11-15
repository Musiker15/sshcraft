# Connections

> The desktop app is being rebuilt (R1 to R7); this page describes the behaviour the rebuild keeps.

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

### Proxy command

*Command* runs a program on this computer whose standard input and output carry the connection,
like OpenSSH's `ProxyCommand` (for example `ssh -W %h:%p bastion` or `cloudflared access ssh
--hostname %h`). `%h`, `%p`, `%r` and `%n` stand for the address, port and user of the first
hop and the host's name; `%%` is a `%`.

- **No shell.** The line is split once, by the quoting rules of `sh`, and the program starts
  with that argument list. A line that needs a shell (`|`, `;`, `&`, `<`, `>`, `$`, backticks,
  parentheses) is refused; a word starting with `#` begins a comment. A replaced token stays one
  argument, whatever the host name holds, and the program itself is never made from a token. An
  address, user or name that starts with `-` (it would be an option to the program) or holds a
  control character is refused when the command uses it.
- **Only after it was allowed on this device.** A command you enter in the host or group form, or
  that comes from your own `~/.ssh/config`, runs without asking. One that arrived through sync, a
  share, an import or an AI agent, or that changed, is shown in full first (*Allow on this
  device* or *Cancel*); the answer is kept per command line. Saving such a host for another
  reason (renaming, a forwarding rule, moving it) does not allow it. AI agents can neither set
  nor change a proxy command, nor create or move a host under one.
- What the program writes to standard error (up to 4 KiB) is shown when the connection fails.

## Reconnecting

A terminal whose connection drops tries again after 1, 2, 4, 8, 16 and 32 seconds, then every
minute, up to *Settings › Terminal › Reconnect* times (10 by default, 0 switches it off).
A network change is acted on at most once a minute, so flapping interfaces do not use up the
attempts. Port
forwarding tries for as long as the rule is on. SSHCraft checks the clock and the network every
five seconds: after sleep or when the network changed (another Wi-Fi, a VPN, a cable), every
connection is pinged at once and given up after five seconds without an answer, instead of
after the 45 seconds of keep-alive, and a reconnect that waits tries now.

## Host keys

A server's host key is kept whole (algorithm and key), several per server (one per algorithm).
A key that matches one of them is trusted; any other key ends the connection, as before.
*Keys › Known hosts* lists them, removes one (the next connection asks again), and reads or
writes a `known_hosts` file. A server SSHCraft does not know yet takes every key of an imported
file; a server it knows takes no key it does not know.

After sign-in an OpenSSH server may name all its host keys (`UpdateHostKeys`). SSHCraft keeps a
new one only after the server proved that it holds it, and drops keys the server no longer
names; only when the key of this connection is among them. *Settings › General › Host keys*
switches it off. Keys known only by their fingerprint (from before this version) gain their key
on the next connection.

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
`russh` supports them. SSHCraft always offers "strict kex" against the Terrapin attack
(CVE-2023-48795); a test makes sure a packet slipped in before the key exchange is refused.

## Importing from `~/.ssh/config`

`ProxyJump` through hosts of the same file (or hosts that already exist at the same place with
that name) becomes jump hosts; other forms (`user@host:port`, a host not in the file) stay a
warning. `ProxyCommand` becomes a proxy command when it needs no shell (see above), otherwise a
warning. `ForwardAgent yes|no` and `SetEnv` are taken over. `Include` is followed (relative to
`~/.ssh`, with `~` and `*`, at most 16 levels and 256 files); `Match` is not.

### Keeping in step and writing back

With *Settings › General › Keep the hosts of ~/.ssh/config in step*, the file and the files it
includes are read at start and again whenever one changes. Their hosts live in the group
`~/.ssh/config` and follow the file: a changed entry updates its host (address, port, user,
key, jump hosts, proxy command, agent forwarding, environment), an edit made in SSHCraft stays
until the entry changes in the file, and a host that leaves the file stays in SSHCraft without
its link. *Write to ~/.ssh/config* in a host's menu writes it back: into its own `Host` block
where it came from (only the lines SSHCraft manages change; comments, `IdentityFile` and every
other line stay), or as a new block before the first `Host` line of `~/.ssh/config`. The file is
replaced in one step (a symbolic link stays a link) and a copy of the old one goes to
`~/.ssh/.sshcraft-backup/`, one per write-back, where no `Include` pattern reads it. A line
SSHCraft has no value for (a `ProxyCommand` that needs a shell, a `ProxyJump` to a host it does
not know) stays as it was; `ForwardAgent no` and a `Port` set in SSHCraft are written out. A
proxy command that is not allowed on this device is not written. When the file is missing or
empty for a moment (an editor saving), the links stay; a host that comes back into the file
takes its old place instead of a second one. At most 4 MiB, 10 000 blocks and 2 000 hosts are
read. AI agents can do neither.
