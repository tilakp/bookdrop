#!/bin/bash
# Assembles Bookdrop.app from the SwiftPM executable + Resources/Info.plist +
# Resources/AppIcon.icns. Needed for the Dock icon to actually show and for
# UNUserNotificationCenter to work (both require a real bundle identifier) —
# `swift run` alone can't provide either.
set -euo pipefail

CONFIG="${1:-debug}"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_DIR="$ROOT_DIR/.build/$CONFIG/Bookdrop.app"

echo "Building (${CONFIG})..."
swift build -c "$CONFIG" --package-path "$ROOT_DIR"

echo "Assembling ${APP_DIR}..."
rm -rf "$APP_DIR"
mkdir -p "$APP_DIR/Contents/MacOS" "$APP_DIR/Contents/Resources"

cp "$ROOT_DIR/.build/$CONFIG/Bookdrop" "$APP_DIR/Contents/MacOS/Bookdrop"
cp "$ROOT_DIR/Resources/Info.plist" "$APP_DIR/Contents/Info.plist"
cp "$ROOT_DIR/Resources/AppIcon.icns" "$APP_DIR/Contents/Resources/AppIcon.icns"

# Refresh Launch Services / Dock icon caches so the new icon shows immediately.
touch "$APP_DIR"

echo "Built ${APP_DIR}"
