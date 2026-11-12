# SSHCraft sync protocol and cryptography

This document describes the formats and checks implemented in `crates/sshcraft-proto`. It is
meant for anyone who writes their own SSHCraft client or server. The crate is the reference;
where this text and the code disagree, the code wins and this text is a bug.

Goal: a self-hosted sync server stores hosts, groups and SSH keys for its users without being
able to read them, and without being able to slip in changes unnoticed, even if the server is
taken over.

## Building blocks

No custom cryptography, only these primitives:

| Purpose | Primitive | Crate |
|---|---|---|
| Signatures | Ed25519, strict verification | `ed25519-dalek` 3 |
| Encrypting to a device | HPKE (RFC 9180), base mode, DHKEM(X25519, HKDF-SHA256), HKDF-SHA256, ChaCha20Poly1305 | `hpke` 0.15 |
| Encrypting records | XChaCha20-Poly1305, random 24-byte nonce | `chacha20poly1305` 0.11 |
| Hashes | SHA-256 | `sha2` 0.11 |

Binary fields in JSON are Base64 with the URL alphabet and no padding.

## Canonical bytes and domains

Everything that is signed, hashed or used as AEAD associated data is built with
`signing_bytes(domain, fields)`: the domain and then every field, each as a big-endian `u32`
length followed by its bytes. Field boundaries are therefore unambiguous, also for empty fields.
Integers are big-endian, UUIDs are their 16 bytes, strings are UTF-8.

Every kind of object has its own domain, so a signature for one kind never verifies as another:

| Domain | Used for |
|---|---|
| `sshcraft/device-cert/v1` | device certificate, signed by the account key |
| `sshcraft/device-log/v1` | device log entry, signed by the account key or a device |
| `sshcraft/device-log-hash/v1` | hash of a device log entry |
| `sshcraft/area-manifest/v1` | area manifest, signed by an owner device |
| `sshcraft/record/v1` | record signature |
| `sshcraft/record-aad/v1` | record header as AEAD associated data |
| `sshcraft/area-key/v1` | HPKE `info` and associated data of key envelopes |
| `sshcraft/safety-number/v1` | safety number |
| `sshcraft/server-auth/v1` | server login signature |
| `sshcraft/area-key-check/v1` | commitment to an area key, inside the manifest |

## Keys

**Account key.** An Ed25519 key, created on the client. It is the root of an account: it signs
device certificates and may add and revoke devices. It is not kept on devices after setup; the
user stores it, for example in a password manager.

Export format: `sshcraft-account-1:` followed by Base32 (RFC 4648, upper case, no padding) of the
32-byte seed and the first 4 bytes of SHA-256 of the seed. On import, all whitespace is removed
and letter case is ignored. A wrong prefix or length is a format error; a wrong checksum is
reported as such, so a typo never yields another valid-looking key.

**Safety number.** SHA-256 over `signing_bytes(sshcraft/safety-number/v1, [account public key])`;
the first 30 bytes in six groups of five bytes, each read as a big-endian number modulo 100000
and written with five digits, separated by spaces. Two people compare it to make sure they see
the same account (the server could otherwise hand out its own key).

**Device keys.** Each device has an Ed25519 signing key (records, log entries, server login) and
an X25519 HPKE key (receiving area keys). The account signs a **device certificate** over device
id, account public key, both device public keys, name, platform and creation time.

