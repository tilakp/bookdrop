mod docx;
mod epub;
mod epub_output;
mod html;
mod htmltext;
mod ir;
mod kindle;
mod pdf;
mod txt;

pub use docx::DocxOutput;
pub use epub::EpubInput;
pub use epub_output::EpubOutput;
pub use html::HtmlOutput;
pub use kindle::KindleInput;
pub use ir::{DocumentIR, Metadata, Resource, SpineItem, TocNode};
pub use pdf::PdfOutput;
pub use txt::TxtOutput;

use std::sync::Arc;

use anyform_core::Registry;

/// The document-family registry — EPUB and Kindle-family (AZW3/KFX/MOBI)
/// input, PDF/EPUB output (via bundled headless Chromium for PDF; EPUB is a
/// faithful repackage of the IR, no external engine). Kindle formats are
/// normalized to EPUB by the bundled `boko` binary, so they reach every
/// output format the engine supports without a dedicated path each.
/// TXT/HTML/DOCX output plugins are Phase 6 follow-up per
/// `ANYFORM-FULL-SPEC.md` §6.
pub fn document_registry() -> Registry<DocumentIR> {
    let mut r = Registry::new();
    r.add_input(Arc::new(EpubInput));
    r.add_input(Arc::new(KindleInput));
    r.add_output(Arc::new(PdfOutput));
    r.add_output(Arc::new(EpubOutput));
    r.add_output(Arc::new(TxtOutput));
    r.add_output(Arc::new(HtmlOutput));
    r.add_output(Arc::new(DocxOutput));
    r
}
