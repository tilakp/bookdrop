//! Markdown output: one `.md` file, plus a `<name>_images/` folder next to
//! it with the images the book shows. Images keep their path inside the
//! book, so two images never get the same name.
//!
//! The text comes from `htmltext::extract_blocks`, like TXT and DOCX
//! output: headings, paragraphs, list items, quotes, code blocks, and bold
//! and italic. Links become plain text, lists are always bulleted, and
//! there is no generated table of contents.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyform_core::{ConvError, Log, Options, OutputPlugin};

use crate::href;
use crate::htmltext::{self, Block, BlockKind, Run};
use crate::ir::DocumentIR;
use crate::options::{self, BookOptions};

/// The images folder of "Book.md" is "Book_images".
const IMAGES_SUFFIX: &str = "_images";

pub struct MarkdownOutput;

impl OutputPlugin<DocumentIR> for MarkdownOutput {
    fn name(&self) -> &'static str {
        "markdown"
    }

    fn extension(&self) -> &'static str {
        "md"
    }

    fn options(&self) -> &'static [&'static str] {
        &[options::INCLUDE_COVER]
    }

    fn companion_suffix(&self) -> Option<&'static str> {
        Some(IMAGES_SUFFIX)
    }

    fn convert(&self, ir: &DocumentIR, output: &Path, opts: &Options, log: &dyn Log) -> Result<(), ConvError> {
        if ir.spine.is_empty() {
            return Err(ConvError::NoChapters);
        }
        let mut images = Images::new(ir, output);
        let mut md = format!("# {}\n\n", escape_line_start(&escape(&ir.metadata.title)));
        if let Some(author) = &ir.metadata.author {
            md.push_str(&format!("{}\n\n", escape_line_start(&escape(author))));
        }

        // A book's own cover page shows the same image again: show it once.
        let mut cover_shown = None;
        if BookOptions::from_options(opts).include_cover {
            if let Some(cover) = &ir.metadata.cover_href {
                if let Some(link) = images.link(cover, "Cover", log)? {
                    md.push_str(&link);
                    md.push_str("\n\n");
                    cover_shown = Some(cover.as_str());
                }
            }
        }

        let total = ir.spine.len();
        for (i, item) in ir.spine.iter().enumerate() {
            if log.is_cancelled() {
                return Err(ConvError::Cancelled);
            }
            log.progress(i as f64 / total as f64, &format!("Writing chapter {} of {}", i + 1, total));
            let base_dir = href::parent_dir(&item.href);
            for block in htmltext::extract_blocks(&ir.read_chapter(item)?) {
                let text = match &block.kind {
                    BlockKind::Image { src, alt } => match href::resolve(base_dir, src) {
                        Some((path, _)) if Some(path.as_str()) != cover_shown => images.link(&path, alt, log)?,
                        _ => None,
                    },
                    _ => render_block(&block),
                };
                if let Some(text) = text {
                    md.push_str(&text);
                    md.push_str("\n\n");
                }
            }
        }

        std::fs::write(output, md.trim_end().to_string() + "\n")?;
        log.progress(1.0, "Done");
        Ok(())
    }
}

/// Copies the images the text links to into the images folder, each once.
struct Images<'a> {
    ir: &'a DocumentIR,
    dir: PathBuf,
    /// The folder name, encoded for a link.
    url_prefix: String,
    copied: HashSet<String>,
}

impl<'a> Images<'a> {
    fn new(ir: &'a DocumentIR, output: &Path) -> Self {
        let dir = anyform_core::companion_dir(output, IMAGES_SUFFIX);
        let url_prefix = link_encode(&dir.file_name().unwrap_or_default().to_string_lossy());
        Images { ir, dir, url_prefix, copied: HashSet::new() }
    }

    /// The Markdown for the image at canonical `path`, copying it on first
    /// use. `None` if the book has no such image file.
    fn link(&mut self, path: &str, alt: &str, log: &dyn Log) -> Result<Option<String>, ConvError> {
        if !self.ir.manifest.values().any(|r| r.href == path && r.media_type.starts_with("image/")) {
            log.info(&format!("skipping image not in the book's manifest: {path}"));
            return Ok(None);
        }
        if self.copied.insert(path.to_string()) {
            let target = self.dir.join(path);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            if std::fs::copy(self.ir.content_dir.join(path), &target).is_err() {
                log.info(&format!("skipping unreadable image: {path}"));
                self.copied.remove(path);
                return Ok(None);
            }
        }
        let url: Vec<String> = path.split('/').map(link_encode).collect();
        let alt = escape(&collapse_whitespace(alt));
        Ok(Some(format!("![{alt}]({}/{})", self.url_prefix, url.join("/"))))
    }
}

/// The Markdown for one text block, or `None` if it has no text.
fn render_block(block: &Block) -> Option<String> {
    if let BlockKind::Preformatted = block.kind {
        let code: String = block.runs.iter().map(|r| r.text.as_str()).collect();
        let code = code.trim_matches('\n');
        if code.trim().is_empty() {
            return None;
        }
        let fence = "`".repeat(longest_backtick_run(code).max(2) + 1);
        return Some(format!("{fence}\n{code}\n{fence}"));
    }

    let lines = inline_lines(&block.runs);
    if lines.is_empty() {
        return None;
    }
    let text = match block.kind {
        BlockKind::Heading(level) => format!("{} {}", "#".repeat(level.clamp(1, 6) as usize), lines.join(" ")),
        BlockKind::ListItem => format!("- {}", lines.join("\\\n  ")),
        BlockKind::Blockquote => lines.iter().map(|l| format!("> {l}")).collect::<Vec<_>>().join("\\\n"),
        _ => lines.join("\\\n"),
    };
    Some(text)
}

