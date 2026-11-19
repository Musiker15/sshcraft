# Releasing SSHCraft

A push to the `release` branch runs [`.github/workflows/release.yml`](../.github/workflows/release.yml).
It builds the packages of every platform, checks that none is missing, and creates or updates a
**draft** GitHub release named `SSHCraft v<version>` with all of them, `SHA256SUMS.txt` and the
SBOM. Nobody sees a draft until a maintainer publishes it; the workflow never publishes. Nothing
here runs on a schedule.

SSHCraft has its own release workflow, not the shared one of the Craft apps
([decisions.md](decisions.md), 2026-11-18): that one could not leave out macOS, ship Windows
unsigned, build Android or fix the Arch package's dependencies.

Names: the app is **SSHCraft** wherever people read it; files, binaries and ids are lowercase
(`sshcraft-<version>-<platform>-<arch>.<ext>`, `de.musiker15.sshcraft`).

| Value | Where |
|---|---|
| Binaries `sshcraft` and `sshcraft-cli` | every script and workflow |
| App id `de.musiker15.sshcraft` (also the keyring service, the data folder and the Arch package name) | `packaging/macos/Info.plist.in`, `packaging/linux/*`, the Flatpak manifests |
| WiX upgrade code `E577A976-EE1A-4C98-96B7-D3A2EBE306CB`, fixed forever | `packaging/windows/sshcraft.wxs`, `install-policy.json` |
| Icon | `assets/app-icon/`, rendered by `packaging/icons.sh` |
| craft-fonts pin | `gate.yml`, `freebsd-package.yml` and `.github/actions/build-setup/action.yml` |
| The files a release must have | `.github/scripts/assemble-release.sh` |

## One-time setup (Moritz)

These need the repository's settings and secrets, so only the owner sets them up:

1. **Branch `release`**, created from `main`, with branch protection: no force push, no deletion.
2. **Environment `release`** (Settings, Environments), limited to the branch `release`. The
   Windows and Android builds and the draft release run in it, and only the `release` branch may
   use it.
