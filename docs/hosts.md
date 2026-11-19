# Hosts and order

> The desktop app is being rebuilt (R1 to R7); this page describes the behaviour the rebuild keeps.

How SSHCraft keeps hosts in groups, with tags and symbols, and how hosts come in from other
programs and go out again. Sync and sharing of all this are described in [sync.md](sync.md).

## The overview

The Hosts page opens on an overview: the hosts you connected to last, then the hosts in no group
(at most twelve, *Show all* lists the rest), then a card for every top-level group with its first
eight hosts and the number of its subgroups. A click on a group's name opens the group. Every host
has an **SSH** button that connects right away and an **SFTP** button that opens its files
([sftp.md](sftp.md)). The button with three dots beside the search holds import, export and the choice
between list and grid. Searching, picking a tag or a dynamic group, or opening a group switches to the list
(or the grid), and the path above it leads back.

## Groups and subgroups

Groups can be nested up to 64 levels deep. A host takes every value it does not set
itself from the nearest group above it that sets it: address and port from the top group,
the user from a middle one, the port on the host itself, for example. Inside a group the page
shows one level at a time; double-click a subgroup to open it, the path above the list leads back.

A group cannot be put into itself or one of its own subgroups. When two devices nest two groups
into each other at the same time, the later change wins and the other group moves to the top
level on every device.

A subgroup always lives where its parent lives (on this device only, or at a sync server), and
moves with it. Deleting a group deletes its subgroups and all their hosts.

## Tags and symbols

Hosts and groups carry tags (at most 32, each at most 64 characters, compared without regard to
case). The chips above the list filter by tag; with several chosen, a host must have all of them.
The search covers names, addresses, users, the groups on the way up and the tags.

The symbol of a host shows its operating system or distribution. After the first successful
connection to a host without a symbol, SSHCraft reads `/etc/os-release` (or `uname -s`) once over
a separate channel and stores the result; it can also be set by hand in the host form. Each
device asks a host only once, also when the answer is not recognized.

## Dynamic groups

A dynamic group is a saved filter: the hosts of every group that carry all of its tags and match
its search. Choose tags or type a search, then **Save as dynamic group** and give it a name. The
dynamic groups sit as chips above the host list. A click shows their hosts, a second click (or a
click on the path) goes back to the groups. Right-click a chip to delete it. Dynamic groups are
kept on this device, at most 32.

## Changing many hosts at once

Ctrl+click picks hosts one by one, Shift+click picks the range from the last click. While hosts
are picked, a bar above the list moves them to a group, adds or removes a tag, or sets the user
(empty takes it from the group again). Each host is saved as if it had been edited on its own:
it gets an earlier version, passwords and keys stay as they are, and sync sends the change.

## Reachability

With *Settings › Appearance › Reachability* switched on, a dot in front of each host shows
whether it answers: green when a TCP connection to its address and port opens within two
seconds, red when not. SSHCraft checks the hosts in view once a minute, at most 16 at a time,
without signing in. Hosts behind a jump host or proxy stay grey, since a direct connection says
nothing about them. It is off by default because it reaches out to every host in the list.

## Import

*Hosts → ⋯ → Import* (desktop only) reads one source, shows a preview and creates exactly the entries left
checked. Entries with a warning start unchecked. A host that already exists at the same place with
the same address and port is not created again when it also has the same name and user, or when
the new entry knows no user (a `known_hosts` entry for a host imported before). All names on one
`known_hosts` line are one host. Groups on a source's path are created
as subgroups, existing ones with the same name on the same level are reused. The import can go
straight to a sync server.

| Source | Where | What comes over |
|---|---|---|
| `~/.ssh/config` | home folder | hosts, users, ports, keys found in the Proton Pass agent |
| `known_hosts` | `~/.ssh/known_hosts` | one host per line; optionally the host keys of every name |
| Key files | `~/.ssh` | private keys in OpenSSH, PEM or PuTTY (`.ppk`) format, with passphrase |
| CSV | any file | columns `name`, `group` (path with `/`), `tags`, `address`, `port`, `username`, `os`; Termius-style headings are understood too |
| PuTTY | Windows registry, or a `.reg` export | SSH sessions with host, port, user and key file |
| MobaXterm | `MobaXterm.ini` | SSH bookmarks with their folders |
| SecureCRT | `Config\Sessions` folder | SSH2 sessions, folders become groups |

Limits on purpose:

- Host keys from `known_hosts` never add to or replace the keys SSHCraft already stores for that
  host: the stored ones were confirmed when connecting, a file can be stale or planted. A host
  new to SSHCraft takes all its keys from the file. Hashed host names and `@cert-authority` or
  `@revoked` lines are skipped. *Keys › Known hosts* also writes a `known_hosts` file.
- Passwords stored by MobaXterm or SecureCRT are never read.
- Files over 10 MiB are refused; broken lines are skipped and listed, never the whole import.
- Key files named in a session (PuTTY, SecureCRT) are pointed out; import them under *Keys*.

## Colour, badge and theme

A host or group can give its terminals a look of their own, so a production server is hard to
mistake:

- **Tab colour:** a line under the tab.
- **Badge:** up to 16 characters, like `PROD`, on the tab and in the terminal's top right corner.
- **Terminal theme:** dark or light, in place of the app's terminal theme.

Each of the three is inherited on its own from the nearest group that sets it. The JSON export
keeps them.

## Trash and earlier versions

A deleted host, group (with its subgroups and their hosts), key or snippet goes to *Settings ›
Trash* for 30 days (1 to 365), also when another device deleted it through sync. *Restore* puts
it back with its passwords and sends it to its sync server again (a group takes back its hosts
that came to the trash on their own); one whose id is in use again stays in the trash, and one
whose share or parent group is gone comes back on this device only. Keys bound to this device
(Windows Hello) do not go to the trash. Deleting for good also removes the earlier versions. Every save of a host, group or snippet keeps the stand before: *Earlier
versions…* in a host's menu lists up to 20 and restores one like an edit.

## Backup

*Settings › General › Backup* writes one file with all hosts, groups, identities, keys (as their
OpenSSH text, still under their own passphrase where they have one), passwords, snippets and
host keys, encrypted with a passphrase of at least 12 characters (Argon2id, XChaCha20-Poly1305).
It opens on any device, also after the system's keyring is gone. Keys bound to a device (Windows
Hello) cannot leave it and are left out. *Restore from backup* adds what is missing and changes
nothing that is there; a wrong passphrase or a changed file adds nothing.

## Export

*Hosts → ⋯ → Export* (desktop only) writes everything, one place or one group with its subgroups:

- **CSV**: one row per host with the values it effectively uses (a flat table has no
  inheritance), in the columns the CSV import reads back.
- **JSON**: groups with their parents and tags, hosts with their own values, so inheritance stays.

No export contains a private key, a passphrase or anything else from the vault. A stored key
appears only with its name and fingerprint, an agent key only with its fingerprint.

How a host connects (passwords, identities, jump hosts, proxies, agent forwarding, environment
variables) is in [connections.md](connections.md). The JSON export carries those settings too,
with `has_password` instead of any password and identities by name.
