use std::io::Write;
use std::path::Path;

use anyform_core::{ConvError, Log, Options, OutputPlugin};
use sha1::{Digest, Sha1};
use zip::write::FileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::ir::{DocumentIR, Resource, TocNode};

/// Repackages a `DocumentIR` back into a valid EPUB3 file — a faithful
/// repackage, not a rebuild: every manifest resource is copied byte-for-byte
/// from `ir.content_dir`, and only `mimetype`/`META-INF/container.xml`/the
/// OPF/the nav document/`toc.ncx` are regenerated. This maximizes fidelity
/// (fonts/images/CSS/chapter markup all survive untouched) and makes a
/// round-trip test (`EpubOutput` → `EpubInput` again) a meaningful
/// regression net for the whole IR, not just this plugin.
///
/// Ignores `include_cover`/`generate_table_of_contents` entirely (see
/// `convert`'s doc comment) - neither concept transfers to a container
/// format the way it does to a rendered PDF/HTML page.
///
/// Two things a round trip through this plugin cannot preserve, because
/// `DocumentIR` doesn't carry them: EPUB spine items with `linear="no"`
/// (dropped by `EpubInput`, indistinguishable from a normal item once
/// parsed), and the source book's own `dc:identifier` (not exposed on the
/// IR at all - a fresh deterministic one is minted instead, see
/// `deterministic_identifier`).
///
/// Verified against `epubcheck` (manually, not gated in CI): the package
/// document/nav/ncx this plugin generates are fully EPUB3-valid on their
/// own (a `minimal.epub` round trip produces zero epubcheck errors). A
/// real, EPUB2-authored book (`origin-of-species.epub`, declares
/// `version="2.0"`) round-trips with 59 epubcheck errors - all in chapter
/// XHTML copied byte-for-byte per this plugin's "faithful repackage, not
/// rebuild" design (D7 in the implementation plan), none in generated
/// files. Root cause: this plugin always declares `version="3.0"` (needed
/// for the nav document EPUB3 requires), so epubcheck applies EPUB3's
/// stricter content-document rules (HTML5 `<!DOCTYPE html>` required) to
/// XHTML 1.1-doctype content that was valid under the source's own EPUB2
/// rules. Rewriting chapter DOCTYPEs to "fix" this would mean no longer
/// copying content byte-for-byte - the same class of risk as the
/// pagination-collapse bug from renaming files/changing parse mode earlier
/// this session - so it's accepted as a known limitation rather than
/// "fixed" here. Real readers (Apple Books, Calibre) are broadly tolerant
/// of this in practice; epubcheck's EPUB3.3 ruleset is strict about it.
pub struct EpubOutput;

