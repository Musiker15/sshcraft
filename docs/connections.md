# Connections

> The desktop app is being rebuilt (R1 to R7); this page describes the behaviour the rebuild keeps.

How SSHCraft signs in and how it reaches a host: passwords, keyboard-interactive, identities,
jump hosts, proxies, agent forwarding and environment variables, plus the two terminals that are
not SSH (a local shell and a serial port).

## Signing in

A host signs in with what it has, in this order:

1. its **key** (`publickey`), from the vault or from the SSH agent (Proton Pass on Windows, the agent `SSH_AUTH_SOCK` names elsewhere);
2. its **stored password** (`password`), if the server offers that method;
3. **keyboard-interactive**, if offered. A stored password answers at most one round, and only a
   round with exactly one hidden question that does not ask for a code. A question for a
   one-time code (`Verification code:`, `Token:`, `OTP`) is answered once by the host's TOTP
   secret, if it has one, and never by the password. Every other round (a visible question, a
   wrong code, several questions at once) goes to you in a dialog;
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
per run (on Linux and FreeBSD polkit asks for your password or fingerprint, see
[security.md](security.md)). *Settings › General › Ask Windows Hello before the first stored key
or password* switches that off; then anything running as your user can use the stored secrets
through SSHCraft without a question. Agents cannot change the setting.

### Passwords from a password manager

