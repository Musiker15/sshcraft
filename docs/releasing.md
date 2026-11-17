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
3. **Secrets** in that environment. On the `release` branch the shared workflow refuses to build
   Windows without a certificate or Azure Trusted Signing, and macOS without the Apple secrets;
   it has no switch to leave a platform out or unsigned. Without them, see
   [Unsigned releases](#unsigned-releases). `LINUX_SIGNING_KEY` is optional: without it the
   Linux packages are unsigned.

   | Secret | For |
   |---|---|
   | `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `KEYCHAIN_PASSWORD` | macOS signing (Developer ID Application, as base64 .p12) |
   | `APPLE_ID`, `APPLE_PASSWORD` (app-specific), `APPLE_TEAM_ID` | macOS notarization |
   | `WINDOWS_CERTIFICATE`, `WINDOWS_CERTIFICATE_PASSWORD` | Windows signing with a .pfx |
   | `AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `AZURE_SIGNING_ENDPOINT`, `AZURE_SIGNING_ACCOUNT`, `AZURE_CERT_PROFILE` | Windows signing with Azure Trusted Signing instead |
   | `LINUX_SIGNING_KEY` | the GPG signature of the rpm packages and the Flatpak bundle |
   | `ANDROID_KEYSTORE_BASE64`, `ANDROID_KEYSTORE_PASSWORD` | the Android release key (the keystore as base64, key alias `sshcraft`) for [`android-release.yml`](../.github/workflows/android-release.yml) |

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
   requests. A `::warning::` in a job summary means the Linux key was missing and those
   packages are unsigned.
5. **Attach the SBOM** to the draft before publishing it:

   ```sh
   cargo xtask sbom                  # dist/release/sshcraft-0.2.0.cdx.json
   gh release upload v0.2.0 dist/release/sshcraft-0.2.0.cdx.json
   ```

   It is CycloneDX 1.6 and lists every crate in the shipped programs on any platform, with
   version, licence and package URL; dev-dependencies stay out. The shared release workflow does
   not make one yet, so this is a step by hand.
6. **Publish** the draft. Publishing creates the tag `v0.2.0`; a version with a suffix
   (`-rc.1`) is marked as a pre-release.
7. **Update the stores** (below).

Pushing to `release` again before publishing rebuilds the same draft. After publishing, the
workflow refuses to touch that version again: set a new one first.

**Test runs:** Actions, Release, Run workflow runs the pipeline by hand, as a dry run by default
(no signing, no draft). The optional `version` input overrides `Cargo.toml` for that run only. A
run by hand with dry run off asks for the `release` environment, so from any branch but `release`
its signing jobs stop there.

## Unsigned releases

SSHCraft has no Windows certificate and no Apple developer account, so the pipeline above stops
on the `release` branch. Until the shared workflow can leave a platform out or unsigned (that
needs a change in `storytold/craft-common-actions`), a release is put together by hand from a dry
run on `main`, which builds everything but macOS without signing:

```sh
gh workflow run release.yml --ref main -f dry-run=true           # wait until it is green
gh run download <run-id> --dir dist/draft                        # every release-* artifact
cd dist/draft && find . -type f -exec mv {} . \; && find . -type d -empty -delete
cargo xtask android                                               # from the workspace root, with the release key
cp dist/android/sshcraft-<v>-android-arm64.apk dist/draft/
cd dist/draft && sha256sum -- * > SHA256SUMS.txt && cd -
cargo xtask sbom dist/draft/sshcraft-<v>.cdx.json
gh release create v<v> --draft --target <commit> --title "SSHCraft v<v>" --generate-notes dist/draft/*
```

The draft then has no macOS packages and nothing signed but the APK, which the release notes
should say. The maintainer checks and publishes it as above.

**Android.** The shared workflow has no Android SDK, so SSHCraft builds the APK in a job of its
own, [`android-release.yml`](../.github/workflows/android-release.yml). A push to `release` signs
it with the release key from the `release` environment, waits for the draft of that version (at
most an hour) and attaches the APK; it stops when the two secrets are missing rather than sign with
another key. A run by hand builds a test APK with a throwaway debug key and keeps it as the
artifact `android-test-apk`. For an unsigned release put together by hand, the APK comes from the
artifact `android-apk` of that job or is built locally with `cargo xtask android`
([android.md](android.md)), with `SSHCRAFT_ANDROID_KEYSTORE` and
`SSHCRAFT_ANDROID_KEYSTORE_PASS_FILE` naming the release key. Every APK of a release must be
signed with that same key, or Android refuses the update. The APK is added after the draft's
`SHA256SUMS.txt` was made; add its line when you attach it by hand.

## What gets built

| Platform | Artifacts |
|---|---|
| macOS 11+ (Apple silicon and Intel) | `sshcraft-<v>-macos-universal.dmg`, `sshcraft-cli-<v>-macos-universal.zip` |
| Windows 10+ x64, ARM64 | `sshcraft-<v>-windows-<arch>.msi`, `sshcraft-<v>-windows-<arch>-portable.zip` |
| Linux x86_64, aarch64 | `sshcraft-<v>-linux-<arch>.{AppImage,AppImage.zsync,deb,rpm,tar.gz}` |
| Flatpak x86_64, aarch64 | `sshcraft-<v>-linux-<arch>.flatpak`, repackaging the Linux tarball |
| FreeBSD 15 x86_64, aarch64 | `sshcraft-<v>-freebsd-<arch>.tar.gz` |
| Android 13+ tablets and phones, arm64 | `sshcraft-<v>-android-arm64.apk`, from `android-release.yml` or built locally ([android.md](android.md)) |
| Sync server, Linux x86_64 and aarch64 | `sshcraft-server-<v>-linux-<arch>.tar.gz`: the program, a systemd unit and an example environment file ([server.md](server.md#install-the-program)) |

SSHCraft has no web build: SSH needs TCP.

**The server's Docker image** is built from [`packaging/docker/Dockerfile`](../packaging/docker/Dockerfile)
by whoever runs it ([server.md](server.md#start)). The shared release workflow pushes no server
image to a registry; a published image needs that workflow first, since CI here stays shared.

## Stores

After a stable release is published, the store entries are filled in from its checksums:

```sh
gh release download v0.2.0 --pattern SHA256SUMS.txt --dir dist
cargo xtask stores dist/SHA256SUMS.txt      # writes dist/stores/
```

The templates are in [`packaging/winget/`](../packaging/winget) and
[`packaging/homebrew/`](../packaging/homebrew); `{{version}}` and `{{sha256:<file>}}` are their
only placeholders, and a file the checksums do not list stops the command. Submitting is the
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
  skips the AppImage.
- **Flatpak:** [`packaging/linux/flatpak-bundle.sh`](../packaging/linux/flatpak-bundle.sh) wraps
  the tarball. The sandbox has the network, the Secret Service and `~/.ssh` (written only when the
  user saves a linked host back); serial ports and the host's own shell stay outside it.
- **FreeBSD:** [`packaging/freebsd/package.sh`](../packaging/freebsd/package.sh) runs in a
  FreeBSD VM ([`freebsd.yml`](../.github/workflows/freebsd.yml)).
- **Sync server:** [`packaging/server/package.sh`](../packaging/server/package.sh) builds
  `sshcraft-server` and packs it with the licences, `docs/server.md`, `sshcraft-server.service`
  and `sshcraft-server.env.example`; `has-server` in `release.yml` turns the job on.

Builds with `CRAFT_FONTS_DIR` embed craft-fonts' interface faces, and every package carries their
licences (`OFL-<family>.txt`) next to `NOTICE` and `ATTRIBUTION.md`.

The packaging scripts are adapted from [storytold/gridcraft](https://github.com/storytold/gridcraft)
(Apache-2.0).
