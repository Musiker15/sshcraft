# Hosts and order

How SSHCraft keeps hosts in groups, with tags and symbols, and how hosts come in from other
programs and go out again. Sync and sharing of all this are described in [sync.md](sync.md).

## Groups and subgroups

Groups can be nested to any depth (at most 64 levels). A host takes every value it does not set
itself from the nearest group above it that sets it: address and port from the top group,
the user from a middle one, the port on the host itself, for example. The hosts page shows one
level at a time; double-click a group to open it, the path above the list leads back.

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
a separate channel and stores the result; it can also be set by hand in the host form.

## Import

*Hosts → New → Import* reads one source, shows a preview and creates exactly the entries left
checked. Entries with a warning start unchecked. A host that already exists in the same group with
the same name, address, port and user is not created again. Groups on a source's path are created
as subgroups, existing ones with the same name on the same level are reused. The import can go
straight to a sync server.

| Source | Where | What comes over |
|---|---|---|
| `~/.ssh/config` | home folder | hosts, users, ports, keys found in the Proton Pass agent |
| `known_hosts` | `~/.ssh/known_hosts` | one host per name and port; optionally the host keys |
| Key files | `~/.ssh` | private keys in OpenSSH, PEM or PuTTY (`.ppk`) format, with passphrase |
| CSV | any file | columns `name`, `group` (path with `/`), `tags`, `address`, `port`, `username`, `os`; Termius-style headings are understood too |
| PuTTY | Windows registry, or a `.reg` export | SSH sessions with host, port, user and key file |
| MobaXterm | `MobaXterm.ini` | SSH bookmarks with their folders |
| SecureCRT | `Config\Sessions` folder | SSH2 sessions, folders become groups |

Limits on purpose:

- Host keys from `known_hosts` never replace a key SSHCraft already stores for that host: the
  stored one was confirmed when connecting, a file can be stale or planted. Hashed host names
  and `@cert-authority` or `@revoked` lines are skipped.
- Passwords stored by MobaXterm or SecureCRT are never read.
- Files over 10 MiB are refused; broken lines are skipped and listed, never the whole import.
- Key files named in a session (PuTTY, SecureCRT) are pointed out; import them under *Keys*.

## Export

*Hosts → New → Export* writes everything, one place or one group with its subgroups:

- **CSV**: one row per host with the values it effectively uses (a flat table has no
  inheritance), in the columns the CSV import reads back.
- **JSON**: groups with their parents and tags, hosts with their own values, so inheritance stays.

No export contains a private key, a passphrase or anything else from the vault. A stored key
appears only with its name and fingerprint, an agent key only with its fingerprint.
