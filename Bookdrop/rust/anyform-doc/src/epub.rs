use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyform_core::{ConvError, InputPlugin, Log, Options};

use crate::ir::{DocumentIR, Metadata, Resource, SpineItem, TocNode};

/// Parsed contents of the EPUB's .opf package document — mirrors Bookdrop's
/// Swift `OPFDocument` (`Sources/Bookdrop/Services/EpubXML.swift`).
struct OpfDocument {
    title: Option<String>,
    author: Option<String>,
    manifest: HashMap<String, Resource>,
    spine_idrefs: Vec<String>,
    toc_ncx_id: Option<String>,
    cover_meta_content_id: Option<String>,
}

impl OpfDocument {
    fn cover_href(&self) -> Option<String> {
        if let Some(item) = self
            .manifest
            .values()
            .find(|r| r.properties.contains("cover-image"))
        {
            return Some(item.href.clone());
        }
        self.cover_meta_content_id
            .as_ref()
            .and_then(|id| self.manifest.get(id))
            .map(|item| item.href.clone())
    }
}

pub struct EpubInput;

impl InputPlugin<DocumentIR> for EpubInput {
    fn name(&self) -> &'static str {
        "epub"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["epub", "kepub"]
    }

    fn convert(&self, input: &Path, _opts: &Options, _log: &dyn Log) -> Result<DocumentIR, ConvError> {
        let file_size_bytes = std::fs::metadata(input).map(|m| m.len()).unwrap_or(0);

        let work_dir = extract_epub(input)?;

        let container_path = work_dir.join("META-INF/container.xml");
        if !container_path.exists() {
            return Err(ConvError::MissingFile("META-INF/container.xml".into()));
        }
        let opf_relative = parse_container(&container_path)?;
        let opf_path = work_dir.join(&opf_relative);
        if !opf_path.exists() {
            return Err(ConvError::MissingFile(opf_relative));
        }
        let content_dir = opf_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| work_dir.clone());

        let opf = parse_opf(&opf_path)?;
        let toc = parse_toc(&opf, &content_dir);
        let cover_href = opf.cover_href();
        let cover = cover_href
            .as_ref()
            .and_then(|href| std::fs::read(content_dir.join(href)).ok());

        let spine: Vec<SpineItem> = opf
            .spine_idrefs
            .iter()
            .filter_map(|idref| opf.manifest.get(idref))
            .map(|item| SpineItem {
                id: item.id.clone(),
                href: item.href.clone(),
                media_type: item.media_type.clone(),
            })
            .collect();

        let title = opf.title.clone().unwrap_or_else(|| {
            input
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Untitled")
                .to_string()
        });

        Ok(DocumentIR {
            metadata: Metadata {
                title,
                author: opf.author.clone(),
                cover,
                cover_href,
            },
            manifest: opf.manifest,
            spine,
            toc,
            file_size_bytes,
            content_dir,
        })
    }
}

/// Extracts the EPUB into a fresh temp directory, mirroring Bookdrop's
/// Swift `EpubParser.parse` — the returned directory stays on disk; callers
/// own cleanup for as long as they need spine/manifest files.
fn extract_epub(input: &Path) -> Result<PathBuf, ConvError> {
    let file = std::fs::File::open(input)?;
    let mut archive = zip::ZipArchive::new(file).map_err(|_| ConvError::InvalidArchive)?;

    let work_dir = anyform_core::fresh_work_dir("Bookdrop")?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|_| ConvError::InvalidArchive)?;
        let Some(relative) = entry.enclosed_name().map(Path::to_path_buf) else {
            continue;
        };
        let dest = work_dir.join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&dest)?;
            continue;
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&dest)?;
        std::io::copy(&mut entry, &mut out)?;
    }

    Ok(work_dir)
}

fn parse_container(container_path: &Path) -> Result<String, ConvError> {
    let xml = std::fs::read_to_string(container_path)
        .map_err(|_| ConvError::MissingFile("META-INF/container.xml".into()))?;
    let doc = roxmltree::Document::parse(&xml)
        .map_err(|e| ConvError::Malformed(format!("container.xml: {e}")))?;
    doc.descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "rootfile")
        .and_then(|n| n.attribute("full-path"))
        .map(String::from)
        .ok_or_else(|| ConvError::Malformed("container.xml has no rootfile".into()))
}

