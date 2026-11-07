//! The only module that calls pdfium. Everything else works on the plain
//! structs it returns, so `pdf_layout` can be tested without the dylib.
//!
//! Thread safety: `pdfium-render`'s `thread_safe` feature locks each FFI
//! call, but that is not enough. Four threads that open and read different
//! documents at the same time crash reliably. So all of `extract()` runs
//! under one process-wide lock (`EXTRACT_LOCK`). That costs little,
//! because the app converts one file at a time.
//! `concurrent_pdf_conversions_do_not_crash_pdfium` keeps this tested:
//! ordinary parallel tests, one PDF each, did not reproduce the crash.

use std::path::Path;
use std::sync::{Mutex, OnceLock};

use pdfium_render::prelude::*;

use anyform_core::{ConvError, Log, Options};

use crate::pdf_layout::{Glyph, OutlineEntry, PageText};
use crate::tools;

/// Bound exactly once for the lifetime of the process (`Pdfium::new` panics
/// on a second call), reused for every subsequent conversion.  Stores a
/// `String` rather than the original error so this can be `Clone`d out of
/// the `OnceLock` without requiring `PdfiumError`/`Pdfium` themselves to be
/// `Clone`.
static PDFIUM: OnceLock<Result<Pdfium, String>> = OnceLock::new();

/// Held for the entire body of `extract()` - see the module doc comment.
static EXTRACT_LOCK: Mutex<()> = Mutex::new(());

/// Binds the library on first use, with the `pdfium_path` in `opts` at that
/// time, and reuses it after that, also for calls with a different path.
/// The app always passes the same bundled path.
fn bindings(opts: &Options) -> Result<&'static Pdfium, ConvError> {
    // A missing library is reported without being cached, so a later call
    // that passes the right `pdfium_path` still succeeds.
    if PDFIUM.get().is_none() {
        let path = tools::PDFIUM.resolve(opts)?;
        PDFIUM.get_or_init(|| {
            let lib = Pdfium::bind_to_library(&path).map_err(|e| format!("failed to load the bundled PDF reader: {e}"))?;
            Ok(Pdfium::new(lib))
        });
    }
    PDFIUM.get().expect("set above").as_ref().map_err(|e| ConvError::Other(e.clone()))
}


pub(crate) struct DocumentProbe {
    pub page_count: usize,
    pub title: Option<String>,
    pub author: Option<String>,
    pub outline: Vec<OutlineEntry>,
    pub pages: Vec<PageText>,
}

/// Opens `path`, reads metadata/outline, and extracts every page's
/// positioned text into plain `PageText`/`Glyph` values. pdfium errors
/// become `ConvError`s here, so `pdf_input.rs` never sees `PdfiumError`.
pub(crate) fn extract(path: &Path, opts: &Options, log: &dyn Log) -> Result<DocumentProbe, ConvError> {
    // Poisoning would only happen if a prior call panicked while holding
    // the lock; recovering the guard anyway is correct here since pdfium's
    // own state (not ours) is what a panic mid-extraction would leave in
    // question, and the next call reopens the document from scratch.
    let _guard = EXTRACT_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

    let pdfium = bindings(opts)?;

    let doc = pdfium.load_pdf_from_file(path, None).map_err(map_open_error)?;

    let page_count = doc.pages().len() as usize;
    if page_count == 0 {
        return Err(ConvError::Malformed("PDF: no pages".into()));
    }

    let metadata = doc.metadata();
    let title = clean_metadata_string(metadata.get(PdfDocumentMetadataTagType::Title).map(|t| t.value().to_string()));
    let author = clean_metadata_string(metadata.get(PdfDocumentMetadataTagType::Author).map(|t| t.value().to_string()));
    let outline = extract_outline(&doc);

    let mut pages = Vec::with_capacity(page_count);
    for (index, page) in doc.pages().iter().enumerate() {
        if log.is_cancelled() {
            return Err(ConvError::Cancelled);
        }
        log.progress(0.20 * (index as f64 / page_count as f64), "Reading PDF");
        pages.push(extract_page(&page, index));
    }

    Ok(DocumentProbe { page_count, title, author, outline, pages })
}

fn clean_metadata_string(value: Option<String>) -> Option<String> {
    value.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn map_open_error(e: PdfiumError) -> ConvError {
    if matches!(e, PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError)) {
        return ConvError::PasswordProtected;
    }
    ConvError::Malformed(format!("PDF: {e}"))
}

fn extract_page(page: &PdfPage, index: usize) -> PageText {
    let width = page.width().value;
    let height = page.height().value;

    let mut glyphs = Vec::new();
    if let Ok(text) = page.text() {
        for ch in text.chars().iter() {
            let Some(c) = ch.unicode_char() else { continue };
            if c.is_control() {
                continue;
            }
            let Ok(bounds) = ch.tight_bounds() else { continue };
            let font_name = ch.font_name();
            let lower = font_name.to_lowercase();
            glyphs.push(Glyph {
                ch: c,
                x0: bounds.left().value,
                y0: bounds.bottom().value,
                x1: bounds.right().value,
                y1: bounds.top().value,
                font_size: ch.scaled_font_size().value,
                bold: ch.font_is_bold_reenforced() || lower.contains("bold"),
                italic: ch.font_is_italic() || lower.contains("italic") || lower.contains("oblique"),
                font_name,
            });
        }
    }

    let mut image_area = 0.0f32;
    for obj in page.objects().iter() {
        if obj.object_type() != PdfPageObjectType::Image {
            continue;
        }
        if let Ok(bounds) = obj.bounds() {
            let w = (bounds.right().value - bounds.left().value).abs();
            let h = (bounds.top().value - bounds.bottom().value).abs();
            image_area += w * h;
        }
    }
    let page_area = (width * height).max(1.0);
    let image_area_ratio = (image_area / page_area).clamp(0.0, 1.0);

    PageText { index, width, height, glyphs, image_area_ratio }
}

/// Walks the top-level bookmark chain (`root()` returns the *first*
/// top-level bookmark, not a synthetic invisible root - `next_sibling()`
/// from there visits the rest) and recurses into `iter_direct_children()`
/// for nested entries, preserving both document order and hierarchy for
/// `ir.toc`. `split_chapters` in `pdf_layout/chapters.rs` only uses the top level
/// for chapter boundaries; the full tree is kept for the nav/TOC.
fn extract_outline(doc: &PdfDocument) -> Vec<OutlineEntry> {
    let mut entries = Vec::new();
    let mut current = doc.bookmarks().root();
    while let Some(bm) = current {
        current = bm.next_sibling();
        entries.push(build_outline_entry(&bm));
    }
    entries
}

fn build_outline_entry(bookmark: &PdfBookmark) -> OutlineEntry {
    let title = bookmark.title().unwrap_or_default();
    let page_index = bookmark.destination().and_then(|d| d.page_index().ok()).map(|p| p as usize);
    let children = bookmark.iter_direct_children().map(|c| build_outline_entry(&c)).collect();
    OutlineEntry { title, page_index, children }
}
