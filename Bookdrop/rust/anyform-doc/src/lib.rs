mod epub;
mod ir;
mod kindle;
mod pdf;

pub use epub::EpubInput;
pub use kindle::KindleInput;
pub use ir::{DocumentIR, Metadata, Resource, SpineItem, TocNode};
pub use pdf::PdfOutput;

use std::sync::Arc;

use anyform_core::Registry;

/// The document-family registry — EPUB and Kindle-family (AZW3/KFX/MOBI)
/// input, PDF output (via bundled headless Chromium). Kindle formats are
/// normalized to EPUB by the bundled `boko` binary, so they reach every
/// output format the engine supports without a dedicated path each.
/// TXT/HTML/DOCX output plugins are Phase 6 follow-up per
/// `ANYFORM-FULL-SPEC.md` §6.
pub fn document_registry() -> Registry<DocumentIR> {
    let mut r = Registry::new();
    r.add_input(Arc::new(EpubInput));
    r.add_input(Arc::new(KindleInput));
    r.add_output(Arc::new(PdfOutput));
    r
}
