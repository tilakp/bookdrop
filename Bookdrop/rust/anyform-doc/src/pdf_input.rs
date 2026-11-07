//! Reads PDFs: `pdfium.rs` extracts positioned text, `pdf_layout`
//! rebuilds chapters, and this plugin writes them as XHTML files, so the IR
//! looks like an extracted EPUB. Every output format then works for PDF
//! input with no extra code.
//!
//! The scope is deliberate: text-layer, single-column PDFs. Scanned and
//! multi-column PDFs are detected and refused with a clear error, instead
//! of producing empty or scrambled text. No OCR, no table extraction. See
//! "PDF as an input format" in `ANYFORM-FULL-SPEC.md` §6.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyform_core::{ConvError, InputPlugin, Log, Options};

use crate::ir::{DocumentIR, Metadata, Resource, SpineItem, TocNode};
use crate::options;
use crate::pdf_layout;
use crate::pdfium::{self, DocumentProbe};

/// Below this average characters-per-page, combined with a high image-area
/// ratio, a PDF is treated as scanned/image-only rather than text-layer.
/// The ratio threshold is deliberately well under "half the page": a real
/// scanned/photographed page is typically edge-to-edge (image area near
/// 90-100%), but a full-bleed image rendered through `PdfOutput`'s own
/// margins (used to generate the `no-text-layer.pdf` test fixture) only
/// covers the content box inside those margins - measured live at ~45% for
/// default margins - so a literal 50%+ threshold would miss that fixture
/// and, by the same logic, any real scan with unusually wide margins.
const SCANNED_CHARS_PER_PAGE_THRESHOLD: usize = 100;
const SCANNED_IMAGE_RATIO_THRESHOLD: f32 = 0.3;

/// `PdfInput` reports progress in `0.0..=0.30` and leaves the rest of the
/// bar to the output plugin, like `KindleInput`, so the bar does not run to
/// 100% and jump back on a long PDF.
const PROGRESS_EXTRACT_END: f64 = 0.20;
const PROGRESS_DONE: f64 = 0.30;

pub struct PdfInput;

impl InputPlugin<DocumentIR> for PdfInput {
    fn name(&self) -> &'static str {
        "pdf"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["pdf"]
    }

    fn options(&self) -> &'static [&'static str] {
        &[options::PDFIUM_PATH, options::PDF_FORCE_SINGLE_COLUMN]
    }

    fn convert(&self, input: &Path, opts: &Options, log: &dyn Log) -> Result<DocumentIR, ConvError> {
        if log.is_cancelled() {
            return Err(ConvError::Cancelled);
        }
        log.progress(0.0, "Reading PDF");

        let probe = pdfium::extract(input, opts, log)?;
        reject_if_scanned(&probe)?;

        log.progress(PROGRESS_EXTRACT_END, "Analyzing layout");
        let force_single_column = opts.get_bool(options::PDF_FORCE_SINGLE_COLUMN, false);
        let title = resolve_title(&probe, input);

        let chapters = pdf_layout::build_chapters(&probe.pages, &probe.outline, &title, force_single_column, log)
            .map_err(|_| ConvError::MultiColumnPdf)?;

        if chapters.is_empty() || chapters.iter().all(|c| c.blocks.is_empty()) {
            return Err(ConvError::Other("This PDF's text couldn't be read in a sensible order.".into()));
        }

        log.progress(0.28, "Building chapters");
        let work_dir = anyform_core::fresh_work_dir("pdf-")?;

        let mut manifest = HashMap::new();
        let mut spine = Vec::with_capacity(chapters.len());
        let mut toc = Vec::with_capacity(chapters.len());

        for (i, chapter) in chapters.iter().enumerate() {
            if log.is_cancelled() {
                return Err(ConvError::Cancelled);
            }
            let id = format!("ch{:03}", i + 1);
            let href = format!("chapter-{:03}.xhtml", i + 1);
            let xhtml = pdf_layout::render_chapter_xhtml(chapter);
            std::fs::write(work_dir.path().join(&href), xhtml)?;

            manifest.insert(
                id.clone(),
                Resource { id: id.clone(), href: href.clone(), media_type: "application/xhtml+xml".into(), properties: HashSet::new() },
            );
            spine.push(SpineItem { id, href: href.clone(), media_type: "application/xhtml+xml".into() });
            toc.push(TocNode { title: chapter.title.clone(), href: Some(href), children: Vec::new() });
        }

        log.progress(PROGRESS_DONE, "Building chapters");

        let file_size_bytes = std::fs::metadata(input).map(|m| m.len()).unwrap_or(0);

        Ok(DocumentIR {
            metadata: Metadata { title, author: probe.author.clone(), language: None, cover: None, cover_href: None },
            manifest,
            spine,
            toc,
            file_size_bytes,
            content_dir: work_dir.path().to_path_buf(),
            work_dir: Some(work_dir),
        })
    }
}

fn reject_if_scanned(probe: &DocumentProbe) -> Result<(), ConvError> {
    let total_chars: usize = probe.pages.iter().map(|p| p.glyphs.len()).sum();
    if total_chars == 0 {
        return Err(ConvError::ScannedPdf);
    }
    let mean_image_ratio = probe.pages.iter().map(|p| p.image_area_ratio).sum::<f32>() / probe.page_count.max(1) as f32;
    let chars_per_page = total_chars / probe.page_count.max(1);
    if chars_per_page < SCANNED_CHARS_PER_PAGE_THRESHOLD && mean_image_ratio > SCANNED_IMAGE_RATIO_THRESHOLD {
        return Err(ConvError::ScannedPdf);
    }
    Ok(())
}


/// PDF `Info` dictionary titles are notoriously unreliable (often empty, a
/// bare filename, or a producer-app default like "Microsoft Word - foo").
/// Falls back to the largest-font text on page 1 (almost always the real
/// title on a title page), then the source filename.
fn resolve_title(probe: &DocumentProbe, input: &Path) -> String {
    if let Some(t) = &probe.title {
        if !looks_like_junk_title(t) {
            return t.clone();
        }
    }

    if let Some(page0) = probe.pages.first() {
        let lines = pdf_layout::group_lines(page0);
        if let Some(biggest) = lines.iter().max_by(|a, b| a.size.partial_cmp(&b.size).unwrap_or(std::cmp::Ordering::Equal)) {
            let text = biggest.text.trim();
            if !text.is_empty() {
                return text.to_string();
            }
        }
    }

    input.file_stem().and_then(|s| s.to_str()).unwrap_or("Untitled").to_string()
}

fn looks_like_junk_title(title: &str) -> bool {
    let lower = title.trim().to_lowercase();
    if lower.is_empty() || lower == "untitled" {
        return true;
    }
    if lower.starts_with("microsoft word") || lower.starts_with("microsoft powerpoint") {
        return true;
    }
    lower.ends_with(".pdf") || lower.ends_with(".doc") || lower.ends_with(".docx")
}

