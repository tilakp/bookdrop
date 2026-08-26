//! Shared "XHTML chapter → structured text" extraction used by `txt.rs` and
//! (Phase 4) `docx.rs`. `html.rs`/`epub_output.rs` don't need this - they
//! preserve or emit markup directly rather than extracting text from it.
//!
//! Roxmltree fast path, regex fallback: EPUB chapters are XHTML by spec, so
//! the primary path reuses `epub::parse_xml` (exact, entity-correct for XML
//! builtins, no new dependency) and walks the DOM. Two things break it in
//! the wild: HTML named entities not declared in any DOCTYPE (`&nbsp;`,
//! `&mdash;`), and genuinely non-well-formed markup from bad producers. The
//! fallback (already-present `regex` dependency) handles both, at the cost
//! of losing heading-level/bold/italic fidelity - acceptable since it's a
//! rare path, not the common case.

use std::sync::OnceLock;

use regex::Regex;

pub(crate) enum BlockKind {
    Heading(u8),
    Paragraph,
    ListItem,
    Blockquote,
    Preformatted,
}

pub(crate) struct Run {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
}

pub(crate) struct Block {
    pub kind: BlockKind,
    pub runs: Vec<Run>,
}

impl Block {
    fn text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }
}

pub(crate) fn extract_blocks(html: &str) -> Vec<Block> {
    match crate::epub::parse_xml(html) {
        Ok(doc) => blocks_from_xml(&doc),
        Err(_) => blocks_from_fallback(html),
    }
}

pub(crate) fn extract_text(html: &str) -> String {
    extract_blocks(html)
        .iter()
        .map(Block::text)
        .filter(|t| !t.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

// ---- roxmltree fast path ----

const CONTAINER_TAGS: &[&str] = &["div", "section", "article", "nav", "header", "footer", "main", "aside", "figure", "body", "html"];
const SKIP_TAGS: &[&str] = &["script", "style", "head", "title"];

fn blocks_from_xml(doc: &roxmltree::Document) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut current: Option<Block> = None;
    walk(doc.root_element(), false, false, &mut blocks, &mut current);
    if let Some(b) = current.take() {
        if !b.runs.is_empty() {
            blocks.push(b);
        }
    }
    blocks
}

fn heading_level(tag: &str) -> Option<u8> {
    match tag {
        "h1" => Some(1),
        "h2" => Some(2),
        "h3" => Some(3),
        "h4" => Some(4),
        "h5" => Some(5),
        "h6" => Some(6),
        _ => None,
    }
}

fn block_kind_for(tag: &str) -> Option<BlockKind> {
    if let Some(level) = heading_level(tag) {
        return Some(BlockKind::Heading(level));
    }
    match tag {
        "p" => Some(BlockKind::Paragraph),
        "li" => Some(BlockKind::ListItem),
        "blockquote" => Some(BlockKind::Blockquote),
        "pre" => Some(BlockKind::Preformatted),
        _ => None,
    }
}

/// Recursively walks the DOM, flushing `current` into `blocks` whenever a
/// new block-level element starts and opening a fresh one, so text sitting
/// directly under a container (no `<p>` wrapper) still becomes an implicit
/// paragraph rather than being dropped.
fn walk(node: roxmltree::Node, bold: bool, italic: bool, blocks: &mut Vec<Block>, current: &mut Option<Block>) {
    if node.is_text() {
        if let Some(text) = node.text() {
            if !text.is_empty() {
                if current.is_none() {
                    *current = Some(Block { kind: BlockKind::Paragraph, runs: Vec::new() });
                }
                current.as_mut().unwrap().runs.push(Run { text: text.to_string(), bold, italic });
            }
        }
        return;
    }
    if !node.is_element() {
        return;
    }
    let tag = node.tag_name().name();
    if SKIP_TAGS.contains(&tag) {
        return;
    }
    if tag == "br" {
        if let Some(b) = current.as_mut() {
            b.runs.push(Run { text: "\n".to_string(), bold, italic });
        }
        return;
    }

    let is_bold = bold || matches!(tag, "b" | "strong");
    let is_italic = italic || matches!(tag, "i" | "em");

    if let Some(kind) = block_kind_for(tag) {
        if let Some(b) = current.take() {
            if !b.runs.is_empty() {
                blocks.push(b);
            }
        }
        *current = Some(Block { kind, runs: Vec::new() });
        for child in node.children() {
            walk(child, is_bold, is_italic, blocks, current);
        }
        if let Some(b) = current.take() {
            if !b.runs.is_empty() {
                blocks.push(b);
            }
        }
        return;
    }

    if CONTAINER_TAGS.contains(&tag) {
        for child in node.children() {
            walk(child, is_bold, is_italic, blocks, current);
        }
        return;
    }

    // Any other inline element (span, a, code, sup, ...): recurse with the
    // current bold/italic context, contributing to whatever block is open.
    for child in node.children() {
        walk(child, is_bold, is_italic, blocks, current);
    }
}

// ---- regex fallback ----

struct FallbackRegexes {
    strip_blocks: Regex,
    block_break: Regex,
    tag: Regex,
    numeric_entity_dec: Regex,
    numeric_entity_hex: Regex,
}

fn fallback_regexes() -> &'static FallbackRegexes {
    static REGEXES: OnceLock<FallbackRegexes> = OnceLock::new();
    REGEXES.get_or_init(|| FallbackRegexes {
        // Rust's `regex` crate has no backreference support, so this can't
        // require the closing tag to match the same name as the opener -
        // matches each element as its own alternative instead. Drops
        // <head>...</head> wholesale (not just <script>/<style>) - the
        // roxmltree fast path already skips head/title via SKIP_TAGS, and
        // without this the fallback leaked <title> text into the visible
        // body (found empirically: "Chapter One" appeared twice, once from
        // <title>, once from the real <h1>).
        strip_blocks: Regex::new(r"(?is)<script\b[^>]*>.*?</script>|<style\b[^>]*>.*?</style>|<head\b[^>]*>.*?</head>").unwrap(),
        block_break: Regex::new(r"(?i)</(p|h[1-6]|li|div|blockquote|section|article)\s*>|<br\s*/?>").unwrap(),
        tag: Regex::new(r"<[^>]*>").unwrap(),
        numeric_entity_dec: Regex::new(r"&#(\d+);").unwrap(),
        numeric_entity_hex: Regex::new(r"(?i)&#x([0-9a-f]+);").unwrap(),
    })
}

