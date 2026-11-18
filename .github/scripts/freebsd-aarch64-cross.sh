#!/bin/sh
# Sets up an x86_64 FreeBSD VM to cross-compile SSHCraft for aarch64-unknown-freebsd, and writes
# the variables cargo needs to $CROSS/env (source it before packaging/freebsd/package.sh).
#
# Why a cross build: an emulated aarch64 guest builds many times slower than the KVM-accelerated
# x86_64 one. aarch64-unknown-freebsd is a tier-3 Rust target, so rustup has no standard library
# for it; the compiler comes from the amd64 lang/rust package (the prepare step installs it) and
# the standard library from the aarch64 lang/rust package of the same version. C code (aws-lc)
# compiles with the base system's clang and links with lld against an arm64 sysroot: the arm64
# base.txz plus the aarch64 packages of the libraries the build may link.
#
# Usage, as root in the VM: sh .github/scripts/freebsd-aarch64-cross.sh
set -eu

target=aarch64-unknown-freebsd
CROSS=${CROSS:-/var/tmp/sshcraft-cross}
sysroot=$CROSS/sysroot
libs="libxkbcommon wayland libX11 libXcursor libXrandr libXi libxcb mesa-libs vulkan-loader fontconfig freetype2 xorgproto"

fail() {
  echo "error: freebsd-aarch64-cross: $*" >&2
  exit 1
}

rm -rf "$CROSS"
mkdir -p "$CROSS/pkgdb" "$CROSS/pkgcache" "$sysroot"
release=$(freebsd-version -u | sed 's/-p[0-9]*$//')
major=${release%%.*}

# pkg for the other architecture, with its own database and cache so the host's stay untouched.
xpkg() {
  pkg -o "ABI=FreeBSD:$major:aarch64" -o "OSVERSION=$(uname -U)" -o IGNORE_OSVERSION=yes \
    -o "PKG_DBDIR=$CROSS/pkgdb" -o "PKG_CACHEDIR=$CROSS/pkgcache" "$@"
}
xpkg update -q

# The standard library must come from the very build of Rust that compiles against it.
host_rust=$(pkg query %v rust)
cross_rust=$(xpkg rquery %v rust)
[ "$host_rust" = "$cross_rust" ] || fail "lang/rust is $host_rust on amd64 but $cross_rust on aarch64"
xpkg fetch -y -q rust
tar -xf "$CROSS/pkgcache/rust-$cross_rust.pkg" -C "$CROSS" --include "*usr/local/lib/rustlib/$target/*"
[ -d "$CROSS/usr/local/lib/rustlib/$target/lib" ] || fail "no $target standard library in the rust package"
rm -rf "/usr/local/lib/rustlib/$target"
cp -R "$CROSS/usr/local/lib/rustlib/$target" /usr/local/lib/rustlib/

# The arm64 base system, checked against the release's MANIFEST before it is unpacked.
base=https://download.freebsd.org/releases/arm64/aarch64/$release
fetch -q -o "$CROSS/MANIFEST" "$base/MANIFEST"
fetch -q -o "$CROSS/base.txz" "$base/base.txz"
want=$(awk '$1 == "base.txz" { print $2 }' "$CROSS/MANIFEST")
if [ -z "$want" ] || [ "$want" != "$(sha256 -q "$CROSS/base.txz")" ]; then
  fail "arm64 base.txz does not match its MANIFEST"
fi
tar -xf "$CROSS/base.txz" -C "$sysroot" ./lib ./usr/lib ./usr/include ./usr/libdata ./usr/share

# The libraries and their dependencies; pkg's cache holds links named after each package.
# shellcheck disable=SC2086 # one package name per word
xpkg fetch -y -q -d $libs
for p in "$CROSS"/pkgcache/*.pkg; do
  [ -e "$p" ] || continue
  case "${p##*/}" in rust-*) continue ;; esac
  tar -xf "$p" -C "$sysroot" --exclude '+*'
done

for tool in cc c++; do
  cat >"$CROSS/$tool" <<EOF
#!/bin/sh
exec $tool --target=$target${release%%-*} --sysroot=$sysroot -fuse-ld=lld -I$sysroot/usr/local/include -L$sysroot/usr/local/lib -Wl,-rpath-link,$sysroot/usr/local/lib "\$@"
EOF
  chmod 755 "$CROSS/$tool"
done

pkgconfig=$sysroot/usr/local/libdata/pkgconfig:$sysroot/usr/local/lib/pkgconfig:$sysroot/usr/local/share/pkgconfig:$sysroot/usr/libdata/pkgconfig
cat >"$CROSS/env" <<EOF
export CARGO_BUILD_TARGET=$target
export CARGO_TARGET_AARCH64_UNKNOWN_FREEBSD_LINKER=$CROSS/cc
export CC_aarch64_unknown_freebsd=$CROSS/cc
export CXX_aarch64_unknown_freebsd=$CROSS/c++
export AR_aarch64_unknown_freebsd=/usr/bin/ar
export PKG_CONFIG_ALLOW_CROSS=1
export PKG_CONFIG_SYSROOT_DIR=$sysroot
export PKG_CONFIG_PATH=$pkgconfig
export PKG_CONFIG_LIBDIR=$pkgconfig
EOF

# Check the result the way the build will use it.
# shellcheck source=/dev/null
. "$CROSS/env"
pkg-config --exists fontconfig || fail "pkg-config finds no fontconfig in the aarch64 sysroot"
rustc --print target-libdir --target "$target" >/dev/null || fail "rustc has no $target standard library"
echo "aarch64 cross toolchain ready: Rust $host_rust, FreeBSD $release, $CROSS/env"
