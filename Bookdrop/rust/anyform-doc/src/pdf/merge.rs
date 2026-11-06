//! Merges the per-chapter PDFs into one document: renumbers objects,
//! repairs cross-chapter links, and builds the outline from the TOC.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use anyform_core::ConvError;
use lopdf::{Bookmark, Dictionary, Object, ObjectId};

use crate::href;
use crate::ir::TocNode;

/// Merges N single-chapter PDFs (each produced by one `print_to_pdf` call)
/// into one document and, if `generate_toc`, builds an outline from `toc`,
/// mapping each TOC entry's href to the first page of the chapter chunk it
/// points at. Object-consolidation logic follows lopdf's own
/// `examples/merge.rs` reference implementation.
pub(super) fn merge_chapter_pdfs(
    chunks: Vec<(Option<String>, lopdf::Document)>,
    toc: &[TocNode],
    generate_toc: bool,
    content_dir: &Path,
    href_to_prefix: &HashMap<String, String>,
) -> Result<lopdf::Document, ConvError> {
    let mut max_id = 1u32;
    let mut documents_pages: BTreeMap<ObjectId, Object> = BTreeMap::new();
    let mut documents_objects: BTreeMap<ObjectId, Object> = BTreeMap::new();
    let mut document = lopdf::Document::with_version("1.5");
    let mut href_to_first_page: HashMap<String, ObjectId> = HashMap::new();
    // Consolidated from every chapter's own (already namespace-prefixed by
    // `namespace_chapter_dests`) Dests dictionary - see that function's
    // doc comment. Collected here, after renumbering, so each entry's
    // destination array already points at this chapter's final,
    // globally-unique page object id.
    let mut merged_dests = Dictionary::new();

    for (href, mut doc) in chunks {
        doc.renumber_objects_with(max_id);
        max_id = doc.max_id + 1;

        let mut first_page: Option<ObjectId> = None;
        for object_id in doc.get_pages().into_values() {
            if first_page.is_none() {
                first_page = Some(object_id);
            }
            if let Ok(obj) = doc.get_object(object_id) {
                documents_pages.insert(object_id, obj.to_owned());
            }
        }
        if let (Some(href), Some(page_id)) = (href, first_page) {
            href_to_first_page.insert(href, page_id);
        }

        if let Some(dests) = doc
            .objects
            .values()
            .find(|o| o.type_name().unwrap_or(b"") == b"Catalog")
            .and_then(|cat| cat.as_dict().ok())
            .and_then(|d| d.get(b"Dests").ok())
            .and_then(|o| o.as_reference().ok())
            .and_then(|id| doc.get_object(id).ok())
            .and_then(|o| o.as_dict().ok())
        {
            for (key, value) in dests.iter() {
                merged_dests.set(key.clone(), value.clone());
            }
        }

        documents_objects.extend(doc.objects);
    }

    let mut catalog_object: Option<(ObjectId, Object)> = None;
    let mut pages_object: Option<(ObjectId, Object)> = None;

    for (object_id, object) in documents_objects.into_iter() {
        match object.type_name().unwrap_or(b"") {
            b"Catalog" => {
                catalog_object = Some((catalog_object.map(|(id, _)| id).unwrap_or(object_id), object));
            }
            b"Pages" => {
                if let Ok(dictionary) = object.as_dict() {
                    let mut dictionary = dictionary.clone();
                    if let Some((_, ref old_object)) = pages_object {
                        if let Ok(old_dictionary) = old_object.as_dict() {
                            dictionary.extend(old_dictionary);
                        }
                    }
                    pages_object = Some((
                        pages_object.map(|(id, _)| id).unwrap_or(object_id),
                        Object::Dictionary(dictionary),
                    ));
                }
            }
            b"Page" => {}
            b"Outlines" => {}
            b"Outline" => {}
            _ => {
                document.objects.insert(object_id, object);
            }
        }
    }

    let (pages_id, pages_object) =
        pages_object.ok_or_else(|| ConvError::Other("rendered chapters produced no Pages root".into()))?;
    let (catalog_id, catalog_object) =
        catalog_object.ok_or_else(|| ConvError::Other("rendered chapters produced no Catalog root".into()))?;

    for (object_id, object) in documents_pages.iter() {
        if let Ok(dictionary) = object.as_dict() {
            let mut dictionary = dictionary.clone();
            dictionary.set("Parent", pages_id);
            document.objects.insert(*object_id, Object::Dictionary(dictionary));
        }
    }

    if let Ok(dictionary) = pages_object.as_dict() {
        let mut dictionary = dictionary.clone();
        dictionary.set("Count", documents_pages.len() as u32);
        dictionary.set(
            "Kids",
            documents_pages.keys().map(|id| Object::Reference(*id)).collect::<Vec<_>>(),
        );
        document.objects.insert(pages_id, Object::Dictionary(dictionary));
    }

    if let Ok(dictionary) = catalog_object.as_dict() {
        let mut dictionary = dictionary.clone();
        dictionary.set("Pages", pages_id);
        if generate_toc {
            dictionary.set("PageMode", "UseOutlines");
        }
        dictionary.remove(b"Outlines");
        document.objects.insert(catalog_id, Object::Dictionary(dictionary));
    }

    document.trailer.set("Root", catalog_id);
    document.max_id = document.objects.len() as u32;

    let dest_names: std::collections::HashSet<Vec<u8>> = merged_dests.iter().map(|(k, _)| k.clone()).collect();
    // Named destination -> the page it is on, for fragment-level bookmarks.
    let dest_pages: HashMap<Vec<u8>, ObjectId> = merged_dests
        .iter()
        .filter_map(|(name, dest)| {
            let page = dest.as_array().ok()?.first()?.as_reference().ok()?;
            Some((name.clone(), page))
        })
        .collect();
    if !merged_dests.is_empty() {
        let dests_id = document.add_object(Object::Dictionary(merged_dests));
        if let Ok(Object::Dictionary(dict)) = document.get_object_mut(catalog_id) {
            dict.set("Dests", Object::Reference(dests_id));
        }
    }
    fix_cross_chapter_links(&mut document, content_dir, href_to_prefix, &href_to_first_page, &dest_names);

    if generate_toc {
        // Bookmarks must be added before the final renumber below — lopdf's
        // renumber_objects_with() walks self.bookmark_table and remaps each
        // bookmark's target page id alongside every other object reference
        // (see processor.rs::renumber_bookmarks), so ids captured here stay
        // valid after compaction.
        let targets = BookmarkTargets { href_to_first_page: &href_to_first_page, href_to_prefix, dest_pages: &dest_pages };
        add_toc_bookmarks(&mut document, toc, None, &targets);
    }

    document.renumber_objects();
    document.adjust_zero_pages();
    if generate_toc {
        if let Some(outline_id) = document.build_outline() {
            if let Ok(Object::Dictionary(dict)) = document.get_object_mut(catalog_id) {
                dict.set("Outlines", Object::Reference(outline_id));
            }
        }
    }

    Ok(document)
}