fn parse_opf(opf_path: &Path) -> Result<OpfDocument, ConvError> {
    let xml = std::fs::read_to_string(opf_path)
        .map_err(|_| ConvError::MissingFile(opf_path.display().to_string()))?;
    let doc =
        roxmltree::Document::parse(&xml).map_err(|e| ConvError::Malformed(format!("OPF: {e}")))?;

    let mut result = OpfDocument {
        title: None,
        author: None,
        manifest: HashMap::new(),
        spine_idrefs: Vec::new(),
        toc_ncx_id: None,
        cover_meta_content_id: None,
    };

    if let Some(metadata_el) = doc
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "metadata")
    {
        for child in metadata_el.descendants().filter(|n| n.is_element()) {
            let local = child.tag_name().name();
            if local == "title" && result.title.is_none() {
                let text = element_text(child);
                if !text.is_empty() {
                    result.title = Some(text);
                }
            } else if local == "creator" && result.author.is_none() {
                let text = element_text(child);
                if !text.is_empty() {
                    result.author = Some(text);
                }
            }
        }
    }

    for item in doc
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "item")
    {
        let (Some(id), Some(href)) = (item.attribute("id"), item.attribute("href")) else {
            continue;
        };
        let media_type = item.attribute("media-type").unwrap_or("").to_string();
        let properties: HashSet<String> = item
            .attribute("properties")
            .unwrap_or("")
            .split_whitespace()
            .map(String::from)
            .collect();
        result.manifest.insert(
            id.to_string(),
            Resource {
                id: id.to_string(),
                href: href.to_string(),
                media_type,
                properties,
            },
        );
    }

    for itemref in doc
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "itemref")
    {
        if let Some(idref) = itemref.attribute("idref") {
            let linear = itemref.attribute("linear").unwrap_or("yes");
            if linear != "no" {
                result.spine_idrefs.push(idref.to_string());
            }
        }
    }

    if let Some(spine_el) = doc
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "spine")
    {
        result.toc_ncx_id = spine_el.attribute("toc").map(String::from);
    }

    for meta in doc
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "meta")
    {
        if meta.attribute("name") == Some("cover") {
            if let Some(content) = meta.attribute("content") {
                result.cover_meta_content_id = Some(content.to_string());
            }
        }
    }

    Ok(result)
}

fn parse_toc(opf: &OpfDocument, content_dir: &Path) -> Vec<TocNode> {
    if let Some(nav_item) = opf.manifest.values().find(|r| r.properties.contains("nav")) {
        let nav_path = content_dir.join(&nav_item.href);
        if let Some(nodes) = parse_nav_toc(&nav_path) {
            if !nodes.is_empty() {
                return nodes;
            }
        }
    }
    if let Some(ncx_id) = &opf.toc_ncx_id {
        if let Some(ncx_item) = opf.manifest.get(ncx_id) {
            let ncx_path = content_dir.join(&ncx_item.href);
            if let Some(nodes) = parse_ncx_toc(&ncx_path) {
                return nodes;
            }
        }
    }
    Vec::new()
}

/// EPUB3 `<nav epub:type="toc">` — the `<ol><li><a>` tree it contains.
fn parse_nav_toc(path: &Path) -> Option<Vec<TocNode>> {
    let xml = std::fs::read_to_string(path).ok()?;
    let doc = roxmltree::Document::parse(&xml).ok()?;
    let nav = doc.descendants().find(|n| {
        n.is_element()
            && n.tag_name().name() == "nav"
            && n.attributes().any(|a| a.name() == "type" && a.value() == "toc")
    })?;
    let ol = nav
        .children()
        .chain(nav.descendants())
        .find(|n| n.is_element() && n.tag_name().name() == "ol")?;
    Some(parse_ol(ol))
}

fn parse_ol(ol: roxmltree::Node) -> Vec<TocNode> {
    ol.children()
        .filter(|n| n.is_element() && n.tag_name().name() == "li")
        .filter_map(|li| {
            let a = li
                .children()
                .chain(li.descendants())
                .find(|n| n.is_element() && n.tag_name().name() == "a")?;
            let title = element_text(a);
            let href = a.attribute("href").map(String::from);
            let children = li
                .children()
                .find(|n| n.is_element() && n.tag_name().name() == "ol")
                .map(parse_ol)
                .unwrap_or_default();
            Some(TocNode { title, href, children })
        })
        .collect()
}

/// EPUB2 `toc.ncx` — the `<navMap>` document.
fn parse_ncx_toc(path: &Path) -> Option<Vec<TocNode>> {
    let xml = std::fs::read_to_string(path).ok()?;
    let doc = roxmltree::Document::parse(&xml).ok()?;
    let nav_map = doc
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "navMap")?;
    Some(parse_nav_points(nav_map))
}

fn parse_nav_points(parent: roxmltree::Node) -> Vec<TocNode> {
    parent
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "navPoint")
        .map(|np| {
            let title = np
                .children()
                .find(|n| n.is_element() && n.tag_name().name() == "navLabel")
                .and_then(|label| {
                    label
                        .children()
                        .find(|n| n.is_element() && n.tag_name().name() == "text")
                })
                .map(element_text)
                .unwrap_or_default();
            let href = np
                .children()
                .find(|n| n.is_element() && n.tag_name().name() == "content")
                .and_then(|c| c.attribute("src"))
                .map(String::from);
            let children = parse_nav_points(np);
            TocNode { title, href, children }
        })
        .collect()
}

fn element_text(node: roxmltree::Node) -> String {
    node.descendants()
        .filter(|n| n.is_text())
        .filter_map(|n| n.text())
        .collect::<String>()
        .trim()
        .to_string()
}
