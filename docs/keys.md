# Keys

> The desktop app is being rebuilt (R1 to R7); this page describes the behaviour the rebuild keeps.

What kinds of keys SSHCraft holds, how their passphrase is kept, how certificates are used, how a
public key gets onto a server, and what FIDO2 security keys and Windows Hello keys are.

## Kinds of keys

| Kind | Where the secret is | Syncs | Can be shared |
|---|---|---|---|
| Vault key (Ed25519, ECDSA, RSA) | sealed in SSHCraft's vault | with key sync on | with "share with keys" |
| FIDO2 security key | on the token; the vault holds only a key handle | with key sync on | with "share with keys" |
| Windows Hello key | in Windows Hello (TPM) on this device | never | never |
| Proton Pass agent key | in Proton Pass | only the reference | only the reference |

**New keys** are Ed25519 (the default), ECDSA on `nistp256`, `nistp384` or `nistp521`, or RSA with
3072 or 4096 bits. Smaller RSA keys are refused. Importing takes OpenSSH, PEM (PKCS#1, PKCS#8) and
PuTTY files of any of these types.

## Passphrase: saved or asked

A key can come with a passphrase, or you can give a new key one. You choose whether SSHCraft
**saves** it:

- **Saved** (the default): the key is kept decrypted inside the vault, which itself is sealed
  with a master key from the Windows Credential Manager. Windows Hello is asked once per run
  before the first such key is used.
- **Asked**: the key stays encrypted with its passphrase inside the vault. SSHCraft asks for the
  passphrase every time the key is used, at most three times per connection; Windows Hello is not
  asked then, the passphrase is the check. The decrypted key exists only in memory while signing
  in. The key list says *Passphrase asked*.

A key can switch either way later: *Save passphrase* or *Protect with passphrase* in its context
menu. The passphrase never goes to disk, a log, the sync server or the UI. A key whose passphrase
is asked syncs and is shared still encrypted with it, so another device asks for the same
passphrase.

## Certificates

An OpenSSH **user certificate** (`id_ed25519-cert.pub` and the like) can be attached to a key:
pasted, from a file, or picked up automatically when a key file is imported and its
`-cert.pub` lies next to it. It must be a user certificate for exactly this key; anything else is
refused. The key list shows until when it is valid.

When connecting, a valid certificate is offered first. If the server turns it down, or if it has
expired (or is not valid yet), the key signs in on its own; an expired certificate leaves the
note *Certificate expired* in the session's status line. Certificates are public data and travel
with their key when it syncs or is shared.

## Adding a key to `authorized_keys`

*Add to authorized_keys* (on a key, or on a host) signs in on the host the usual way, with all its
questions, and runs one command:

```sh
sh -c 'umask 077; d="$HOME/.ssh"; f="$d/authorized_keys"; mkdir -p "$d" && touch "$f" || exit 1;
       if grep -qF "<type> <base64>" "$f"; then echo SSHCRAFT_PRESENT; exit 0; fi;
       if [ -s "$f" ] && [ -n "$(tail -c 1 "$f")" ]; then echo >> "$f"; fi;
       printf "%s\n" "<line>" >> "$f" && echo SSHCRAFT_ADDED'
```

- The line is built from the key itself: type, Base64 data and its comment cut to letters,
  digits and `@._-`. Nothing typed in the UI goes into the command.
- A key that is there already (same type and data, whatever options or comment its line has)
  is not added twice.
- A file without a final newline gets the key on a line of its own.
- A FIDO2 key that asks for its PIN gets `verify-required` in front, so the server checks it.
- It needs a POSIX `sh` on the server. Windows' OpenSSH server and forced commands do not run
  it; SSHCraft then says so instead of reporting success.

## FIDO2 security keys

A YubiKey or a similar FIDO2 token holds the private key; it never leaves the token. SSHCraft
creates an OpenSSH `sk-` key on it (`sk-ecdsa-sha2-nistp256@openssh.com` by default,
`sk-ssh-ed25519@openssh.com` on request) with the application `ssh:`, the same as
`ssh-keygen -t ecdsa-sk` does. Keys made with `ssh-keygen` can be imported as well.

Each sign-in asks the token: you touch it, and with *PIN on every sign-in* (the default) also
enter its PIN. Windows shows that dialog; SSHCraft talks to the token through the Windows
WebAuthn API, as Windows requires since 1903. The server needs OpenSSH 8.2 or newer.

What the vault keeps of such a key is only its handle on the token, so it syncs like any key: on
another device it works as soon as the token is plugged in there.

## Windows Hello keys

A Windows Hello key is created inside Windows Hello, backed by the TPM where the PC has one, and
**never leaves this device**: it does not sync, is not shared (not even with "share with keys")
and cannot be moved to a server. A host that uses it shows *Key missing on this device* on other
devices. Each sign-in asks Windows Hello (fingerprint, face or PIN). It is an
`sk-ecdsa-sha2-nistp256@openssh.com` key, so the server needs OpenSSH 8.2 or newer.

Deleting the key in SSHCraft also removes it from Windows Hello where Windows allows that
(WebAuthn API version 4 and later).

## Platforms

FIDO2 and Windows Hello keys work on Windows for now; on other systems SSHCraft says they are not
supported there. macOS, Linux and FreeBSD follow with their platform plans.
