use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyform_core::{ConvError, Log, Options, StdLog, Value};

/// Needs the vendored chrome-headless-shell binary: run
/// `rust/scripts/fetch-chromium.sh` once before `cargo test`. These tests
/// use the real bundled renderer, not a mock, because what they prove is
/// that the bundled Chromium pipeline makes a valid PDF.
fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/minimal.epub")
}

// cargo test runs these in parallel threads in one process, so
// std::process::id() alone does not make a unique file name. Each test
// adds its own name, so no two tests write the same file.
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
        opts.set(name, value.clone());
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
fn wide_pre_blocks_wrap_instead_of_clipping() {
    // css-edge-cases.epub has a <pre><code> block with one long unbroken
    // line, wider than the printable page - the same "no shrink to fit"
    // failure mode that caused the earlier image-clipping bug, confirmed
    // by visually rendering this exact fixture before adding the fix (a
    // saved screenshot showed the line cut off mid-word at the page
    // edge). CSS multi-column layout was audited the same way and did
    // *not* reproduce a clipping bug - Chrome sizes columns to fit their
    // container - so no corresponding fix/test exists for that case.
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/css-edge-cases.epub");
    let output = std::env::temp_dir().join(format!("anyform-pdf-test-wide-pre-{}.pdf", std::process::id()));
    let registry = anyform_doc::document_registry();
    registry.convert(&fixture, &output, &Options::new(), &StdLog).expect("css-edge-cases.epub should convert");
    let doc = lopdf::Document::load(&output).expect("output should be a valid PDF");
    let _ = std::fs::remove_file(&output);

    // Wrapping a ~480-character unbroken line at ~15pt in a 6.5in
    // printable width takes far more vertical space than one line, which
    // pushes the chapter past a single page. Before the fix the same
    // fixture produced exactly 1 page (the whole chapter, including the
    // unwrapped code line, fit - by clipping rather than wrapping).
    let pages = doc.get_pages().len();
    assert!(pages >= 2, "expected the wrapped code block to overflow onto a 2nd page, got {pages} page(s)");

    let page_numbers: Vec<u32> = doc.get_pages().keys().copied().collect();
    let text: String = page_numbers.iter().map(|n| doc.extract_text(&[*n]).unwrap_or_default()).collect();
    assert!(text.contains("codewordcodeword"), "expected the code block's text content to survive");
    // Wrapped lines extract with a newline between them rather than a
    // space, so check the two halves of a known phrase separately instead
    // of the exact run.
    assert!(text.contains("Decorative column") && text.contains("paragraph 1"), "expected the multi-column section's text to survive");
}

/// Reports cancelled once `progress()` has been called `after` times -
/// simulates a user hitting cancel partway through a real conversion.
struct CancelAfter {
    calls: AtomicUsize,
    after: usize,
}

impl Log for CancelAfter {
    fn info(&self, _msg: &str) {}
    fn progress(&self, _fraction: f64, _stage: &str) {
        self.calls.fetch_add(1, Ordering::SeqCst);
    }
    fn is_cancelled(&self) -> bool {
        self.calls.load(Ordering::SeqCst) >= self.after
    }
}

#[test]
fn cancellation_stops_in_flight_chapter_rendering() {
    // Chapters render across a pool of concurrent Chrome tabs (see
    // render_chapters_parallel in pdf/render.rs). This proves every worker
    // actually stops polling the same shared cancellation flag instead of
    // running its whole chunk to completion once cancelled, using a real
    // multi-chapter book (minimal.epub only has 2 chapters, not enough to
    // meaningfully exercise a worker pool).
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/origin-of-species.epub");
    let output = std::env::temp_dir().join(format!("anyform-pdf-test-cancel-{}.pdf", std::process::id()));
    let log = CancelAfter { calls: AtomicUsize::new(0), after: 3 };
    let registry = anyform_doc::document_registry();

    let result = registry.convert(&fixture, &output, &Options::new(), &log);

    assert!(matches!(result, Err(ConvError::Cancelled)), "expected Cancelled, got {result:?}");
    assert!(!output.exists(), "cancelled conversion should not have written an output file");
    // Counted in chapters, not seconds, so a slow or busy machine cannot
    // fail it. Each progress call is one rendered chapter (or the cover).
    // After cancellation, each worker may finish only the chapter it is
    // on: at most 3 + one per worker (8), far below the book's 29.
    let rendered = log.calls.load(Ordering::SeqCst);
    assert!(rendered <= 3 + 8, "{rendered} chapters rendered after cancelling at 3: workers did not stop");
}

