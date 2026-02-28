#!/bin/bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "$0")/.." && pwd)
REPO_DIR="$ROOT_DIR"
TMP_DIR="$ROOT_DIR/tmp"
TAP_DIR="$TMP_DIR/homebrew-tap"
VERSION_FILES=("package.json" "src-tauri/Cargo.toml" "src-tauri/tauri.conf.json")
SKIP_BUMP="${SKIP_BUMP:-0}"
BUILD_DIR="${BUILD_DIR:-$REPO_DIR/build}"
TAP_REPO="${TAP_REPO:-}"
FORMULA_PATH="${FORMULA_PATH:-}"
ASSET_PATH="${ASSET_PATH:-}"

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Missing required command: $1" >&2
    exit 1
  fi
}

read_version() {
  node -p "require('$REPO_DIR/package.json').version"
}

bump_patch_version() {
  node -e "
const fs = require('fs');
const path = '$REPO_DIR/package.json';
const pkg = JSON.parse(fs.readFileSync(path, 'utf8'));
const parts = pkg.version.split('.').map(Number);
if (parts.length !== 3 || parts.some(Number.isNaN)) {
  throw new Error('Invalid package.json version: ' + pkg.version);
}
parts[2] += 1;
pkg.version = parts.join('.');
fs.writeFileSync(path, JSON.stringify(pkg, null, 2) + '\\n');
console.log(pkg.version);
"
}

update_tauri_version() {
  local old_version="$1"
  local new_version="$2"
  sed -i '' "s/^version = \"$old_version\"/version = \"$new_version\"/" "$REPO_DIR/src-tauri/Cargo.toml"
  sed -i '' "s/\"version\": \"$old_version\"/\"version\": \"$new_version\"/" "$REPO_DIR/src-tauri/tauri.conf.json"
}

choose_asset() {
  local version="$1"

  if [ -n "$ASSET_PATH" ]; then
    if [ ! -f "$ASSET_PATH" ]; then
      echo "ASSET_PATH does not exist: $ASSET_PATH" >&2
      exit 1
    fi
    echo "$ASSET_PATH"
    return
  fi

  if [ ! -d "$BUILD_DIR" ]; then
    echo "Build directory not found: $BUILD_DIR" >&2
    echo "Provide ASSET_PATH explicitly, or place release artifact in build/." >&2
    exit 1
  fi

  local candidate
  candidate=$(ls -t "$BUILD_DIR"/*"$version"*.tar.gz "$BUILD_DIR"/*"$version"*.zip 2>/dev/null | head -1 || true)
  if [ -z "$candidate" ]; then
    candidate=$(ls -t "$BUILD_DIR"/*.tar.gz "$BUILD_DIR"/*.zip 2>/dev/null | head -1 || true)
  fi

  if [ -z "$candidate" ]; then
    echo "No artifact found in $BUILD_DIR (.tar.gz or .zip)." >&2
    echo "Provide ASSET_PATH explicitly." >&2
    exit 1
  fi

  echo "$candidate"
}

replace_formula_field() {
  local formula="$1"
  local key="$2"
  local value="$3"

  case "$key" in
    version)
      sed -i '' "s#^  version \".*\"#  version \"$value\"#" "$formula"
      ;;
    url)
      sed -i '' "s#^  url \".*\"#  url \"$value\"#" "$formula"
      ;;
    sha256)
      sed -i '' "s#^  sha256 \".*\"#  sha256 \"$value\"#" "$formula"
      ;;
    *)
      echo "Unsupported field: $key" >&2
      exit 1
      ;;
  esac
}

require_cmd bun
require_cmd git
require_cmd gh
require_cmd shasum
require_cmd node

if ! gh auth status >/dev/null 2>&1; then
  echo "GitHub CLI not authenticated. Run: gh auth login" >&2
  exit 1
fi

if [ -z "$TAP_REPO" ]; then
  echo "TAP_REPO is required, e.g. TAP_REPO=yourname/homebrew-tap" >&2
  exit 1
fi

if [ -z "$FORMULA_PATH" ]; then
  echo "FORMULA_PATH is required, e.g. FORMULA_PATH=Formula/typemore.rb" >&2
  exit 1
fi

cd "$REPO_DIR"

if [ "$SKIP_BUMP" != "1" ] && ! git diff --quiet -- "${VERSION_FILES[@]}"; then
  echo "Version files have local changes. Commit or stash them first:" >&2
  printf '  %s\n' "${VERSION_FILES[@]}" >&2
  exit 1
fi

VERSION=$(read_version)
DID_BUMP=0

if [ "$SKIP_BUMP" = "1" ]; then
  echo "SKIP_BUMP=1, publishing current version: $VERSION"
else
  OLD_VERSION="$VERSION"
  NEW_VERSION=$(bump_patch_version)
  update_tauri_version "$OLD_VERSION" "$NEW_VERSION"
  VERSION=$(read_version)

  if [ "$VERSION" != "$NEW_VERSION" ]; then
    echo "Version mismatch after bump: expected $NEW_VERSION, got $VERSION" >&2
    exit 1
  fi
  DID_BUMP=1
fi

TAG="v$VERSION"
if [ "$SKIP_BUMP" != "1" ] && git rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then
  echo "Tag already exists: $TAG" >&2
  exit 1
fi

if ! git diff --quiet -- "${VERSION_FILES[@]}"; then
  echo "Version files updated to $VERSION"
fi

RELEASE_DONE=0
cleanup() {
  if [ "$RELEASE_DONE" -eq 0 ] && [ "$DID_BUMP" -eq 1 ]; then
    git checkout -- "${VERSION_FILES[@]}" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

REPO_SLUG=$(gh repo view --json nameWithOwner -q .nameWithOwner)
ARTIFACT=$(choose_asset "$VERSION")
ASSET_NAME=$(basename "$ARTIFACT")
DOWNLOAD_URL="https://github.com/$REPO_SLUG/releases/download/$TAG/$ASSET_NAME"

if [ "$DID_BUMP" -eq 1 ]; then
  git add "${VERSION_FILES[@]}"
  git commit -m "new version: $VERSION"
  git push
  git tag "$TAG"
  git push origin "$TAG"
fi

if gh release view "$TAG" >/dev/null 2>&1; then
  gh release upload "$TAG" "$ARTIFACT" --clobber
else
  gh release create "$TAG" "$ARTIFACT" -t "$TAG" -n "Typemore $TAG"
fi

SHA256=$(shasum -a 256 "$ARTIFACT" | awk '{print $1}')

echo "Refreshing Homebrew tap at $TAP_DIR ..."
rm -rf "$TAP_DIR"
mkdir -p "$TMP_DIR"
git clone "https://github.com/$TAP_REPO.git" "$TAP_DIR"

cd "$TAP_DIR"

if [ ! -f "$FORMULA_PATH" ]; then
  echo "Formula not found: $TAP_DIR/$FORMULA_PATH" >&2
  exit 1
fi

replace_formula_field "$FORMULA_PATH" version "$VERSION"
replace_formula_field "$FORMULA_PATH" url "$DOWNLOAD_URL"
replace_formula_field "$FORMULA_PATH" sha256 "$SHA256"

git add "$FORMULA_PATH"
git commit -m "bump typemore to $VERSION"
git push

RELEASE_DONE=1
echo "Done. Released $TAG and updated Homebrew formula."