impl OutputPlugin<DocumentIR> for EpubOutput {
    fn name(&self) -> &'static str {
        "epub"
    }

    fn extension(&self) -> &'static str {
        "epub"
    }

    /// EPUB3 requires a nav document and a language regardless of user
    /// preference - there's no "skip the TOC" concept for a valid EPUB, and
    /// an EPUB missing its cover resource is strictly worse, not a
    /// legitimate option. So this plugin never reads `include_cover` or
    /// `generate_table_of_contents` from `opts` at all, unlike every other
    /// output plugin.
    fn convert(&self, ir: &DocumentIR, output: &Path, _opts: &Options, log: &dyn Log) -> Result<(), ConvError> {
        if ir.spine.is_empty() {
            return Err(ConvError::Other("This book has no readable chapters.".into()));
        }

        let nav_item = ir.manifest.values().find(|r| r.properties.contains("nav"));
        let ncx_item = ir.manifest.values().find(|r| r.media_type == "application/x-dtbncx+xml");
        // The directory the regenerated nav/ncx must land in for `ir.toc`'s
        // hrefs (relative to whichever of these actually supplied them) to
        // stay valid — see `TocNode::href`'s doc comment. Prefers nav's
        // directory when a nav item exists, matching EpubInput's own
        // nav-first precedence when choosing where ir.toc actually came from.
        let nav_href = nav_item.map(|r| r.href.clone()).unwrap_or_else(|| "nav.xhtml".to_string());
        let ncx_href = ncx_item.map(|r| r.href.clone()).unwrap_or_else(|| "toc.ncx".to_string());
        let nav_id = nav_item.map(|r| r.id.clone());
        let ncx_id = ncx_item.map(|r| r.id.clone());

        let mut items: Vec<&Resource> = ir.manifest.values().collect();
        items.sort_by(|a, b| a.id.cmp(&b.id));

        let file = std::fs::File::create(output)?;
        let mut zip = ZipWriter::new(file);

        zip.start_file("mimetype", FileOptions::default().compression_method(CompressionMethod::Stored))
            .map_err(|e| ConvError::Other(format!("failed to start mimetype entry: {e}")))?;
        zip.write_all(b"application/epub+zip")?;

        zip.start_file("META-INF/container.xml", FileOptions::default())
            .map_err(|e| ConvError::Other(format!("failed to start container.xml entry: {e}")))?;
        zip.write_all(CONTAINER_XML.as_bytes())?;

        let total = items.len().max(1);
        for (i, item) in items.iter().enumerate() {
            if log.is_cancelled() {
                return Err(ConvError::Cancelled);
            }
            log.progress(i as f64 / total as f64 * 0.8, "Packaging resources");

            // The nav/ncx entries are regenerated fresh below, not copied
            // from disk - their manifest metadata (href/media-type/
            // properties) is still emitted from `items` like any other
            // resource, just not their file content.
            if Some(&item.id) == nav_id.as_ref() || Some(&item.id) == ncx_id.as_ref() {
                continue;
            }

            let Some(zip_path) = normalize_oebps_path(&item.href) else {
                log.info(&format!("skipping resource outside the archive root: {}", item.href));
                continue;
            };
            let src = ir.content_dir.join(&item.href);
            let Ok(bytes) = std::fs::read(&src) else {
                let is_spine_chapter = ir.spine.iter().any(|s| s.href == item.href);
                if is_spine_chapter {
                    return Err(ConvError::MissingFile(item.href.clone()));
                }
                log.info(&format!("skipping unreadable resource: {}", item.href));
                continue;
            };
            zip.start_file(zip_path, FileOptions::default())
                .map_err(|e| ConvError::Other(format!("failed to start {} entry: {e}", item.href)))?;
            zip.write_all(&bytes)?;
        }

        if log.is_cancelled() {
            return Err(ConvError::Cancelled);
        }
        log.progress(0.85, "Writing table of contents");
        let Some(nav_zip_path) = normalize_oebps_path(&nav_href) else {
            return Err(ConvError::Other(format!("nav document path escapes the archive root: {nav_href}")));
        };
        zip.start_file(nav_zip_path, FileOptions::default())
            .map_err(|e| ConvError::Other(format!("failed to start nav entry: {e}")))?;
        zip.write_all(render_nav_xhtml(&ir.toc, &ir.metadata.title).as_bytes())?;

        let Some(ncx_zip_path) = normalize_oebps_path(&ncx_href) else {
            return Err(ConvError::Other(format!("ncx path escapes the archive root: {ncx_href}")));
        };
        let ncx_id_str = ncx_id.unwrap_or_else(|| "ncx".to_string());
        zip.start_file(ncx_zip_path, FileOptions::default())
            .map_err(|e| ConvError::Other(format!("failed to start ncx entry: {e}")))?;
        zip.write_all(render_toc_ncx(&ir.toc, &ir.metadata.title, &deterministic_identifier(ir)).as_bytes())?;

        log.progress(0.95, "Writing package document");
        let nav_id_str = nav_id.unwrap_or_else(|| "nav".to_string());
        let opf = render_opf(ir, &items, &nav_id_str, &ncx_id_str, nav_item.is_none(), ncx_item.is_none(), &nav_href, &ncx_href);
        zip.start_file("OEBPS/content.opf", FileOptions::default())
            .map_err(|e| ConvError::Other(format!("failed to start content.opf entry: {e}")))?;
        zip.write_all(opf.as_bytes())?;

        zip.finish().map_err(|e| ConvError::Other(format!("failed to finish EPUB archive: {e}")))?;
        log.progress(1.0, "Done");
        Ok(())
    }
}

