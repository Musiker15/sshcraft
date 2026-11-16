# Gaps

> **Last updated:** 2026-11-16

The ranked list of open work. Pick from the top. When work lands, remove or update its row.

| Rank | Gap | Where it is planned |
|---|---|---|
| 1 | A new mark in the log viewer is placed by line breaks: with long wrapped lines above it, it lands a few lines higher than seen | later |
| 2 | `ui.inspect` of the control channel lists no widgets (the AccessKit tree is built after the frame); scripts click by coordinates from a screenshot | later |
| 2a | A second start of the app hands over only an `ssh://` link or `--open-host`; started plainly, it says SSHCraft is already open instead of bringing the window forward | later |
| 2b | `ssh://` links do not reach SSHCraft on macOS: macOS hands links over as Apple Events, which winit does not pass on, so no handler is registered there | later, needs a Mac to try |
| 2c | No portable mode: the vault's master key lives in the system keyring, so a copy on a USB stick needs a passphrase unlock before the engine starts | later |
| 2f | No images in the terminal (Sixel, Kitty, iTerm2) and no file transfer through the terminal (ZMODEM, trzsz); files go over SFTP once S1 is there | not planned for now |
| 2e | No Mosh and no Eternal Terminal: both would need their own UDP or TCP protocol beside SSH and a server program; tmux sessions that stay cover a dropped connection | not planned for now |
| 2d | No secure keyboard entry on macOS: it needs `EnableSecureEventInput`, reachable only through `unsafe` outside `crates/fido`, and a Mac to try | later |
| 3 | The Windows x64 MSI is tried by hand (installs and runs); the Linux packages, the Flatpak sandbox and the CLI on `PATH` are not yet | later |
| 4 | The MSI stops while the Tauri version is installed and says to uninstall it, instead of removing it itself (spec section 2). Running that uninstaller from the MSI waits until its silent mode is tried on a machine with the Tauri version, so it cannot delete the data | later |
| 4a | FreeBSD: the last output of a local shell that ends at once (a `sh -c` command) can be lost; an interactive shell is not affected | later |
| 6 | The app lock needs Windows Hello: Touch ID on macOS (through a safe wrapper such as `robius-authentication`) and a way on Linux and FreeBSD are not there yet | later, needs a Mac to try |
| 7 | No SFTP | S1 |
| 7a | Releases are unsigned on Windows and have no macOS packages: there is no certificate or Apple account, and the shared release workflow cannot leave a platform out or unsigned, so a release is assembled by hand from a dry run ([releasing.md](releasing.md#unsigned-releases)) | needs the shared workflow |
| 9 | The output of a command cannot be folded: the emulator's grid cannot hide lines; jumping, the gutter and copying the output are there | later |
| 10 | Shell integration is not set up on connect: every way found either leaves files on the server or replaces how the login shell starts; [terminal.md](terminal.md#shell-integration) has the lines to add by hand | later |
| 8 | No server image in a registry: the shared release workflow pushes none, so the image is built from `packaging/docker/Dockerfile` | needs the shared workflow |
