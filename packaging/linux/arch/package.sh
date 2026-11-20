#!/usr/bin/env bash
# Repack a SSHCraft Linux release tarball as an Arch Linux package:
#
#   <out dir>/sshcraft-<version>-linux-<arch>.pkg.tar.zst
#
# Usage: packaging/linux/arch/package.sh TARBALL [OUT_DIR]
#
# TARBALL is sshcraft-<version>-linux-<x86_64|aarch64>.tar.gz from packaging/linux/package.sh;
# version and architecture are read from its name and checked before anything is written.
# OUT_DIR defaults to the directory of the tarball. Nothing is compiled, so an x86_64 machine
# packs both architectures.
#
# makepkg refuses to run as root and exists only on Arch, so the script runs in up to three
# stages: without makepkg it starts itself in an Arch Linux container (docker; ARCH_IMAGE picks
# the image), as root in there it hands over to an unprivileged user, and that user builds.
set -euo pipefail

die() {
  echo "arch-package: $*" >&2
  exit 1
}

[[ $# -ge 1 && $# -le 2 ]] || die "usage: package.sh TARBALL [OUT_DIR]"
tarball="$1"
[[ -f "$tarball" ]] || die "$tarball not found"
out_dir="${2:-$(dirname "$tarball")}"
mkdir -p "$out_dir"
tarball="$(cd "$(dirname "$tarball")" && pwd)/$(basename "$tarball")"
out_dir="$(cd "$out_dir" && pwd)"
script="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")"

# Everything below is written into a PKGBUILD, which makepkg runs as bash: only values that
# match these patterns get that far.
name="$(basename "$tarball")"
semver='(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?'
[[ "$name" =~ ^sshcraft-($semver)-linux-(x86_64|aarch64)\.tar\.gz$ ]] ||
  die "$name is not sshcraft-<version>-linux-<x86_64|aarch64>.tar.gz"
version="${BASH_REMATCH[1]}"
arch="${BASH_REMATCH[7]}"
top="sshcraft-$version-linux-$arch"

# ---- stage 1: no makepkg here, so run in an Arch container ------------------------------------
if ! command -v makepkg >/dev/null; then
  command -v docker >/dev/null || die "neither makepkg nor docker found"
  image="${ARCH_IMAGE:-archlinux:base-devel}"
  docker run --rm \
    -e SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-}" \
    -v "$script:/arch-package.sh:ro" \
    -v "$tarball:/in/$name:ro" \
    -v "$out_dir:/out" \
    "$image" bash /arch-package.sh "/in/$name" /out
  exit 0
fi

# ---- stage 2: root in the container hands the build to a user ----------------------------------
if [[ "$(id -u)" -eq 0 ]]; then
  id builder >/dev/null 2>&1 || useradd --create-home builder
  work="$(mktemp -d)"
  cp "$tarball" "$work/$name"
  chown -R builder "$work"
  runuser -u builder -- env SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-}" \
    bash "$script" "$work/$name" "$work/out"
  install -m 644 "$work/out/$top.pkg.tar.zst" "$out_dir/"
  rm -rf "$work"
  exit 0
fi

# ---- stage 3: build ----------------------------------------------------------------------------
# pacman's version order: a letter after the numbers sorts before the release (0.4.0rc.1 <
# 0.4.0), so the first hyphen goes, with "pre" in front of a number (0.4.0-1 would read
# 0.4.01). pkgver allows no hyphen at all; any later one becomes an underscore.
release="${version%%-*}"
pkgver="$release"
if [[ "$version" == *-* ]]; then
  pre="${version#*-}"
  [[ "$pre" =~ ^[0-9] ]] && pre="pre$pre"
  pkgver="$release${pre//-/_}"
fi
[[ "$pkgver" =~ ^[0-9A-Za-z._]+$ ]] || die "cannot make a pkgver of $version"

