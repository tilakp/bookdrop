use std::collections::HashMap;
use std::path::PathBuf;

use serde::Serialize;

/// Kept shallow deliberately: a manifest of resources plus an ordered spine
/// of *raw normalized HTML*, not a fully typed DOM tree — see
/// ANYFORM-FULL-SPEC.md §2. Field names/shapes deliberately mirror
/// Bookdrop's Swift `Book`/`SpineItem`/`ManifestItem`/`TocNode`
/// (`Sources/Bookdrop/Models/Book.swift`) so a JSON dump of this struct is a
/// drop-in replacement for what `EpubParser.parse` currently returns.
#[derive(Debug, Serialize)]
pub struct DocumentIR {
    pub metadata: Metadata,
    pub manifest: HashMap<String, Resource>,
    pub spine: Vec<SpineItem>,
    pub toc: Vec<TocNode>,
    pub file_size_bytes: u64,
    /// Directory (inside the extracted working copy) containing the OPF —
    /// spine/manifest hrefs are relative to this. Not serialized to JSON —
    /// it's a local filesystem path meaningful only within this process.
    #[serde(skip)]
    pub content_dir: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct Metadata {
    pub title: String,
    pub author: Option<String>,
    /// EPUB3 requires `<dc:language>` on every package; `EpubOutput` needs
    /// a value to emit even when the source didn't have one (falls back to
    /// `"en"`).
    pub language: Option<String>,
    #[serde(skip)]
    pub cover: Option<Vec<u8>>,
    /// Manifest href of the cover *image* resource (not the spine page that
    /// displays it, if any) — used by `PdfOutput` to detect and skip a
    /// dedicated cover spine page so the cover isn't rendered twice.
    #[serde(skip)]
    pub cover_href: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Resource {
    pub id: String,
    pub href: String,
    pub media_type: String,
    pub properties: std::collections::HashSet<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SpineItem {
    pub id: String,
    pub href: String,
    pub media_type: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TocNode {
    pub title: String,
    /// Relative to the TOC document's own directory (the EPUB3 nav
    /// document, or the EPUB2 `toc.ncx`) - *not* `content_dir` - since
    /// `epub.rs`'s parser stores the raw href/src attribute verbatim and
    /// only uses `content_dir` to locate the nav/ncx file itself. May
    /// include a "#fragment". `EpubOutput` must regenerate the nav/ncx at
    /// the same relative directory as the source or every link here
    /// breaks - see `nested-nav.epub` in the test fixtures.
    pub href: Option<String>,
    pub children: Vec<TocNode>,
}