3. **Secrets** in that environment. Only the Android key is required; without the others the
   files are built unsigned and the run says so in a warning.

   | Secret | For |
   |---|---|
   | `ANDROID_KEYSTORE_BASE64`, `ANDROID_KEYSTORE_PASSWORD` | the Android release key (the keystore as base64, key alias `sshcraft`). **Required**: a real run stops without it, since an APK signed with any other key cannot update an installed SSHCraft |
   | `WINDOWS_CERTIFICATE`, `WINDOWS_CERTIFICATE_PASSWORD` | Windows signing with a .pfx (optional) |
   | `AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `AZURE_SIGNING_ENDPOINT`, `AZURE_SIGNING_ACCOUNT`, `AZURE_CERT_PROFILE` | Windows signing with Azure Trusted Signing instead (optional) |
   | `LINUX_SIGNING_KEY` | an OpenPGP secret key without passphrase that signs the rpm and deb packages, the Arch packages (`.sig`) and `SHA256SUMS.txt` (`.asc`), published as `sshcraft-signing-key.asc` (optional) |

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

2. **Try it as a dry run** (below) and wait until it is green.
3. **Merge `main` into `release`** (or fast-forward it) and push. The workflow starts by itself.
   It stops at once if that version is already published.
4. **Wait for the draft.** The FreeBSD VM takes longest. The draft `SSHCraft v0.2.0` then has
   every file, `SHA256SUMS.txt` and the SBOM `sshcraft-0.2.0.cdx.json`, its notes generated from
   the merged pull requests.
5. **Check it.** Install a package or two and go through the hand-test list of the last pull
   requests. Windows is unsigned unless a certificate is set up, and there are no macOS
   packages; the release notes should say so.
6. **Publish** the draft. Publishing creates the tag `v0.2.0`; a version with a suffix
   (`-rc.1`) is marked as a pre-release.
7. **Update the stores** (below).

Pushing to `release` again before publishing builds the draft anew and replaces its files. A
published version is never touched again: set a new one first.

**Dry runs** build everything without signing and without a draft, from any branch:

```sh
gh workflow run release.yml --ref main -f dry-run=true             # Cargo.toml's version
gh workflow run release.yml --ref main -f version=0.4.0-rc.1       # or another one, for this run only
```

The last job collects all files with `SHA256SUMS.txt` into the artifact `sshcraft-<v>-all`
(`gh run download <run-id> --name sshcraft-<v>-all`). The APKs of a dry run carry a throwaway
debug key, so they install only where no SSHCraft from a release is. A run by hand with dry run
off asks for the `release` environment, so from any branch but `release` its Windows and Android
jobs stop there, and it never makes a draft.

## What gets built

| Platform | Job | Artifacts |
|---|---|---|
| Windows 10+ x64, ARM64 | `windows` | `sshcraft-<v>-windows-<arch>.msi`, `sshcraft-<v>-windows-<arch>-portable.zip`; each MSI is installed, run and removed on its own architecture |
| Linux x86_64, aarch64 (glibc 2.35+, built on Ubuntu 22.04) | `linux` | `sshcraft-<v>-linux-<arch>.{AppImage,AppImage.zsync,deb,rpm,tar.gz}` |
| Linux riscv64 (glibc 2.39+, cross-built on Ubuntu 24.04) | `linux-riscv64` | `sshcraft-<v>-linux-riscv64.{deb,rpm,tar.gz}` |
| Flatpak x86_64, aarch64 | `flatpak` | `sshcraft-<v>-linux-<arch>.flatpak`, repackaging the Linux tarball |
| Arch Linux x86_64, aarch64 | `arch` | `sshcraft-<v>-linux-<arch>.pkg.tar.zst`, repackaging the Linux tarball |
| FreeBSD 15 x86_64, aarch64 | `freebsd` | `sshcraft-<v>-freebsd-<arch>.tar.gz` |
| Android 13+ tablets and phones: arm64, armv7 (older 32-bit devices), x86_64 (Chromebooks, x86 tablets) | `android` | `sshcraft-<v>-android-<abi>.apk`, also built locally with `cargo xtask android --abi <abi>` ([android.md](android.md)) |
| Sync server, Linux x86_64 and aarch64 | `server` | `sshcraft-server-<v>-linux-<arch>.tar.gz`: the program, a systemd unit and an example environment file ([server.md](server.md#install-the-program)) |
| All | `check`, `release` | `sshcraft-<v>.cdx.json` (CycloneDX 1.6 SBOM from `cargo xtask sbom`), `SHA256SUMS.txt` |

Before any build, the `check` job refuses git dependencies from anywhere but
`github.com/storytold`, a cargo configuration that replaces or patches a source, and a
`Cargo.lock` that does not match the manifests. The SBOM lists every crate in the shipped
programs on any platform, with version, licence and package URL; dev-dependencies stay out.

**macOS** is not built: there is no Apple developer account. The scripts are there
(`packaging/macos/`); a `macos` job would go into `release.yml` beside `windows`, and its files
into the list in `assemble-release.sh`.

SSHCraft has no web build: SSH needs TCP.

**The server's Docker image** goes to `ghcr.io/musiker15/sshcraft-server` when a release is
published: [`server-image.yml`](../.github/workflows/server-image.yml) takes the two server archives
of that release, checks them against its `SHA256SUMS.txt` and builds the image for linux/amd64 and
linux/arm64 from [`Dockerfile.release`](../packaging/docker/Dockerfile.release). It is tagged with
the version, and `latest` for a stable release; a draft gets no image. For a release published
before the workflow existed, or a push that failed:

```sh
gh workflow run server-image.yml -f tag=v0.3.1              # builds only
gh workflow run server-image.yml -f tag=v0.3.1 -f push=true # and pushes
```

After the first push, check on the package page that the image is public and linked to the
repository. [`packaging/docker/Dockerfile`](../packaging/docker/Dockerfile) still builds the same
image from source ([server.md](server.md#start)).

## Stores

After a stable release is published, the store entries are filled in from its checksums:

```sh
gh release download v0.2.0 --pattern SHA256SUMS.txt --dir dist
cargo xtask stores dist/SHA256SUMS.txt      # writes dist/stores/
```

The templates are in [`packaging/winget/`](../packaging/winget) and
[`packaging/homebrew/`](../packaging/homebrew); `{{version}}` and `{{sha256:<file>}}` are their
only placeholders, and a file the checksums do not list stops the command. A store whose
platform the release has no files for at all is left out with a note: without macOS packages
there is no Homebrew cask. Submitting is the
maintainer's step, never an automatic one:

| Store | What to submit |
|---|---|
| winget | `dist/stores/winget/*.yaml` as `manifests/m/MSKScripts/SSHCraft/<v>/` in a pull request to `microsoft/winget-pkgs` (`winget validate` first). Id `MSKScripts.SSHCraft`; per-user by default, per-machine with `ALLUSERS=1` |
| Homebrew | `dist/stores/homebrew/sshcraft.rb`. `Homebrew/homebrew-cask` takes only signed, notarized apps that are notable enough; until then it goes into an own tap |
| Flathub | the from-source manifest [`de.musiker15.sshcraft.yml`](../packaging/linux/flatpak/de.musiker15.sshcraft.yml) with a generated `cargo-sources.json` (how is at its top), as a new-app pull request to `flathub/flathub` |

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
  skips the AppImage. The deb asks for the newest glibc version the binaries use, read from them
  with `readelf`, so it always matches the machine they were built on.
- **Flatpak:** [`packaging/linux/flatpak-bundle.sh`](../packaging/linux/flatpak-bundle.sh) wraps
  the tarball. The sandbox has the network, the Secret Service and `~/.ssh` (written only when the
  user saves a linked host back); serial ports and the host's own shell stay outside it.
- **Arch Linux:** [`packaging/linux/arch/package.sh`](../packaging/linux/arch/package.sh) writes
  a PKGBUILD for the tarball and runs makepkg as an unprivileged user in an Arch Linux container.
  It depends on everything the window needs on X11 (libx11, libxcursor, libxi, libxrandr,
  libxkbcommon-x11) besides libxkbcommon and the Vulkan loader; Wayland and the GPU drivers are
  optional. The package keeps the name `de.musiker15.sshcraft`, so `pacman -U` upgrades 0.3.0.
- **FreeBSD:** [`packaging/freebsd/package.sh`](../packaging/freebsd/package.sh) runs in a
  FreeBSD VM ([`freebsd-package.yml`](../.github/workflows/freebsd-package.yml)); aarch64 is
  cross-compiled in the x86_64 guest. [`freebsd.yml`](../.github/workflows/freebsd.yml) builds the
  x86_64 package in pull requests that change the FreeBSD packaging.
- **Sync server:** [`packaging/server/package.sh`](../packaging/server/package.sh) builds
  `sshcraft-server` and packs it with the licences, `docs/server.md`, `sshcraft-server.service`
  and `sshcraft-server.env.example`.

Builds with `CRAFT_FONTS_DIR` embed craft-fonts' interface faces, and every package carries their
licences (`OFL-<family>.txt`) next to `NOTICE` and `ATTRIBUTION.md`.

The packaging scripts are adapted from [storytold/gridcraft](https://github.com/storytold/gridcraft)
(Apache-2.0).
