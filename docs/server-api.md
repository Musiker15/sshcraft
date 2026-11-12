# Sync server API

Version 1, all paths under `/v1`. Request and response bodies are JSON. Binary fields, and the
account key in paths, are Base64 with the URL alphabet and no padding. The protocol objects
(`DeviceLog`, `LogEntry`, `AreaManifest`, `KeyEnvelope`, `SealedRecord`) are described in
[protocol.md](protocol.md).

## Access

| Kind | How |
|---|---|
| none | |
| API key | Header `X-Api-Key: <key>`; the operator hands it out |
| session | Header `Authorization: Bearer <token>`; the token comes from login |

A session is checked on every request: it fails as soon as the account is locked, the device is
revoked in its log or blocked by the operator, or the token is older than 30 days.

## Errors

Every error is `{"code": "...", "detail": "..."}`; `detail` is optional and meant for logs, not for
people. Version conflicts add `"current": <number>`.

| Code | Status | Meaning |
|---|---|---|
| `bad_api_key` | 401 | API key missing or wrong |
| `unauthorized` | 401 | No valid session, or the login failed |
| `forbidden` | 403 | Not allowed for this account |
| `account_locked` | 403 | The account (or the owner of the area) is locked by the operator |
| `device_inactive` | 403 | The device is revoked or blocked |
| `not_found` | 404 | No such account, area, envelope or route |
| `conflict` | 409 | Someone else wrote first; `current` is the stored version or log length |
| `email_taken` | 409 | Another account uses this email |
| `account_exists` | 409 | The account key is already registered |
| `device_id_taken` | 409 | The device id belongs to another device |
| `invalid` | 400 | Malformed or failed a check |
| `too_large` | 413 | Body over 1 MiB, or record ciphertext over 64 KiB |
| `internal` | 500 | Server fault; the cause is in the server log only |

## Server

`GET /v1/server` (none): `{"name": "sshcraft-server", "version": "...", "api": 1, "origin": "https://sync.example.org"}`.
`origin` is the address logins must be signed for.

## Accounts and login

`POST /v1/accounts` (API key): `{"account", "name", "email", "log"}`. The log holds exactly one
entry, the account key adding the first device. Name 1 to 100 characters, email with one `@` and
at most 254 characters, compared in lower case. Device name 1 to 100, platform 1 to 32 characters.
Answer `201 {"account"}`.

`POST /v1/auth/challenge` (none): `{"challenge"}`, 56 bytes (random nonce, expiry, and an HMAC
only this server process can make), valid for 60 seconds and for one successful login. The server
stores nothing for a challenge until it is used, so asking for many costs it nothing.

`POST /v1/auth/login` (none; API key on a device's first login): `{"device_id", "challenge",
"signature"}`, where the signature is `DeviceKeys::sign_auth(challenge, device_id, origin)`.
Answer `{"token", "expires_at", "account"}`; the session lasts 30 days. After that the device logs
in again with its key alone.

`GET /v1/accounts/me` (session): `{"account", "name", "email", "created_at"}`.

`GET /v1/accounts/me/devices` (session): `{"devices": [{"device_id", "active", "admin_blocked",
"last_seen"}]}`. Names and platforms are in the device log.

`GET /v1/accounts/lookup?email=...` (session): `{"account", "name"}` or `not_found`.

`GET /v1/accounts/{account}` (session): `{"account", "name"}`, no email.

## Device log

`GET /v1/accounts/{account}/log?from=<seq>` (session): `{"entries": [LogEntry]}` from `from`
(default 0). **Any logged-in account may read any account's log and name**: a client has to
encrypt to the devices of the person it shares with. This shows device names and platforms to the
other users of the server.

`POST /v1/accounts/{account}/log` (session of that account, or API key without a session):
`{"entries": [LogEntry]}`, 1 to 100 entries; the first one must have `seq` equal to the current
length (else `conflict` with `current`). The whole log must still verify; at most 1,000 entries
per account. A new device uses the API key to add itself with an entry signed by the account key;
without a session, entries signed by a device are `forbidden`.
Answer `{"len"}`. Revoked devices lose their sessions and WebSockets at once.

## Areas

`GET /v1/areas` (session): `{"areas": [{"owner", "manifest"}]}`, the areas the account owns or
is a member of.

`PUT /v1/areas/{area_id}/manifest` (session of the owner): an `AreaManifest`. It must be signed by
an active device of the owner, name only accounts of this server as members (not the owner, none
twice, at most 1000), and carry version 1 for a new area or the stored version plus one. Another
account's area id: `forbidden`. A higher key version drops the old envelopes. Answer `{"version"}`.

`DELETE /v1/areas/{area_id}` (session of the owner).

`PUT /v1/areas/{area_id}/envelopes` (session of the owner): `{"envelopes": [KeyEnvelope]}`, at
most 1000, all for this area and its current key version, each for an active device of the owner
or a member. Answer `{"count"}`.

`GET /v1/areas/{area_id}/envelope` (session of owner or member): the envelope for the calling
device, or `not_found`.

## Records

`PUT /v1/areas/{area_id}/records/{record_id}` (session): a `SealedRecord` whose header names this
area and record, the calling account and device, and the area's current key version. The account
must be the owner or a member with `write`; the owner must not be locked. Version 1 for a new
record, else the stored version plus one. Ciphertext at most 64 KiB. Answer `{"seq"}`, the area's
change number.

`GET /v1/areas/{area_id}/records?since=<seq>&limit=<n>` (session of owner or member):
`{"records": [{"seq", "record"}], "more"}`, records whose latest write is after `since`, oldest
first. `limit` defaults to 500, at most 1000. The server does not check record signatures; clients
do.

## Live updates

`GET /v1/events` (session): a WebSocket. The server sends JSON text messages and a ping every
30 seconds; at each ping it checks the session again and ends the connection with
`session_ended` if it is no longer valid. An event only says that something changed; fetch the
data over HTTP, and resync after every new connection.

| `type` | Fields | Sent to |
|---|---|---|
| `device_log` | `account`, `len` | the account and everyone sharing an area with it |
| `manifest` | `area_id`, `version` | owner and members, including members just removed |
| `envelopes` | `area_id`, `key_version` | owner and members |
| `record` | `area_id`, `record_id`, `version`, `seq` | owner and members |
| `area_deleted` | `area_id` | owner and members |
| `session_ended` | | the device whose session ended; the connection closes |

A connection that falls 256 events behind is closed.