const CONTAINER_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>
"#;

/// Joins `href` onto `OEBPS/` and lexically resolves `.`/`..` segments,
/// since every IR-carried href is relative to `content_dir` and this plugin
/// places all content under a single uniform `OEBPS/` prefix (the IR has no
/// record of the source EPUB's own internal layout, only paths relative to
/// its OPF). Returns `None` if the result would escape the archive root
/// (legal EPUB layout in principle, very rare in practice).
fn normalize_oebps_path(href: &str) -> Option<String> {
    let full = format!("OEBPS/{href}");
    let mut parts: Vec<&str> = Vec::new();
    for seg in full.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

fn render_nav_xhtml(toc: &[TocNode], title: &str) -> String {
    fn render_list(nodes: &[TocNode]) -> String {
        if nodes.is_empty() {
            return String::new();
        }
        let mut out = String::from("<ol>\n");
        for node in nodes {
            out.push_str("<li><a href=\"");
            out.push_str(&xml_escape(node.href.as_deref().unwrap_or("")));
            out.push_str("\">");
            out.push_str(&xml_escape(&node.title));
            out.push_str("</a>");
            if !node.children.is_empty() {
                out.push_str(&render_list(&node.children));
            }
            out.push_str("</li>\n");
        }
        out.push_str("</ol>\n");
        out
    }

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE html>\n\
         <html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\">\n\
         <head><title>{title}</title></head>\n\
         <body>\n<nav epub:type=\"toc\" id=\"toc\">\n<h1>{title}</h1>\n{list}</nav>\n</body>\n</html>\n",
        title = xml_escape(title),
        list = render_list(toc),
    )
}

fn render_toc_ncx(toc: &[TocNode], title: &str, identifier: &str) -> String {
    let mut play_order = 0u32;
    fn render_nav_points(nodes: &[TocNode], play_order: &mut u32) -> String {
        let mut out = String::new();
        for node in nodes {
            *play_order += 1;
            out.push_str(&format!(
                "<navPoint id=\"navPoint-{po}\" playOrder=\"{po}\">\n\
                 <navLabel><text>{title}</text></navLabel>\n\
                 <content src=\"{href}\"/>\n",
                po = play_order,
                title = xml_escape(&node.title),
                href = xml_escape(node.href.as_deref().unwrap_or("")),
            ));
            out.push_str(&render_nav_points(&node.children, play_order));
            out.push_str("</navPoint>\n");
        }
        out
    }

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE ncx PUBLIC \"-//NISO//DTD ncx 2005-1//EN\" \"http://www.daisy.org/z3986/2005/ncx-2005-1.dtd\">\n\
         <ncx xmlns=\"http://www.daisy.org/z3986/2005/ncx/\" version=\"2005-1\">\n\
         <head><meta name=\"dtb:uid\" content=\"{identifier}\"/></head>\n\
         <docTitle><text>{title}</text></docTitle>\n\
         <navMap>\n{points}</navMap>\n</ncx>\n",
        identifier = xml_escape(identifier),
        title = xml_escape(title),
        points = render_nav_points(toc, &mut play_order),
    )
}

