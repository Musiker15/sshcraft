#!/usr/bin/env bash
# Prepares an Ubuntu amd64 runner to cross-compile SSHCraft for riscv64gc-unknown-linux-gnu: the
# riscv64 C toolchain (aws-lc-sys compiles C), the riscv64 development libraries the build may
# probe for, and qemu-user to run the finished CLI once.
#
# Why the apt sources change: apt installs a library for two architectures only at one version
# (Multi-Arch: same), so amd64 and riscv64 have to come from one snapshot of the archive. The
# runner's mirror can trail ports.ubuntu.com, which carries riscv64, by hours, and apt then
# reports held broken packages. So amd64 is read from archive.ubuntu.com and security.ubuntu.com
# and riscv64 from ports.ubuntu.com, which Canonical publishes together, and the image's own
# Ubuntu sources are switched off for this job.
#
# Usage: sudo .github/scripts/riscv64-cross.sh
set -euo pipefail

[[ "$(id -u)" -eq 0 ]] || { echo "::error::riscv64-cross: run as root"; exit 1; }

# cloud-init may still be writing the image's apt sources when the job starts.
if command -v cloud-init >/dev/null; then cloud-init status --wait >/dev/null || true; fi

# shellcheck source=/dev/null
codename="$(. /etc/os-release && echo "$VERSION_CODENAME")"
[[ "$codename" =~ ^[a-z]+$ ]] || { echo "::error::riscv64-cross: no Ubuntu codename"; exit 1; }
parts="main restricted universe multiverse"

dpkg --add-architecture riscv64
{
  echo "deb [arch=amd64] https://archive.ubuntu.com/ubuntu $codename $parts"
  echo "deb [arch=amd64] https://archive.ubuntu.com/ubuntu $codename-updates $parts"
  echo "deb [arch=amd64] https://security.ubuntu.com/ubuntu $codename-security $parts"
  echo "deb [arch=riscv64] https://ports.ubuntu.com/ubuntu-ports $codename $parts"
  echo "deb [arch=riscv64] https://ports.ubuntu.com/ubuntu-ports $codename-updates $parts"
  echo "deb [arch=riscv64] https://ports.ubuntu.com/ubuntu-ports $codename-security $parts"
} >/etc/apt/sources.list.d/sshcraft-cross.list
# apt reads only *.list and *.sources, so a renamed file is off.
if [[ -f /etc/apt/sources.list.d/ubuntu.sources ]]; then
  mv /etc/apt/sources.list.d/ubuntu.sources /etc/apt/sources.list.d/ubuntu.sources.off
fi
if [[ -f /etc/apt/sources.list ]]; then
  sed -i -E 's/^(deb|deb-src)[[:space:]]/# &/' /etc/apt/sources.list
fi

packages=(
  gcc-riscv64-linux-gnu binutils-riscv64-linux-gnu libc6-dev-riscv64-cross qemu-user
  pkg-config desktop-file-utils appstream
  libc6-dev:riscv64 libxkbcommon-dev:riscv64 libwayland-dev:riscv64 libx11-dev:riscv64
  libxcursor-dev:riscv64 libxrandr-dev:riscv64 libxi-dev:riscv64 libfontconfig-dev:riscv64
)
export DEBIAN_FRONTEND=noninteractive
for attempt in 1 2 3 4 5; do
  if apt-get update -o Acquire::Retries=3 &&
    apt-get install -y --no-install-recommends -o Acquire::Retries=3 "${packages[@]}"; then
    exit 0
  fi
  echo "::warning::riscv64-cross: attempt $attempt failed, trying again"
  sleep $((attempt * 15))
done
echo "::error::riscv64-cross: the cross packages could not be installed"
exit 1
