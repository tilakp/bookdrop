#!/usr/bin/env bash
# Builds anyform-ffi for both macOS architectures, lipo's them into a
# universal static lib, and packages it as an xcframework Package.swift's
# `.binaryTarget` can link (see plan Phase 3/4). Run this before `swift
# build`/`swift test` in Bookdrop/ — build-app.sh does this automatically;
# run it manually after changing anyform-ffi's public surface (and re-run
# `cbindgen --crate anyform-ffi --output include/anyform.h` in
# anyform-ffi/ first if the C API itself changed).
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

cargo build --release --target aarch64-apple-darwin -p anyform-ffi
cargo build --release --target x86_64-apple-darwin -p anyform-ffi

mkdir -p target/universal
lipo -create \
    target/aarch64-apple-darwin/release/libanyform_ffi.a \
    target/x86_64-apple-darwin/release/libanyform_ffi.a \
    -output target/universal/libanyform_ffi.a

rm -rf target/AnyformFFI.xcframework
xcodebuild -create-xcframework \
    -library target/universal/libanyform_ffi.a -headers anyform-ffi/include \
    -output target/AnyformFFI.xcframework

echo "build-ffi.sh: wrote $ROOT_DIR/target/AnyformFFI.xcframework"

# SwiftPM links this static lib via raw -L/-l unsafeFlags (see Package.swift),
# not a tracked target dependency, so it has NO idea the .a file's *content*
# changed when only its mtime/path stay the same — `swift build`/`swift test`
# silently skip relinking and keep serving a stale binary that still has
# whatever Rust bug you just "fixed". Confirmed the hard way: multiple
# rounds of real Rust fixes never reached the installed .app because of
# exactly this. Force it: delete every previously-linked Bookdrop
# executable and test bundle so the next `swift build`/`swift test`/
# `Scripts/build-app.sh` is guaranteed to relink against what was just built.
#
# A universal (`swift build --arch arm64 --arch x86_64`, used by
# `build-app.sh release`) writes to a different, capitalized layout —
# `.build/apple/Products/<Config>/Bookdrop` for the lipo'd product and
# `.build/apple/Intermediates.noindex/.../Objects-normal/<arch>/Binary/Bookdrop`
# per-arch — neither of which the original debug/release patterns below
# match (case and shape both differ), so both are covered explicitly.
BOOKDROP_ROOT="$(cd "$ROOT_DIR/.." && pwd)"
find "$BOOKDROP_ROOT/.build" \( -path "*/debug/Bookdrop" -o -path "*/release/Bookdrop" \
     -o -path "*/Products/Release/Bookdrop" -o -path "*/Products/Debug/Bookdrop" \
     -o -path "*/Objects-normal/*/Binary/Bookdrop" -o -name "BookdropPackageTests.xctest" \) \
    -exec rm -rf {} + 2>/dev/null || true
echo "build-ffi.sh: cleared stale linked Bookdrop/test binaries to force relink"
