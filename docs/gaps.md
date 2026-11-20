# Gaps

> **Last updated:** 2026-11-20

The ranked list of open work. Pick from the top. When work lands, remove or update its row.

| Rank | Gap | Where it is planned |
|---|---|---|
| 2b | `ssh://` links do not reach SSHCraft on macOS: macOS hands links over as Apple Events, which winit does not pass on, so no handler is registered there | later, needs a Mac to try |
| 2f | No images in the terminal (Sixel, Kitty, iTerm2) and no file transfer through the terminal (ZMODEM, trzsz); files go over SFTP | not planned for now |
| 2e | No Mosh and no Eternal Terminal: both would need their own UDP or TCP protocol beside SSH and a server program; tmux sessions that stay cover a dropped connection | not planned for now |
| 2g | No Kerberos (GSSAPI) sign-in: the SSH library has no GSSAPI, and adding it means binding the system's Kerberos libraries on every platform | not planned for now |
| 2h | No smart cards or hardware tokens through PKCS#11: loading a PKCS#11 module calls C code from a library the user names, which needs `unsafe` outside `crates/fido`; FIDO2 security keys work | not planned for now |
| 2d | No secure keyboard entry on macOS: it needs `EnableSecureEventInput`, reachable only through `unsafe` outside `crates/fido`, and a Mac to try | later |
| 3 | The Windows x64 MSI and the x86_64 Linux packages are tried by hand (the .deb on Debian 13 and Ubuntu 24.04, the .rpm on Fedora, the Arch package, the AppImage and the tarball, in containers under Xvfb: installed, `sshcraft-cli` on `PATH`, the window opens); the Flatpak sandbox is not yet | later |
| 6 | The app lock needs Windows Hello or, on Android, the fingerprint, face or screen lock: Touch ID on macOS (through a safe wrapper such as `robius-authentication`) and a way on Linux and FreeBSD are not there yet | later, needs a Mac to try |
| 7a | Releases are unsigned on Windows and have no macOS packages: there is no certificate or Apple account. SSHCraft's own release workflow builds Windows unsigned while no certificate is set up and leaves macOS out ([releasing.md](releasing.md#what-gets-built)) | later, needs a certificate and an Apple account |
| 10 | Shell integration is not set up on connect: every way found either leaves files on the server or replaces how the login shell starts; [terminal.md](terminal.md#shell-integration) has the lines to add by hand | later |
| 2i | Android: no screen reader support (accesskit needs GameActivity) | M2 |
