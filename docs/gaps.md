# Gaps

> **Last updated:** 2026-11-15

The ranked list of open work. Pick from the top. When work lands, remove or update its row.

| Rank | Gap | Where it is planned |
|---|---|---|
| 1 | A new mark in the log viewer is placed by line breaks: with long wrapped lines above it, it lands a few lines higher than seen | later |
| 2 | `ui.inspect` of the control channel lists no widgets (the AccessKit tree is built after the frame); scripts click by coordinates from a screenshot | later |
| 2a | A second start of the app does not hand over to the running one; it says SSHCraft is already open | later |
| 3 | The packages come from the release workflow (a dry run built them all); the installers, the CLI on `PATH`, the Flatpak sandbox and the native macOS menu are untried by hand | V1 |
| 4 | The MSI stops while the Tauri version is installed and says to uninstall it, instead of removing it itself (spec section 2). Running that uninstaller from the MSI waits until its silent mode is tried on a machine with the Tauri version, so it cannot delete the data | V1 |
| 4a | FreeBSD: the last output of a local shell that ends at once (a `sh -c` command) can be lost; an interactive shell is not affected | later |
| 6 | The app lock needs Windows Hello: Touch ID on macOS (through a safe wrapper such as `robius-authentication`) and a way on Linux and FreeBSD are not there yet | later, needs a Mac to try |
| 7 | No SFTP | S1 |
| 8 | No server image in a registry: the shared release workflow pushes none, so the image is built from `packaging/docker/Dockerfile` | needs the shared workflow |
