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

// Origin of Species has real footnote links within a chapter
// (<a href="thisfile.html#fn1">) and cross-reference links into other
// chapters (an index page linking into specific chapters) - both were
// silently dead after conversion: Chrome resolves a fragment link as
// same-document navigation only when the href's filename exactly matches
// the URL of the page it's loaded from, and every chapter used to render
// from a renamed temp file, so *no* internal link (same-chapter or
// cross-chapter) ever matched. Verifies no dead file:// links survive,
// and that at least one link became a real named destination pointing at
// an actual page in the merged document (not just a chapter-top
// fallback).
#[test]
fn origin_of_species_internal_links_do_not_go_dead() {
    let doc = convert("links", "origin-of-species.epub");
    let page_ids: std::collections::HashSet<lopdf::ObjectId> = doc.get_pages().into_values().collect();

    let mut dead_file_links = 0usize;
    let mut named_dest_targets: Vec<Vec<u8>> = Vec::new();
    for object in doc.objects.values() {
        let Ok(dict) = object.as_dict() else { continue };
        if dict.get(b"Subtype").and_then(|o| o.as_name()).unwrap_or(b"") != b"Link" {
            continue;
        }
        if let Ok(uri) = dict.get(b"A").and_then(|a| a.as_dict()).and_then(|a| a.get(b"URI")).and_then(|u| u.as_str()) {
            if uri.starts_with(b"file://") {
                dead_file_links += 1;
            }
        }
        if let Ok(name) = dict.get(b"Dest").and_then(|d| d.as_name()) {
            named_dest_targets.push(name.to_vec());
        }
    }

    assert_eq!(dead_file_links, 0, "no internal link should still point at a deleted temp render file");
    assert!(!named_dest_targets.is_empty(), "expected at least one exact-anchor named destination (a same-chapter footnote link)");

    // Every named destination should resolve, through the merged Dests
    // dictionary, to a page that actually exists in this document - not a
    // dangling reference into a chapter's now-discarded local object
    // table.
    let dests = doc.catalog().unwrap().get(b"Dests").and_then(|d| d.as_reference()).and_then(|id| doc.get_object(id)).and_then(|o| o.as_dict()).expect("catalog should reference a merged Dests dictionary");
    for name in &named_dest_targets {
        let target = dests.get(name).unwrap_or_else(|_| panic!("Dests dict missing entry for {name:?}"));
        let array = target.as_array().expect("a destination should be an array [page /Fit ...]");
        let page_ref = array.first().and_then(|o| o.as_reference().ok()).expect("destination array's first element should be a page reference");
        assert!(page_ids.contains(&page_ref), "named destination {name:?} points at a page not present in the merged document");
    }
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

#[test]
fn origin_of_species_chapters_stay_in_spine_order() {
    // Parallel rendering must not scramble chapter order in the merged
    // PDF even though workers finish out of order - "Chapter I." should
    // appear well before "INDEX" in extracted page order.
    let doc = convert("order-check", "origin-of-species.epub");
    let pages = doc.get_pages();
    let mut page_numbers: Vec<u32> = pages.keys().copied().collect();
    page_numbers.sort();
    let mut chapter_one_page = None;
    let mut index_page = None;
    for n in &page_numbers {
        let text = doc.extract_text(&[*n]).unwrap_or_default();
        if chapter_one_page.is_none() && text.contains("VARIATION UNDER DOMESTICATION") {
            chapter_one_page = Some(*n);
        }
        if text.to_uppercase().contains("INDEX") && *n > page_numbers[page_numbers.len() / 2] {
            index_page = Some(*n);
            break;
        }
    }
    let chapter_one_page = chapter_one_page.expect("Chapter I heading should appear somewhere");
    let index_page = index_page.expect("INDEX should appear in the back half of the book");
    assert!(chapter_one_page < index_page, "Chapter I (page {chapter_one_page}) should come before the index (page {index_page})");
}
