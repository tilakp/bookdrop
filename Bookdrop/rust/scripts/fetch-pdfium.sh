#!/usr/bin/env bash
# Downloads the pinned PDFium release for BOTH macOS architectures into
# rust/vendor/pdfium/<platform>/ so PdfInput can read PDFs without depending
# on anything installed on the user's machine — same bundling approach as
# fetch-chromium.sh/fetch-boko.sh.
#
# Unlike boko, PDFium (BSD-3-Clause, bundled third-party components under
# their own permissive licenses — see licenses/ extracted below) can be
# linked/loaded directly into Bookdrop with no source-offer obligation, so
# there's no source tarball to fetch, just attribution text.
#
# Version is pinned to a specific upstream release tag (never "latest") so
# builds are reproducible; bump PDFIUM_TAG deliberately when updating.
# bblanchon/pdfium-binaries doesn't publish a per-asset .sha256 sibling file
# (unlike boko's releases), so the expected digest is pinned literally below
# instead — computed once at vendoring time and cross-checked against the
# release's Sigstore attestation (`gh attestation verify <file> --repo
# bblanchon/pdfium-binaries`) before being committed here. A bundled binary
# that ships inside a signed .app should never be taken on trust.
set -euo pipefail

PDFIUM_TAG="chromium/8021"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VENDOR_DIR="$ROOT_DIR/vendor/pdfium"
BASE_URL="https://github.com/bblanchon/pdfium-binaries/releases/download/${PDFIUM_TAG//\//%2F}"

# Bookdrop's platform names (matching vendor/chromium/, vendor/boko/) ->
# pdfium-binaries' own asset naming, and the pinned SHA-256 for each
# (verified via `gh attestation verify` when PDFIUM_TAG was last bumped).
sha256_for() {
    case "$1" in
        mac-arm64) echo "994600fa28974ce09a1c51c35039e808a6bc8ea3839050322c101ab229ad5c96" ;;
        mac-x64)   echo "0e770fda56c6726a08fab84c6306ad91eceb10589020ce3a407fad3ebcbe7bb2" ;;
        *) echo "fetch-pdfium.sh: unknown platform '$1'" >&2; return 1 ;;
    esac
}

verify_sha256() {
    local file="$1" expected="$2" actual
    actual="$(shasum -a 256 "$file" | awk '{print $1}')"
    if [ "$expected" != "$actual" ]; then
        echo "fetch-pdfium.sh: checksum mismatch for $file" >&2
        echo "  expected: $expected" >&2
        echo "  actual:   $actual" >&2
        return 1
    fi
}

fetch_platform() {
    local platform="$1"
    local dest_dir dylib_path expected tarball
    dest_dir="$VENDOR_DIR/$platform"
    dylib_path="$dest_dir/libpdfium.dylib"

    if [ -f "$dylib_path" ]; then
        echo "fetch-pdfium.sh: $platform already present at $dylib_path"
        return 0
    fi

    expected="$(sha256_for "$platform")"
    mkdir -p "$dest_dir"
    tarball="$(mktemp -t pdfium-$platform).tgz"
    trap 'rm -f "$tarball"' RETURN

    echo "fetch-pdfium.sh: downloading pdfium $PDFIUM_TAG for $platform"
    curl -fSL "$BASE_URL/pdfium-$platform.tgz" -o "$tarball"
    verify_sha256 "$tarball" "$expected"

    # Pull just the dylib and the license texts out of the archive; skip the
    # C headers (anyform-doc binds against pdfium-render's own bundled
    # bindings, not these) and every other platform's build metadata.
    tar -xzf "$tarball" -C "$dest_dir" lib/libpdfium.dylib LICENSE licenses
    mv "$dest_dir/lib/libpdfium.dylib" "$dylib_path"
    rmdir "$dest_dir/lib"

    # macOS quarantines downloaded files; a build-time dependency isn't
    # something the user downloaded themselves, so strip it rather than
    # making them right-click-Open a library they never see.
    xattr -dr com.apple.quarantine "$dest_dir" 2>/dev/null || true

    echo "fetch-pdfium.sh: installed $platform to $dylib_path"
}

write_license_notice() {
    local notice="$VENDOR_DIR/LICENSE-NOTICE.txt"
    if [ -f "$notice" ]; then
        return 0
    fi
    cat > "$notice" <<EOF
PDFium (build $PDFIUM_TAG) — https://pdfium.googlesource.com/pdfium/
Prebuilt by https://github.com/bblanchon/pdfium-binaries, BSD-3-Clause.

Bookdrop bundles libpdfium.dylib to read PDF files (PdfInput). Unlike
boko, PDFium's license is permissive and imposes no source-offer
obligation — it is loaded directly (dlopen) rather than run as a
subprocess. The exact license texts for PDFium itself and every
third-party component it statically includes (freetype, libjpeg-turbo,
zlib, lcms, icu, abseil, ...) are preserved verbatim in mac-arm64/LICENSE
and mac-arm64/licenses/ (identical across both architectures — only one
copy is kept per platform directory since that's how the upstream
release ships them).
EOF
    echo "fetch-pdfium.sh: wrote $notice"
}

if [ $# -ge 1 ]; then
    fetch_platform "$1"
else
    fetch_platform "mac-arm64"
    fetch_platform "mac-x64"
fi
write_license_notice
