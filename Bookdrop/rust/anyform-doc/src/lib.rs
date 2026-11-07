mod docx;
mod epub;
mod epub_output;
mod href;
mod html;
mod htmltext;
mod ir;
mod kindle;
mod markup;
mod options;
mod pdf;
mod pdf_input;
mod pdf_layout;
mod pdfium;
mod tools;
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

/// The document registry. Input: EPUB, Kindle (AZW3, AZW, KFX, MOBI) and
/// PDF. Output: PDF, EPUB, TXT, HTML and DOCX. The registry picks the input
/// and output plugins separately by extension, so every input reaches
/// every output. Kindle files become EPUB through the bundled `boko`, and
/// PDF input writes XHTML chapters, so both look like an EPUB to the
/// output plugins. Only PDF input (pdfium) and PDF output (Chromium) need
/// a bundled engine. The other formats are pure Rust.
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
