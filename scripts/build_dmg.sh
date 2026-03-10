#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

echo "Building Tauri app bundle (.dmg)..."
bun run tauri build

DMG_DIR="$ROOT_DIR/src-tauri/target/release/bundle/dmg"
echo "DMG output directory: $DMG_DIR"

if [ -d "$DMG_DIR" ]; then
  VERSION="$(node -p "require('$ROOT_DIR/package.json').version")"
  VERSIONED_DMG="$(ls -t "$DMG_DIR/TypeMore_${VERSION}_"*.dmg 2>/dev/null | head -1 || true)"
  if [ -n "$VERSIONED_DMG" ]; then
    cp -f "$VERSIONED_DMG" "$DMG_DIR/TypeMore.dmg"
  fi
  ls -lh "$DMG_DIR"/*.dmg 2>/dev/null || true
  if [ "${OPEN_DIR:-1}" = "1" ]; then
    open "$DMG_DIR"
  fi
fi