#[allow(clippy::too_many_arguments)]
fn render_opf(
    ir: &DocumentIR,
    items: &[&Resource],
    nav_id: &str,
    ncx_id: &str,
    synthesize_nav: bool,
    synthesize_ncx: bool,
    nav_href: &str,
    ncx_href: &str,
) -> String {
    let mut manifest = String::new();
    for item in items {
        manifest.push_str(&format!(
            "<item id=\"{id}\" href=\"{href}\" media-type=\"{media_type}\"{properties}/>\n",
            id = xml_escape(&item.id),
            href = xml_escape(&item.href),
            media_type = xml_escape(&item.media_type),
            properties = manifest_properties_attr(&item.properties),
        ));
    }
    if synthesize_nav {
        manifest.push_str(&format!(
            "<item id=\"{nav_id}\" href=\"{href}\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>\n",
            href = xml_escape(nav_href),
        ));
    }
    if synthesize_ncx {
        manifest.push_str(&format!(
            "<item id=\"{ncx_id}\" href=\"{href}\" media-type=\"application/x-dtbncx+xml\"/>\n",
            href = xml_escape(ncx_href),
        ));
    }

    let mut spine = String::new();
    for item in &ir.spine {
        let idref = items
            .iter()
            .find(|r| r.href == item.href)
            .map(|r| r.id.as_str())
            .unwrap_or(&item.id);
        spine.push_str(&format!("<itemref idref=\"{}\"/>\n", xml_escape(idref)));
    }

    let author_meta = ir
        .metadata
        .author
        .as_deref()
        .map(|a| format!("<dc:creator>{}</dc:creator>\n", xml_escape(a)))
        .unwrap_or_default();
    let cover_meta = ir
        .metadata
        .cover_href
        .as_ref()
        .and_then(|href| items.iter().find(|r| &r.href == href))
        .map(|r| format!("<meta name=\"cover\" content=\"{}\"/>\n", xml_escape(&r.id)))
        .unwrap_or_default();

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <package xmlns=\"http://www.idpf.org/2007/opf\" unique-identifier=\"bookid\" version=\"3.0\">\n\
         <metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n\
         <dc:identifier id=\"bookid\">{identifier}</dc:identifier>\n\
         <dc:title>{title}</dc:title>\n\
         <dc:language>{language}</dc:language>\n\
         {author_meta}\
         <meta property=\"dcterms:modified\">{modified}</meta>\n\
         {cover_meta}\
         </metadata>\n\
         <manifest>\n{manifest}</manifest>\n\
         <spine toc=\"{ncx_id}\">\n{spine}</spine>\n\
         </package>\n",
        identifier = xml_escape(&deterministic_identifier(ir)),
        title = xml_escape(&ir.metadata.title),
        language = xml_escape(ir.metadata.language.as_deref().unwrap_or("en")),
        modified = iso8601_utc_now(),
    )
}

fn manifest_properties_attr(properties: &std::collections::HashSet<String>) -> String {
    if properties.is_empty() {
        String::new()
    } else {
        let mut sorted: Vec<&String> = properties.iter().collect();
        sorted.sort();
        let joined = sorted.iter().map(|s| xml_escape(s)).collect::<Vec<_>>().join(" ");
        format!(" properties=\"{joined}\"")
    }
}

/// Mints a deterministic `urn:uuid:`-shaped identifier from a SHA-1 of the
/// book's title/author/spine hrefs, rather than a random UUID - the source
/// book's own `dc:identifier` isn't exposed on `DocumentIR` and can't be
/// recovered, but a deterministic one is reproducible and testable, unlike
/// a fresh random one on every conversion.
fn deterministic_identifier(ir: &DocumentIR) -> String {
    let mut hasher = Sha1::new();
    hasher.update(ir.metadata.title.as_bytes());
    hasher.update(b"\0");
    hasher.update(ir.metadata.author.as_deref().unwrap_or("").as_bytes());
    hasher.update(b"\0");
    for item in &ir.spine {
        hasher.update(item.href.as_bytes());
        hasher.update(b"\0");
    }
    let h = hasher.finalize();
    format!(
        "urn:uuid:{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7], h[8], h[9], h[10], h[11], h[12], h[13], h[14], h[15]
    )
}

fn iso8601_utc_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = (secs / 86400) as i64;
    let time_of_day = secs % 86400;
    let (h, m, s) = (time_of_day / 3600, (time_of_day / 60) % 60, time_of_day % 60);
    let (y, mo, d) = civil_from_days(days);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

/// Howard Hinnant's `civil_from_days`: days since the Unix epoch ->
/// (year, month, day), proleptic Gregorian calendar. Well-known, public
/// domain algorithm - avoids pulling in a date/time crate for one
/// `dcterms:modified` timestamp.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}
