#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_DIR="$ROOT_DIR"
TMP_DIR="$ROOT_DIR/tmp"
TAP_DIR="${TAP_DIR:-$TMP_DIR/homebrew-tap}"
TAP_REPO="${TAP_REPO:-everettjf/homebrew-tap}"
CASK_PATH="${CASK_PATH:-Casks/typemore.rb}"
SIGNING_IDENTITY="${SIGNING_IDENTITY:-Developer ID Application: Feng Zhu (YPV49M8592)}"
NOTARYTOOL_PROFILE="${NOTARYTOOL_PROFILE:-}"
APPLE_ID="${APPLE_ID:-}"
APPLE_TEAM_ID="${APPLE_TEAM_ID:-}"
APPLE_APP_SPECIFIC_PASSWORD="${APPLE_APP_SPECIFIC_PASSWORD:-${APPLE_PASSWORD:-${APP_SPECIFIC_PASSWORD:-}}}"
VERSION_FILES=("package.json" "src-tauri/Cargo.toml" "src-tauri/tauri.conf.json")
SKIP_BUMP="${SKIP_BUMP:-0}"

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
fs.writeFileSync(path, JSON.stringify(pkg, null, 2) + '\n');
console.log(pkg.version);
"
}

update_synced_versions() {
  local old_version="$1"
  local new_version="$2"
  sed -i '' "s/^version = \"$old_version\"/version = \"$new_version\"/" "$REPO_DIR/src-tauri/Cargo.toml"
  sed -i '' "s/\"version\": \"$old_version\"/\"version\": \"$new_version\"/" "$REPO_DIR/src-tauri/tauri.conf.json"
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

require_cmd bun
require_cmd cargo
require_cmd codesign
require_cmd gh
require_cmd git
require_cmd node
require_cmd shasum
require_cmd spctl
require_cmd xcrun

if ! gh auth status >/dev/null 2>&1; then
  echo "GitHub CLI not authenticated. Run: gh auth login" >&2
  exit 1
fi

if ! security find-identity -v -p codesigning | grep -Fq "\"$SIGNING_IDENTITY\""; then
  cat >&2 <<EOF
Signing identity not available in keychain:
  $SIGNING_IDENTITY

Available identities:
$(security find-identity -v -p codesigning | sed 's/^/  /')
EOF
  exit 1
fi

if [ -z "$NOTARYTOOL_PROFILE" ] && { [ -z "$APPLE_ID" ] || [ -z "$APPLE_TEAM_ID" ] || [ -z "$APPLE_APP_SPECIFIC_PASSWORD" ]; }; then
  cat >&2 <<EOF
Notarization credentials missing.
Set one of:
  1) NOTARYTOOL_PROFILE=<keychain-profile-name>
  2) APPLE_ID + APPLE_TEAM_ID + APPLE_APP_SPECIFIC_PASSWORD
EOF
  exit 1
fi

cd "$REPO_DIR"

if ! git diff --quiet || ! git diff --cached --quiet; then
  echo "Working tree must be clean before deploy." >&2
  exit 1
fi

VERSION="$(read_version)"
DID_BUMP=0

if [ "$SKIP_BUMP" = "1" ]; then
  echo "SKIP_BUMP=1, releasing current version: $VERSION"
else
  OLD_VERSION="$VERSION"
  NEW_VERSION="$(bump_patch_version)"
  update_synced_versions "$OLD_VERSION" "$NEW_VERSION"
  VERSION="$(read_version)"
  DID_BUMP=1
fi

TAG="v$VERSION"
REPO_SLUG="$(gh repo view --json nameWithOwner -q .nameWithOwner)"
DMG_URL="https://github.com/$REPO_SLUG/releases/download/$TAG/TypeMore.dmg"
RELEASE_BODY_FILE="$TMP_DIR/release-notes-$VERSION.md"
RELEASE_DONE=0

