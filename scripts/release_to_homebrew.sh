#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
REPO_DIR="$ROOT_DIR"
TMP_DIR="$ROOT_DIR/tmp"
TAP_DIR="${TAP_DIR:-$TMP_DIR/homebrew-tap}"
TAP_REPO="${TAP_REPO:-everettjf/homebrew-tap}"
CASK_PATH="${CASK_PATH:-Casks/typemore.rb}"
VERSION="${VERSION:-$(node -p "require('$REPO_DIR/package.json').version")}"
TAG="${TAG:-v$VERSION}"
DMG_PATH="${DMG_PATH:-$REPO_DIR/src-tauri/target/release/bundle/dmg/TypeMore.dmg}"

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Missing required command: $1" >&2
    exit 1
  fi
}

replace_cask_field() {
  local file="$1"
  local field="$2"
  local value="$3"

  case "$field" in
    version)
      sed -i '' "s#^  version \".*\"#  version \"$value\"#" "$file"
      ;;
    sha256)
      sed -i '' "s#^  sha256 \".*\"#  sha256 \"$value\"#" "$file"
      ;;
    url)
      sed -i '' "s#^  url \".*\"#  url \"$value\"#" "$file"
      ;;
    *)
      echo "Unsupported cask field: $field" >&2
      exit 1
      ;;
  esac
}

require_cmd git
require_cmd node
require_cmd shasum

if [ ! -f "$DMG_PATH" ]; then
  echo "DMG not found: $DMG_PATH" >&2
  exit 1
fi

DOWNLOAD_URL="https://github.com/everettjf/typemore/releases/download/$TAG/TypeMore.dmg"
SHA256="$(shasum -a 256 "$DMG_PATH" | awk '{print $1}')"

rm -rf "$TAP_DIR"
mkdir -p "$TMP_DIR"
git clone "git@github.com:$TAP_REPO.git" "$TAP_DIR"

cd "$TAP_DIR"

if [ ! -f "$CASK_PATH" ]; then
  mkdir -p "$(dirname "$CASK_PATH")"
  cp "$REPO_DIR/docs/homebrew/typemore.rb.example" "$CASK_PATH"
fi

replace_cask_field "$CASK_PATH" version "$VERSION"
replace_cask_field "$CASK_PATH" sha256 "$SHA256"
replace_cask_field "$CASK_PATH" url "$DOWNLOAD_URL"

git add "$CASK_PATH"
git commit -m "bump typemore to $VERSION"
git push

echo "Updated Homebrew cask to $VERSION"
