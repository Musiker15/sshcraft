# Security policy

## Reporting a vulnerability

Please report security problems privately through GitHub:
**[Report a vulnerability](https://github.com/Musiker15/sshcraft/security/advisories/new)**
(Security tab, Advisories, Report a vulnerability).

Don't open a public issue or pull request for a vulnerability until a fix is released.

Useful to include:

- the SSHCraft version or commit, and the platform (Windows, macOS, Linux, BSD), or the version of
  `sshcraft-server` and its database (SQLite or PostgreSQL);
- what an attacker can do, and what they need (a malicious server, a crafted import file, a
  compromised sync server, MCP or control-channel access, a local account);
- steps or a sample file that reproduce it.

We'll acknowledge the report, keep you updated while we work on a fix, and credit you in the
advisory unless you'd rather not be named.

## Supported versions

Fixes go into `main` and ship in the next release. Older releases don't get backported fixes.

## Scope

In scope: anything that lets a server, a sync server, an import file, terminal output, an MCP or
control-channel request, or a network response

- read a private key, a password or a passphrase, or move a stored credential to another address;
- read or change hosts, keys or session logs through the sync server, which must never be able to
  read them (end-to-end encryption, signed records, protection against rollback);
- run code or write files it shouldn't;
- let an AI agent do more than its rights allow, or confirm its own request;
- crash SSHCraft or `sshcraft-server` and lose the user's work.

Crashes from hostile input count, because SSHCraft treats every panic as a bug (see "Never crash"
in [AGENTS.md](AGENTS.md)). The rules that keep these from happening are in
[docs/security.md](docs/security.md).
