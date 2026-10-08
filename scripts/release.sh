#!/usr/bin/env bash
# Usage: ./scripts/release.sh 0.1.1
set -euo pipefail

VERSION="${1:?Usage: $0 <version>}"
# Where to publish. No default on purpose: this checkout's `origin` is the upstream project,
# which must never receive tags or releases from here.
RELEASE_REPO="${ROADEEP_RELEASE_REPO:?Set ROADEEP_RELEASE_REPO=<owner>/<repo> (the GitHub repository to publish to)}"
RELEASE_REMOTE="${ROADEEP_RELEASE_REMOTE:?Set ROADEEP_RELEASE_REMOTE=<git remote name> for that repository}"
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BUILD_DIR="/tmp/roadeep-release-$VERSION"
APP="$BUILD_DIR/Roadeep.app"
ZIP="$BUILD_DIR/Roadeep.zip"

# ── 1. Find Developer ID identity ─────────────────────────────────────────────
IDENTITY=$(security find-identity -v -p codesigning | grep "Developer ID Application" | head -1 | sed 's/.*"\(Developer ID Application[^"]*\)".*/\1/')
if [ -z "$IDENTITY" ]; then
  echo "error: No 'Developer ID Application' certificate found. Install it via Xcode → Settings → Accounts." >&2
  exit 1
fi
echo "Signing with: $IDENTITY"

# ── 2. xcodegen + Release build ───────────────────────────────────────────────
cd "$REPO_ROOT/NotchBuddy"
xcodegen generate
rm -rf "$BUILD_DIR" && mkdir -p "$BUILD_DIR"

xcodebuild \
  -project NotchBuddy.xcodeproj \
  -scheme NotchBuddy \
  -configuration Release \
  build \
  CODE_SIGN_IDENTITY="$IDENTITY" \
  CODE_SIGNING_REQUIRED=YES \
  CODE_SIGNING_ALLOWED=YES \
  CONFIGURATION_BUILD_DIR="$BUILD_DIR"

# ── 3. Zip + notarize ─────────────────────────────────────────────────────────
ditto -c -k --keepParent "$APP" "$ZIP"
xcrun notarytool submit "$ZIP" --keychain-profile roadeep-notary --wait

# ── 4. Staple + verify ────────────────────────────────────────────────────────
xcrun stapler staple "$APP"
spctl -a -vv "$APP"

# ── 5. Re-zip (with stapled app) ──────────────────────────────────────────────
rm "$ZIP"
ditto -c -k --keepParent "$APP" "$ZIP"
echo "Release zip ready: $ZIP"

# ── 6. Tag + GitHub release ───────────────────────────────────────────────────
cd "$REPO_ROOT"
git tag "v$VERSION"
git push "$RELEASE_REMOTE" "v$VERSION"

gh release create "v$VERSION" "$ZIP" \
  --repo "$RELEASE_REPO" \
  --title "Roadeep $VERSION" \
  --notes "$(cat <<EOF
## Install

Download **Roadeep.zip**, unzip and move **Roadeep.app** to \`/Applications\`. Launch — no extra steps needed.

## Build from source

\`\`\`bash
brew install xcodegen
# from a checkout of this repository
cd NotchBuddy && xcodegen && open NotchBuddy.xcodeproj
\`\`\`
EOF
)"

echo "✓ v$VERSION released: https://github.com/$RELEASE_REPO/releases/tag/v$VERSION"
