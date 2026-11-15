# Releasing SSHCraft

A push to the `release` branch runs [`.github/workflows/release.yml`](../.github/workflows/release.yml).
It calls the shared release workflow of the Craft apps, which builds the packages of every
platform and creates or updates a **draft** GitHub release named `SSHCraft v<version>`. Nobody
sees a draft until a maintainer publishes it. Nothing here runs on a schedule.

Names: the app is **SSHCraft** wherever people read it; files, binaries and ids are lowercase
(`sshcraft-<version>-<platform>-<arch>.<ext>`, `de.musiker15.sshcraft`).

| Value | Where |
|---|---|
| Binaries `sshcraft` and `sshcraft-cli` | every script and workflow |
| App id `de.musiker15.sshcraft` (also the keyring service and the data folder) | `packaging/macos/Info.plist.in`, `packaging/linux/*`, the Flatpak manifests |
| WiX upgrade code `E577A976-EE1A-4C98-96B7-D3A2EBE306CB`, fixed forever | `packaging/windows/sshcraft.wxs`, `install-policy.json` |
| Icon | `assets/app-icon/`, rendered by `packaging/icons.sh` |
| craft-fonts pin | `craft-fonts-ref` in `ci.yml`, `freebsd.yml` and `release.yml` |

## One-time setup (Moritz)

These need the repository's settings and secrets, so only the owner sets them up:

1. **Branch `release`**, created from `main`, with branch protection: no force push, no deletion.
2. **Environment `release`** (Settings, Environments), limited to the branch `release`. The
   signing jobs and the draft release run in it, and only the `release` branch may use it.
3. **Secrets** in that environment, each optional: a missing one leaves its artifact unsigned
   and the job summary says so.

   | Secret | For |
   |---|---|
   | `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `KEYCHAIN_PASSWORD` | macOS signing (Developer ID Application, as base64 .p12) |
   | `APPLE_ID`, `APPLE_PASSWORD` (app-specific), `APPLE_TEAM_ID` | macOS notarization |
   | `WINDOWS_CERTIFICATE`, `WINDOWS_CERTIFICATE_PASSWORD` | Windows signing with a .pfx |
   | `AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `AZURE_SIGNING_ENDPOINT`, `AZURE_SIGNING_ACCOUNT`, `AZURE_CERT_PROFILE` | Windows signing with Azure Trusted Signing instead |
   | `LINUX_SIGNING_KEY` | the GPG signature of the rpm packages and the Flatpak bundle |

The maintainer field of the deb and rpm packages says `SSHCraft maintainers`;
`SSHCRAFT_MAINTAINER` (`Name <address>`) in the environment of `packaging/linux/package.sh`
changes it.

## Cutting a release

1. **Set the version** on `main` through a pull request. It lives only in
   `[workspace.package] version` of the root `Cargo.toml`:

   ```sh
   cargo xtask version              # prints it, e.g. 0.1.0
   cargo xtask version set 0.2.0    # or 0.2.0-rc.1; updates Cargo.toml and Cargo.lock
   ```

2. **Merge `main` into `release`** (or fast-forward it) and push. The workflow starts by itself.
3. **Wait for the draft.** Notarization and the FreeBSD VM take longest. The draft
   `SSHCraft v0.2.0` then has every artifact and `SHA256SUMS.txt`, its notes generated from the
   merged pull requests.
4. **Check it.** Install a package or two and go through the hand-test list of the last pull
   requests. A `::warning::` in a job summary means a secret was missing and that artifact is
   unsigned.
5. **Publish** the draft. Publishing creates the tag `v0.2.0`; a version with a suffix
   (`-rc.1`) is marked as a pre-release.

Pushing to `release` again before publishing rebuilds the same draft. After publishing, the
workflow refuses to touch that version again: set a new one first.

**Test runs:** Actions, Release, Run workflow runs the pipeline by hand, as a dry run by default
(no signing, no draft). The optional `version` input overrides `Cargo.toml` for that run only. A
run by hand with dry run off asks for the `release` environment, so from any branch but `release`
its signing jobs stop there.

## What gets built

| Platform | Artifacts |
|---|---|
| macOS 11+ (Apple silicon and Intel) | `sshcraft-<v>-macos-universal.dmg`, `sshcraft-cli-<v>-macos-universal.zip` |
| Windows 10+ x64, x86, ARM64 | `sshcraft-<v>-windows-<arch>.msi`, `sshcraft-<v>-windows-<arch>-portable.zip` |
| Linux x86_64, aarch64 | `sshcraft-<v>-linux-<arch>.{AppImage,AppImage.zsync,deb,rpm,tar.gz}` |
| Flatpak x86_64, aarch64 | `sshcraft-<v>-linux-<arch>.flatpak`, repackaging the Linux tarball |
| FreeBSD 14 x86_64 | `sshcraft-<v>-freebsd-x86_64.tar.gz` |

SSHCraft has no web build: SSH needs TCP. The sync server's Docker image and Linux program get
their release path later (V1).

### Per platform

- **macOS:** [`packaging/macos/package.sh`](../packaging/macos/package.sh) builds both
  architectures, joins them with `lipo`, signs inside-out with the hardened runtime
  (`entitlements.plist` is empty on purpose), notarizes and staples the app and the DMG. Without
  a certificate it signs ad hoc, which is enough to try it locally:
  `packaging/macos/package.sh --arch aarch64`.
- **Windows:** [`packaging/windows/package.ps1`](../packaging/windows/package.ps1) builds with a
  static C runtime, checks the PE headers (the app is a GUI program, the CLI a console one),
  signs, builds the MSI with WiX 5 and checks it against the installation policy before signing
  it ([`packaging/windows/INSTALLATION.md`](../packaging/windows/INSTALLATION.md)).
- **Linux:** [`packaging/linux/package.sh`](../packaging/linux/package.sh) stages one file tree
  (binaries, desktop entry, AppStream metadata, icons, licences) and makes the tarball, the deb
  and rpm with nfpm, and the AppImage with a pinned appimagetool. `--formats "deb rpm tar"`
  skips the AppImage.
- **Flatpak:** [`packaging/linux/flatpak-bundle.sh`](../packaging/linux/flatpak-bundle.sh) wraps
  the tarball. The sandbox has the network, the Secret Service and `~/.ssh` read-only; serial
  ports and the host's own shell stay outside it.
- **FreeBSD:** [`packaging/freebsd/package.sh`](../packaging/freebsd/package.sh) runs in a
  FreeBSD VM ([`freebsd.yml`](../.github/workflows/freebsd.yml)).

Builds with `CRAFT_FONTS_DIR` embed craft-fonts' interface faces, and every package carries their
licences (`OFL-<family>.txt`) next to `NOTICE` and `ATTRIBUTION.md`.

The packaging scripts are adapted from [storytold/gridcraft](https://github.com/storytold/gridcraft)
(Apache-2.0).
