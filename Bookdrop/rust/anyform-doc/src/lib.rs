mod epub;
mod ir;
mod pdf;

pub use epub::EpubInput;
pub use ir::{DocumentIR, Metadata, Resource, SpineItem, TocNode};
pub use pdf::PdfOutput;

use std::sync::Arc;

use anyform_core::Registry;

/// The document-family registry — EPUB input, PDF output (via bundled
/// headless Chromium). TXT/HTML/DOCX output plugins are Phase 6 follow-up
/// per `ANYFORM-FULL-SPEC.md` §6.
pub fn document_registry() -> Registry<DocumentIR> {
    let mut r = Registry::new();
    r.add_input(Arc::new(EpubInput));
    r.add_output(Arc::new(PdfOutput));
    r
}
