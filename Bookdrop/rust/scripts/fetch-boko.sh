#!/usr/bin/env bash
# Downloads the pinned `boko` release binary for BOTH macOS architectures
# into rust/vendor/boko/<platform>/ so KindleInput can convert Kindle-family
# formats (AZW3/KFX/MOBI) without depending on anything installed on the
# user's machine — same bundling approach as fetch-chromium.sh.
#
# Why a bundled subprocess rather than a linked crate: boko is
# GPL-3.0-or-later and Bookdrop is MIT. Invoking it as a separate program
# does not trigger the GPL's linking clause, so Bookdrop stays MIT.
# Distributing the binary still carries GPL-3 obligations for that binary,
# which is why the matching `source.tar.gz` is fetched alongside it (GPL-3
# §6) and recorded in vendor/boko/LICENSE-NOTICE.txt.
#
# Version is pinned (not "latest") so builds are reproducible; bump
# BOKO_VERSION deliberately when updating. Every download is checksum-
# verified against the sha256 the release publishes — a bundled binary that
# ships inside a signed .app should never be taken on trust.
set -euo pipefail

BOKO_VERSION="0.5.0"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VENDOR_DIR="$ROOT_DIR/vendor/boko"
BASE_URL="https://github.com/zacharydenton/boko/releases/download/v$BOKO_VERSION"

# Bookdrop's platform names (matching vendor/chromium/) -> Rust target triples.
target_triple_for() {
    case "$1" in
        mac-arm64) echo "aarch64-apple-darwin" ;;
        mac-x64)   echo "x86_64-apple-darwin" ;;
        *) echo "fetch-boko.sh: unknown platform '$1'" >&2; return 1 ;;
    esac
}

verify_sha256() {
    local file="$1" expected_url="$2" expected actual
    expected="$(curl -fsSL "$expected_url" | awk '{print $1}')"
    actual="$(shasum -a 256 "$file" | awk '{print $1}')"
    if [ "$expected" != "$actual" ]; then
        echo "fetch-boko.sh: checksum mismatch for $file" >&2
        echo "  expected: $expected" >&2
        echo "  actual:   $actual" >&2
        return 1
    fi
}

fetch_platform() {
    local platform="$1"
    local triple dest_dir binary_path tarball
    triple="$(target_triple_for "$platform")"
    dest_dir="$VENDOR_DIR/$platform"
    binary_path="$dest_dir/boko"

    if [ -x "$binary_path" ]; then
        echo "fetch-boko.sh: $platform already present at $binary_path"
        return 0
    fi

    mkdir -p "$dest_dir"
    tarball="$(mktemp -t boko-$platform).tar.xz"
    trap 'rm -f "$tarball"' RETURN

    echo "fetch-boko.sh: downloading boko v$BOKO_VERSION for $platform"
    curl -fSL "$BASE_URL/boko-$triple.tar.xz" -o "$tarball"
    verify_sha256 "$tarball" "$BASE_URL/boko-$triple.tar.xz.sha256"

    # The archive contains a boko-<triple>/ directory; pull just the
    # executable out of it so the layout here stays flat and predictable.
    tar -xJf "$tarball" -C "$dest_dir" --strip-components=1 "boko-$triple/boko"

    # macOS quarantines downloaded binaries; a build-time dependency isn't
    # something the user downloaded themselves, so strip it rather than
    # making them right-click-Open a subprocess they never see.
    xattr -dr com.apple.quarantine "$dest_dir" 2>/dev/null || true
    chmod +x "$binary_path"

    echo "fetch-boko.sh: installed $platform to $binary_path"
}

# GPL-3 §6: shipping the binary obliges us to make the corresponding source
# available. The release publishes a source tarball, so fetch it once and
# keep it next to the binaries rather than relying on an upstream URL
# staying live.
fetch_source() {
    local source_tarball="$VENDOR_DIR/boko-$BOKO_VERSION-source.tar.gz"
    if [ -f "$source_tarball" ]; then
        echo "fetch-boko.sh: source tarball already present"
        return 0
    fi
    mkdir -p "$VENDOR_DIR"
    echo "fetch-boko.sh: downloading boko v$BOKO_VERSION source (GPL-3 §6)"
    curl -fSL "$BASE_URL/source.tar.gz" -o "$source_tarball"
    verify_sha256 "$source_tarball" "$BASE_URL/source.tar.gz.sha256"

    cat > "$VENDOR_DIR/LICENSE-NOTICE.txt" <<EOF
boko v$BOKO_VERSION — https://github.com/zacharydenton/boko
Copyright (c) the boko authors.
Licensed under the GNU General Public License v3.0 or later.

Bookdrop bundles the boko executable to convert Kindle-family ebook
formats (AZW3/KFX/MOBI). It is invoked as a separate program; no boko
code is linked into Bookdrop, which remains MIT-licensed.

The complete corresponding source for the bundled binary is included
alongside it as boko-$BOKO_VERSION-source.tar.gz, and is also available
from the project's release page above.
EOF
    echo "fetch-boko.sh: wrote $VENDOR_DIR/LICENSE-NOTICE.txt"
}

if [ $# -ge 1 ]; then
    fetch_platform "$1"
else
    fetch_platform "mac-arm64"
    fetch_platform "mac-x64"
fi
fetch_source