# The tarball holds one directory with regular files and directories only: no links, no absolute
# paths and no "..", so unpacking it cannot write or point anywhere else.
types="$(tar -tvzf "$tarball" | cut -c1 | sort -u | tr -d '\n')" || die "cannot read $tarball"
[[ "$types" =~ ^[-d]+$ ]] || die "$name holds something other than files and directories ($types)"
while IFS= read -r path; do
  [[ "$path" == "$top/"* || "$path" == "$top" ]] || die "$name holds $path outside $top/"
  [[ "/$path/" != *"/../"* ]] || die "$name holds a path with ..: $path"
done < <(tar -tzf "$tarball")

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
tar -xzf "$tarball" -C "$work" --no-same-owner
mv "$work/$top" "$work/root"
for f in bin/sshcraft bin/sshcraft-cli share/applications/de.musiker15.sshcraft.desktop \
  share/doc/sshcraft/LICENSE-MIT share/doc/sshcraft/LICENSE-APACHE; do
  [[ -f "$work/root/$f" ]] || die "$name has no $f"
done

# Two fixed lines carry the checked version and architecture; the rest is SSHCraft's own.
# The name stays the app id that 0.3.0 shipped under, so pacman -U upgrades it in place.
{
  printf 'pkgver=%s\n' "$pkgver"
  printf 'arch=(%s)\n' "$arch"
  cat <<'PKGBUILD'
pkgname=de.musiker15.sshcraft
pkgrel=1
pkgdesc='SSH client with end-to-end encrypted, self-hosted sync'
url='https://github.com/musiker15/sshcraft'
license=('MIT OR Apache-2.0')
# The binaries link only glibc and libgcc_s; winit loads the rest at runtime. On X11 and XWayland
# it needs all of libx11, libxcursor, libxi, libxrandr and libxkbcommon-x11, or the window does
# not open (0.3.0 listed libxcursor only as optional, and it failed on Arch without it).
depends=(
  glibc
  gcc-libs
  hicolor-icon-theme
  libxkbcommon
  libxkbcommon-x11
  vulkan-icd-loader
  libx11
  libxcursor
  libxi
  libxrandr
)
optdepends=(
  'wayland: draw on Wayland without XWayland'
  'mesa: GPU drivers for Vulkan and EGL'
  'libglvnd: draw through EGL where Vulkan is missing'
  'vulkan-driver: draw through Vulkan'
  'xdg-desktop-portal: file dialogs'
  'org.freedesktop.secrets: keep keys and passwords in the desktop keyring'
  'polkit: ask for your password before stored keys and to unlock the app lock'
)
provides=("sshcraft=$pkgver")
conflicts=(sshcraft sshcraft-bin sshcraft-git)
options=(!strip !debug)

package() {
  install -d "$pkgdir/usr"
  cp -r --no-preserve=ownership "$startdir/root/bin" "$startdir/root/share" "$pkgdir/usr/"
  chmod 755 "$pkgdir/usr/bin/sshcraft" "$pkgdir/usr/bin/sshcraft-cli"
  # Copies, not links: pacman may be set to skip usr/share/doc (NoExtract), the licences stay.
  local doc="$startdir/root/share/doc/sshcraft"
  install -Dm644 -t "$pkgdir/usr/share/licenses/$pkgname" "$doc/LICENSE-MIT" "$doc/LICENSE-APACHE"
}
PKGBUILD
} >"$work/PKGBUILD"

# CARCH decides the package's architecture field, so it is set to the target, not the host.
cat >"$work/makepkg.conf" <<EOF
source /etc/makepkg.conf
CARCH=$arch
PACKAGER='SSHCraft maintainers'
PKGEXT=.pkg.tar.zst
EOF

echo "==> Arch package of SSHCraft $version ($pkgver) for $arch"
(cd "$work" && SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(date +%s)}" \
  makepkg --config "$work/makepkg.conf" --nodeps --force --noconfirm)
built=("$work"/*.pkg.tar.zst)
[[ ${#built[@]} -eq 1 && -f "${built[0]}" ]] || die "makepkg did not write exactly one package"
mkdir -p "$out_dir"
cp "${built[0]}" "$out_dir/$top.pkg.tar.zst"
echo "wrote $out_dir/$top.pkg.tar.zst"
