use std::collections::HashMap;
use std::path::PathBuf;

use serde::Serialize;

/// Kept shallow deliberately: a manifest of resources plus an ordered spine
/// of raw chapter XHTML, not a typed DOM tree (see ANYFORM-FULL-SPEC.md §2).
///
/// Every href in the IR (manifest, spine, TOC, cover) is canonical: a
/// percent-decoded path relative to `content_dir`, with no `.`/`..`
/// segments, made by `href::resolve`. So `content_dir.join(href)` always
/// stays inside the book, and plugins can compare hrefs as plain strings.
#[derive(Debug, Serialize)]
pub struct DocumentIR {
    pub metadata: Metadata,
    pub manifest: HashMap<String, Resource>,
    pub spine: Vec<SpineItem>,
    pub toc: Vec<TocNode>,
    pub file_size_bytes: u64,
    /// Directory (inside the extracted working copy) containing the OPF.
    /// Spine and manifest hrefs are relative to it. Not serialized to JSON:
    /// it is a local path that has meaning only in this process.
    #[serde(skip)]
    pub content_dir: PathBuf,
    /// Owns the temp directory that holds `content_dir`, if the input
    /// plugin made one. Dropping the IR deletes it.
    #[serde(skip)]
    pub work_dir: Option<anyform_core::TempDir>,
}

impl DocumentIR {
    /// The markup of one spine chapter. Every output plugin reads chapters
    /// through this, so all of them fail the same way: a missing chapter is
    /// an error, not a gap in the book. Bytes that are not UTF-8 become
    /// U+FFFD instead of failing the whole book.
    pub fn read_chapter(&self, item: &SpineItem) -> Result<String, anyform_core::ConvError> {
        let bytes = std::fs::read(self.content_dir.join(&item.href))
            .map_err(|_| anyform_core::ConvError::MissingFile(item.href.clone()))?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
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
    /// displays it, if any). `PdfOutput` uses it to detect and skip a
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
    /// Canonical like every IR href (see `DocumentIR`), plus an optional
    /// "#fragment". `None` when the source entry had no usable link.
    pub href: Option<String>,
    pub children: Vec<TocNode>,
}
