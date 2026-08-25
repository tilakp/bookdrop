#!/usr/bin/env bash
# Builds Release, installs to /Applications, and packages a distributable
# DMG in release/. Needs create-dmg (`brew install create-dmg`) for the
# disk-image step — everything else (Rust engine, Chromium bundling,
# codesigning) is handled by build-app.sh.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

APP_NAME="Bookdrop.app"
VERSION="$(/usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" Resources/Info.plist)"
OUT="release"
STAGING="$OUT/staging"
DMG="$OUT/Bookdrop-$VERSION.dmg"

echo "==> Building Release (v$VERSION)"
"$ROOT_DIR/Scripts/build-app.sh" release

APP=".build/release/$APP_NAME"
[ -d "$APP" ] || { echo "Build did not produce $APP"; exit 1; }

echo "==> Installing to /Applications"
rm -rf "/Applications/$APP_NAME"
cp -R "$APP" /Applications/

echo "==> Packaging $DMG"
rm -rf "$STAGING" "$DMG"
mkdir -p "$STAGING"
cp -R "$APP" "$STAGING/"

if command -v create-dmg >/dev/null 2>&1; then
    create-dmg \
        --volname "Bookdrop" \
        --window-pos 200 120 \
        --window-size 660 400 \
        --icon-size 100 \
        --icon "$APP_NAME" 165 190 \
        --hide-extension "$APP_NAME" \
        --app-drop-link 495 185 \
        "$DMG" \
        "$STAGING/"
else
    echo "!! create-dmg not found — skipping DMG. Install it with: brew install create-dmg"
fi

rm -rf "$STAGING"
echo "==> Done. Installed /Applications/$APP_NAME"
[ -f "$DMG" ] && echo "    DMG: $DMG"
