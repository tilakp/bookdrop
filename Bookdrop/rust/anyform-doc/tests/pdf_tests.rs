use std::path::PathBuf;

use anyform_core::{Options, Priority, StdLog, Value};

/// Requires the vendored chrome-headless-shell binary — run
/// `rust/scripts/fetch-chromium.sh` once before `cargo test` (see plan
/// Phase 2). These are integration tests against the real bundled
/// renderer, not mocked, since the whole point of this phase is proving
/// the bundled-Chromium pipeline actually produces a valid PDF.
fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/minimal.epub")
}

// cargo test runs these in parallel threads within one process, so
// std::process::id() alone isn't a unique-enough output filename — each
// test needs its own name suffix to avoid racing another test's file.
fn convert_fixture(test_name: &str) -> lopdf::Document {
    convert_fixture_with_options(test_name, Options::new())
}

fn convert_fixture_with_options(test_name: &str, opts: Options) -> lopdf::Document {
    let output = std::env::temp_dir().join(format!("anyform-pdf-test-{test_name}-{}.pdf", std::process::id()));
    let registry = anyform_doc::document_registry();
    registry
        .convert(&fixture_path(), &output, &opts, &StdLog)
        .expect("minimal.epub should convert to PDF");
    let doc = lopdf::Document::load(&output).expect("output should be a valid PDF");
    let _ = std::fs::remove_file(&output);
    doc
}

fn opts(pairs: &[(&str, Value)]) -> Options {
    let mut opts = Options::new();
    for (name, value) in pairs {
        opts.set(name, value.clone(), Priority::UserSet);
    }
    opts
}

fn page_media_box(doc: &lopdf::Document, page_id: lopdf::ObjectId) -> (f64, f64, f64, f64) {
    let dict = doc.get_object(page_id).unwrap().as_dict().unwrap();
    let media_box = dict.get(b"MediaBox").unwrap().as_array().unwrap();
    let n = |o: &lopdf::Object| o.as_float().map(f64::from).unwrap_or_else(|_| o.as_i64().unwrap() as f64);
    (n(&media_box[0]), n(&media_box[1]), n(&media_box[2]), n(&media_box[3]))
}

#[test]
fn produces_one_page_per_chapter_plus_cover() {
    let doc = convert_fixture("pages");
    // cover + ch1 + ch2
    assert_eq!(doc.get_pages().len(), 3);
}

#[test]
fn chapter_text_is_readable() {
    let doc = convert_fixture("text");
    let pages = doc.get_pages();
    let page_numbers: Vec<u32> = pages.keys().copied().collect();
    let all_text: String = page_numbers
        .iter()
        .map(|n| doc.extract_text(&[*n]).unwrap_or_default())
        .collect();
    assert!(all_text.contains("Chapter One"));
    assert!(all_text.contains("Chapter Two"));
}

#[test]
fn sets_title_and_author_metadata() {
    let output = std::env::temp_dir().join(format!("anyform-pdf-test-meta-{}.pdf", std::process::id()));
    let registry = anyform_doc::document_registry();
    registry
        .convert(&fixture_path(), &output, &Options::new(), &StdLog)
        .unwrap();
    let meta = lopdf::Document::load_metadata(&output).unwrap();
    let _ = std::fs::remove_file(&output);
    assert_eq!(meta.title.as_deref(), Some("Minimal Fixture Book"));
    assert_eq!(meta.author.as_deref(), Some("Test Author"));
}

#[test]
fn builds_outline_from_toc() {
    let doc = convert_fixture("outline");
    let catalog = doc.catalog().unwrap();
    let outlines_id = catalog
        .get(b"Outlines")
        .and_then(|o| o.as_reference())
        .expect("catalog should reference an Outlines dict");
    let outlines = doc.get_object(outlines_id).unwrap().as_dict().unwrap();
    let count = outlines.get(b"Count").and_then(|o| o.as_i64()).unwrap_or(0);
    assert_eq!(count, 2, "should have two top-level bookmarks (Chapter One, Chapter Two)");
}

#[test]
fn respects_custom_page_size() {
    // A4-ish custom size in inches, distinct from the 8.5x11 default, so a
    // wrong/ignored option would fail this test.
    let doc = convert_fixture_with_options(
        "custom-page-size",
        opts(&[
            ("page_width_in", Value::Float(6.0)),
            ("page_height_in", Value::Float(9.0)),
            ("margin_in", Value::Float(0.25)),
            ("include_cover", Value::Bool(false)),
        ]),
    );
    let pages = doc.get_pages();
    let first_page_id = *pages.values().next().unwrap();
    let (x0, y0, x1, y1) = page_media_box(&doc, first_page_id);
    let width_in = (x1 - x0) / 72.0;
    let height_in = (y1 - y0) / 72.0;
    assert!((width_in - 6.0).abs() < 0.05, "expected ~6in width, got {width_in}in");
    assert!((height_in - 9.0).abs() < 0.05, "expected ~9in height, got {height_in}in");
}

#[test]
fn excludes_cover_when_disabled() {
    let doc = convert_fixture_with_options("no-cover", opts(&[("include_cover", Value::Bool(false))]));
    // ch1 + ch2, no cover page.
    assert_eq!(doc.get_pages().len(), 2);
}

#[test]
fn omits_outline_when_toc_disabled() {
    let doc = convert_fixture_with_options(
        "no-toc",
        opts(&[("generate_table_of_contents", Value::Bool(false))]),
    );
    let catalog = doc.catalog().unwrap();
    assert!(
        catalog.get(b"Outlines").is_err(),
        "catalog should not reference an Outlines dict when TOC generation is disabled"
    );
}

#[test]
fn page_numbers_and_headers_appear_in_rendered_text() {
    let doc = convert_fixture_with_options(
        "headers-footers",
        opts(&[
            ("include_cover", Value::Bool(false)),
            ("show_page_numbers", Value::Bool(true)),
            ("include_headers", Value::Bool(true)),
            ("include_footers", Value::Bool(true)),
        ]),
    );
    let pages = doc.get_pages();
    let page_numbers: Vec<u32> = pages.keys().copied().collect();
    let all_text: String = page_numbers
        .iter()
        .map(|n| doc.extract_text(&[*n]).unwrap_or_default())
        .collect();
    assert!(all_text.contains("Minimal Fixture Book"), "expected book title from header/footer");
    assert!(all_text.contains('1') && all_text.contains('2'), "expected page numbers 1 and 2");
}

#[test]
fn typography_options_are_applied() {
    // font_size_pt/line_spacing get injected as a !important CSS override
    // on every chapter — assert indirectly via a distinctly large custom
    // page + font combination still producing valid, readable output
    // (a wrong/ignored font-size wouldn't itself be visible via text
    // extraction, so this mainly guards against the injection breaking
    // the chapter markup outright).
    let doc = convert_fixture_with_options(
        "typography",
        opts(&[
            ("font_size_pt", Value::Float(18.0)),
            ("line_spacing", Value::Float(2.0)),
            ("font_family", Value::Str("Georgia".into())),
            ("preserve_epub_styling", Value::Bool(false)),
            ("remove_publisher_styling", Value::Bool(true)),
        ]),
    );
    let pages = doc.get_pages();
    let page_numbers: Vec<u32> = pages.keys().copied().collect();
    let all_text: String = page_numbers
        .iter()
        .map(|n| doc.extract_text(&[*n]).unwrap_or_default())
        .collect();
    assert!(all_text.contains("Chapter One"));
    assert!(all_text.contains("Chapter Two"));
}