**Server login.** The device signs
`signing_bytes(sshcraft/server-auth/v1, [challenge, device id, origin])` with its signing key;
the server checks it against the device certificate. `origin` is the server address in normal
form (`normalize_origin`: scheme and host in lower case, the port only when it is not the
scheme's default, no path): `HTTPS://Sync.Example.org:443/api/` signs as
`https://sync.example.org`. Binding the origin keeps a server from relaying a login to another
server. Device keys are still meant for one server each: every server has its own account.

A valid login signature only proves the key. The server must also check that the device is
active in the account's verified device log before it accepts the login.

## Device log

Each account has an append-only log. Entry fields: `seq`, `prev_hash`, `action` (add a device
certificate, or revoke a device id), `signer` (the account, or a device id), `created_at` and the
signature over all of them. The hash of an entry is SHA-256 over
`signing_bytes(sshcraft/device-log-hash/v1, [signed bytes, signature])`.

A client accepts a log only if, replayed from the start:

- `seq` starts at 0 and has no gaps, and `prev_hash` is 32 zero bytes for the first entry and the
  hash of the previous entry after that;
- an *add* is signed by the account key, carries a valid certificate of this account and a device
  id not seen before;
- a *revoke* is signed by the account key or by a device that is active **at that point** of the
  log, and targets an active device.

A revoked device can sign nothing afterwards. If two devices revoke each other at the same time,
both entries point to the same predecessor and only one of them can become the next entry; the
other can never be chained. **The server decides which one that is.** A server working with a
thief who holds a stolen device can therefore keep the stolen device and drop the legitimate one.
The way out is the account key: it can revoke any device and enroll a new one (under a new
device id, ids are never reused).

A server must run the full log verification on every append. One invalid entry would make the
append-only log unverifiable for every client.

A client remembers the last head (`seq`, hash) it accepted. A later log that is shorter is a
**rollback**; one with a different entry at that position is a **fork**. Both are refused.

## Areas and area keys

An area is a person's own data or a share. It has a random 32-byte key and a key version. The
key is sent to every device that may read the area in a **key envelope**: HPKE seal to the
device's HPKE key with `info = "sshcraft/area-key/v1"` and associated data
`signing_bytes(sshcraft/area-key/v1, [area id, key version, device id])`. A key envelope moved to
another area, key version or device does not open.

Envelopes are **not authenticated**: anyone who knows a device's public HPKE key, the server
included, can seal one. What ties a key to the owner is the **key commitment** in the signed
manifest: SHA-256 over `signing_bytes(sshcraft/area-key-check/v1, [area id, key version, key])`.
A client opens an envelope only against a verified manifest and refuses a key whose commitment
does not match (`WrongAreaKey`). Without this, a server could hand a device a key it knows and
read everything that device writes afterwards.

The **area manifest** names the owner account, the members with their role (`read` or `write`),
the key version and a version number. It is signed by one of the owner's active devices. A
personal area has no members. Rotating the key (after revoking a device or a member) means a new
key version, new envelopes for everyone who still has access, and a new manifest.

Only the owner's devices can sign a manifest, so only the owner can rotate an area. When a
member revokes one of their own devices, the owner's client has to notice it in the member's
device log and rotate every area it shares with that member.

After a rotation, and after a device is revoked, records written under the old key version or
by the revoked device no longer open: `open` requires the current key version and an active
author device. They have to be re-sealed by an active device under the new key.

## Records

A record holds one item (a host, a group, a key, ...). Its header is readable for the server:
record id, area id, version, key version, author account, author device. The body is encrypted
with the area key; the associated data is the header (`sshcraft/record-aad/v1`). The author
device signs `signing_bytes(sshcraft/record/v1, [header bytes, nonce, ciphertext])`.

A client opens a record only after it has verified the area's manifest (its signature by the
owner and the key checksum, see above); `SealedRecord::open` takes that manifest as given. Then
these checks run, in this order:

1. area id and key version match the manifest;
2. the author account may write the area (owner, or member with `write`);
3. the author device is active in the author's verified device log and belongs to that account;
4. the signature is valid;
5. the body decrypts.

**Versions.** A client keeps, per record id and, separately, per area manifest, the highest
version it has accepted and a digest of it (SHA-256 of the signature). A lower version is a
rollback; the same version with a different digest is a fork. Both are refused. Only records
that opened and manifests that verified are counted (`VersionTracker::accept_record` and
`accept_manifest` do both steps), so nothing unverified can move a counter.

Two rules keep normal offline edits from looking like a fork:

- the server accepts a new version of an id only as `previous + 1` (compare and swap) and
  rejects the second of two concurrent writes;
- a client counts its own write only after the server acknowledged it. A rejected write is a
  conflict: the client fetches the current version, merges ("last change wins" per item) and
  writes `current + 1`.

## What the server sees

The server stores and forwards ciphertext and public data. It sees: account public keys, device
certificates (including device names and platforms), device logs, area manifests (who shares with
whom and with which role), record headers, and the number, size and timing of records. It does
not see host names, addresses, user names, passwords or keys. Hiding this metadata is out of
scope for now.
