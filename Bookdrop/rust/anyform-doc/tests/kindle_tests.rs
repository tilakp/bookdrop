use std::path::PathBuf;

use anyform_core::{InputPlugin, Options, StdLog};
use anyform_doc::KindleInput;

/// Kindle-family input goes through the bundled `boko` binary, so these
/// need `rust/scripts/fetch-boko.sh` to have run (same requirement shape
/// as `pdf_tests.rs` and the vendored Chromium). `minimal.azw3` is
/// `minimal.epub` converted by boko, so the two must parse to the same
/// `DocumentIR` — that equivalence is the whole point of normalizing
/// Kindle formats to EPUB rather than writing a separate parser.
fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

#[test]
fn azw3_parses_to_the_same_ir_as_its_source_epub() {
    let from_azw3 = KindleInput
        .convert(&fixture("minimal.azw3"), &Options::new(), &StdLog)
        .expect("minimal.azw3 should parse via the bundled converter");
    let from_epub = anyform_doc::EpubInput
        .convert(&fixture("minimal.epub"), &Options::new(), &StdLog)
        .expect("minimal.epub should parse");

    assert_eq!(from_azw3.metadata.title, from_epub.metadata.title);
    assert_eq!(from_azw3.metadata.author, from_epub.metadata.author);
    assert_eq!(from_azw3.spine.len(), from_epub.spine.len(), "spine length should survive the round trip");

    let azw3_toc: Vec<&str> = from_azw3.toc.iter().map(|n| n.title.as_str()).collect();
    let epub_toc: Vec<&str> = from_epub.toc.iter().map(|n| n.title.as_str()).collect();
    assert_eq!(azw3_toc, epub_toc, "TOC entries should survive the round trip");
}

#[test]
fn azw3_reports_the_original_file_size_not_the_intermediate_epub() {
    let ir = KindleInput.convert(&fixture("minimal.azw3"), &Options::new(), &StdLog).unwrap();
    let actual = std::fs::metadata(fixture("minimal.azw3")).unwrap().len();
    assert_eq!(ir.file_size_bytes, actual, "size shown in the UI should describe the file the user picked");
}

#[test]
fn azw3_converts_end_to_end_to_pdf() {
    let output = std::env::temp_dir().join(format!("anyform-kindle-test-{}.pdf", std::process::id()));
    let registry = anyform_doc::document_registry();
    registry
        .convert(&fixture("minimal.azw3"), &output, &Options::new(), &StdLog)
        .expect("azw3 should convert straight through to PDF");
    let doc = lopdf::Document::load(&output).expect("output should be a valid PDF");
    let _ = std::fs::remove_file(&output);

    assert!(doc.get_pages().len() >= 2, "expected at least one page per chapter");
    let page_numbers: Vec<u32> = doc.get_pages().keys().copied().collect();
    let text: String = page_numbers.iter().map(|n| doc.extract_text(&[*n]).unwrap_or_default()).collect();
    assert!(text.contains("Chapter One"), "chapter text should survive AZW3 -> EPUB -> PDF");
    assert!(text.contains("Chapter Two"));
}

#[test]
fn registry_dispatches_every_kindle_extension_to_this_plugin() {
    // Guards the wiring rather than the conversion: a format missing from
    // `extensions()` would fall through to "no input plugin registered"
    // with a confusing error, and nothing else in the suite would notice.
    for ext in ["azw3", "azw", "kfx", "mobi"] {
        assert!(
            KindleInput.extensions().contains(&ext),
            "{ext} should be handled by KindleInput"
        );
    }
}

#[test]
fn unreadable_kindle_file_reports_a_useful_error() {
    let bogus = std::env::temp_dir().join(format!("anyform-not-a-real-{}.azw3", std::process::id()));
    std::fs::write(&bogus, b"this is definitely not a kindle book").unwrap();
    let result = KindleInput.convert(&bogus, &Options::new(), &StdLog);
    let _ = std::fs::remove_file(&bogus);

    let err = result.expect_err("a garbage file should not parse").to_string();
    assert!(
        err.contains("couldn't be read") || err.contains("DRM-protected"),
        "error should be user-facing, got: {err}"
    );
}
