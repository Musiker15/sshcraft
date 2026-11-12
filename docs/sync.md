# Sync on the device

How the SSHCraft app keeps hosts, groups and keys in step through your own sync servers. The
server side is described in [server.md](server.md) and [server-api.md](server-api.md), the
formats and checks in [protocol.md](protocol.md).

## Places

Every group, host and key lives in one place: **this device only**, or **one sync server**.
The host list marks synced items with the server's name. Right-click an item and choose *Move
to* to change its place:

- A group takes all its hosts along. A host inside a group always lives where its group lives
  and cannot be moved on its own.
- Moving an item off a server removes it there (and so on your other devices); it stays on the
  device that moved it.
- Removing a server in the settings keeps its items on this device, as local items.
- Keys move on the *Keys* page, and only to a server with key sync on.

## Setting up a server

In *Settings → Sync → Add server* there are two ways:

- **New account.** Enter the server address (exactly the address the server was set up for),
  the API key from the operator, your name and email. The app creates the account key on this
  device, registers the account, creates your personal area and shows the **account key once**,
  with a QR code. Store it in Proton Pass or another password manager. Whoever loses it loses
  access to the account; the server cannot restore it. The account key is never stored.
- **Join with account key.** On another device, paste the account key. The device adds itself
  to the account and reads the area keys on its own, without any other device being online.

*Add device* shows a pasted account key as a QR code for a phone, without storing it.

If a step fails (server not reachable, wrong API key, email already taken, typo in the account
key), nothing is kept on this device. If the account was already created on the server when a
later step failed, the app still shows the account key: keep it and add the server again with
*Join with account key*, which finishes the setup. A join that fails halfway takes the device it
added out of the account again, so retrying leaves no stray devices.

### The recovery device

Area keys are sealed to devices. So that the account key alone can reach your data when no
device is left, every account has a **recovery device** whose keys follow from the account key.
It appears in the device list as *Account key (recovery)* and cannot be revoked. Joining with
the account key uses it to read the area keys.

## What syncs

Per server you can switch **Sync** on or off and choose whether **keys** sync too. With key sync
off, keys stay on the device; a synced host that uses a key shows *Key missing on this device*
on the other devices until you assign one there. Keys from the Proton Pass agent never sync,
only the reference to them.

Everything is sealed on the device before it leaves it: the server sees ciphertext, version
numbers and who wrote what, never names, addresses, users or keys.

## Live and offline

Each server has one background connection. A change on another device arrives within a moment;
a change here is sent at once. Without a connection the app keeps working with what it has and
queues local changes; it reconnects on its own (after 1, 2, 4 … up to 60 seconds) and sends
them then.

**Conflicts:** when two devices change the same item, the later change wins (by the time of the
change on the device that made it). A deletion counts as a change.

## Revoking a device

*Settings → Sync → Devices* lists every device of the account with name, platform and when it
was last active. Revoking a device:

1. adds the revocation to the account's device log, signed by this device;
2. replaces the key of every area, so the revoked device cannot read anything new;
3. seals every record again with the new key.

SSH keys that were already on the revoked device may have been copied. Replace them as well.
A device that was revoked while it was offline notices on its next connection, stops syncing
and keeps its local data.

If the rotation is interrupted (network, app closed), the app stored what it was doing and
finishes it on the next sync, so the account never ends up without a usable key.

A device the server's **operator** blocked is not revoked: it shows *Blocked by the server's
operator*, keeps trying, and syncs again once the block is lifted.

With key sync off, the key a host uses is chosen on each device: it is not sent to the server,
and another device's choice does not replace it.

## Sharing

A host or a group at a server can be shared with other accounts on the **same** server:
right-click it on the hosts page and choose *Share*. Each share is an area of its own with the
recipients as members; the server sees who shares with whom, never what.

**What goes along.** A group can be shared with *all hosts*: the group itself moves into the
share with its subgroups, and hosts and subgroups added to it later go along. Or with *only these hosts*: the share then holds
copies of the group and the picked hosts (also from subgroups, laid out flat in the copied group
with what they inherit in between written into them), and a single host is shared the same way. The owner's
devices keep each copy in step with its original in both directions, last change wins. Keys go
along only when asked for, and keys from the SSH agent (Proton Pass) never do; the recipient then
assigns a key of their own, which stays on their device. The key the owner uses for a shared
item without its key stays private to the owner but follows to the owner's other devices.

**Roles.** *Read* shows the items; changes the recipient makes are not sent and are reset to the
owner's state. *Read and change* sends changes back. In a share of some hosts or of one host,
changing is all a recipient may do: adding and removing hosts needs a group shared with all
hosts. Recipients can never share further, move or delete a shared group, and never change or
delete a shared key. A group of an own share is deleted only after its share is ended.

**Safety number.** Recipients are found by e-mail, but the e-mail is only a search term: what
counts is the account key. Before the first share the app shows the recipient's safety number
(six groups of five digits). Compare it over another channel, by phone or in person, and tick
that you did. The app remembers the key. If the server later names another key for the same
e-mail, the app warns, shows the old and the new number and does not share until the new one is
accepted on its own.

**Taking someone out.** *Settings → Sync → Shares* lists the own shares with their recipients
and roles, and the shares received. Removing a recipient replaces the share's key at once, so
they cannot read what follows; their copy of the items goes on their next sync. The others need
not do anything. If keys were shared, they may have been copied: the app says so, replace them on
your servers. *End share* returns a group shared with all hosts to the personal area and deletes
copies.

**Devices of recipients.** When a recipient sets up a new device, it can read the share once the
owner's app has synced. When a recipient revokes a device, the owner's app notices it in their
device log on its next sync and replaces the share's key.

**Owner deleted.** If the server's operator deletes the owner's account, the share goes to the
first recipient who may change it (see [server.md](server.md)). That recipient's app signs the
share anew as its owner with a new key; the other recipients accept the change only because the
new owner was a recipient with *read and change* before.

## What the app refuses

The app verifies everything that comes from the server: the device log must extend the state it
saw before, every record must be signed by an active device of an account allowed to write, and
an older version or a second, different record with the same version is refused. What does not
pass is skipped and reported; the local state stays as it was.
