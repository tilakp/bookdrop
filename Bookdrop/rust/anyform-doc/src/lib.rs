mod docx;
mod epub;
mod epub_output;
mod html;
mod htmltext;
mod ir;
mod kindle;
mod pdf;
mod pdf_input;
mod pdf_layout;
mod pdfium;
mod txt;

pub use docx::DocxOutput;
pub use epub::EpubInput;
pub use epub_output::EpubOutput;
pub use html::HtmlOutput;
pub use kindle::KindleInput;
pub use ir::{DocumentIR, Metadata, Resource, SpineItem, TocNode};
pub use pdf::PdfOutput;
pub use pdf_input::PdfInput;
pub use txt::TxtOutput;

use std::sync::Arc;

use anyform_core::Registry;

/// The document-family registry — EPUB, Kindle-family (AZW3/KFX/MOBI), and
/// PDF input; PDF/EPUB/TXT/HTML/DOCX output. Kindle formats are normalized
/// to EPUB by the bundled `boko` binary, so they reach every output format
/// below without a dedicated path each; registering `PdfInput` similarly
/// makes `.pdf` sources reach every output format for free, since
/// `Registry::convert` dispatches input and output plugins independently
/// by extension. PDF input/output are the only formats needing an external
/// engine (bundled pdfium / headless Chromium respectively); the rest are
/// pure Rust — EPUB is a faithful repackage of the IR, TXT/DOCX share
/// `htmltext.rs`'s chapter-XHTML extractor, HTML emits markup directly.
/// `PdfInput` is a deliberately-scoped MVP (text-layer, single-column PDFs
/// only — see its own doc comment) that reconstructs reflowable chapter
/// structure via `pdf_layout.rs`'s heuristics.
pub fn document_registry() -> Registry<DocumentIR> {
    let mut r = Registry::new();
    r.add_input(Arc::new(EpubInput));
    r.add_input(Arc::new(KindleInput));
    r.add_input(Arc::new(PdfInput));
    r.add_output(Arc::new(PdfOutput));
    r.add_output(Arc::new(EpubOutput));
    r.add_output(Arc::new(TxtOutput));
    r.add_output(Arc::new(HtmlOutput));
    r.add_output(Arc::new(DocxOutput));
    r
}
