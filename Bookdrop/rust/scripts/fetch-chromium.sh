#!/usr/bin/env bash
# Downloads the pinned chrome-headless-shell build (Chrome for Testing) for
# BOTH macOS architectures into rust/vendor/chromium/<platform>/ so
# PdfOutput has a bundled, self-contained headless renderer with no
# dependency on a system Chrome install, and build-app.sh can bundle
# whichever binary matches the architecture the app is actually running on
# (see plan item 4 — universal Chromium bundling). Not run automatically by
# `cargo build` — invoked explicitly by build-app.sh (and by
# developers/CI ahead of testing).
#
# Version is pinned (not "latest") so builds are reproducible; bump
# CFT_VERSION deliberately when updating. Note: this fetches both
# platforms unconditionally (~180MB total download) — the Rust build/test
# itself only needs the host machine's own platform (see
# resolve_chromium_path's dev fallback in anyform-doc/src/pdf.rs), so
# `fetch-chromium.sh mac-arm64` (or `mac-x64`) fetches just one if you
# don't need both.
set -euo pipefail

CFT_VERSION="152.0.7977.54"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VENDOR_DIR="$ROOT_DIR/vendor/chromium"

fetch_platform() {
    local platform="$1"
    local dest_dir="$VENDOR_DIR/$platform"
    local binary_path="$dest_dir/chrome-headless-shell-$platform/chrome-headless-shell"

    if [ -x "$binary_path" ]; then
        echo "fetch-chromium.sh: $platform already present at $binary_path"
        return 0
    fi

    local url="https://storage.googleapis.com/chrome-for-testing-public/$CFT_VERSION/$platform/chrome-headless-shell-$platform.zip"
    mkdir -p "$dest_dir"
    local tmp_zip
    tmp_zip="$(mktemp -t chrome-headless-shell).zip"
    trap 'rm -f "$tmp_zip"' RETURN

    echo "fetch-chromium.sh: downloading $url"
    curl -fSL "$url" -o "$tmp_zip"
    unzip -q -o "$tmp_zip" -d "$dest_dir"

    # macOS Gatekeeper quarantines downloaded binaries; a bundled build-time
    # dependency isn't something the user "downloaded" themselves, so strip
    # the quarantine attribute rather than making them right-click-Open it.
    xattr -dr com.apple.quarantine "$dest_dir" 2>/dev/null || true
    chmod +x "$binary_path"

    echo "fetch-chromium.sh: installed $platform to $binary_path"
}

if [ $# -ge 1 ]; then
    fetch_platform "$1"
else
    fetch_platform "mac-arm64"
    fetch_platform "mac-x64"
fi