Instead of storing a password, a host can name where it is: *From a password manager* in the
host form, with Proton Pass (`pass://vault/item/password`), 1Password (`op://vault/item/password`),
Bitwarden (the item's id, with an unlocked `bw` session) or KeePassXC
(`<database file>#<entry path>`; its password is asked once per run and kept only in memory).
When connecting, SSHCraft runs the manager's own program with an argument list and takes what
it prints; nothing is kept. A reference that could pass an option to the program is refused.

A reference is used for a host's address only after it was allowed on this device: entering it
in the form allows it. One that came through sync or a share, or whose host now points
elsewhere, is shown first (*Send a password from your password manager?*), because it could
otherwise send another of your passwords to a server someone else chose. Copies in a share never
carry a reference.

### One-time codes

*One-time codes* in the host form takes a TOTP secret, as an authenticator app gets it (Base32,
or the `otpauth://` link of the QR code; a link for other settings than SHA-1, 6 digits and 30
seconds is refused). It is sealed in the vault like a password, syncs, moves and is shared with
its host under the same rules (only with keys), and answers a server's question for a code once per
connection (RFC 6238: SHA-1, 30 seconds, 6 digits).

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

## Login scripts

Some servers want input before the shell: a menu on a jump server, a question after login. A
login script answers them. Each step waits for a text in the output (colours are ignored) and
then sends a line and Enter, once. A step that does not see its text within 15 seconds stops
the script, and the terminal says so.

A login script runs commands with your credentials, so it is treated like a proxy command: it
runs only once you allowed exactly these steps on this device. Writing or changing it in the
host or group form allows it when you save. A script that came another way (sync, a share, an
agent) does not run; the terminal says so until you tick **Allow this login script on this
device** in the form and save. Agents can never allow one.

Never put a password in a login script: it is stored as plain settings, not in the vault. Limits:
16 steps, 256 characters to wait for, 1024 to send, no control characters.

## Launch actions

Launch actions are snippets that SSHCraft types into the terminal once a session has connected,
for example `cd /srv/app` or a `tail -f` you always start. Pick up to eight in the host or group
form; they are typed in the order you ticked them (the form numbers them), each followed by
Enter, after a login script has finished. They run on the first connect of a tab only, neither
after a reconnect nor after a login script that stopped. A host that keeps its session in tmux or
screen gets none, since a program may already run there, and a Telnet host only gets them after a
login script signed in. Untick all of them on a host to type none of its group's.

Like a login script, launch actions run only once you allowed exactly these texts for this host
or group on this device. Picking them in the form allows them when you save. If a snippet's text
changes later (through sync, say), it has to be allowed again. Launch actions that came another
way (sync, a share, an agent) are not typed; the terminal says so until you tick **Allow the
launch actions on this device** and save. Changing the list alone does not allow those. A snippet with placeholders is skipped, since nobody is there to fill them in.

## Steps before connecting and after the session

A host or group can do something before SSHCraft connects and after the session ends:

- **Wake-on-LAN** sends the magic packet for a MAC address (to `255.255.255.255` or a broadcast
  address you give) and then waits as long as you say, so the computer has time to start.
- **Port knocking** opens a connection to each port of the host's address in turn, with a pause
  between them. The ports are meant to stay closed, so a refused knock is fine.
- **A program** on this device, for example `wg-quick up office` before and `wg-quick down office`
  after. It starts from its argument list, never through a shell, and may run for a minute.

A program is treated like a proxy command: it runs only once you allowed exactly these
arguments on this device. A program you write in the host or group form is allowed when you
save, and only that one; one that came through sync, a share or an agent does not run, and
connecting asks before it starts. The steps after the session run when the last tab, forward or
command using the connection is gone, and a program there that was never allowed is left out
without asking. Wake-on-LAN and knocking need no approval: a failed wake or knock does
not stop the connection. Limits: 16 steps per list, 64 ports per knock, waits of at most 300
seconds.

## Sharing one connection

Tabs, port forwards and commands to the same host share one SSH connection while it is open, so
a second tab opens at once and asks nothing again. The connection closes when the last user of
it is gone. Remote forwards keep a connection of their own, because the server routes their
traffic per connection, and commands from AI agents always get a connection of their own, so
signing in asks you where the host needs it. Tabs opened at the same moment wait for one
connection. Locking the app closes every shared connection. Switch it off under
*Settings › Terminal › Connections* to give every tab its own.

## Agent forwarding

Off unless switched on for a host or group. When on, the target (never a jump host) may use the
SSH agent of this device while the session is open: Proton Pass on Windows, elsewhere the agent
`SSH_AUTH_SOCK` names (ssh-agent, gpg-agent, the desktop keyring, Proton Pass or 1Password).
Where no agent is running, the status line under the terminal says so. Keys from SSHCraft's
own vault are never forwarded. Anyone with root on the server can use the forwarded agent in that
time, so switch it on only where you need it. A server that opens an agent channel without
forwarding switched on is refused.

## X11 forwarding

Off unless switched on for a host or group. Programs on the server then open their windows on
this computer's X server: `DISPLAY` on Linux, FreeBSD and macOS (XQuartz), display `:0` on
Windows (VcXsrv, X410 and similar listen there). The server gets a random cookie, never the real
one. Each X11 connection the server opens must start with that cookie; SSHCraft swaps it for the
real cookie from `xauth` before the bytes reach the X server, and closes any connection with
another cookie. A server that opens an X11 channel without forwarding switched on is refused.
Anyone with root on the server can watch an X11 session, so switch it on only where you need it.

Switching it on in the host or group form allows it on this device. X11 forwarding that came
another way (sync, a share, an import) stays off, and the status line says so, until you tick
**Allow X11 forwarding on this device** in the form and save.

## Environment variables

Name and value pairs on hosts and groups; a name set lower in the tree wins. SSHCraft sends them
before the shell starts, but the server only takes what its `AcceptEnv` allows (OpenSSH accepts
`LANG` and `LC_*` by default). Names are letters, digits and underscores, not starting with a
digit, at most 64 per level.

## Quick connect and `ssh://` links

*Quick connect* (the new-tab page, the command palette or `Ctrl+Shift+O`) takes `user@host:port`,
`host` or `[2001:db8::1]:22`. A saved host with the same address, port and user opens; otherwise
SSHCraft saves one under that name and opens it, so host keys, questions and history work as for
every host. You can delete it later.

An `ssh://user@host:port` link from a browser or another program fills the same dialog and waits
for you to press Connect; a link never connects by itself. The link is checked first: a user or
host starting with `-`, control characters, spaces, a password in the link or a port outside
1 to 65535 are refused, and so are host names outside ASCII (an international name works in
its `xn--` form). A link comes alone: SSHCraft refuses one that arrives with other options. When
SSHCraft already runs, the link goes to the open window. On Windows
the installer offers SSHCraft for `ssh://` links in the default apps, on Linux and FreeBSD the
desktop file declares it. On macOS links do not reach SSHCraft yet ([gaps.md](gaps.md)).

## Desktop shortcuts

*Desktop shortcut* in a host's context menu puts a shortcut on the desktop that opens that host:
a `.lnk` file on Windows, a `.desktop` file on Linux and FreeBSD. It starts
`sshcraft --open-host <id>` and carries nothing but the host's id, no address and no password.
When SSHCraft already runs, the host opens there, and only when it is saved on this device.

## Server resources

With *Settings › Terminal › Server resources* switched on, the status line under an SSH terminal
shows the server's load, memory and disk. SSHCraft reads them every ten seconds while the tab is
shown, with one fixed command over the tab's connection (`/proc/loadavg`, `/proc/meminfo` and
`df -P /`). It is off by default because it runs a command on the server; servers without
`/proc` show what they can. A key with a forced command (`command=` in `authorized_keys`) runs
that command instead, every ten seconds, so leave the bar off for such hosts.

