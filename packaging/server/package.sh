#!/usr/bin/env bash
# Builds and packages the sync server for this machine (docs/server.md):
#
#   $DIST/sshcraft-server-<version>-<os>-<arch>.tar.gz
#
# One folder with the sshcraft-server binary, the licences, docs/server.md, a systemd unit and an
# example environment file. A headless program: no desktop files, no fonts, no icons.
#
# Usage: packaging/server/package.sh [--skip-build]
set -euo pipefail
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"
HERE="$ROOT/packaging/server"

SKIP_BUILD=0
case "${1:-}" in
  --skip-build) SKIP_BUILD=1 ;;
  "") ;;
  -h | --help) sed -n '2,9p' "$0"; exit 0 ;;
  *) echo "unknown argument: $1" >&2; exit 2 ;;
esac

OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"
case "$ARCH" in
  arm64) ARCH=aarch64 ;;
  amd64) ARCH=x86_64 ;;
  *) ;;
esac
NAME="sshcraft-server-$VERSION-$OS-$ARCH"
echo "==> sshcraft-server $VERSION for $OS $ARCH"

if [[ "$SKIP_BUILD" = 0 ]]; then
  (cd "$ROOT" && cargo build --release --locked -p sshcraft-server)
fi
WORK="$CARGO_TARGET_DIR/server-package"
STAGE="$WORK/$NAME"
rm -rf "$WORK"
mkdir -p "$STAGE"
install -m 755 "$CARGO_TARGET_DIR/release/sshcraft-server" "$STAGE/"
strip "$STAGE/sshcraft-server" 2>/dev/null || true
for f in LICENSE-MIT LICENSE-APACHE NOTICE docs/server.md; do cp "$ROOT/$f" "$STAGE/"; done
install -m 644 "$HERE/sshcraft-server.service" "$HERE/sshcraft-server.env.example" "$STAGE/"

# The binary runs and reports the version being shipped.
"$STAGE/sshcraft-server" --version | grep -F "$VERSION"

tar -C "$WORK" -czf "$DIST/$NAME.tar.gz" "$NAME"
echo "==> $DIST/$NAME.tar.gz"
