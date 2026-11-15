# Windows installation policy checks

The MSI installs SSHCraft for the current account under `%LOCALAPPDATA%\Programs\SSHCraft` by
default. The first setup page offers installation for everyone, under
`Program Files\SSHCraft`. Scope is selected before duplicate checks and directory costing. An
update uses the existing context's remembered folder; `INSTALLFOLDER` supplied explicitly is
never replaced. It is a secure MSI property so the chosen folder reaches the elevated installer.
Silent machine updates pass `ALLUSERS=1`. A cross-context copy is refused; changing context
means uninstalling the old copy first.

What the package installs:

- `sshcraft.exe` (the app, with a Start menu entry) and `sshcraft-cli.exe` (the command line and
  MCP server for agents), with the folder on `PATH`: the user's for a per-user install, the
  system's for a per-machine one.
- A desktop shortcut only when its box is ticked; the choice is kept for updates.
- No file types and no `ssh://` handler. Links never run anything; a link handler comes with F1
  and only opens the connection dialog.

The upgrade code (`install-policy.json`) never changes: it is how a new version replaces an old
one.

## Repeatable metadata tests

On Windows with WiX 5.0.2:

```powershell
./packaging/windows/test-install-policy.ps1 -OutputDirectory ./target/install-policy-tests
```

The script builds x64, x86 and arm64 packages with deliberately non-executable payloads from the
real package source. A safe Windows Installer session checks the scope, registration and
maintenance cases, remembered and explicit custom folders, sequencing, the upgrade identity and
the registrations recorded in `install-policy.json`. `package.ps1` runs the same checker before
signing, and the packaging lint workflow runs the fixture builds without compiling Rust.

Never install or distribute the `NOT-INSTALLABLE.msi` fixtures. Passing these checks does not
prove a real installation, elevation, upgrades, the `PATH` change, the optional shortcut,
cancellation, rollback or uninstall. Those need the real payload on a clean Windows machine.

The scripts here are adapted from [storytold/gridcraft](https://github.com/storytold/gridcraft)
(Apache-2.0).
