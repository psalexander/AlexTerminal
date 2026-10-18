#!/usr/bin/env bash
# Builds dist/AlexTerminal.app and dist/AlexTerminal-<version>.dmg.
#
#   UNIVERSAL=1        build arm64 + x86_64 and merge with lipo
#   SIGN_IDENTITY=...  "Developer ID Application: Name (TEAMID)" → codesign with hardened runtime
#   NOTARY_PROFILE=... keychain profile from `xcrun notarytool store-credentials` → notarize + staple
set -euo pipefail
cd "$(dirname "$0")/../.."
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
DIST=dist
APP="$DIST/AlexTerminal.app"
BINS=(alexterminal alexterminal-server)

if [[ "${UNIVERSAL:-0}" == 1 ]]; then
  rustup target add aarch64-apple-darwin x86_64-apple-darwin
  cargo build --release --target aarch64-apple-darwin
  cargo build --release --target x86_64-apple-darwin
  mkdir -p target/universal
  for b in "${BINS[@]}"; do
    lipo -create -output "target/universal/$b" "target/aarch64-apple-darwin/release/$b" "target/x86_64-apple-darwin/release/$b"
  done
  BIN_DIR=target/universal
else
  cargo build --release
  BIN_DIR=target/release
fi

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources/fonts"
for b in "${BINS[@]}"; do cp "$BIN_DIR/$b" "$APP/Contents/MacOS/"; done
sed "s/__VERSION__/$VERSION/g" packaging/macos/Info.plist > "$APP/Contents/Info.plist"
cp assets/fonts/*.ttf "$APP/Contents/Resources/fonts/" 2>/dev/null || true

# Icon: PNG master → iconset → icns.
ICONSET=$(mktemp -d)/AlexTerminal.iconset
mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
  sips -z $s $s assets/icon-512.png --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
  d=$((s * 2))
  sips -z $d $d assets/icon-512.png --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AlexTerminal.icns"

if [[ -n "${SIGN_IDENTITY:-}" ]]; then
  for b in "${BINS[@]}"; do
    codesign --force --options runtime --timestamp --entitlements packaging/macos/entitlements.plist -s "$SIGN_IDENTITY" "$APP/Contents/MacOS/$b"
  done
  codesign --force --options runtime --timestamp --entitlements packaging/macos/entitlements.plist -s "$SIGN_IDENTITY" "$APP"
  codesign --verify --strict --verbose=2 "$APP"
else
  codesign --force --deep -s - "$APP"   # ad-hoc, local use only
fi

DMG="$DIST/AlexTerminal-$VERSION.dmg"
rm -f "$DMG"
STAGE=$(mktemp -d)
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
hdiutil create -volname AlexTerminal -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null

if [[ -n "${SIGN_IDENTITY:-}" ]]; then
  codesign --force --timestamp -s "$SIGN_IDENTITY" "$DMG"
fi
if [[ -n "${NOTARY_PROFILE:-}" ]]; then
  xcrun notarytool submit "$DMG" --keychain-profile "$NOTARY_PROFILE" --wait
  xcrun stapler staple "$DMG"
  xcrun stapler staple "$APP"
fi
echo "$APP"
echo "$DMG"