/// A link from one chapter to a fragment in *another* chapter (a common
/// nonfiction pattern: cross-references, an index linking into chapters)
/// can't become a working same-document destination during rendering -
/// each chapter is printed as an independent single-file PDF, so Chrome
/// has no way to know the target exists at all. It falls back to a `/URI`
/// action pointing at the target chapter's original file path (resolved
/// against the temp render file's own location), which is dead once the
/// temp files are cleaned up. This repairs those: if the target chapter
/// was also rendered and the exact fragment survived into the merged
/// Dests dictionary (via `namespace_chapter_dests`), the link becomes an
/// exact jump to that anchor; otherwise, if the target chapter is known
/// at all, it falls back to a jump to the top of that chapter rather than
/// leaving a link that does nothing.
fn fix_cross_chapter_links(
    document: &mut lopdf::Document,
    content_dir: &Path,
    href_to_prefix: &HashMap<String, String>,
    href_to_first_page: &HashMap<String, ObjectId>,
    dest_names: &std::collections::HashSet<Vec<u8>>,
) {
    let path_to_href: HashMap<String, &String> =
        href_to_prefix.keys().map(|href| (format!("file://{}", content_dir.join(href).display()), href)).collect();
    if path_to_href.is_empty() {
        return;
    }

    for object in document.objects.values_mut() {
        let Ok(dict) = object.as_dict_mut() else { continue };
        if dict.get(b"Subtype").and_then(|o| o.as_name()).unwrap_or(b"") != b"Link" {
            continue;
        }
        let Ok(uri_bytes) = dict.get(b"A").and_then(|a| a.as_dict()).and_then(|a| a.get(b"URI")).and_then(|u| u.as_str())
        else {
            continue;
        };
        let uri = String::from_utf8_lossy(uri_bytes).into_owned();
        let (encoded_path, fragment) = match uri.split_once('#') {
            Some((p, f)) => (p, Some(f)),
            None => (uri.as_str(), None),
        };
        // Chrome percent-encodes the URI; `path_to_href` keys are plain paths.
        let Some(path) = href::percent_decode(encoded_path) else { continue };
        let Some(href) = path_to_href.get(&path) else { continue };
        let prefix = &href_to_prefix[*href];

        let exact = fragment
            .map(|f| format!("{prefix}::{f}"))
            .filter(|name| dest_names.contains(name.as_bytes()))
            .map(|name| Object::Name(name.into_bytes()));
        let fallback = || {
            href_to_first_page
                .get(*href)
                .map(|page_id| Object::Array(vec![Object::Reference(*page_id), Object::Name(b"Fit".to_vec())]))
        };

        if let Some(dest) = exact.or_else(fallback) {
            dict.remove(b"A");
            dict.set("Dest", dest);
        }
    }
}

struct BookmarkTargets<'a> {
    href_to_first_page: &'a HashMap<String, ObjectId>,
    href_to_prefix: &'a HashMap<String, String>,
    dest_pages: &'a HashMap<Vec<u8>, ObjectId>,
}

impl BookmarkTargets<'_> {
    /// The page of the fragment's named destination if Chrome emitted one,
    /// else the first page of the chapter. `None` if the chapter was not
    /// rendered (for example a non-linear spine item).
    fn page_for(&self, href: &str) -> Option<ObjectId> {
        let (path, fragment) = href.split_once('#').map_or((href, None), |(p, f)| (p, Some(f)));
        let fragment_page = fragment.zip(self.href_to_prefix.get(path)).and_then(|(f, prefix)| {
            self.dest_pages.get(format!("{prefix}::{f}").as_bytes()).copied()
        });
        fragment_page.or_else(|| self.href_to_first_page.get(path).copied())
    }
}

fn add_toc_bookmarks(doc: &mut lopdf::Document, nodes: &[TocNode], parent: Option<u32>, targets: &BookmarkTargets) {
    for node in nodes {
        let target = node.href.as_deref().and_then(|h| targets.page_for(h));
        // An entry with no rendered target keeps its place in the outline
        // (so its children stay grouped under it) and opens the first page.
        let page = target.or_else(|| targets.href_to_first_page.values().min().copied()).unwrap_or((0, 0));
        let bookmark_id = doc.add_bookmark(Bookmark::new(node.title.clone(), [0.0, 0.0, 0.0], 0, page), parent);
        add_toc_bookmarks(doc, &node.children, Some(bookmark_id), targets);
    }
}
