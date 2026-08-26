use std::path::PathBuf;

use anyform_core::{Options, StdLog};

/// Regression suite against real, structurally diverse public-domain
/// books, not the synthetic `minimal.epub` fixture. Three real bugs this
/// session (duplicate cover page, pagination collapse, image clipping)
/// were only caught on real books - the tiny fixture's markup was too
/// simple to exercise them. These fixtures were chosen to cover the
/// shapes that broke before: an image-heavy book, a book with real
/// footnotes and a large back-of-book index, and a book with data tables.
/// All four are public domain (Project Gutenberg) so they can ship in
/// git history.
fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn convert(name: &str, epub: &str) -> lopdf::Document {
    let output = std::env::temp_dir().join(format!("anyform-real-book-{name}-{}.pdf", std::process::id()));
    let registry = anyform_doc::document_registry();
    registry
        .convert(&fixtures_dir().join(epub), &output, &Options::new(), &StdLog)
        .unwrap_or_else(|e| panic!("{epub} should convert to PDF: {e}"));
    let doc = lopdf::Document::load(&output).expect("output should be a valid PDF");
    let _ = std::fs::remove_file(&output);
    doc
}

fn all_text(doc: &lopdf::Document) -> String {
    let page_numbers: Vec<u32> = doc.get_pages().keys().copied().collect();
    page_numbers.iter().map(|n| doc.extract_text(&[*n]).unwrap_or_default()).collect()
}

fn image_xobject_count(doc: &lopdf::Document) -> usize {
    doc.objects
        .values()
        .filter(|obj| {
            obj.as_stream()
                .ok()
                .and_then(|s| s.dict.get(b"Subtype").ok())
                .and_then(|t| t.as_name().ok())
                .is_some_and(|n| n == b"Image")
        })
        .count()
}

// The Story of Doctor Dolittle (Project Gutenberg #501) - image-heavy:
// author's own illustrations throughout, one of the exact failure modes
// that caused the image-clipping bug (oversized images with no
// max-width).
#[test]
fn doctor_dolittle_image_heavy_book_converts_with_images_intact() {
    let doc = convert("doctor-dolittle", "doctor-dolittle.epub");
    let pages = doc.get_pages().len();
    assert!((10..=250).contains(&pages), "expected a plausible page count, got {pages}");

    let text = all_text(&doc);
    assert!(text.contains("Dolittle"), "expected the protagonist's name to survive text extraction");

    let images = image_xobject_count(&doc);
    assert!(images >= 10, "expected the book's illustrations to be embedded as image XObjects, found {images}");
}

// Pride and Prejudice (Project Gutenberg #1342) - a long, plain-text
// novel with many chapters, good coverage for the pagination-collapse
// bug class (broke on real multi-chapter markup, not the 2-chapter
// synthetic fixture) and for spine/merge ordering across many chapters.
#[test]
fn pride_and_prejudice_long_book_converts_in_full() {
    let doc = convert("pride-and-prejudice", "pride-and-prejudice.epub");
    let pages = doc.get_pages().len();
    assert!((200..=600).contains(&pages), "expected a plausible page count for a full novel, got {pages}");

    let text = all_text(&doc);
    assert!(
        text.contains("truth universally acknowledged"),
        "expected the novel's opening line to survive conversion"
    );
    assert!(text.contains("Darcy"), "expected a major character's name to appear");
    assert!(text.contains("Elizabeth"), "expected a major character's name to appear");
}

// On the Origin of Species (Project Gutenberg #2009) - has real footnotes
// (class="footnote" in the source), a glossary, and a large back-of-book
// index, exercising markup shapes the 2-chapter synthetic fixture never
// touches.
#[test]
fn origin_of_species_footnotes_and_index_survive_conversion() {
    let doc = convert("origin-of-species", "origin-of-species.epub");
    let pages = doc.get_pages().len();
    assert!((200..=900).contains(&pages), "expected a plausible page count, got {pages}");

    let text = all_text(&doc);
    assert!(text.contains("NATURAL SELECTION"), "expected a real chapter heading to survive");
    assert!(text.to_uppercase().contains("INDEX"), "expected the back-of-book index chapter to render");
    assert!(text.to_uppercase().contains("GLOSSARY"), "expected the glossary chapter to render");
}

// Scientific American Supplement, Feb 24 1877 (Project Gutenberg #19406)
// - a 19th-century magazine issue with real <table> markup (price lists,
// a table of contents with page numbers), a shape none of the other
// fixtures exercise.
#[test]
fn scientific_american_tables_render_as_readable_text() {
    let doc = convert("scientific-american-supplement", "scientific-american-supplement.epub");
    let pages = doc.get_pages().len();
    assert!((5..=250).contains(&pages), "expected a plausible page count, got {pages}");

    let text = all_text(&doc);
    assert!(text.contains("SCIENTIFIC AMERICAN"), "expected the masthead title to survive");
    assert!(
        text.contains("$3.20") || text.contains("$5.60"),
        "expected a price from one of the data tables to survive conversion"
    );
}

// All four real fixtures should exist and be non-trivial in size; catches
// an accidentally-corrupted or emptied fixture file before it silently
// starts producing meaningless "conversion succeeded" passes.
#[test]
fn real_book_fixtures_are_present_and_nonempty() {
    for name in ["doctor-dolittle.epub", "pride-and-prejudice.epub", "origin-of-species.epub", "scientific-american-supplement.epub"] {
        let path = fixtures_dir().join(name);
        let size = std::fs::metadata(&path).unwrap_or_else(|e| panic!("missing fixture {name}: {e}")).len();
        assert!(size > 50_000, "{name} is suspiciously small ({size} bytes) for a real book fixture");
    }
}
