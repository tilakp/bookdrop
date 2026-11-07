//! Classifies lines into headings and paragraphs from font size, gaps and indents.

use super::*;

pub(super) struct ClassifiedBlocks {
    pub(super) blocks: Vec<Block>,
    pub(super) low_confidence: bool,
}


// Calibrated up from an initial 1.15 after a real fixture (heading-tiers.pdf,
// body text ~9.5pt averaged per-line) produced a false-positive heading: a
// paragraph's own last line happened to average to 11pt (~1.16x body) purely
// from per-line font-size-averaging noise, not a real heading. Real headings
// in the same fixture measured 1.47x and 2.53x body size - comfortably
// above a stricter cutoff, so 1.3 removes the false positive without
// touching genuine headings.
const HEADING_SIZE_RATIO: f32 = 1.3;
const HEADING_CHAR_FRACTION_GUARD: f32 = 0.20;
const MAX_HEADING_TIERS: usize = 3;
const PARAGRAPH_GAP_MULTIPLIER: f32 = 1.5;
const INDENT_MULTIPLIER: f32 = 1.0;
const SHORT_LINE_FRACTION: f32 = 0.80;

// The macro's final `flush_paragraph!()` call resets the accumulator
// variables one last time right before the function returns them unused -
// harmless, but the compiler can't see that from inside a macro_rules!
// expansion, hence the blanket allow rather than restructuring the macro
// just to dodge a false positive.
#[allow(unused_assignments)]
pub(super) fn classify_blocks(lines: &[Line]) -> ClassifiedBlocks {
    if lines.is_empty() {
        return ClassifiedBlocks { blocks: Vec::new(), low_confidence: true };
    }

    let body_size = body_font_size(lines);
    let (tiers, low_confidence) = heading_tiers(lines, body_size);

    let mut blocks: Vec<Block> = Vec::new();
    let mut anchor_counter = 0usize;
    let mut paragraph_lines: Vec<&Line> = Vec::new();
    let mut paragraph_left_margin = f32::MAX;
    let mut paragraph_right_edges: Vec<f32> = Vec::new();

    macro_rules! flush_paragraph {
        () => {
            if !paragraph_lines.is_empty() {
                let text = join_paragraph_lines(&paragraph_lines);
                if !text.trim().is_empty() {
                    let anchor = format!("b{anchor_counter}");
                    anchor_counter += 1;
                    blocks.push(Block { kind: BlockKind::Paragraph, text, page: paragraph_lines[0].page, anchor });
                }
                paragraph_lines.clear();
                paragraph_left_margin = f32::MAX;
                paragraph_right_edges.clear();
            }
        };
    }

    let median_gap = median_gap(lines);
    let mut prev_line: Option<&Line> = None;
    for line in lines {
        if line.text.trim().is_empty() {
            continue;
        }

        if let Some(tier) = tiers.iter().position(|&s| (s - line.size).abs() < 0.01) {
            flush_paragraph!();
            let anchor = format!("b{anchor_counter}");
            anchor_counter += 1;
            blocks.push(Block {
                kind: BlockKind::Heading((tier + 1) as u8),
                text: line.text.trim().to_string(),
                page: line.page,
                anchor,
            });
            prev_line = None;
            continue;
        }

        let mut break_before = false;
        if let Some(prev) = prev_line {
            let gap = prev.y_bottom - line.y_top;
            if prev.page != line.page {
                break_before = false; // cross-page handled by caller
            } else if gap > PARAGRAPH_GAP_MULTIPLIER * median_gap
                || line.x0 > paragraph_left_margin + INDENT_MULTIPLIER * line.size
            {
                break_before = true;
            } else if !paragraph_right_edges.is_empty() {
                let median_right = median(paragraph_right_edges.iter().copied());
                if prev.x1 < median_right * SHORT_LINE_FRACTION && (line.x0 - paragraph_left_margin).abs() < line.size {
                    break_before = true;
                }
            }
        }

        if break_before {
            flush_paragraph!();
        }

        if paragraph_lines.is_empty() {
            paragraph_left_margin = line.x0;
        } else {
            paragraph_left_margin = paragraph_left_margin.min(line.x0);
        }
        paragraph_right_edges.push(line.x1);
        paragraph_lines.push(line);
        prev_line = Some(line);
    }
    flush_paragraph!();

    ClassifiedBlocks { blocks, low_confidence }
}

pub(super) fn join_paragraph_lines(lines: &[&Line]) -> String {
    let mut out = String::new();
    for line in lines {
        let trimmed = line.text.trim();
        if trimmed.is_empty() {
            continue;
        }
        if out.ends_with('-') && trimmed.chars().next().is_some_and(|c| c.is_lowercase()) {
            out.pop();
            out.push_str(trimmed);
        } else if !out.is_empty() {
            out.push(' ');
            out.push_str(trimmed);
        } else {
            out.push_str(trimmed);
        }
    }
    out
}

pub(super) fn body_font_size(lines: &[Line]) -> f32 {
    let mut weight_by_size: HashMap<i32, u32> = HashMap::new();
    for line in lines {
        let bucket = (line.size * 2.0).round() as i32;
        *weight_by_size.entry(bucket).or_insert(0) += line.text.chars().count() as u32;
    }
    weight_by_size
        .into_iter()
        .max_by_key(|&(_, weight)| weight)
        .map(|(bucket, _)| bucket as f32 / 2.0)
        .unwrap_or(0.0)
}

/// Returns up to `MAX_HEADING_TIERS` distinct sizes (descending) that
/// qualify as headings, and whether the heading signal is too weak/absent
/// to trust: when more than `HEADING_CHAR_FRACTION_GUARD` of the text is
/// heading-sized (a large-print book, for example), no size is trusted.
pub(super) fn heading_tiers(lines: &[Line], body_size: f32) -> (Vec<f32>, bool) {
    let mut weight_by_size: HashMap<i32, u32> = HashMap::new();
    let mut total_chars = 0u32;
    for line in lines {
        let n = line.text.chars().count() as u32;
        total_chars += n;
        if line.size > body_size * HEADING_SIZE_RATIO {
            let bucket = (line.size * 2.0).round() as i32;
            *weight_by_size.entry(bucket).or_insert(0) += n;
        }
    }

    if total_chars == 0 {
        return (Vec::new(), true);
    }

    let heading_chars: u32 = weight_by_size.values().sum();
    if heading_chars == 0 {
        return (Vec::new(), true);
    }
    if (heading_chars as f32) / (total_chars as f32) > HEADING_CHAR_FRACTION_GUARD {
        return (Vec::new(), true);
    }

    let mut sizes: Vec<f32> = weight_by_size.keys().map(|&b| b as f32 / 2.0).collect();
    sizes.sort_by(|a, b| b.partial_cmp(a).unwrap());
    sizes.truncate(MAX_HEADING_TIERS);
    (sizes, false)
}

fn median_gap(lines: &[Line]) -> f32 {
    let gaps: Vec<f32> = lines
        .windows(2)
        .filter(|w| w[0].page == w[1].page)
        .map(|w| (w[0].y_bottom - w[1].y_top).max(0.0))
        .filter(|g| *g > 0.0)
        .collect();
    if gaps.is_empty() {
        return 1.0;
    }
    median(gaps.into_iter())
}
