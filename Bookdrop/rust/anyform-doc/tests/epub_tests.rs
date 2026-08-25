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

#[test]
fn throws_on_invalid_archive() {
    let bogus = std::env::temp_dir().join("anyform-not-a-real.epub");
    std::fs::write(&bogus, b"not a zip file").unwrap();
    let result = EpubInput.convert(&bogus, &Options::new(), &StdLog);
    let _ = std::fs::remove_file(&bogus);
    assert!(result.is_err());
}