## Sessions that stay

*Keep the session* in the host or group form runs the shell inside tmux or screen on the server.
Every connect attaches to the same session again, also after the connection dropped or the
computer slept, so programs keep running in between. The session name has letters, digits,
dots, dashes and underscores only; SSHCraft puts it into a fixed command
(`tmux new-session -A -s NAME` or `screen -xRR NAME`) and refuses any other name, also one
that came through sync. tmux or screen must be installed on the server.

*tmux, windows as tabs* runs tmux in control mode (`tmux -CC`). Each tmux window becomes a tab,
each of its panes a pane in that tab, and typing goes to tmux as keys. Closing a tab only stops
showing it; once the last one is closed, SSHCraft detaches and the tmux session keeps running.
SSHCraft tells tmux the size of each pane. With more than two panes in a window, tmux may lay them
out a little differently from SSHCraft's grid, and a tab shows at most 16 panes. Split and move
panes of a tmux tab with tmux itself; SSHCraft keeps the tab as tmux has it. tmux answers the
programs' terminal queries in its panes, so SSHCraft does not answer them a second time. After a
reconnect SSHCraft lists the windows again: windows closed in between go, new ones come.

## Containers

A host can open a shell in a container on it instead of its own shell: pick Docker, Podman or
Kubernetes under *Container* in the host or group form, give the container's name and choose the
shell (`sh`, `bash`, `zsh` or `ash`). For Kubernetes the name is the pod's, and a namespace is
optional; without one, kubectl's current namespace is used. For a saved host, **List** asks it
which containers or pods run there (in the namespace typed) and offers their names to pick from.

SSHCraft builds a fixed command from these values, such as `docker exec -it NAME bash` or
`kubectl exec -it -n NAMESPACE NAME -- sh`. Names may only hold letters, digits, dots, dashes and
underscores and must not start with a dash (Kubernetes names lowercase only); SSHCraft refuses
any other name when saving and again before connecting, also one that came through sync. A host
cannot have both a container and a session that stays; when a group sets one and the host the
other, the container wins. Like a session that stays, a container set by a group cannot be turned
off on one host. The container runtime must be installed
on the server and usable by the user who signs in.

## Telnet

Some devices (switches, old routers, lab gear) speak only Telnet. Set *Protocol* to Telnet in the
host form; without a port of its own, the host uses port 23. Telnet is not encrypted, so the
form and the status line say so. SSHCraft never sends a stored password or key over it: the
device asks, and you type. Telnet goes straight to the host, never through a jump host or proxy.
Choosing Telnet in the form allows it on this device; Telnet that came through sync, a share or
an import is not used until you tick **Allow Telnet on this device**, so nobody else can switch
your connection to plaintext. The steps before connecting and after the session run as for SSH.
SSHCraft agrees to echo, suppress go-ahead, binary, terminal type and window size, and refuses
every other option.

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
