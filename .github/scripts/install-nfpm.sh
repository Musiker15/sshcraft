#!/usr/bin/env bash
# Installs nfpm, which packaging/linux/package.sh needs for the deb and rpm, from its GitHub
# release, checked against a hash kept here (from the release's checksums.txt), so a replaced
# download cannot get onto the runner.
# Usage: .github/scripts/install-nfpm.sh <amd64|arm64>   (the runner's own architecture)
set -euo pipefail

NFPM_VERSION=2.49.0
case "${1:-}" in
  amd64) sha256=45650c5f6109685436fa6ba91958eb93e09a899bb0d65dd5b7c3c66020c056d2 ;;
  arm64) sha256=f6c77951e74a267fade0c6a14fa66eee35a750cd23da483e39d2e92a6b307527 ;;
  *)
    echo "::error::install-nfpm: architecture must be amd64 or arm64, not '${1:-}'"
    exit 1
    ;;
esac

deb="${RUNNER_TEMP:-/tmp}/nfpm_${NFPM_VERSION}_$1.deb"
curl --proto '=https' --proto-redir '=https' -fsSL --retry 3 -o "$deb" \
  "https://github.com/goreleaser/nfpm/releases/download/v${NFPM_VERSION}/nfpm_${NFPM_VERSION}_$1.deb"
echo "$sha256  $deb" | sha256sum -c -
sudo dpkg -i "$deb"
nfpm --version
