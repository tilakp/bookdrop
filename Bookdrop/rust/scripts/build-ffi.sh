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
