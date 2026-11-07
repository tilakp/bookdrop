#!/usr/bin/env bash
# Builds anyform-ffi for both macOS architectures and lipo's them into the
# universal static lib that Package.swift links. Run this before `swift
# build`/`swift test` in Bookdrop/ (build-app.sh does this automatically).
# If the C API changed, first re-run `cbindgen --crate anyform-ffi --output
# include/anyform.h` in anyform-ffi/ and copy the header to
# Sources/CAnyform/include/.
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

echo "build-ffi.sh: wrote $ROOT_DIR/target/universal/libanyform_ffi.a"

# SwiftPM links this static lib via raw -L/-l unsafeFlags (see Package.swift),
# not a tracked target dependency, so it does not see that the .a file's
# content changed. `swift build` and `swift test` then skip relinking and
# keep a stale binary with the old Rust code. So delete every linked
# Bookdrop executable and test bundle, and the next build must relink.
#
# A universal build (`swift build --arch arm64 --arch x86_64`, used by
# `build-app.sh release`) uses a different, capitalized layout:
# `.build/apple/Products/<Config>/Bookdrop` for the lipo'd product and
# `.build/apple/Intermediates.noindex/.../Objects-normal/<arch>/Binary/Bookdrop`
# for each arch. The patterns below cover both layouts.
BOOKDROP_ROOT="$(cd "$ROOT_DIR/.." && pwd)"
find "$BOOKDROP_ROOT/.build" \( -path "*/debug/Bookdrop" -o -path "*/release/Bookdrop" \
     -o -path "*/Products/Release/Bookdrop" -o -path "*/Products/Debug/Bookdrop" \
     -o -path "*/Objects-normal/*/Binary/Bookdrop" -o -name "BookdropPackageTests.xctest" \) \
    -exec rm -rf {} + 2>/dev/null || true
echo "build-ffi.sh: cleared stale linked Bookdrop/test binaries to force relink"
