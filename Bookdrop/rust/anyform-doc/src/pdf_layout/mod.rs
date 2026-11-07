//! Layout reconstruction for PDF input: glyphs -> lines -> blocks ->
//! chapters. It works only on plain owned data built by `pdfium.rs`, with
//! no FFI and no I/O, so every heuristic is unit-testable with hand-built
//! fixtures. Its `BlockKind`/`Block` types are separate from `htmltext.rs`:
//! a PDF has no markup to recover lists, quotes or bold runs from.
//!
//! The threshold constants are starting points calibrated on the fixtures
//! in `tests/fixtures/`, not derived values. `pdfium.rs` removes control
//! characters first, so every `Glyph` has a printable character and a box.

use std::collections::HashMap;

use anyform_core::Log;

mod blocks;
mod chapters;
mod columns;
mod lines;
mod running_heads;
#[cfg(test)]
mod tests;
mod xhtml;

use blocks::{classify_blocks, ClassifiedBlocks};
use chapters::split_chapters;
use columns::detect_columns;
pub(crate) use lines::group_lines;
use running_heads::strip_running_heads;
pub(crate) use xhtml::render_chapter_xhtml;

// `font_name`/`italic` (Glyph), `bold_ratio` (Line), and `width` (PageText)
// are all populated for real by pdfium.rs (pdfium exposes them cheaply
// alongside the fields v1's heuristics do use) but not yet read by any
// current classify_blocks/detect_columns logic - kept as part of the
// vocabulary rather than dropped, since a v1.1 pass (bold-weighted heading
// detection, italic-run emphasis, width-relative margin checks) is the
// obvious next refinement and would otherwise have to re-plumb them
// through pdfium.rs from scratch.
#[derive(Debug, Clone)]
pub(crate) struct Glyph {
    pub ch: char,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub font_size: f32,
    #[allow(dead_code)]
    pub font_name: String,
    pub bold: bool,
    #[allow(dead_code)]
    pub italic: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct Line {
    pub text: String,
    pub x0: f32,
    pub x1: f32,
    pub y_top: f32,
    pub y_bottom: f32,
    pub size: f32,
    #[allow(dead_code)]
    pub bold_ratio: f32,
    pub page: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct PageText {
    pub index: usize,
    #[allow(dead_code)]
    pub width: f32,
    pub height: f32,
    pub glyphs: Vec<Glyph>,
    pub image_area_ratio: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlockKind {
    Heading(u8),
    Paragraph,
}

#[derive(Debug, Clone)]
pub(crate) struct Block {
    pub kind: BlockKind,
    pub text: String,
    pub page: usize,
    pub anchor: String,
}

#[derive(Debug, Clone)]
pub(crate) struct Chapter {
    pub title: String,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone)]
pub(crate) struct OutlineEntry {
    pub title: String,
    pub page_index: Option<usize>,
    pub children: Vec<OutlineEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(clippy::enum_variant_names)] // "SingleColumn/TwoColumn/MultiColumn" reads clearer than dropping the shared word
pub(crate) enum ColumnVerdict {
    SingleColumn,
    TwoColumn { gutter_x: f32 },
    MultiColumn(usize),
}

fn median(values: impl Iterator<Item = f32>) -> f32 {
    let mut v: Vec<f32> = values.filter(|x| x.is_finite() && *x > 0.0).collect();
    if v.is_empty() {
        return 1.0;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

// ---- Orchestration entry point used by pdf_input.rs ----
/// Runs the whole pipeline (lines, running heads, columns, blocks,
/// chapters) on extracted glyph data. Returns `Err` only when the layout is
/// multi-column. Every other problem (no headings, too many headings,
/// oversized chapters) falls back to a simpler split instead of failing.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_chapters(
    pages: &[PageText],
    outline: &[OutlineEntry],
    fallback_title: &str,
    force_single_column: bool,
    log: &dyn Log,
) -> Result<Vec<Chapter>, ColumnVerdict> {
    let mut page_lines: Vec<Vec<Line>> = pages.iter().map(group_lines).collect();
    let heights: Vec<f32> = pages.iter().map(|p| p.height).collect();

    strip_running_heads(&mut page_lines, &heights, log);

    if !force_single_column {
        let verdict = detect_columns(pages);
        if verdict != ColumnVerdict::SingleColumn {
            return Err(verdict);
        }
    }

    let mut all_blocks = Vec::new();
    let mut any_low_confidence = false;
    let mut prev_block_end: Option<(usize, bool)> = None; // (page, ends_with_terminal_punct)

    for lines in &page_lines {
        let ClassifiedBlocks { blocks, low_confidence } = classify_blocks(lines);
        any_low_confidence |= low_confidence && !blocks.is_empty();

        for block in blocks {
            if let (Some((prev_page, ends_terminal)), BlockKind::Paragraph) = (prev_block_end, block.kind) {
                let starts_lowercase = block.text.chars().next().is_some_and(|c| c.is_lowercase());
                if !ends_terminal && starts_lowercase {
                    if let Some(last) = all_blocks.last_mut() {
                        let last: &mut Block = last;
                        if last.page == prev_page && matches!(last.kind, BlockKind::Paragraph) {
                            last.text.push(' ');
                            last.text.push_str(&block.text);
                            prev_block_end = Some((block.page, ends_with_terminal_punctuation(&last.text)));
                            continue;
                        }
                    }
                }
            }
            prev_block_end = Some((block.page, ends_with_terminal_punctuation(&block.text)));
            all_blocks.push(block);
        }
    }

    let low_confidence_overall = all_blocks.is_empty() || any_low_confidence && all_blocks.iter().all(|b| matches!(b.kind, BlockKind::Paragraph));
    let chapters = split_chapters(all_blocks, low_confidence_overall, outline, fallback_title, log);
    Ok(chapters)
}

fn ends_with_terminal_punctuation(text: &str) -> bool {
    matches!(text.trim_end().chars().last(), Some('.') | Some('!') | Some('?') | Some('"') | Some('\u{2019}') | Some('\u{201d}'))
}
