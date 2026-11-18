# Gaps

> **Last updated:** 2026-11-18

The ranked list of open work. Pick from the top. When work lands, remove or update its row.

| Rank | Gap | Where it is planned |
|---|---|---|
| 2b | `ssh://` links do not reach SSHCraft on macOS: macOS hands links over as Apple Events, which winit does not pass on, so no handler is registered there | later, needs a Mac to try |
| 2c | No portable mode: the vault's master key lives in the system keyring, so a copy on a USB stick needs a passphrase unlock before the engine starts | later |
| 2f | No images in the terminal (Sixel, Kitty, iTerm2) and no file transfer through the terminal (ZMODEM, trzsz); files go over SFTP once S1 is there | not planned for now |
| 2e | No Mosh and no Eternal Terminal: both would need their own UDP or TCP protocol beside SSH and a server program; tmux sessions that stay cover a dropped connection | not planned for now |
| 2g | No Kerberos (GSSAPI) sign-in: the SSH library has no GSSAPI, and adding it means binding the system's Kerberos libraries on every platform | not planned for now |
| 2h | No smart cards or hardware tokens through PKCS#11: loading a PKCS#11 module calls C code from a library the user names, which needs `unsafe` outside `crates/fido`; FIDO2 security keys work | not planned for now |
| 2d | No secure keyboard entry on macOS: it needs `EnableSecureEventInput`, reachable only through `unsafe` outside `crates/fido`, and a Mac to try | later |
| 3 | The Windows x64 MSI is tried by hand (installs and runs); the Linux packages, the Flatpak sandbox and the CLI on `PATH` are not yet | later |
| 4a | FreeBSD: the last output of a local shell that ends at once (a `sh -c` command) can be lost; an interactive shell is not affected | later |
| 6 | The app lock needs Windows Hello: Touch ID on macOS (through a safe wrapper such as `robius-authentication`) and a way on Linux and FreeBSD are not there yet | later, needs a Mac to try |
| 7 | No SFTP | S1 |
| 7a | Releases are unsigned on Windows and have no macOS packages: there is no certificate or Apple account, and the shared release workflow cannot leave a platform out or unsigned, so a release is assembled by hand from a dry run ([releasing.md](releasing.md#unsigned-releases)) | needs the shared workflow |
| 9 | The output of a command cannot be folded: the emulator's grid cannot hide lines; jumping, the gutter and copying the output are there | later |
| 10 | Shell integration is not set up on connect: every way found either leaves files on the server or replaces how the login shell starts; [terminal.md](terminal.md#shell-integration) has the lines to add by hand | later |
| 2i | Android: a word the keyboard is still composing is not shown until it is committed, and there is no screen reader support (accesskit needs GameActivity) | M2 |
| 2j | Android: no x86_64 or armv7 build and no file picking | M2 |
| 2k | No Android build checks a pull request: `android-release.yml` builds the APK for a release and by hand, but the shared CI has no Android SDK | needs the shared workflows |
| 8 | No server image in a registry: the shared release workflow pushes none, so the image is built from `packaging/docker/Dockerfile` | needs the shared workflow |
