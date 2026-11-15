#!/usr/bin/env bash
# Draws the app icon in every size the packages need, from assets/app-icon/sshcraft.svg.
# The outputs are committed; run this again after changing the drawing.
# Needs rsvg-convert (librsvg2-bin), icotool (icoutils) and png2icns (icnsutils), as in Debian.
set -euo pipefail
cd "$(dirname "$0")/.."
src=assets/app-icon/sshcraft.svg
out=assets/app-icon
id=de.musiker15.sshcraft
rsvg-convert -w 1024 -h 1024 "$src" -o "$out/sshcraft-1024.png"
for size in 16 24 32 48 64 128 256 512; do
  dir="$out/hicolor/${size}x${size}/apps"
  mkdir -p "$dir"
  rsvg-convert -w "$size" -h "$size" "$src" -o "$dir/$id.png"
done
mkdir -p "$out/hicolor/scalable/apps"
cp "$src" "$out/hicolor/scalable/apps/$id.svg"
# Windows: one .ico with the sizes Explorer and the taskbar ask for.
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
for size in 16 20 24 32 40 48 64 128 256; do
  rsvg-convert -w "$size" -h "$size" "$src" -o "$tmp/$size.png"
done
icotool -c -o "$out/sshcraft.ico" "$tmp"/16.png "$tmp"/20.png "$tmp"/24.png "$tmp"/32.png "$tmp"/40.png "$tmp"/48.png "$tmp"/64.png "$tmp"/128.png "$tmp"/256.png
# macOS draws icons on a grid with a margin: the body takes 824 of 1024.
sed 's/viewBox="0 0 512 512"/viewBox="-62 -62 636 636"/' "$src" > "$tmp/macos.svg"
for size in 16 32 128 256 512; do
  rsvg-convert -w "$size" -h "$size" "$tmp/macos.svg" -o "$tmp/mac-$size.png"
done
cp "$tmp/mac-512.png" "$out/sshcraft-macos-512.png"
png2icns "$out/sshcraft.icns" "$tmp"/mac-16.png "$tmp"/mac-32.png "$tmp"/mac-128.png "$tmp"/mac-256.png "$tmp"/mac-512.png >/dev/null
