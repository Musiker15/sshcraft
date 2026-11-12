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
key), nothing is kept on this device. A failed account creation can leave the account on the
server; trying again with the same email then reports it as taken.

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

## What the app refuses

The app verifies everything that comes from the server: the device log must extend the state it
saw before, every record must be signed by an active device of an account allowed to write, and
an older version or a second, different record with the same version is refused. What does not
pass is skipped and reported; the local state stays as it was.