fn named_entity(name: &str) -> Option<char> {
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{00A0}',
        "mdash" => '\u{2014}',
        "ndash" => '\u{2013}',
        "hellip" => '\u{2026}',
        "ldquo" => '\u{201C}',
        "rdquo" => '\u{201D}',
        "lsquo" => '\u{2018}',
        "rsquo" => '\u{2019}',
        "copy" => '\u{00A9}',
        "eacute" => '\u{00E9}',
        _ => return None,
    })
}

fn decode_entities(s: &str) -> String {
    let re = fallback_regexes();
    let s = re.numeric_entity_dec.replace_all(s, |caps: &regex::Captures| {
        caps[1].parse::<u32>().ok().and_then(char::from_u32).map(String::from).unwrap_or_default()
    });
    let s = re.numeric_entity_hex.replace_all(&s, |caps: &regex::Captures| {
        u32::from_str_radix(&caps[1], 16).ok().and_then(char::from_u32).map(String::from).unwrap_or_default()
    });
    let mut out = String::with_capacity(s.len());
    let mut rest = s.as_ref();
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        if let Some(semi) = after.find(';').filter(|&i| i <= 10) {
            let name = &after[..semi];
            if let Some(c) = named_entity(name) {
                out.push(c);
                rest = &after[semi + 1..];
                continue;
            }
        }
        out.push('&');
        rest = after;
    }
    out.push_str(rest);
    out
}

fn blocks_from_fallback(html: &str) -> Vec<Block> {
    let re = fallback_regexes();
    let without_scripts = re.strip_blocks.replace_all(html, "");
    let with_breaks = re.block_break.replace_all(&without_scripts, "\n");
    let stripped = re.tag.replace_all(&with_breaks, "");
    let decoded = decode_entities(&stripped);

    decoded
        .split('\n')
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .map(|line| Block { kind: BlockKind::Paragraph, runs: vec![Run { text: line.to_string(), bold: false, italic: false }] })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_tags_and_joins_paragraphs() {
        let html = "<html><body><h1>Title</h1><p>First.</p><p>Second.</p></body></html>";
        let text = extract_text(html);
        assert_eq!(text, "Title\nFirst.\nSecond.");
    }

    #[test]
    fn drops_script_and_style() {
        let html = "<html><body><script>var x = 1;</script><style>body{color:red}</style><p>Real text.</p></body></html>";
        let text = extract_text(html);
        assert!(!text.contains("var x"));
        assert!(!text.contains("color:red"));
        assert!(text.contains("Real text."));
    }

    #[test]
    fn captures_bold_and_italic_runs() {
        let html = "<html><body><p>plain <b>bold</b> and <i>italic</i></p></body></html>";
        let blocks = extract_blocks(html);
        assert_eq!(blocks.len(), 1);
        let bold_run = blocks[0].runs.iter().find(|r| r.text.contains("bold")).unwrap();
        assert!(bold_run.bold);
        let italic_run = blocks[0].runs.iter().find(|r| r.text.contains("italic")).unwrap();
        assert!(italic_run.italic);
    }

    #[test]
    fn heading_level_is_detected() {
        let html = "<html><body><h2>Section</h2></body></html>";
        let blocks = extract_blocks(html);
        assert!(matches!(blocks[0].kind, BlockKind::Heading(2)));
    }

    #[test]
    fn malformed_markup_falls_back_and_still_extracts_text() {
        // An unclosed <br> and a bare & make this non-well-formed XML.
        let html = "<html><body><p>Line one<br>Line two & more</p></body>";
        let text = extract_text(html);
        assert!(text.contains("Line one"));
        assert!(text.contains("Line two"));
    }

    #[test]
    fn fallback_decodes_named_and_numeric_entities() {
        let html = "<p>space&nbsp;here&mdash;and&#8212;numeric&#x2014;hex<br></p>"; // unclosed <br> forces fallback
        let text = extract_text(html);
        assert!(text.contains('\u{2014}'));
        assert!(text.contains('\u{00A0}'));
    }
}
