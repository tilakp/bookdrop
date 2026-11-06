//! Prepares a chapter's markup for printing: typography overrides, style
//! stripping, and hidden links that make Chrome emit TOC destinations.

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use anyform_core::ConvError;
use regex::Regex;

use crate::ir::TocNode;

use super::RenderOptions;

/// Regexes that strip the book's own CSS when `!preserve_epub_styling ||
/// remove_publisher_styling`. Text surgery on the raw markup is enough:
/// XHTML tag names are always lowercase.
struct StyleStripRegexes {
    style_tag: Regex,
    link_tag: Regex,
    style_attr_dq: Regex,
    style_attr_sq: Regex,
}

fn style_strip_regexes() -> &'static StyleStripRegexes {
    static REGEXES: OnceLock<StyleStripRegexes> = OnceLock::new();
    REGEXES.get_or_init(|| StyleStripRegexes {
        style_tag: Regex::new(r"(?is)<style\b[^>]*>.*?</style>").unwrap(),
        link_tag: Regex::new(r#"(?i)<link[^>]*\brel\s*=\s*['"]stylesheet['"][^>]*/?>"#).unwrap(),
        style_attr_dq: Regex::new(r#"\sstyle\s*=\s*"[^"]*""#).unwrap(),
        style_attr_sq: Regex::new(r"\sstyle\s*=\s*'[^']*'").unwrap(),
    })
}

/// Overwrites `original` in place with the typography overrides (and, if
/// requested, without the book's own styling) and returns the original
/// bytes, so the caller can restore them after rendering. Each worker
/// thread has different spine items, so no two threads write one file.
///
/// The file is changed in place, not copied to a new name, because of
/// footnote links. EPUB writes them as `<a href="thisfile.xhtml#note3">`
/// even inside the same file, and Chrome makes a working `/Dest` only when
/// that href resolves to the exact URL of the loaded page. With a renamed
/// copy, every internal link became a dead `file://` URI. See
/// `namespace_chapter_dests` and `fix_cross_chapter_links` for how the
/// links survive the merge.
pub(super) fn prepare_chapter_html(original: &Path, opts: &RenderOptions, toc_fragments: &[String]) -> Result<Vec<u8>, ConvError> {
    let original_bytes = std::fs::read(original)
        .map_err(|e| ConvError::Other(format!("failed to read {}: {e}", original.display())))?;
    let needs_strip = !opts.preserve_epub_styling || opts.remove_publisher_styling;
    let mut html = String::from_utf8_lossy(&original_bytes).into_owned();

    if needs_strip {
        let re = style_strip_regexes();
        html = re.style_tag.replace_all(&html, "").into_owned();
        html = re.link_tag.replace_all(&html, "").into_owned();
        html = re.style_attr_dq.replace_all(&html, "").into_owned();
        html = re.style_attr_sq.replace_all(&html, "").into_owned();
    }

    let css = typography_css(opts);
    html = inject_style(&html, &css);
    html = inject_anchor_links(&html, toc_fragments);

    std::fs::write(original, html)?;
    Ok(original_bytes)
}

fn typography_css(opts: &RenderOptions) -> String {
    let font_family_rule = match &opts.font_family {
        Some(family) => format!(" font-family: '{}', serif !important;", family.replace(['\'', '"'], "")),
        None => String::new(),
    };
    format!(
        "html, body {{ margin: 0 !important; }} \
         body {{ font-size: {}pt !important; line-height: {} !important;{font_family_rule} }} \
         {SHRINK_TO_FIT_CSS}",
        opts.font_size_pt, opts.line_spacing
    )
}

/// Chrome's print_to_pdf does not shrink oversized content to fit the page
/// (no "shrink to fit" like some print dialogs offer) — anything wider than
/// the printable area just gets clipped at the page edge. Some EPUBs supply
/// their own `max-width: 100%` reset for images (calibre's own default
/// stylesheet does), but not all do — a real scanned-page EPUB shipped
/// images at native pixel width (1268px) under a class rule of `width:
/// auto` with no upper bound, so a wide diagram got clipped down its right
/// side in the rendered PDF. Applied unconditionally (not tied to any
/// typography option) since this is a correctness fix, not a style choice:
/// an image that already fits is unaffected by `max-width: 100%`.
///
/// `pre`/`code` blocks hit the exact same "no shrink to fit" failure mode:
/// a long unbroken line (a URL, a hex digest, an unwrapped code line)
/// clips at the page edge instead of wrapping - confirmed with a synthetic
/// fixture (`css-edge-cases.epub`) before adding this rule. CSS multi-
/// column layouts (`column-count`) were audited the same way and turned
/// out *not* to reproduce a clipping bug: Chrome sizes columns to fit
/// their container rather than overflowing it, so no override is added for
/// that case - there was no confirmed failure to fix.
const SHRINK_TO_FIT_CSS: &str = "img, svg { max-width: 100% !important; height: auto !important; } \
     table { max-width: 100% !important; } \
     pre, code { white-space: pre-wrap !important; overflow-wrap: break-word !important; \
     word-break: break-word !important; max-width: 100% !important; }";

/// Inserts `<style>{css}</style>` into `head`. EPUB content documents are
/// XHTML, which mandates lowercase element names, so a literal (not
/// case-insensitive) search for `</head>`/`<body` is sufficient and avoids
/// the byte-offset hazards of matching against a lowercased copy.
fn inject_style(html: &str, css: &str) -> String {
    let style_block = format!("<style>{css}</style>");
    if let Some(pos) = html.find("</head>") {
        format!("{}{}{}", &html[..pos], style_block, &html[pos..])
    } else if let Some(pos) = html.find("<body") {
        format!("{}<head>{}</head>{}", &html[..pos], style_block, &html[pos..])
    } else {
        format!("<head>{style_block}</head>{html}")
    }
}

/// Canonical chapter path -> the fragments TOC entries point at in it.
pub(super) fn toc_fragments_by_chapter(toc: &[TocNode]) -> HashMap<String, Vec<String>> {
    fn walk(nodes: &[TocNode], out: &mut HashMap<String, Vec<String>>) {
        for node in nodes {
            if let Some((path, fragment)) = node.href.as_deref().and_then(|h| h.split_once('#')) {
                out.entry(path.to_string()).or_default().push(fragment.to_string());
            }
            walk(&node.children, out);
        }
    }
    let mut out = HashMap::new();
    walk(toc, &mut out);
    out
}

/// Chrome writes a named destination only for an id that a link in the
/// same document points at. A TOC entry like `ch1.xhtml#chapter-3` usually
/// has no such link, so its bookmark could only open the chapter file's
/// first page. One invisible link per TOC fragment makes Chrome emit the
/// destination, so the bookmark lands on the right page.
fn inject_anchor_links(html: &str, fragments: &[String]) -> String {
    if fragments.is_empty() {
        return html.to_string();
    }
    let links: String = fragments
        .iter()
        .map(|f| format!("<a href=\"#{}\" style=\"display:inline-block;width:1px;height:1px\"></a>", xml_escape_attr(f)))
        .collect();
    let block = format!("<div aria-hidden=\"true\" style=\"position:absolute;left:0;top:0;width:1px;height:1px;overflow:hidden\">{links}</div>");
    match html.rfind("</body>") {
        Some(pos) => format!("{}{}{}", &html[..pos], block, &html[pos..]),
        None => format!("{html}{block}"),
    }
}

fn xml_escape_attr(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;")
}

/// True if `spine_path`'s markup references `cover_href`'s filename (e.g.
/// via `<img src="../images/cover.jpg">`) — i.e. this spine page's whole
/// job is displaying the cover image, so it shouldn't also be rendered as
/// a regular chapter once a synthetic cover page has already been added.
pub(super) fn spine_item_is_cover_page(spine_path: &Path, cover_href: &str) -> bool {
    let Some(cover_filename) = Path::new(cover_href).file_name().and_then(|f| f.to_str()) else {
        return false;
    };
    let Ok(html) = std::fs::read_to_string(spine_path) else {
        return false;
    };
    html.contains(cover_filename)
}
