#!/usr/bin/env bash
# Checks that DIR holds exactly the files of an SSHCraft release, signs the Linux packages when
# LINUX_SIGNING_KEY is set, and writes SHA256SUMS.txt over everything.
#
# Usage: .github/scripts/assemble-release.sh DIR VERSION
#
# The list of files is fixed here, so a platform that silently produced nothing (or something
# unexpected) stops the release instead of shipping without it. A new platform or format is
# added to the list in the same change as its job.
#
# With LINUX_SIGNING_KEY (an ASCII-armored OpenPGP secret key without passphrase):
#   *.rpm           signed inside the package (rpmsign), checked with rpmkeys
#   *.deb           an origin signature inside the package (debsigs), checked with gpgv
#   *.pkg.tar.zst   <file>.sig beside it, the detached signature pacman looks for
#   SHA256SUMS.txt  SHA256SUMS.txt.asc
#   the public key  sshcraft-signing-key.asc
# Signing changes the rpm and deb files, so the checksums are taken afterwards.
set -euo pipefail

dir="${1:?usage: assemble-release.sh DIR VERSION}"
version="${2:?usage: assemble-release.sh DIR VERSION}"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || { echo "::error::bad version '$version'"; exit 1; }
cd "$dir"
shopt -s nullglob
# What this script writes itself, from an earlier attempt.
rm -f SHA256SUMS.txt SHA256SUMS.txt.asc sshcraft-signing-key.asc ./*.pkg.tar.zst.sig

v="$version"
expected=(
  "sshcraft-$v.cdx.json"
  "sshcraft-$v-windows-x64.msi" "sshcraft-$v-windows-x64-portable.zip"
  "sshcraft-$v-windows-arm64.msi" "sshcraft-$v-windows-arm64-portable.zip"
  "sshcraft-$v-linux-riscv64.deb" "sshcraft-$v-linux-riscv64.rpm" "sshcraft-$v-linux-riscv64.tar.gz"
  "sshcraft-$v-freebsd-x86_64.tar.gz" "sshcraft-$v-freebsd-aarch64.tar.gz"
  "sshcraft-$v-android-arm64.apk" "sshcraft-$v-android-armv7.apk" "sshcraft-$v-android-x86_64.apk"
)
for arch in x86_64 aarch64; do
  for ext in AppImage AppImage.zsync deb rpm tar.gz flatpak pkg.tar.zst; do
    expected+=("sshcraft-$v-linux-$arch.$ext")
  done
  expected+=("sshcraft-server-$v-linux-$arch.tar.gz")
done

ok=1
for f in "${expected[@]}"; do
  if [[ ! -s "$f" ]]; then
    echo "::error::missing or empty: $f"
    ok=0
  fi
done
for f in *; do
  case " ${expected[*]} " in
    *" $f "*) ;;
    *)
      echo "::error::not part of a release: $f"
      ok=0
      ;;
  esac
done
[[ "$ok" = 1 ]] || exit 1
echo "All ${#expected[@]} files of SSHCraft $v are here"

if [[ -n "${LINUX_SIGNING_KEY:-}" ]]; then
  GNUPGHOME="$(mktemp -d)"
  check="$(mktemp -d)"
  export GNUPGHOME
  trap 'rm -rf "$GNUPGHOME" "$check"' EXIT
  printf '%s\n' "$LINUX_SIGNING_KEY" | gpg --batch --quiet --import
  # The last usable key or subkey with the signing capability.
  fpr="$(gpg --batch --with-colons --list-secret-keys |
    awk -F: '$1 == "sec" || $1 == "ssb" { can = ($2 !~ /[erd]/ && $12 ~ /[sS]/) } $1 == "fpr" && can { f = $10 } END { print f }')"
  [[ -n "$fpr" ]] || { echo "::error::LINUX_SIGNING_KEY has no key that can sign"; exit 1; }
  gpg --batch --armor --export "$fpr" >sshcraft-signing-key.asc

  # Checks use only the exported public key, never the secret keyring.
  gpg --homedir "$check" --batch --quiet --import sshcraft-signing-key.asc
  gpg --homedir "$check" --batch --export >"$check/keyring.gpg"
  verify() { gpgv --quiet --keyring "$check/keyring.gpg" "$@" 2>/dev/null; }

  # Ubuntu's rpm macros call gpg2, which current GnuPG does not install under that name.
  rpmsign --addsign --define "__gpg $(command -v gpg)" --define "_gpg_name $fpr" \
    --define "_gpg_path $GNUPGHOME" --define "_gpg_digest_algo sha256" ./*.rpm >/dev/null
  rpm --dbpath "$check/rpmdb" --initdb
  rpmkeys --dbpath "$check/rpmdb" --import sshcraft-signing-key.asc
  for f in *.rpm; do
    rpmkeys --dbpath "$check/rpmdb" --checksig "$f" | grep -q 'signatures OK' ||
      { echo "::error::$f: rpm signature does not verify"; exit 1; }
  done

  for f in *.deb; do
    debsigs --sign=origin --default-key="$fpr" "$f"
    # The origin signature covers debian-binary, control and data, read in that order.
    members=()
    while IFS= read -r m; do [[ "$m" == _gpg* ]] || members+=("$m"); done < <(ar t "$f")
    for m in "${members[@]}"; do ar p "$f" "$m"; done >"$check/payload"
    ar p "$f" _gpgorigin >"$check/origin.sig"
    verify "$check/origin.sig" "$check/payload" || { echo "::error::$f: deb signature does not verify"; exit 1; }
  done

  for f in *.pkg.tar.zst; do
    gpg --batch --yes --local-user "$fpr" --detach-sign -o "$f.sig" "$f"
    verify "$f.sig" "$f" || { echo "::error::$f.sig does not verify"; exit 1; }
  done
  echo "Signed the rpm, deb and Arch packages with $fpr"
else
  echo "::notice::LINUX_SIGNING_KEY is not set: the Linux packages and SHA256SUMS.txt are unsigned"
fi

sha256sum -- * >SHA256SUMS.txt
if [[ -n "${fpr:-}" ]]; then
  gpg --batch --yes --local-user "$fpr" --armor --detach-sign -o SHA256SUMS.txt.asc SHA256SUMS.txt
  verify SHA256SUMS.txt.asc SHA256SUMS.txt || { echo "::error::SHA256SUMS.txt.asc does not verify"; exit 1; }
fi
cat SHA256SUMS.txt