/// The runs as escaped, styled Markdown, one string per `<br>`-separated
/// line, with empty lines at the start and end dropped.
fn inline_lines(runs: &[Run]) -> Vec<String> {
    let mut lines = vec![String::new()];
    let mut i = 0;
    while i < runs.len() {
        if runs[i].line_break {
            lines.push(String::new());
            i += 1;
            continue;
        }
        // Runs with the same style become one span, so "a" "b" in bold is
        // **ab**, not **a****b**.
        let (bold, italic) = (runs[i].bold, runs[i].italic);
        let mut text = String::new();
        while i < runs.len() && !runs[i].line_break && (runs[i].bold, runs[i].italic) == (bold, italic) {
            text.push_str(&runs[i].text);
            i += 1;
        }
        let line = lines.last_mut().expect("lines is never empty");
        line.push_str(&styled(&collapse_whitespace(&text), bold, italic));
    }
    let lines: Vec<String> = lines.iter().map(|l| collapse_spaces(l)).collect();
    let start = lines.iter().position(|l| !l.is_empty());
    let end = lines.iter().rposition(|l| !l.is_empty());
    match (start, end) {
        (Some(s), Some(e)) => lines[s..=e].iter().map(|l| escape_line_start(l)).collect(),
        _ => Vec::new(),
    }
}

/// `text` escaped and wrapped in emphasis markers. Spaces at its ends stay
/// outside the markers: "** bold**" is not bold in Markdown.
fn styled(text: &str, bold: bool, italic: bool) -> String {
    let marker = match (bold, italic) {
        (true, true) => "***",
        (true, false) => "**",
        (false, true) => "*",
        (false, false) => "",
    };
    let core = text.trim();
    if marker.is_empty() || core.is_empty() {
        return escape(text);
    }
    let lead = if text.starts_with(' ') { " " } else { "" };
    let trail = if text.ends_with(' ') { " " } else { "" };
    format!("{lead}{marker}{}{marker}{trail}", escape(core))
}

/// Every run of whitespace (source line breaks and indents too) as one space.
fn collapse_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            if !in_space {
                out.push(' ');
            }
            in_space = true;
        } else {
            out.push(c);
            in_space = false;
        }
    }
    out
}

/// Joining spans can put two spaces together. Also trims the line.
fn collapse_spaces(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    for c in line.trim().chars() {
        if !(c == ' ' && out.ends_with(' ')) {
            out.push(c);
        }
    }
    out
}

/// Backslash-escapes the characters that start Markdown syntax inside a
/// line.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Escapes what would make an ordinary line a heading, quote, list item
/// or rule: a leading "#", ">", "-", "+", "=" or "|", or "1." and "1)".
fn escape_line_start(line: &str) -> String {
    if line.starts_with(['#', '>', '-', '+', '=', '|']) {
        return format!("\\{line}");
    }
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && line[digits..].starts_with(['.', ')']) {
        return format!("{}\\{}", &line[..digits], &line[digits..]);
    }
    line.to_string()
}

/// A path segment for a Markdown link: percent-encoded like any URL
/// segment, plus parentheses, which would end the link.
fn link_encode(segment: &str) -> String {
    href::percent_encode(segment).replace('(', "%28").replace(')', "%29")
}

fn longest_backtick_run(text: &str) -> usize {
    text.split(|c| c != '`').map(str::len).max().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str, bold: bool, italic: bool) -> Run {
        Run { text: text.into(), bold, italic, line_break: false }
    }

    #[test]
    fn spaces_stay_outside_emphasis_markers() {
        let runs = [run("plain ", false, false), run(" bold ", true, false), run("and ", false, false), run("it", false, true)];
        assert_eq!(inline_lines(&runs), ["plain **bold** and *it*"]);
    }

    #[test]
    fn markdown_characters_in_text_are_escaped() {
        assert_eq!(inline_lines(&[run("a *star* and [link](x) <tag>", false, false)]), [r"a \*star\* and \[link\](x) \<tag\>"]);
        assert_eq!(inline_lines(&[run("# not a heading", false, false)]), [r"\# not a heading"]);
        assert_eq!(inline_lines(&[run("1984. A year", false, false)]), [r"1984\. A year"]);
        assert_eq!(inline_lines(&[run("- not a list", false, false)]), [r"\- not a list"]);
    }

    #[test]
    fn line_breaks_split_lines_but_source_newlines_do_not() {
        let runs = [run("one\n   two", false, false), Run { text: "\n".into(), bold: false, italic: false, line_break: true }, run("three", false, false)];
        assert_eq!(inline_lines(&runs), ["one two", "three"]);
    }

    #[test]
    fn code_fences_are_longer_than_any_backticks_inside() {
        let block = Block { kind: BlockKind::Preformatted, runs: vec![run("let s = ```;\n*not emphasis*", false, false)] };
        assert_eq!(render_block(&block).unwrap(), "````\nlet s = ```;\n*not emphasis*\n````");
    }

    #[test]
    fn link_paths_are_encoded() {
        assert_eq!(link_encode("My Book (1)_images"), "My%20Book%20%281%29_images");
    }
}
