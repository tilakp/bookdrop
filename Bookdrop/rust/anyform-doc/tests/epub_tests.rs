use std::path::PathBuf;

use anyform_core::{InputPlugin, Options, StdLog};
use anyform_doc::EpubInput;

/// Same fixture as `Bookdrop/Tests/BookdropTests/EpubParserTests.swift` —
/// these tests assert the same behavior as that file, so the Rust parser
/// has a known-good behavioral target from day one (see plan Phase 1).
fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/minimal.epub")
}

fn parse() -> anyform_doc::DocumentIR {
    EpubInput
        .convert(&fixture_path(), &Options::new(), &StdLog)
        .expect("minimal.epub should parse")
}

#[test]
fn parses_metadata() {
    let ir = parse();
    assert_eq!(ir.metadata.title, "Minimal Fixture Book");
    assert_eq!(ir.metadata.author.as_deref(), Some("Test Author"));
    assert!(ir.file_size_bytes > 0);
}

#[test]
fn parses_spine_in_order() {
    let ir = parse();
    let hrefs: Vec<&str> = ir.spine.iter().map(|s| s.href.as_str()).collect();
    assert_eq!(hrefs, vec!["ch1.xhtml", "ch2.xhtml"]);
}

#[test]
fn parses_cover_image() {
    let ir = parse();
    let cover = ir.metadata.cover.expect("cover should be present");
    assert!(!cover.is_empty());
}

#[test]
fn parses_nested_table_of_contents() {
    let ir = parse();
    assert_eq!(ir.toc.len(), 2);
    assert_eq!(ir.toc[0].title, "Chapter One");
    assert_eq!(ir.toc[0].href.as_deref(), Some("ch1.xhtml"));
    assert_eq!(ir.toc[1].title, "Chapter Two");
    assert_eq!(ir.toc[1].children.len(), 1);
    assert_eq!(ir.toc[1].children[0].title, "Section One");
    assert_eq!(ir.toc[1].children[0].href.as_deref(), Some("ch2.xhtml#section1"));
}

#[test]
fn extracts_readable_chapter_files() {
    let ir = parse();
    let ch1_path = ir.content_dir.join(&ir.spine[0].href);
    let contents = std::fs::read_to_string(ch1_path).expect("chapter file should be readable");
    assert!(contents.contains("Chapter One"));
}

/// EPUB TOC documents commonly carry a DTD: EPUB2 `toc.ncx` files
/// conventionally open with the NISO ncx DOCTYPE, and EPUB3 `nav.xhtml`
/// files with `<!DOCTYPE html>`. roxmltree rejects any document with a
/// DTD unless `allow_dtd` is set, and both TOC parse sites swallow errors
/// with `.ok()?`, so a DOCTYPE used to make the entire table of contents
/// silently disappear - no PDF bookmarks, no outline, nothing logged.
/// Every Gutenberg fixture happens to omit the DOCTYPE, which is exactly
/// why the rest of the suite never caught this; these two fixtures exist
/// only to pin the behavior for both TOC formats.
#[test]
fn parses_toc_when_nav_document_has_a_doctype() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/doctype-nav.epub");
    let ir = EpubInput.convert(&path, &Options::new(), &StdLog).expect("doctype-nav.epub should parse");
    assert_eq!(ir.toc.len(), 1, "EPUB3 nav TOC should survive a <!DOCTYPE html>");
    assert_eq!(ir.toc[0].title, "Chapter One");
}

#[test]
fn parses_toc_when_ncx_document_has_a_doctype() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/doctype-ncx.epub");
    let ir = EpubInput.convert(&path, &Options::new(), &StdLog).expect("doctype-ncx.epub should parse");
    assert_eq!(ir.toc.len(), 1, "EPUB2 NCX TOC should survive the standard NISO DOCTYPE");
    assert_eq!(ir.toc[0].title, "Chapter One");
}

#[test]
fn deobfuscates_both_font_obfuscation_schemes() {
    // drm-fonts.epub's two font files were obfuscated by an independent
    // Python script (using hashlib/uuid directly, not this codebase) that
    // mirrors calibre's actual process_encryption implementation
    // (epub_input.py) byte for byte - the plaintext bytes are regenerated
    // here from the same formula the fixture-generation script used, so
    // this checks the real deobfuscated output against a known-correct
    // answer rather than round-tripping against our own encoder.
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/drm-fonts.epub");
    let ir = EpubInput.convert(&path, &Options::new(), &StdLog).expect("drm-fonts.epub should parse");

    let plain_adobe: Vec<u8> = (0..2000u32).map(|i| ((i * 7 + 3) % 256) as u8).collect();
    let plain_idpf: Vec<u8> = (0..2000u32).map(|i| ((i * 11 + 17) % 256) as u8).collect();

    let adobe_out = std::fs::read(ir.content_dir.join("fonts/adobe.otf")).expect("adobe font should be readable");
    let idpf_out = std::fs::read(ir.content_dir.join("fonts/idpf.otf")).expect("idpf font should be readable");
    assert_eq!(adobe_out, plain_adobe, "Adobe-scheme font should be fully de-obfuscated");
    assert_eq!(idpf_out, plain_idpf, "IDPF-scheme font should be fully de-obfuscated");
}

#[test]
fn throws_on_invalid_archive() {
    let bogus = std::env::temp_dir().join("anyform-not-a-real.epub");
    std::fs::write(&bogus, b"not a zip file").unwrap();
    let result = EpubInput.convert(&bogus, &Options::new(), &StdLog);
    let _ = std::fs::remove_file(&bogus);
    assert!(result.is_err());
}
