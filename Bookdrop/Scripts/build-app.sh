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
"$ROOT_DIR/rust/scripts/fetch-boko.sh"
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

# Bundle the vendored `boko` binary for both architectures, at
# Contents/Resources/Boko/<arch>/ — used by KindleInput to normalize
# AZW3/KFX/MOBI into EPUB before the regular pipeline runs. Unlike
# Chromium this *is* a standalone executable with no sibling resources,
# so a plain file copy is enough.
#
# It ships as a separate program rather than linked code on purpose: boko
# is GPL-3.0-or-later and Bookdrop is MIT (see rust/scripts/fetch-boko.sh).
# The GPL-3 §6 source tarball and licence notice are copied in beside it so
# the obligation travels with the .app rather than living only in the
# build tree.
for BOKO_PLATFORM in mac-arm64 mac-x64; do
    BOKO_SRC="$ROOT_DIR/rust/vendor/boko/$BOKO_PLATFORM/boko"
    if [ ! -x "$BOKO_SRC" ]; then
        echo "build-app.sh: missing $BOKO_SRC — run rust/scripts/fetch-boko.sh" >&2
        exit 1
    fi
    mkdir -p "$APP_DIR/Contents/Resources/Boko/$BOKO_PLATFORM"
    cp "$BOKO_SRC" "$APP_DIR/Contents/Resources/Boko/$BOKO_PLATFORM/boko"
    chmod +x "$APP_DIR/Contents/Resources/Boko/$BOKO_PLATFORM/boko"
done
cp "$ROOT_DIR/rust/vendor/boko/LICENSE-NOTICE.txt" "$APP_DIR/Contents/Resources/Boko/LICENSE-NOTICE.txt"
cp "$ROOT_DIR"/rust/vendor/boko/boko-*-source.tar.gz "$APP_DIR/Contents/Resources/Boko/"

# Ad-hoc sign so Gatekeeper doesn't block the bundled Chromium binary or a
# statically-linked Rust std that isn't signed by a real Developer ID —
# fine for local builds; a distributed build needs a real signing identity.
codesign --force --deep --sign - "$APP_DIR"

# Refresh Launch Services / Dock icon caches so the new icon shows immediately.
touch "$APP_DIR"

echo "Built ${APP_DIR}"
