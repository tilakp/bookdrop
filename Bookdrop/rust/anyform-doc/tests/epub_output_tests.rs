use std::path::PathBuf;

use anyform_core::{InputPlugin, OutputPlugin, Options, StdLog};
use anyform_doc::{DocumentIR, EpubInput};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Parses `fixture`, converts it to a fresh temp `.epub` via the full
/// registry (proving the plugin is actually wired into `document_registry`,
/// not just directly constructible), re-parses the result with `EpubInput`,
/// and returns both IRs. `name` keeps the temp filename unique per test
/// (see `pdf_tests.rs`/`real_book_tests.rs` for this session's established
/// convention - `std::process::id()` alone isn't unique enough since
/// `cargo test` runs tests in parallel within one process).
fn round_trip(name: &str, fixture: &str) -> (DocumentIR, DocumentIR) {
    let before = EpubInput.convert(&fixtures_dir().join(fixture), &Options::new(), &StdLog).expect("fixture should parse");

    let output = std::env::temp_dir().join(format!("anyform-epub-out-{name}-{}.epub", std::process::id()));
    let registry = anyform_doc::document_registry();
    registry.convert(&fixtures_dir().join(fixture), &output, &Options::new(), &StdLog).expect("EpubOutput should convert");

    let after = EpubInput.convert(&output, &Options::new(), &StdLog).expect("regenerated EPUB should parse");
    let _ = std::fs::remove_file(&output);
    (before, after)
}

#[test]
fn round_trips_metadata() {
    let (before, after) = round_trip("metadata", "minimal.epub");
    assert_eq!(before.metadata.title, after.metadata.title);
    assert_eq!(before.metadata.author, after.metadata.author);
    assert_eq!(before.metadata.language, after.metadata.language);
    // file_size_bytes is measured from the source path at parse time (see
    // EpubInput's doc comment) - it will legitimately differ between the
    // original and regenerated archive, so only assert it's plausible.
    assert!(after.file_size_bytes > 0);
}

#[test]
fn round_trips_spine_order() {
    let (before, after) = round_trip("spine-order", "minimal.epub");
    let before_hrefs: Vec<&str> = before.spine.iter().map(|s| s.href.as_str()).collect();
    let after_hrefs: Vec<&str> = after.spine.iter().map(|s| s.href.as_str()).collect();
    assert_eq!(before_hrefs, after_hrefs);
}

#[test]
fn round_trips_manifest_resources() {
    let (before, after) = round_trip("manifest", "minimal.epub");
    for (id, resource) in &before.manifest {
        let regenerated = after.manifest.get(id).unwrap_or_else(|| panic!("manifest lost resource {id}"));
        assert_eq!(regenerated.href, resource.href, "href changed for {id}");
        assert_eq!(regenerated.media_type, resource.media_type, "media_type changed for {id}");
        assert_eq!(regenerated.properties, resource.properties, "properties changed for {id}");
    }
}

#[test]
fn round_trips_toc_tree() {
    // minimal.epub's nav has a nested child (Chapter Two > Section One) -
    // this is the first *output*-path test to exercise that nesting;
    // TocNode derives PartialEq so this is a real structural comparison,
    // not just flattened title checks.
    let (before, after) = round_trip("toc-tree", "minimal.epub");
    assert_eq!(before.toc, after.toc);
}

#[test]
fn round_trips_cover_bytes() {
    let (before, after) = round_trip("cover", "minimal.epub");
    assert_eq!(before.metadata.cover, after.metadata.cover);
    assert!(after.metadata.cover.is_some());
}

#[test]
fn preserves_chapter_bytes_exactly() {
    let (before, after) = round_trip("chapter-bytes", "minimal.epub");
    for item in &before.spine {
        let original = std::fs::read(before.content_dir.join(&item.href)).expect("original chapter should be readable");
        let regenerated = std::fs::read(after.content_dir.join(&item.href)).expect("regenerated chapter should be readable");
        assert_eq!(original, regenerated, "chapter bytes changed for {}", item.href);
    }
}

#[test]
fn mimetype_entry_is_stored_and_first() {
    let output = std::env::temp_dir().join(format!("anyform-epub-out-mimetype-{}.pdf.epub", std::process::id()));
    let registry = anyform_doc::document_registry();
    registry
        .convert(&fixtures_dir().join("minimal.epub"), &output, &Options::new(), &StdLog)
        .expect("EpubOutput should convert");

    let file = std::fs::File::open(&output).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    let _ = std::fs::remove_file(&output);

    let first = archive.by_index(0).unwrap();
    assert_eq!(first.name(), "mimetype");
    assert_eq!(first.compression(), zip::CompressionMethod::Stored);
}

