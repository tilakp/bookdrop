//! The only module in this crate that touches the pdfium FFI. Everything
//! else (`pdf_layout.rs`'s heuristics, `pdf_input.rs`'s orchestration) works
//! on the plain owned structs this module produces - see the PDF-input
//! plan's §2 for why that split exists (testability: `pdf_layout.rs` needs
//! no bundled dylib to unit-test).
//!
//! Binding and path resolution mirror `resolve_boko_path` in `kindle.rs`
//! and `resolve_chromium_path` in `pdf.rs` exactly: an explicit `opts`
//! value wins, then an env var, then the vendored dev-tree copy so `cargo
//! test`/`anyform-cli` work without going through `build-app.sh`.
//!
//! Thread safety: `pdfium-render`'s `thread_safe` feature (enabled in
//! `Cargo.toml`) claims to serialize every FFI call behind its own internal
//! mutex - see that crate's README "Multi-threading" section - but this was
//! verified empirically insufficient: a test that opens/extracts from
//! separate `PdfDocument`s on four real concurrent threads reliably
//! segfaults/traps with only that feature relied on. Whatever the internal
//! mutex actually covers, it does not make the coarser "open a document,
//! extract every page, drop the document" sequence safe against another
//! thread doing the same for a *different* document at the same time -
//! plausibly a `Drop`-ordering or shared-global-state issue inside pdfium
//! itself that per-call locking doesn't reach. `EXTRACT_LOCK` below fixes
//! this the blunt way: the entire `extract()` body runs under one
//! process-wide lock, so pdfium genuinely never sees concurrent work,
//! regardless of what pdfium-render's own locking grants. Cheap in
//! practice - the app converts one file at a time anyway
//! (`MultiConversionModel.run` is a plain sequential loop) - and
//! `concurrent_pdf_conversions_do_not_crash_pdfium` in
//! `pdf_input_tests.rs` exists specifically to keep this regression-tested,
//! since `cargo test` alone (parallel *tests*, but each usually a single
//! PDF) did not reliably reproduce the crash - only genuinely concurrent
//! multi-thread use inside one test did.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use pdfium_render::prelude::*;

use anyform_core::{ConvError, Log, Options};

use crate::pdf_layout::{Glyph, OutlineEntry, PageText};

/// Bound exactly once for the lifetime of the process (`Pdfium::new` panics
/// on a second call), reused for every subsequent conversion.  Stores a
/// `String` rather than the original error so this can be `Clone`d out of
/// the `OnceLock` without requiring `PdfiumError`/`Pdfium` themselves to be
/// `Clone`.
static PDFIUM: OnceLock<Result<Pdfium, String>> = OnceLock::new();

/// Held for the entire body of `extract()` - see the module doc comment.
static EXTRACT_LOCK: Mutex<()> = Mutex::new(());

/// Binds the library on first use (honoring a caller-supplied `pdfium_path`
/// in `opts` at that point) and reuses the same instance thereafter - once
/// bound, later calls with a different `opts` still reuse it, matching how
/// `resolve_boko_path` is also only meaningfully consulted once per process
/// in practice (the app always passes the same bundled path).
fn bindings(opts: &Options) -> Result<&'static Pdfium, ConvError> {
    let result = PDFIUM.get_or_init(|| {
        let path = resolve_pdfium_path(opts)?;
        let lib = Pdfium::bind_to_library(&path).map_err(|e| format!("failed to load the bundled PDF reader: {e}"))?;
        Ok(Pdfium::new(lib))
    });
    result.as_ref().map_err(|e| ConvError::Other(e.clone()))
}

fn resolve_pdfium_path(opts: &Options) -> Result<PathBuf, String> {
    if let Some(p) = opts.get_str("pdfium_path") {
        let path = PathBuf::from(p);
        if path.exists() {
            return Ok(path);
        }
    }
    if let Ok(p) = std::env::var("ANYFORM_PDFIUM_PATH") {
        let path = PathBuf::from(p);
        if path.exists() {
            return Ok(path);
        }
    }
    let platform = if cfg!(target_arch = "aarch64") { "mac-arm64" } else { "mac-x64" };
    let dev_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../vendor/pdfium")
        .join(platform)
        .join("libpdfium.dylib");
    if dev_path.exists() {
        return Ok(dev_path);
    }

    Err("no bundled PDF reader found — set the \"pdfium_path\" option, \
         ANYFORM_PDFIUM_PATH, or run scripts/fetch-pdfium.sh"
        .into())
}

pub(crate) struct DocumentProbe {
    pub page_count: usize,
    pub title: Option<String>,
    pub author: Option<String>,
    pub outline: Vec<OutlineEntry>,
    pub pages: Vec<PageText>,
}

/// Opens `path`, reads metadata/outline, and extracts every page's
/// positioned text into plain `PageText`/`Glyph` values. Maps every pdfium
/// failure mode to the exact §4 error table in the PDF-input plan at this
/// boundary, so `pdf_input.rs` never has to match on `PdfiumError` itself.
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
        return ConvError::Other(
            "This PDF is password-protected, so it can't be converted. \
             Open it with the password, save an unprotected copy, and convert that."
                .into(),
        );
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
/// `ir.toc`. `split_chapters` in `pdf_layout.rs` only uses the top level
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