#[test]
fn typography_options_are_applied() {
    // font_size_pt and line_spacing become a !important CSS override on
    // every chapter. Text extraction cannot see a font size, so this checks
    // that the override does not break the chapter markup. The test
    // `pdf_output_prints_the_prepared_markup_and_leaves_the_book_unchanged`
    // checks that the prepared markup is what Chrome prints.
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

/// Page number (1-based) of every outline entry, in outline order. `None`
/// for an entry whose target is not a page of `doc`.
fn outline_target_pages(doc: &lopdf::Document) -> Vec<Option<u32>> {
    let page_numbers: std::collections::HashMap<lopdf::ObjectId, u32> =
        doc.get_pages().into_iter().map(|(number, id)| (id, number)).collect();
    let mut out = Vec::new();
    fn walk(doc: &lopdf::Document, first: Option<lopdf::ObjectId>, pages: &std::collections::HashMap<lopdf::ObjectId, u32>, out: &mut Vec<Option<u32>>) {
        let mut next = first;
        while let Some(id) = next {
            let item = doc.get_object(id).unwrap().as_dict().unwrap();
            let action = item.get(b"A").and_then(|a| doc.dereference(a)).map(|(_, a)| a.clone());
            let target = action
                .ok()
                .and_then(|a| a.as_dict().ok()?.get(b"D").ok()?.as_array().ok()?.first()?.as_reference().ok())
                .and_then(|page| pages.get(&page).copied());
            out.push(target);
            walk(doc, item.get(b"First").and_then(|o| o.as_reference()).ok(), pages, out);
            next = item.get(b"Next").and_then(|o| o.as_reference()).ok();
        }
    }
    let outlines_id = doc.catalog().unwrap().get(b"Outlines").and_then(|o| o.as_reference()).unwrap();
    let first = doc.get_object(outlines_id).unwrap().as_dict().unwrap().get(b"First").and_then(|o| o.as_reference()).ok();
    walk(doc, first, &page_numbers, &mut out);
    out
}

fn convert_named_fixture(name: &str, test_name: &str) -> lopdf::Document {
    let input = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
    let output = std::env::temp_dir().join(format!("anyform-pdf-test-{test_name}-{}.pdf", std::process::id()));
    anyform_doc::document_registry().convert(&input, &output, &Options::new(), &StdLog).unwrap();
    let doc = lopdf::Document::load(&output).unwrap();
    let _ = std::fs::remove_file(&output);
    doc
}

#[test]
fn toc_fragments_get_their_own_bookmark_pages() {
    // Several chapters share one XHTML file in this Gutenberg book, so its
    // TOC links are "file.html#chapter-n". Bookmarks used to ignore the
    // fragment: 63 entries opened only 15 distinct pages.
    let doc = convert_named_fixture("pride-and-prejudice.epub", "fragments");
    let targets = outline_target_pages(&doc);
    assert!(targets.iter().all(Option::is_some), "every bookmark must open a real page");
    let distinct: std::collections::HashSet<_> = targets.iter().collect();
    assert!(distinct.len() > 50, "expected ~one page per chapter, got {} distinct of {}", distinct.len(), targets.len());
    assert!(targets.windows(2).all(|w| w[0] <= w[1]), "bookmarks must follow reading order");
}

#[test]
fn bookmarks_work_when_the_nav_is_in_a_subfolder() {
    // nested-nav.epub's TOC hrefs are relative to OEBPS/text/, not to the
    // OPF. Bookmarks used to point at object "0 0 R" (no page at all).
    let doc = convert_named_fixture("nested-nav.epub", "nested-nav");
    let targets = outline_target_pages(&doc);
    assert_eq!(targets.len(), 2);
    assert!(targets.iter().all(Option::is_some), "bookmarks must open real pages: {targets:?}");
    assert_ne!(targets[0], targets[1]);
}

#[test]
fn non_ascii_metadata_is_utf16() {
    use anyform_core::{InputPlugin, OutputPlugin};
    let mut ir = anyform_doc::EpubInput.convert(&fixture_path(), &Options::new(), &StdLog).unwrap();
    ir.metadata.title = "Les Misérables – 東京".into();
    let output = std::env::temp_dir().join(format!("anyform-pdf-test-utf16-{}.pdf", std::process::id()));
    anyform_doc::PdfOutput.convert(&ir, &output, &Options::new(), &StdLog).unwrap();
    let doc = lopdf::Document::load(&output).unwrap();
    let _ = std::fs::remove_file(&output);
    let info_id = doc.trailer.get(b"Info").and_then(|o| o.as_reference()).unwrap();
    let title = doc.get_object(info_id).unwrap().as_dict().unwrap().get(b"Title").unwrap().as_str().unwrap().to_vec();
    assert_eq!(&title[..2], &[0xFE, 0xFF], "non-ASCII text strings need a UTF-16BE byte order mark");
    let units: Vec<u16> = title[2..].chunks(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
    assert_eq!(String::from_utf16(&units).unwrap(), "Les Misérables – 東京");
}
