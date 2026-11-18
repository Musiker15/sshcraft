#!/usr/bin/env bash
# Installs Debian/Ubuntu packages on a CI runner, retrying a mirror that fails for a moment.
# Usage: .github/scripts/apt-install.sh <package>...
set -euo pipefail

if [ "$#" -eq 0 ]; then
  echo "::error::apt-install: no packages given"
  exit 1
fi
for pkg in "$@"; do
  # Package names only: nothing from here may reach apt as an option.
  if ! [[ "$pkg" =~ ^[a-z0-9][a-z0-9+.-]*$ ]]; then
    echo "::error::apt-install: invalid package name '$pkg'"
    exit 1
  fi
done

export DEBIAN_FRONTEND=noninteractive
for attempt in 1 2 3 4 5; do
  if sudo apt-get update -o Acquire::Retries=3 &&
    sudo apt-get install -y --no-install-recommends -o Acquire::Retries=3 "$@"; then
    exit 0
  fi
  echo "::warning::apt-install: attempt $attempt failed, trying again"
  sleep $((attempt * 10))
done
echo "::error::apt-install: the packages could not be installed"
exit 1
