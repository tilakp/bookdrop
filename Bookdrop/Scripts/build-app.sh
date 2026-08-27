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
"$ROOT_DIR/rust/scripts/fetch-pdfium.sh"
"$ROOT_DIR/rust/scripts/build-ffi.sh"

# Only `release` builds universal (arm64 + x86_64) — it's the one config that
# ships in a DMG. `debug` stays host-arch-only on purpose: building the whole
# Swift module twice would tax every local `build-app.sh debug` iteration for
# a slice nobody runs during development. If this ever needs revisiting,
# that's a deliberate choice to reconsider, not an oversight to "fix".
if [ "$CONFIG" = "release" ]; then
    echo "Building (release, universal arm64+x86_64)..."
    swift build -c release --arch arm64 --arch x86_64 --package-path "$ROOT_DIR"
    # SwiftPM's multi-arch build lands the lipo'd product under
    # .build/apple/Products/<Config>/ (the Xcode-style "Products" layout),
    # not the normal single-arch .build/<config>/ used below — confirmed by
    # running the build and inspecting it directly, not assumed.
    BUILT_EXE="$ROOT_DIR/.build/apple/Products/Release/Bookdrop"

    # A single-arch release binary must never silently ship — the failure
    # mode (an Intel user's app refusing to launch) is otherwise invisible
    # until someone actually tries it on Intel hardware.
    [ -f "$BUILT_EXE" ] || { echo "build-app.sh: universal build produced no binary at $BUILT_EXE" >&2; exit 1; }
    LIPO_INFO="$(lipo -info "$BUILT_EXE")"
    case "$LIPO_INFO" in
        *x86_64*arm64*|*arm64*x86_64*) ;;
        *) echo "build-app.sh: $BUILT_EXE is not universal: $LIPO_INFO" >&2; exit 1 ;;
    esac
else
    echo "Building (${CONFIG})..."
    swift build -c "$CONFIG" --package-path "$ROOT_DIR"
    BUILT_EXE="$ROOT_DIR/.build/$CONFIG/Bookdrop"
fi

echo "Assembling ${APP_DIR}..."
rm -rf "$APP_DIR"
mkdir -p "$APP_DIR/Contents/MacOS" "$APP_DIR/Contents/Resources"

cp "$BUILT_EXE" "$APP_DIR/Contents/MacOS/Bookdrop"
cp "$ROOT_DIR/Resources/Info.plist" "$APP_DIR/Contents/Info.plist"
cp "$ROOT_DIR/Resources/AppIcon.icns" "$APP_DIR/Contents/Resources/AppIcon.icns"

# Bundle the vendored headless-Chromium binary for BOTH architectures
# (see rust/scripts/fetch-chromium.sh), at Contents/Resources/Chromium/<arch>/
# — RustConversionEngine.swift picks the matching one at runtime via
# `#if arch(...)`, same as Rust's own resolve_chromium_path fallback does
# via `cfg!(target_arch)`. For `release`, the Bookdrop executable above is
# itself a universal (arm64 + x86_64) binary, so `#if arch(...)` is a
# *compile-time* branch baked into each slice — the kernel/Rosetta picks
# the slice at launch, and that slice's own compiled-in branch resolves
# the matching Chromium/Boko directory below. Bundling both variants is
# what makes that resolution correct once the executable itself is fat;
# `debug` stays host-arch-only (see above), so on debug only the host's
# own Chromium/Boko variant is ever actually exercised.
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

# Bundle the vendored `libpdfium.dylib` for both architectures, at
# Contents/Resources/Pdfium/<arch>/ — used by PdfInput to read PDF files.
# Unlike boko, PDFium's license (BSD-3-Clause + permissively-licensed
# third-party components — see rust/scripts/fetch-pdfium.sh) is directly
# linkable/loadable with no source-offer obligation, so only the license
# attribution text travels with it, no source tarball.
for PDFIUM_PLATFORM in mac-arm64 mac-x64; do
    PDFIUM_SRC="$ROOT_DIR/rust/vendor/pdfium/$PDFIUM_PLATFORM/libpdfium.dylib"
    if [ ! -f "$PDFIUM_SRC" ]; then
        echo "build-app.sh: missing $PDFIUM_SRC — run rust/scripts/fetch-pdfium.sh" >&2
        exit 1
    fi
    mkdir -p "$APP_DIR/Contents/Resources/Pdfium/$PDFIUM_PLATFORM"
    cp "$PDFIUM_SRC" "$APP_DIR/Contents/Resources/Pdfium/$PDFIUM_PLATFORM/libpdfium.dylib"
done
cp "$ROOT_DIR/rust/vendor/pdfium/LICENSE-NOTICE.txt" "$APP_DIR/Contents/Resources/Pdfium/LICENSE-NOTICE.txt"

# Ad-hoc sign so Gatekeeper doesn't block the bundled Chromium binary or a
# statically-linked Rust std that isn't signed by a real Developer ID —
# fine for local builds; a distributed build needs a real signing identity.
codesign --force --deep --sign - "$APP_DIR"

# Refresh Launch Services / Dock icon caches so the new icon shows immediately.
touch "$APP_DIR"

echo "Built ${APP_DIR}"
