# Running the sync server

`sshcraft-server` stores what SSHCraft clients sync: accounts, device logs, areas, key envelopes
and encrypted records. It cannot read hosts, keys or names; see
[What the server sees](protocol.md#what-the-server-sees). The HTTP interface is described in
[server-api.md](server-api.md).

The server speaks plain HTTP on one port. Put a reverse proxy with TLS in front of it.

## Settings

| Option | Environment | Default | Meaning |
|---|---|---|---|
| `--public-origin` | `SSHCRAFT_PUBLIC_ORIGIN` | (required) | The address clients type in, e.g. `https://sync.example.org` |
| `--listen` | `SSHCRAFT_LISTEN` | `127.0.0.1:8470` | Address and port the server listens on |
| `--database-url` | `SSHCRAFT_DATABASE_URL` | `sqlite://sshcraft.db` | `sqlite://<path>` or `postgres://user:password@host/database` |
| | `RUST_LOG` | `info` | Log level |

**The public origin must match what clients use.** Every login is signed for this address, so a
server cannot pass a login on to another server. Scheme and host count, the default port and any
path do not: `https://sync.example.org` and `https://Sync.Example.org:443/` are the same.

## Start

As a program:

```bash
sshcraft-server serve --public-origin https://sync.example.org --database-url sqlite:///var/lib/sshcraft/sshcraft.db
```

With Docker (the image keeps its SQLite file in the volume `/data`):

```bash
docker build -f crates/sshcraft-server/Dockerfile -t sshcraft-server .
docker run -d --name sshcraft -v sshcraft-data:/data \
  -e SSHCRAFT_PUBLIC_ORIGIN=https://sync.example.org \
  -p 127.0.0.1:8470:8470 sshcraft-server
```

On first start the server creates the **API key** and writes it to the log once. People need it
to register and to log in a new device. Show it again any time:

```bash
sshcraft-server api-key show
```

In Docker: `docker exec sshcraft sshcraft-server api-key show`.

## PostgreSQL

SQLite is the default and enough for a few dozen users. For PostgreSQL create an empty database
and pass its URL; the server creates its tables on start.

```bash
sshcraft-server serve --public-origin https://sync.example.org \
  --database-url postgres://sshcraft:secret@localhost/sshcraft
```

The connection uses no TLS, so the database should run on the same host. `--help` never prints
the value of `SSHCRAFT_DATABASE_URL`, but the URL is still a secret: keep it out of shell history,
for example in a systemd `EnvironmentFile` readable only by the service user.

## Apache as reverse proxy

Apache 2.4.47 or newer forwards the WebSocket for live updates with `upgrade=websocket`. Needed
modules: `proxy`, `proxy_http`, `ssl`, `headers`.

```apache
<VirtualHost *:443>
    ServerName sync.example.org

    SSLEngine on
    SSLCertificateFile    /etc/ssl/certs/sync.example.org.pem
    SSLCertificateKeyFile /etc/ssl/private/sync.example.org.key

    ProxyPreserveHost On
    ProxyPass        / http://127.0.0.1:8470/ upgrade=websocket
    ProxyPassReverse / http://127.0.0.1:8470/

    # Open WebSockets stay idle between pings (every 30 s).
    ProxyTimeout 120
</VirtualHost>
```

## Operator commands

All commands work on the database directly and take the same `--database-url`. They do not need
the server to run.

| Command | Effect |
|---|---|
| `sshcraft-server users list` | All users: email, name, created, active devices, locked |
| `sshcraft-server users lock <email>` | No more logins; open sessions end. Areas the user owns stay readable for members but nobody can change them |
| `sshcraft-server users unlock <email>` | Lifts the lock |
| `sshcraft-server users delete <email>` | Shows what would happen to the user's areas, changes nothing |
| `sshcraft-server users delete <email> --yes` | Deletes the user. An area with a member who may write goes to the first such member; every other area of the user is deleted for everyone |
| `sshcraft-server devices list <email>` | The user's devices: id, name, platform, state, last seen |
| `sshcraft-server devices block <email> <device-id>` | The device can no longer log in; open sessions and WebSockets end within 30 seconds |
| `sshcraft-server devices unblock <email> <device-id>` | Lifts the block |
| `sshcraft-server api-key show` | Prints the API key |
| `sshcraft-server api-key rotate` | Creates a new API key. Devices that have logged in before keep working; new users and new devices need the new key |

The server knows no names of areas (they are encrypted), so commands name areas by id and member
count.

**What blocking a device does not do.** The server holds no key and cannot sign a revocation. A
blocked device is turned away, but the area keys it already has stay valid until the user revokes
the device from another device; only then are the keys replaced. Tell the user to do that.

## Backup

- SQLite: copy the database file while the server is stopped, or use `sqlite3 sshcraft.db ".backup backup.db"` while it runs.
- PostgreSQL: `pg_dump`.

A backup holds only what the server sees anyway: public keys, device logs, area manifests and
encrypted records. The API key is in it too.
