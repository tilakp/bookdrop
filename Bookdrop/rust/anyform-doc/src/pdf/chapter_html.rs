//! Prepares a chapter's markup for printing: typography overrides, style
//! stripping, and hidden links that make Chrome emit TOC destinations.

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;

use crate::ir::TocNode;
use crate::markup::xml_escape;

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

/// A chapter's markup as it is printed: with the typography overrides,
/// without the book's own styling if requested, and with the hidden TOC
/// anchor links. `BookTab::print` serves it at the chapter's own URL, so
/// the file on disk stays unchanged.
pub(super) fn prepare_chapter_html(mut html: String, opts: &RenderOptions, toc_fragments: &[String]) -> String {
    if !opts.preserve_epub_styling || opts.remove_publisher_styling {
        let re = style_strip_regexes();
        html = re.style_tag.replace_all(&html, "").into_owned();
        html = re.link_tag.replace_all(&html, "").into_owned();
        html = re.style_attr_dq.replace_all(&html, "").into_owned();
        html = re.style_attr_sq.replace_all(&html, "").into_owned();
    }
    let html = inject_style(&html, &typography_css(opts));
    inject_anchor_links(&html, toc_fragments)
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

/// Chrome's print_to_pdf does not shrink content to fit the page: anything
/// wider than the printable area is cut off at the page edge. Not every
/// EPUB limits image width. A real scanned-page EPUB had 1268 px images
/// under `width: auto`, and a wide diagram lost its right side. The rule
/// applies always, not only with a typography option, because it fixes
/// output and changes nothing for an image that fits.
///
/// `pre` and `code` blocks have the same problem: a long unbroken line (a
/// URL, a hex digest) is cut off instead of wrapped. `css-edge-cases.epub`
/// shows it. CSS columns (`column-count`) do not have it, because Chrome
/// sizes columns to their container, so they get no rule.
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
        .map(|f| format!("<a href=\"#{}\" style=\"display:inline-block;width:1px;height:1px\"></a>", xml_escape(f)))
        .collect();
    let block = format!("<div aria-hidden=\"true\" style=\"position:absolute;left:0;top:0;width:1px;height:1px;overflow:hidden\">{links}</div>");
    match html.rfind("</body>") {
        Some(pos) => format!("{}{}{}", &html[..pos], block, &html[pos..]),
        None => format!("{html}{block}"),
    }
}

/// True if `spine_path`'s markup references `cover_href`'s filename (e.g.
/// via `<img src="../images/cover.jpg">`), that is, this spine page's whole
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