#[test]
fn epub2_ncx_only_source_gets_an_epub3_nav() {
    // doctype-ncx.epub (from the DOCTYPE regression suite) has only a
    // toc.ncx, no EPUB3 nav document at all.
    let (before, after) = round_trip("ncx-only", "doctype-ncx.epub");
    let has_nav = after.manifest.values().any(|r| r.properties.contains("nav"));
    assert!(has_nav, "EpubOutput must synthesize an EPUB3 nav item even for an EPUB2-only source");
    assert_eq!(before.toc, after.toc);
}

#[test]
fn nested_toc_directory_keeps_hrefs_valid() {
    // The only fixture that pins the "TocNode.href is relative to the nav
    // document's own directory, not content_dir" finding: nested-nav.epub's
    // nav lives at OEBPS/text/nav.xhtml with chapter hrefs written relative
    // to text/ (bare "ch1.xhtml", not "text/ch1.xhtml"). If EpubOutput ever
    // regenerated the nav at the wrong directory, every TOC link would
    // silently point at a nonexistent file - EpubInput's own nav-then-ncx
    // fallback masks this from a plain before/after `toc` comparison (both
    // the nav and the always-generated ncx carry identical href text, so a
    // broken nav silently falls through to the ncx with the same-looking
    // TOC), so this checks the zip entry's *physical location* directly
    // against what the manifest itself declares, not just parsed TOC text.
    let output = std::env::temp_dir().join(format!("anyform-epub-out-nested-toc-{}.epub", std::process::id()));
    let registry = anyform_doc::document_registry();
    registry.convert(&fixtures_dir().join("nested-nav.epub"), &output, &Options::new(), &StdLog).expect("EpubOutput should convert");

    let file = std::fs::File::open(&output).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    let opf_bytes = {
        let mut opf = archive.by_name("OEBPS/content.opf").expect("OPF should exist");
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut opf, &mut buf).unwrap();
        buf
    };
    assert!(opf_bytes.contains("href=\"text/nav.xhtml\""), "OPF should still declare the nav at its original directory");
    assert!(archive.by_name("OEBPS/text/nav.xhtml").is_ok(), "the regenerated nav file must physically exist at the directory its own manifest entry declares");

    let after = EpubInput.convert(&output, &Options::new(), &StdLog).expect("regenerated EPUB should parse");
    let _ = std::fs::remove_file(&output);

    let before = EpubInput.convert(&fixtures_dir().join("nested-nav.epub"), &Options::new(), &StdLog).unwrap();
    assert_eq!(before.toc, after.toc);
    for node in &after.toc {
        let href = node.href.as_deref().expect("toc node should have an href");
        let resolved = after.content_dir.join("text").join(href.split('#').next().unwrap());
        assert!(resolved.exists(), "TOC link {href} (resolved to {resolved:?}) should point at a real file");
    }
}

#[test]
fn empty_spine_is_rejected() {
    let ir = DocumentIR {
        metadata: anyform_doc::Metadata { title: "Empty".into(), author: None, language: None, cover: None, cover_href: None },
        manifest: std::collections::HashMap::new(),
        spine: Vec::new(),
        toc: Vec::new(),
        file_size_bytes: 0,
        content_dir: std::env::temp_dir(),
    };
    let output = std::env::temp_dir().join(format!("anyform-epub-out-empty-{}.epub", std::process::id()));
    let result = anyform_doc::EpubOutput.convert(&ir, &output, &Options::new(), &StdLog);
    let _ = std::fs::remove_file(&output);
    assert!(result.is_err());
}

#[test]
fn creates_missing_output_directory() {
    // Every other output plugin (pdf/txt/html/docx) creates its output's
    // parent directory before writing; EpubOutput originally didn't,
    // which every existing test here missed since they all write into
    // std::env::temp_dir() directly (always exists) - only caught by a
    // Swift-side end-to-end test that used a genuinely fresh UUID-named
    // directory, the same shape a real "Convert Again" into a brand-new
    // output folder would hit. Regression-tests that specifically.
    let dir = std::env::temp_dir().join(format!("anyform-epub-out-newdir-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!dir.exists(), "test setup: directory must not already exist");

    let output = dir.join("out.epub");
    let registry = anyform_doc::document_registry();
    let result = registry.convert(&fixtures_dir().join("minimal.epub"), &output, &Options::new(), &StdLog);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(result.is_ok(), "expected EpubOutput to create its output directory, got {result:?}");
}