cleanup() {
  rm -f "$RELEASE_BODY_FILE"
  if [ "$RELEASE_DONE" -eq 0 ] && [ "$DID_BUMP" -eq 1 ]; then
    git checkout -- "${VERSION_FILES[@]}" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

if git rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then
  echo "Tag already exists: $TAG" >&2
  exit 1
fi

if [ "$DID_BUMP" -eq 1 ]; then
  git add "${VERSION_FILES[@]}"
  git commit -m "new version: $VERSION"
  git push
fi

echo "Cleaning previous bundle artifacts..."
rm -rf "$REPO_DIR/src-tauri/target/release/bundle"

echo "Building signed macOS app..."
APPLE_SIGNING_IDENTITY="$SIGNING_IDENTITY" \
APPLE_ID="$APPLE_ID" \
APPLE_PASSWORD="$APPLE_APP_SPECIFIC_PASSWORD" \
APPLE_TEAM_ID="$APPLE_TEAM_ID" \
bun run tauri build

DMG_PATH="$(ls -t "$REPO_DIR/src-tauri/target/release/bundle/dmg/TypeMore_${VERSION}_"*.dmg 2>/dev/null | head -1 || true)"
if [ -z "$DMG_PATH" ]; then
  echo "No versioned DMG found for $VERSION." >&2
  exit 1
fi

DMG_DIR="$(dirname "$DMG_PATH")"
RELEASE_DMG_PATH="$DMG_DIR/TypeMore.dmg"
cp -f "$DMG_PATH" "$RELEASE_DMG_PATH"

echo "Submitting DMG for notarization..."
if [ -n "$NOTARYTOOL_PROFILE" ]; then
  xcrun notarytool submit "$RELEASE_DMG_PATH" --keychain-profile "$NOTARYTOOL_PROFILE" --wait
else
  xcrun notarytool submit "$RELEASE_DMG_PATH" \
    --apple-id "$APPLE_ID" \
    --team-id "$APPLE_TEAM_ID" \
    --password "$APPLE_APP_SPECIFIC_PASSWORD" \
    --wait
fi

echo "Stapling notarization ticket..."
xcrun stapler staple "$RELEASE_DMG_PATH"
xcrun stapler validate "$RELEASE_DMG_PATH"

APP_PATH="$(ls -td "$REPO_DIR"/src-tauri/target/release/bundle/macos/*.app 2>/dev/null | head -1 || true)"
if [ -z "$APP_PATH" ]; then
  echo "No .app found at src-tauri/target/release/bundle/macos/" >&2
  exit 1
fi

echo "Verifying app signature..."
codesign --verify --deep --strict --verbose=2 "$APP_PATH"
spctl --assess -vv "$APP_PATH"

cat > "$RELEASE_BODY_FILE" <<EOF
# TypeMore $TAG

Install with Homebrew:

\`\`\`bash
brew install --cask everettjf/tap/typemore
\`\`\`

Upgrade from Homebrew:

\`\`\`bash
brew upgrade --cask typemore
\`\`\`

Prefer a direct download? Download the notarized DMG here:

$DMG_URL
EOF

git tag "$TAG"
git push origin "$TAG"

if gh release view "$TAG" >/dev/null 2>&1; then
  gh release upload "$TAG" "$RELEASE_DMG_PATH" "$DMG_PATH" --clobber
  gh release edit "$TAG" --title "$TAG" --notes-file "$RELEASE_BODY_FILE"
else
  gh release create "$TAG" "$RELEASE_DMG_PATH" "$DMG_PATH" -t "$TAG" -F "$RELEASE_BODY_FILE"
fi

SHA256="$(shasum -a 256 "$RELEASE_DMG_PATH" | awk '{print $1}')"

echo "Updating Homebrew cask..."
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
replace_cask_field "$CASK_PATH" url "$DMG_URL"

git add "$CASK_PATH"
git commit -m "bump typemore to $VERSION"
git push

RELEASE_DONE=1
echo "Released $TAG"
echo "DMG: $RELEASE_DMG_PATH"
