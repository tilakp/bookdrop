#!/bin/bash
# Assembles Bookdrop.app from the SwiftPM executable + Resources/Info.plist +
# Resources/AppIcon.icns. Needed for the Dock icon to actually show and for
# UNUserNotificationCenter to work (both require a real bundle identifier) —
# `swift run` alone can't provide either.
set -euo pipefail

CONFIG="${1:-debug}"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_DIR="$ROOT_DIR/.build/$CONFIG/Bookdrop.app"

echo "Building anyform engine (Rust)..."
"$ROOT_DIR/rust/scripts/fetch-chromium.sh"
"$ROOT_DIR/rust/scripts/build-ffi.sh"

echo "Building (${CONFIG})..."
swift build -c "$CONFIG" --package-path "$ROOT_DIR"

echo "Assembling ${APP_DIR}..."
rm -rf "$APP_DIR"
mkdir -p "$APP_DIR/Contents/MacOS" "$APP_DIR/Contents/Resources"

cp "$ROOT_DIR/.build/$CONFIG/Bookdrop" "$APP_DIR/Contents/MacOS/Bookdrop"
cp "$ROOT_DIR/Resources/Info.plist" "$APP_DIR/Contents/Info.plist"
cp "$ROOT_DIR/Resources/AppIcon.icns" "$APP_DIR/Contents/Resources/AppIcon.icns"

# Bundle the vendored headless-Chromium binary for BOTH architectures
# (see rust/scripts/fetch-chromium.sh), at Contents/Resources/Chromium/<arch>/
# — RustConversionEngine.swift picks the matching one at runtime via
# `#if arch(...)`, same as Rust's own resolve_chromium_path fallback does
# via `cfg!(target_arch)`. Note: the Bookdrop *executable* itself is only
# built for the host architecture below (`swift build` with no `--arch`
# flags) — bundling both Chromium variants is necessary but not
# sufficient for cross-architecture distribution; that also needs a
# universal Bookdrop binary (`swift build --arch arm64 --arch x86_64` +
# `lipo`), not attempted here.
#
# Copies the *entire* chrome-headless-shell-<platform>/ directory, not
# just the executable — it's not a standalone binary, it needs its sibling
# icudtl.dat/*.pak/v8_context_snapshot*/dylibs/resources/ alongside it at
# runtime or it SIGTRAPs on startup (found live: the executable ran fine
# invoked directly with those siblings present, but crashed instantly once
# copied out on its own — Chromium's own fatal-init-failure abort, not a
# code-signing issue despite the crash report's misleading "codeSigningMonitor"
# field).
for CHROMIUM_PLATFORM in mac-arm64 mac-x64; do
    CHROMIUM_SRC_DIR="$ROOT_DIR/rust/vendor/chromium/$CHROMIUM_PLATFORM/chrome-headless-shell-$CHROMIUM_PLATFORM"
    if [ ! -x "$CHROMIUM_SRC_DIR/chrome-headless-shell" ]; then
        echo "build-app.sh: missing $CHROMIUM_SRC_DIR/chrome-headless-shell — run rust/scripts/fetch-chromium.sh" >&2
        exit 1
    fi
    mkdir -p "$APP_DIR/Contents/Resources/Chromium/$CHROMIUM_PLATFORM"
    cp -R "$CHROMIUM_SRC_DIR/." "$APP_DIR/Contents/Resources/Chromium/$CHROMIUM_PLATFORM/"
    chmod +x "$APP_DIR/Contents/Resources/Chromium/$CHROMIUM_PLATFORM/chrome-headless-shell"
done

# Ad-hoc sign so Gatekeeper doesn't block the bundled Chromium binary or a
# statically-linked Rust std that isn't signed by a real Developer ID —
# fine for local builds; a distributed build needs a real signing identity.
codesign --force --deep --sign - "$APP_DIR"

# Refresh Launch Services / Dock icon caches so the new icon shows immediately.
touch "$APP_DIR"

echo "Built ${APP_DIR}"
