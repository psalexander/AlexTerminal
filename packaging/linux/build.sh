#!/usr/bin/env bash
# Builds dist/alexterminal_<version>_<arch>.deb (cargo-deb) and an AppImage (linuxdeploy).
set -euo pipefail
cd "$(dirname "$0")/../.."
mkdir -p dist
# Icons for the hicolor theme.
mkdir -p target/icons
for s in 16 32 48 64 128 256 512; do
  if command -v rsvg-convert >/dev/null; then
    rsvg-convert -w $s -h $s assets/icon.svg -o target/icons/$s.png
  else
    convert -background none -resize ${s}x${s} assets/icon-512.png target/icons/$s.png
  fi
done
cargo build --release
command -v cargo-deb >/dev/null || cargo install cargo-deb
cargo deb -p at-app --no-build --output dist/
if command -v linuxdeploy >/dev/null; then
  APPDIR=target/AppDir
  rm -rf "$APPDIR"
  mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/alexterminal/fonts"
  cp target/release/alexterminal "$APPDIR/usr/bin/"
  cp assets/fonts/*.ttf "$APPDIR/usr/share/alexterminal/fonts/" 2>/dev/null || true
  linuxdeploy --appdir "$APPDIR" -d packaging/linux/alexterminal.desktop -i target/icons/256.png --icon-filename alexterminal --output appimage
  mv AlexTerminal*.AppImage dist/
else
  echo "linuxdeploy not found: AppImage skipped" >&2
fi
ls -la dist
