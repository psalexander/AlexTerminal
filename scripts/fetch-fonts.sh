#!/usr/bin/env bash
# Downloads JetBrains Mono (SIL OFL 1.1) into assets/fonts so builds bundle it.
# Without it the app falls back to SF Mono / Menlo / DejaVu Sans Mono.
set -euo pipefail
cd "$(dirname "$0")/.."
VER=2.304
TMP=$(mktemp -d)
curl -fsSL -o "$TMP/jbm.zip" "https://github.com/JetBrains/JetBrainsMono/releases/download/v$VER/JetBrainsMono-$VER.zip"
unzip -q "$TMP/jbm.zip" -d "$TMP/jbm"
mkdir -p assets/fonts
for f in Regular Bold Italic BoldItalic; do
  cp "$TMP/jbm/fonts/ttf/JetBrainsMono-$f.ttf" assets/fonts/
done
cp "$TMP/jbm/OFL.txt" assets/fonts/JetBrainsMono-OFL.txt
ls assets/fonts
