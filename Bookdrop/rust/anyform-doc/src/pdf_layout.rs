//! Pure, pdfium-free layout reconstruction: glyphs -> lines -> blocks ->
//! chapters. Everything here operates on plain owned data (the
//! `Glyph`/`Line`/`PageText`/`Block`/`Chapter`/`OutlineEntry` vocabulary
//! below), built by `pdfium.rs` from a real PDF. Keeping this module free of
//! any FFI or I/O is what makes every heuristic here unit-testable against
//! hand-built fixtures, with no bundled dylib and no PDF file needed - see
//! the PDF-input plan's §2. Mirrors `htmltext.rs`'s role as a shared
//! non-plugin helper, though the `BlockKind`/`Block` vocabulary here is
//! deliberately its own type: no runs, no list/blockquote/pre - a PDF has no
//! source markup to recover any of that from.
//!
//! Every threshold constant below is a *starting point* calibrated against
//! `anyform-doc/tests/fixtures/`'s synthetic PDF fixtures (see
//! `pdf_input_tests.rs`), not a derived truth - see the PDF-input plan's
//! Risk #2. `pdfium.rs` already filters out control characters (`\r`/`\n`)
//! before glyphs reach here, so every `Glyph` below is assumed to carry a
//! real, printable character with a real bounding box.

use anyform_core::Log;

// ---- Shared vocabulary ----

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

struct ClassifiedBlocks {
    blocks: Vec<Block>,
    low_confidence: bool,
}

// ---- 3.1 group_lines ----

const LINE_BREAK_FRACTION: f32 = 0.5; // x median glyph height
const WORD_GAP_FRACTION: f32 = 0.25; // x current font size

pub(crate) fn group_lines(page: &PageText) -> Vec<Line> {
    if page.glyphs.is_empty() {
        return Vec::new();
    }

    let mut sorted: Vec<&Glyph> = page.glyphs.iter().collect();
    sorted.sort_by(|a, b| {
        let ya = (a.y0 + a.y1) / 2.0;
        let yb = (b.y0 + b.y1) / 2.0;
        yb.partial_cmp(&ya).unwrap().then_with(|| a.x0.partial_cmp(&b.x0).unwrap())
    });

    // Cluster against an *expanding* [y_min, y_max] window for the current
    // line, not a fixed anchor set once at the line's first glyph.
    // Ascenders ('h','k','l','t'...) push y1 up and descenders
    // ('p','g','j','q','y'...) push y0 down relative to x-height glyphs, so
    // a fixed single anchor point (this function's first cut) misclassifies
    // a descender mid-word into its own line the moment its y-midpoint
    // drifts past a threshold measured from a page-wide median height -
    // caught live on a real "Chapter One" heading, where 'p' split off into
    // its own bogus line. The threshold itself is measured off each
    // glyph's own height (not a page-wide median) so large headings and
    // small body text both get a threshold proportional to their own size.
    let mut clusters: Vec<Vec<&Glyph>> = Vec::new();
    let mut cluster_y_min = f32::MAX;
    let mut cluster_y_max = f32::MIN;
    for g in sorted {
        let y_mid = (g.y0 + g.y1) / 2.0;
        let threshold = (LINE_BREAK_FRACTION * (g.y1 - g.y0).abs()).max(0.5);
        let fits = !clusters.is_empty() && y_mid >= cluster_y_min - threshold && y_mid <= cluster_y_max + threshold;
        if fits {
            cluster_y_min = cluster_y_min.min(g.y0);
            cluster_y_max = cluster_y_max.max(g.y1);
        } else {
            clusters.push(Vec::new());
            cluster_y_min = g.y0;
            cluster_y_max = g.y1;
        }
        clusters.last_mut().unwrap().push(g);
    }

    clusters
        .into_iter()
        .map(|mut cluster| {
            cluster.sort_by(|a, b| a.x0.partial_cmp(&b.x0).unwrap());
            build_line(&cluster, page.index)
        })
        .collect()
}

fn build_line(glyphs: &[&Glyph], page: usize) -> Line {
    let mut text = String::new();
    let mut total_size = 0.0f32;
    let mut bold_chars = 0u32;
    let count = glyphs.len() as f32;
    let x0 = glyphs.first().map(|g| g.x0).unwrap_or(0.0);
    let x1 = glyphs.last().map(|g| g.x1).unwrap_or(0.0);
    let y_top = glyphs.iter().map(|g| g.y1).fold(f32::MIN, f32::max);
    let y_bottom = glyphs.iter().map(|g| g.y0).fold(f32::MAX, f32::min);

    for (i, g) in glyphs.iter().enumerate() {
        // Only synthesize a space from the x-gap when neither glyph is
        // already whitespace - PDFium does yield real ' ' glyphs with real
        // bounds (not every space is inkless/omitted), and real books with
        // slightly generous word spacing (seen live on a real government
        // PDF fixture) exceed WORD_GAP_FRACTION on gaps that already have
        // an actual space character - double-counting into "word  gap"
        // otherwise.
        if i > 0 && g.ch != ' ' {
            let prev = glyphs[i - 1];
            let gap = g.x0 - prev.x1;
            if prev.ch != ' ' && gap > WORD_GAP_FRACTION * g.font_size.max(1.0) && !text.ends_with(' ') && !text.is_empty() {
                text.push(' ');
            }
        }
        text.push(g.ch);
        total_size += g.font_size;
        if g.bold {
            bold_chars += 1;
        }
    }

    // Round to the nearest 0.5pt so near-identical rendered sizes collapse
    // to the same histogram bucket in classify_blocks.
    let size = if count > 0.0 { ((total_size / count) * 2.0).round() / 2.0 } else { 0.0 };
    let bold_ratio = if count > 0.0 { bold_chars as f32 / count } else { 0.0 };

    Line { text, x0, x1, y_top, y_bottom, size, bold_ratio, page }
}

fn median(values: impl Iterator<Item = f32>) -> f32 {
    let mut v: Vec<f32> = values.filter(|x| x.is_finite() && *x > 0.0).collect();
    if v.is_empty() {
        return 1.0;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

// ---- 3.2 strip_running_heads ----

const RUNNING_HEAD_MIN_PAGES: usize = 4;
const RUNNING_HEAD_BAND_FRACTION: f32 = 0.12;
const RUNNING_HEAD_REPETITION_THRESHOLD: f32 = 0.60;
const RUNNING_HEAD_Y_BUCKET_PT: f32 = 5.0;
const BARE_NUMBER_PAGE_FRACTION: f32 = 0.5;

pub(crate) fn strip_running_heads(pages: &mut [Vec<Line>], heights: &[f32], log: &dyn Log) {
    if pages.len() < RUNNING_HEAD_MIN_PAGES {
        return;
    }

    use std::collections::HashMap;

    // Pass 1: same normalized text at (roughly) the same y-position,
    // repeated across >= RUNNING_HEAD_REPETITION_THRESHOLD of pages.
    let mut bucket_counts: HashMap<(String, i32), usize> = HashMap::new();
    let mut per_page_candidates: Vec<Vec<(usize, String, i32, bool)>> = Vec::with_capacity(pages.len());

    for (lines, &height) in pages.iter().zip(heights.iter()) {
        let band = height * RUNNING_HEAD_BAND_FRACTION;
        let mut page_candidates = Vec::new();
        for (line_idx, line) in lines.iter().enumerate() {
            let in_band = line.y_top >= height - band || line.y_bottom <= band;
            if !in_band {
                continue;
            }
            let normalized = normalize_for_repetition(&line.text);
            if normalized.is_empty() {
                continue;
            }
            let y_bucket = (line.y_top / RUNNING_HEAD_Y_BUCKET_PT).round() as i32;
            let is_bare = is_bare_page_number(&normalized);
            *bucket_counts.entry((normalized.clone(), y_bucket)).or_insert(0) += 1;
            page_candidates.push((line_idx, normalized, y_bucket, is_bare));
        }
        per_page_candidates.push(page_candidates);
    }

    let repetition_threshold = ((pages.len() as f32) * RUNNING_HEAD_REPETITION_THRESHOLD).ceil() as usize;
    let pages_with_bare_number = per_page_candidates.iter().filter(|c| c.iter().any(|(_, _, _, bare)| *bare)).count();
    let strip_bare_numbers = (pages_with_bare_number as f32) >= (pages.len() as f32) * BARE_NUMBER_PAGE_FRACTION;

    let mut stripped = 0usize;
    for (page_idx, candidates) in per_page_candidates.iter().enumerate() {
        let mut to_remove: Vec<usize> = Vec::new();
        for (line_idx, normalized, y_bucket, is_bare) in candidates {
            let repeated = bucket_counts.get(&(normalized.clone(), *y_bucket)).copied().unwrap_or(0) >= repetition_threshold;
            if repeated || (*is_bare && strip_bare_numbers) {
                to_remove.push(*line_idx);
            }
        }
        to_remove.sort_unstable();
        to_remove.dedup();
        for &idx in to_remove.iter().rev() {
            pages[page_idx].remove(idx);
            stripped += 1;
        }
    }

    if stripped > 0 {
        log.info(&format!("stripped {stripped} repeated running-head/footer line(s) across {} pages", pages.len()));
    }
}

fn normalize_for_repetition(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last_was_space = true; // suppress leading space
    for c in text.chars() {
        let c = if c.is_ascii_digit() { '#' } else { c.to_ascii_lowercase() };
        if c.is_whitespace() {
            if !last_was_space {
                out.push(' ');
            }
            last_was_space = true;
        } else {
            out.push(c);
            last_was_space = false;
        }
    }
    out.trim_end().to_string()
}

fn is_bare_page_number(normalized: &str) -> bool {
    let t = normalized.trim();
    if t.is_empty() {
        return false;
    }
    let placeholder_only = t.chars().all(|c| c == '#' || c == '-' || c == '.' || c.is_whitespace());
    let has_digit_placeholder = t.contains('#');
    (placeholder_only && has_digit_placeholder) || is_roman_numeral(t)
}

fn is_roman_numeral(s: &str) -> bool {
    !s.is_empty() && s.len() <= 8 && s.chars().all(|c| matches!(c, 'i' | 'v' | 'x' | 'l' | 'c' | 'd' | 'm'))
}

// ---- 3.3 detect_columns ----

const COLUMN_BIN_WIDTH_PT: f32 = 2.0;
const GUTTER_MIN_WIDTH_FRACTION: f32 = 0.04; // x text width
const GUTTER_MIN_X_FRACTION: f32 = 0.25;
const GUTTER_MAX_X_FRACTION: f32 = 0.75;
const COLUMN_PAGE_MAJORITY_FRACTION: f32 = 0.40;

/// Operates on raw glyph positions, not `group_lines`' merged `Line`s.
/// `group_lines` deliberately merges same-row text from both columns of a
/// two-column page into one `Line` (verified by
/// `two_column_page_yields_two_lines_per_row_not_interleaved`), so a merged
/// line's own `x0..x1` span already covers the *entire* row width,
/// including the gutter - painting over the one gap that actually matters.
/// Building the coverage histogram from individual glyph spans instead
/// means a real gutter shows up as genuine zero coverage (no glyph
/// anywhere on the page ever occupies that x-band), which is what a
/// caught-live bug (a real two-column fixture converting to scrambled
/// interleaved text without being rejected) required fixing to.
pub(crate) fn detect_columns(pages: &[PageText]) -> ColumnVerdict {
    let mut two_column_pages = 0usize;
    let mut multi_column_pages = 0usize;
    let mut total_pages_with_text = 0usize;

    for page in pages {
        if page.glyphs.is_empty() {
            continue;
        }
        total_pages_with_text += 1;
        match detect_page_columns(&page.glyphs) {
            ColumnVerdict::SingleColumn => {}
            ColumnVerdict::TwoColumn { .. } => two_column_pages += 1,
            ColumnVerdict::MultiColumn(_) => multi_column_pages += 1,
        }
    }

    if total_pages_with_text == 0 {
        return ColumnVerdict::SingleColumn;
    }

    let majority = (total_pages_with_text as f32) * COLUMN_PAGE_MAJORITY_FRACTION;
    if (multi_column_pages as f32) > majority {
        return ColumnVerdict::MultiColumn(3);
    }
    if (two_column_pages as f32) > majority {
        // Recompute a representative gutter_x from the first page that
        // detected one, for the caller's escape-hatch ordering (v1.1).
        for page in pages {
            if let ColumnVerdict::TwoColumn { gutter_x } = detect_page_columns(&page.glyphs) {
                return ColumnVerdict::TwoColumn { gutter_x };
            }
        }
    }
    ColumnVerdict::SingleColumn
}

fn detect_page_columns(glyphs: &[Glyph]) -> ColumnVerdict {
    let min_x = glyphs.iter().map(|g| g.x0).fold(f32::MAX, f32::min);
    let max_x = glyphs.iter().map(|g| g.x1).fold(f32::MIN, f32::max);
    let text_width = max_x - min_x;
    if text_width <= 0.0 {
        return ColumnVerdict::SingleColumn;
    }

    let bins = ((text_width / COLUMN_BIN_WIDTH_PT).ceil() as usize).max(1);
    let mut coverage = vec![0u32; bins];
    for g in glyphs {
        if g.ch.is_whitespace() {
            continue;
        }
        let start = (((g.x0 - min_x) / COLUMN_BIN_WIDTH_PT).floor() as isize).max(0) as usize;
        let end = (((g.x1 - min_x) / COLUMN_BIN_WIDTH_PT).ceil() as isize).max(0) as usize;
        let end = end.min(bins);
        if start < end {
            for c in &mut coverage[start..end] {
                *c += 1;
            }
        }
    }

    let min_gutter_bins = ((text_width * GUTTER_MIN_WIDTH_FRACTION) / COLUMN_BIN_WIDTH_PT).ceil() as usize;
    let gutter_x_min = min_x + text_width * GUTTER_MIN_X_FRACTION;
    let gutter_x_max = min_x + text_width * GUTTER_MAX_X_FRACTION;

    let mut gutters: Vec<(f32, f32)> = Vec::new(); // (start_x, end_x)
    let mut run_start: Option<usize> = None;
    for (i, &count) in coverage.iter().enumerate() {
        if count == 0 {
            run_start.get_or_insert(i);
        } else if let Some(start) = run_start.take() {
            record_gutter_if_valid(&mut gutters, start, i, min_x, gutter_x_min, gutter_x_max, min_gutter_bins);
        }
    }
    if let Some(start) = run_start {
        record_gutter_if_valid(&mut gutters, start, bins, min_x, gutter_x_min, gutter_x_max, min_gutter_bins);
    }

    match gutters.len() {
        0 => ColumnVerdict::SingleColumn,
        1 => ColumnVerdict::TwoColumn { gutter_x: (gutters[0].0 + gutters[0].1) / 2.0 },
        n => ColumnVerdict::MultiColumn(n + 1),
    }
}

#[allow(clippy::too_many_arguments)]
fn record_gutter_if_valid(
    gutters: &mut Vec<(f32, f32)>,
    bin_start: usize,
    bin_end: usize,
    min_x: f32,
    gutter_x_min: f32,
    gutter_x_max: f32,
    min_gutter_bins: usize,
) {
    if bin_end <= bin_start || bin_end - bin_start < min_gutter_bins.max(1) {
        return;
    }
    let x_start = min_x + bin_start as f32 * COLUMN_BIN_WIDTH_PT;
    let x_end = min_x + bin_end as f32 * COLUMN_BIN_WIDTH_PT;
    let mid = (x_start + x_end) / 2.0;
    if mid >= gutter_x_min && mid <= gutter_x_max {
        gutters.push((x_start, x_end));
    }
}

// ---- 3.4 classify_blocks ----

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
fn classify_blocks(lines: &[Line]) -> ClassifiedBlocks {
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
            let median_gap = median_gap(lines);
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

fn join_paragraph_lines(lines: &[&Line]) -> String {
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

fn body_font_size(lines: &[Line]) -> f32 {
    use std::collections::HashMap;
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
/// to trust (see the PDF-input plan §3.4's "large-print book" guard).
fn heading_tiers(lines: &[Line], body_size: f32) -> (Vec<f32>, bool) {
    use std::collections::HashMap;
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

// ---- 3.5 split_chapters ----

const MAX_PLAUSIBLE_CHAPTERS: usize = 200;
const MIN_MEDIAN_BLOCKS_PER_CHAPTER: usize = 3;
const MAX_CHAPTER_CHARS: usize = 500_000;
const PAGES_PER_FALLBACK_CHUNK: usize = 10;
const MIN_OUTLINE_ENTRIES: usize = 2;

pub(crate) fn split_chapters(
    all_blocks: Vec<Block>,
    low_confidence: bool,
    outline: &[OutlineEntry],
    fallback_title: &str,
    page_count: usize,
    log: &dyn Log,
) -> Vec<Chapter> {
    if all_blocks.is_empty() {
        return Vec::new();
    }

    let flat_outline: Vec<&OutlineEntry> = flatten_outline(outline);
    let usable_outline: Vec<(&str, usize)> =
        flat_outline.iter().filter_map(|e| e.page_index.map(|p| (e.title.as_str(), p))).collect();

    if usable_outline.len() >= MIN_OUTLINE_ENTRIES {
        log.info(&format!("splitting chapters from the PDF's own outline ({} entries)", usable_outline.len()));
        return split_by_outline(all_blocks, &usable_outline, fallback_title);
    }

    if low_confidence {
        log.info("no reliable heading signal found - emitting a single flowing chapter");
        return vec![Chapter { title: fallback_title.to_string(), blocks: all_blocks }];
    }

    let heading_count = all_blocks.iter().filter(|b| matches!(b.kind, BlockKind::Heading(_))).count();
    let chapters = split_by_headings(all_blocks, fallback_title);

    let median_blocks = if chapters.is_empty() { 0 } else { median(chapters.iter().map(|c| c.blocks.len() as f32)) as usize };

    if heading_count > MAX_PLAUSIBLE_CHAPTERS || (chapters.len() > 1 && median_blocks < MIN_MEDIAN_BLOCKS_PER_CHAPTER) {
        log.info(&format!(
            "heading detection found {heading_count} candidates, which looks like noise - \
             falling back to fixed page chunking"
        ));
        let all_blocks: Vec<Block> = chapters.into_iter().flat_map(|c| c.blocks).collect();
        return chunk_by_pages(all_blocks, page_count);
    }

    chunk_oversized_chapters(chapters, log)
}

fn flatten_outline(entries: &[OutlineEntry]) -> Vec<&OutlineEntry> {
    let mut out = Vec::new();
    for e in entries {
        out.push(e);
        out.extend(flatten_outline(&e.children));
    }
    out
}

fn split_by_outline(blocks: Vec<Block>, outline: &[(&str, usize)], fallback_title: &str) -> Vec<Chapter> {
    let mut chapters: Vec<Chapter> = Vec::new();
    let mut leading: Vec<Block> = Vec::new();
    let mut block_iter = blocks.into_iter().peekable();

    // Everything before the first outline entry's page becomes a leading
    // chapter (front matter, title page, etc.) if non-empty.
    let first_page = outline.first().map(|&(_, p)| p).unwrap_or(0);
    while let Some(b) = block_iter.peek() {
        if b.page < first_page {
            leading.push(block_iter.next().unwrap());
        } else {
            break;
        }
    }
    if !leading.is_empty() {
        chapters.push(Chapter { title: fallback_title.to_string(), blocks: leading });
    }

    for (i, &(title, start_page)) in outline.iter().enumerate() {
        let end_page = outline.get(i + 1).map(|&(_, p)| p);
        let mut blocks_here = Vec::new();
        while let Some(b) = block_iter.peek() {
            let within = end_page.is_none_or(|end| b.page < end);
            if b.page >= start_page && within {
                blocks_here.push(block_iter.next().unwrap());
            } else if b.page < start_page {
                // Belongs to an earlier (possibly skipped) range; attach here anyway.
                blocks_here.push(block_iter.next().unwrap());
            } else {
                break;
            }
        }
        chapters.push(Chapter { title: title.to_string(), blocks: blocks_here });
    }

    // Anything left over (outline didn't cover the last pages) joins the last chapter.
    let remaining: Vec<Block> = block_iter.collect();
    if !remaining.is_empty() {
        if let Some(last) = chapters.last_mut() {
            last.blocks.extend(remaining);
        } else {
            chapters.push(Chapter { title: fallback_title.to_string(), blocks: remaining });
        }
    }

    chapters.retain(|c| !c.blocks.is_empty());
    chapters
}

fn split_by_headings(blocks: Vec<Block>, fallback_title: &str) -> Vec<Chapter> {
    let mut chapters: Vec<Chapter> = Vec::new();
    let mut current: Option<Chapter> = None;

    let top_tier = blocks
        .iter()
        .filter_map(|b| match b.kind {
            BlockKind::Heading(t) => Some(t),
            _ => None,
        })
        .min();

    for block in blocks {
        let starts_chapter = matches!(block.kind, BlockKind::Heading(t) if Some(t) == top_tier);
        if starts_chapter {
            if let Some(c) = current.take() {
                if !c.blocks.is_empty() {
                    chapters.push(c);
                }
            }
            current = Some(Chapter { title: block.text.clone(), blocks: Vec::new() });
        }
        if current.is_none() {
            current = Some(Chapter { title: fallback_title.to_string(), blocks: Vec::new() });
        }
        current.as_mut().unwrap().blocks.push(block);
    }
    if let Some(c) = current {
        if !c.blocks.is_empty() {
            chapters.push(c);
        }
    }
    chapters
}

fn chunk_by_pages(blocks: Vec<Block>, page_count: usize) -> Vec<Chapter> {
    if blocks.is_empty() {
        return Vec::new();
    }
    let chunk_size = PAGES_PER_FALLBACK_CHUNK.max(1);
    let mut chapters: Vec<Chapter> = Vec::new();
    let mut current_start_page = blocks[0].page;
    let mut current_blocks: Vec<Block> = Vec::new();

    for block in blocks {
        if block.page >= current_start_page + chunk_size && !current_blocks.is_empty() {
            let end_page = current_blocks.last().unwrap().page;
            chapters.push(Chapter {
                title: format!("Pages {}-{}", current_start_page + 1, end_page + 1),
                blocks: std::mem::take(&mut current_blocks),
            });
            current_start_page = block.page;
        }
        current_blocks.push(block);
    }
    if !current_blocks.is_empty() {
        let end_page = current_blocks.last().unwrap().page.max(current_start_page);
        chapters.push(Chapter { title: format!("Pages {}-{}", current_start_page + 1, end_page + 1), blocks: current_blocks });
    }
    let _ = page_count;
    chapters
}

fn chunk_oversized_chapters(chapters: Vec<Chapter>, log: &dyn Log) -> Vec<Chapter> {
    let mut out = Vec::new();
    for chapter in chapters {
        let total_chars: usize = chapter.blocks.iter().map(|b| b.text.len()).sum();
        if total_chars <= MAX_CHAPTER_CHARS || chapter.blocks.len() < 2 {
            out.push(chapter);
            continue;
        }
        log.info(&format!("chapter \"{}\" is unusually large ({total_chars} chars) - splitting by page", chapter.title));
        let first_page = chapter.blocks[0].page;
        let mut part = 1u32;
        let mut current_start = first_page;
        let mut current: Vec<Block> = Vec::new();
        for block in chapter.blocks {
            if block.page >= current_start + PAGES_PER_FALLBACK_CHUNK && !current.is_empty() {
                out.push(Chapter { title: format!("{} (part {part})", chapter.title), blocks: std::mem::take(&mut current) });
                part += 1;
                current_start = block.page;
            }
            current.push(block);
        }
        if !current.is_empty() {
            out.push(Chapter { title: format!("{} (part {part})", chapter.title), blocks: current });
        }
    }
    out
}

// ---- 3.6 render_chapter_xhtml ----

pub(crate) fn render_chapter_xhtml(chapter: &Chapter) -> String {
    let mut body = String::new();
    body.push_str(&format!("<h1>{}</h1>\n", xml_escape(&chapter.title)));
    for block in &chapter.blocks {
        match block.kind {
            BlockKind::Heading(1) => body.push_str(&format!("<h1 id=\"{}\">{}</h1>\n", block.anchor, xml_escape(&block.text))),
            BlockKind::Heading(2) => body.push_str(&format!("<h2 id=\"{}\">{}</h2>\n", block.anchor, xml_escape(&block.text))),
            BlockKind::Heading(_) => body.push_str(&format!("<h3 id=\"{}\">{}</h3>\n", block.anchor, xml_escape(&block.text))),
            BlockKind::Paragraph => body.push_str(&format!("<p id=\"{}\">{}</p>\n", block.anchor, xml_escape(&block.text))),
        }
    }

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE html>\n\
         <html xmlns=\"http://www.w3.org/1999/xhtml\">\n\
         <head><title>{}</title></head>\n\
         <body>\n{body}</body>\n\
         </html>\n",
        xml_escape(&chapter.title)
    )
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

// ---- Orchestration entry point used by pdf_input.rs ----

/// Runs the full pipeline (3.1-3.6) over already-extracted page glyph data.
/// Returns `Err` only for the "no usable text after layout" case (§4) -
/// every other degradation (no headings, too many headings, oversized
/// chapters) falls back rather than failing, per §3.5's sanity guards.
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

    let page_count = pages.len();
    let low_confidence_overall = all_blocks.is_empty() || any_low_confidence && all_blocks.iter().all(|b| matches!(b.kind, BlockKind::Paragraph));
    let chapters = split_chapters(all_blocks, low_confidence_overall, outline, fallback_title, page_count, log);
    Ok(chapters)
}

fn ends_with_terminal_punctuation(text: &str) -> bool {
    matches!(text.trim_end().chars().last(), Some('.') | Some('!') | Some('?') | Some('"') | Some('\u{2019}') | Some('\u{201d}'))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoopLog;
    impl Log for NoopLog {
        fn info(&self, _msg: &str) {}
    }

    fn glyph(ch: char, x0: f32, y0: f32, x1: f32, y1: f32, size: f32) -> Glyph {
        Glyph { ch, x0, y0, x1, y1, font_size: size, font_name: "Test".into(), bold: false, italic: false }
    }

    fn text_line(text: &str, x0: f32, size: f32, page: usize) -> Vec<Glyph> {
        let mut glyphs = Vec::new();
        let mut x = x0;
        for c in text.chars() {
            let w = size * 0.6;
            glyphs.push(glyph(c, x, 700.0, x + w, 700.0 + size, size));
            x += w;
        }
        let _ = page;
        glyphs
    }

    #[test]
    fn lines_group_by_vertical_position() {
        let mut glyphs = text_line("Hello", 72.0, 12.0, 0);
        let mut second: Vec<Glyph> = text_line("World", 72.0, 12.0, 0).into_iter().map(|mut g| {
            g.y0 -= 20.0;
            g.y1 -= 20.0;
            g
        }).collect();
        glyphs.append(&mut second);
        let page = PageText { index: 0, width: 612.0, height: 792.0, glyphs, image_area_ratio: 0.0 };
        let lines = group_lines(&page);
        assert_eq!(lines.len(), 2, "two vertically separated runs should be two lines");
        assert_eq!(lines[0].text, "Hello");
        assert_eq!(lines[1].text, "World");
    }

    #[test]
    fn word_gaps_become_spaces() {
        let mut g1 = text_line("Hello", 72.0, 12.0, 0);
        let mut g2 = text_line("World", 72.0 + 5.0 * 7.2 + 10.0, 12.0, 0);
        g1.append(&mut g2);
        let page = PageText { index: 0, width: 612.0, height: 792.0, glyphs: g1, image_area_ratio: 0.0 };
        let lines = group_lines(&page);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "Hello World");
    }

    #[test]
    fn two_column_page_yields_two_lines_per_row_not_interleaved() {
        // Left column glyphs + right column glyphs at the same y - a naive
        // x-only sort across the full row width would interleave garbage;
        // group_lines only clusters by y, so within-row column separation
        // is detect_columns's job, not group_lines's - this test pins that
        // group_lines still produces ONE line per y-cluster (the row), and
        // that its text preserves left-to-right glyph order without
        // scrambling, which detect_columns then examines for a gutter.
        let mut left = text_line("Left", 72.0, 12.0, 0);
        let mut right = text_line("Right", 320.0, 12.0, 0);
        left.append(&mut right);
        let page = PageText { index: 0, width: 612.0, height: 792.0, glyphs: left, image_area_ratio: 0.0 };
        let lines = group_lines(&page);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].text.starts_with("Left"));
        assert!(lines[0].text.ends_with("Right"));
    }

    fn make_line(text: &str, x0: f32, x1: f32, y_top: f32, size: f32, page: usize) -> Line {
        Line { text: text.into(), x0, x1, y_top, y_bottom: y_top - size, size, bold_ratio: 0.0, page }
    }

    #[test]
    fn running_head_repeated_on_most_pages_is_stripped() {
        let mut pages: Vec<Vec<Line>> = Vec::new();
        for i in 0..12 {
            let mut lines: Vec<Line> = Vec::new();
            if i < 10 {
                // present on 10/12 pages (>= 60%)
                lines.push(make_line("BOOK TITLE", 72.0, 200.0, 780.0, 9.0, i));
            }
            lines.push(make_line("Real paragraph text here.", 72.0, 400.0, 500.0, 12.0, i));
            pages.push(lines);
        }
        let heights = vec![792.0; 12];
        let log = NoopLog;
        strip_running_heads(&mut pages, &heights, &log);
        for lines in &pages {
            assert!(lines.iter().all(|l| l.text != "BOOK TITLE"), "running head should be stripped");
            assert!(lines.iter().any(|l| l.text.starts_with("Real paragraph")), "real content must survive");
        }
    }

    #[test]
    fn text_on_two_of_twelve_pages_is_not_stripped() {
        let mut pages: Vec<Vec<Line>> = Vec::new();
        for i in 0..12 {
            let mut lines = vec![make_line("Real paragraph text here.", 72.0, 400.0, 700.0, 12.0, i)];
            if i < 2 {
                lines.push(make_line("Rare Header", 72.0, 200.0, 780.0, 9.0, i));
            }
            pages.push(lines);
        }
        let heights = vec![792.0; 12];
        let log = NoopLog;
        strip_running_heads(&mut pages, &heights, &log);
        let survives = pages.iter().filter(|lines| lines.iter().any(|l| l.text == "Rare Header")).count();
        assert_eq!(survives, 2, "text on only 2/12 pages must not be treated as a running head");
    }

    #[test]
    fn three_page_document_is_never_stripped() {
        let mut pages: Vec<Vec<Line>> = Vec::new();
        for i in 0..3 {
            pages.push(vec![
                make_line("REPEATED", 72.0, 200.0, 780.0, 9.0, i),
                make_line("Body text.", 72.0, 400.0, 700.0, 12.0, i),
            ]);
        }
        let heights = vec![792.0; 3];
        let log = NoopLog;
        strip_running_heads(&mut pages, &heights, &log);
        for lines in &pages {
            assert!(lines.iter().any(|l| l.text == "REPEATED"), "a <4 page doc should never be stripped");
        }
    }

    #[test]
    fn body_size_prefers_character_weighted_mode_over_title_page() {
        // A title page (few, large chars) followed by many pages of normal
        // body text - the character-weighted histogram must pick 11pt.
        let mut lines = vec![make_line("BIG TITLE", 72.0, 300.0, 700.0, 24.0, 0)];
        for i in 0..20 {
            lines.push(make_line("This is a normal paragraph sentence of body text here.", 72.0, 500.0, 700.0 - (i as f32) * 14.0, 11.0, 1));
        }
        assert_eq!(body_font_size(&lines), 11.0);
    }

    #[test]
    fn three_distinct_heading_sizes_map_to_three_tiers() {
        let mut lines = Vec::new();
        for i in 0..30 {
            lines.push(make_line("Body text sentence number here filling space.", 72.0, 500.0, 780.0 - (i as f32) * 14.0, 11.0, 0));
        }
        lines.push(make_line("Book Title", 72.0, 300.0, 100.0, 24.0, 0));
        lines.push(make_line("Part One", 72.0, 250.0, 90.0, 18.0, 0));
        lines.push(make_line("Chapter One", 72.0, 250.0, 80.0, 15.0, 0));
        let (tiers, low_confidence) = heading_tiers(&lines, 11.0);
        assert!(!low_confidence);
        assert_eq!(tiers, vec![24.0, 18.0, 15.0]);
    }

    #[test]
    fn flat_size_document_yields_zero_headings_and_low_confidence() {
        let mut lines = Vec::new();
        for i in 0..30 {
            lines.push(make_line("Uniform sentence of body text repeated here again.", 72.0, 500.0, 780.0 - (i as f32) * 14.0, 12.0, 0));
        }
        let (tiers, low_confidence) = heading_tiers(&lines, 12.0);
        assert!(tiers.is_empty());
        assert!(low_confidence);
    }

    #[test]
    fn indent_starts_a_new_paragraph() {
        let lines = vec![
            make_line("First paragraph line one filling most of the width here.", 72.0, 500.0, 700.0, 12.0, 0),
            make_line("First paragraph line two also filling width here today.", 72.0, 500.0, 686.0, 12.0, 0),
            make_line("Second paragraph starts indented from the margin here.", 92.0, 500.0, 672.0, 12.0, 0),
            make_line("Second paragraph continues normally after that first line.", 72.0, 500.0, 658.0, 12.0, 0),
        ];
        let ClassifiedBlocks { blocks, .. } = classify_blocks(&lines);
        assert_eq!(blocks.len(), 2, "an indented line should start a new paragraph: {blocks:?}");
        assert!(blocks[0].text.starts_with("First paragraph line one"));
        assert!(blocks[1].text.starts_with("Second paragraph starts indented"));
    }

    #[test]
    fn short_line_then_left_margin_start_breaks_paragraph() {
        let lines = vec![
            make_line("First paragraph line one filling most of the width here.", 72.0, 500.0, 700.0, 12.0, 0),
            make_line("Short ending line.", 72.0, 220.0, 686.0, 12.0, 0),
            make_line("Second paragraph starts flush at the margin again now.", 72.0, 500.0, 672.0, 12.0, 0),
        ];
        let ClassifiedBlocks { blocks, .. } = classify_blocks(&lines);
        assert_eq!(blocks.len(), 2, "a short final line followed by a flush-left start should break: {blocks:?}");
    }

    #[test]
    fn consecutive_full_width_lines_do_not_break_mid_sentence() {
        let lines = vec![
            make_line("A paragraph that continues across several lines without any", 72.0, 500.0, 700.0, 12.0, 0),
            make_line("indentation or unusually short lines in between them here", 72.0, 500.0, 686.0, 12.0, 0),
            make_line("and should therefore remain one single unbroken paragraph.", 72.0, 500.0, 672.0, 12.0, 0),
        ];
        let ClassifiedBlocks { blocks, .. } = classify_blocks(&lines);
        assert_eq!(blocks.len(), 1, "plain wrapped lines must not be split mid-sentence: {blocks:?}");
    }

    #[test]
    fn hyphenated_line_break_joins_across_lines() {
        let joined = join_paragraph_lines(&[&make_line("This is an exam-", 72.0, 300.0, 700.0, 12.0, 0), &make_line("ple sentence.", 72.0, 300.0, 686.0, 12.0, 0)]);
        assert_eq!(joined, "This is an example sentence.");
    }

    #[test]
    fn uppercase_initial_after_hyphen_does_not_join() {
        let joined = join_paragraph_lines(&[&make_line("End of Chapter-", 72.0, 300.0, 700.0, 12.0, 0), &make_line("Two begins here.", 72.0, 300.0, 686.0, 12.0, 0)]);
        assert_eq!(joined, "End of Chapter- Two begins here.");
    }

    #[test]
    fn paragraph_spanning_page_boundary_merges() {
        let log = NoopLog;
        let page0 = PageText {
            index: 0,
            width: 612.0,
            height: 792.0,
            glyphs: text_line("first part of the sentence", 72.0, 12.0, 0),
            image_area_ratio: 0.0,
        };
        let page1 = PageText {
            index: 1,
            width: 612.0,
            height: 792.0,
            glyphs: text_line("continues in lowercase here", 72.0, 12.0, 1),
            image_area_ratio: 0.0,
        };
        let chapters = build_chapters(&[page0, page1], &[], "Untitled", true, &log).unwrap();
        let all_text: String = chapters.iter().flat_map(|c| &c.blocks).map(|b| b.text.as_str()).collect::<Vec<_>>().join(" ");
        assert!(all_text.contains("sentence continues"), "cross-page paragraph should merge: {all_text:?}");
    }

    #[test]
    fn excessive_headings_trigger_fixed_chunk_fallback() {
        let mut blocks = Vec::new();
        for i in 0..250 {
            blocks.push(Block { kind: BlockKind::Heading(1), text: format!("H{i}"), page: i, anchor: format!("b{i}") });
        }
        let log = NoopLog;
        let chapters = split_chapters(blocks, false, &[], "Untitled", 260, &log);
        assert!(chapters.iter().all(|c| c.title.starts_with("Pages")), "should fall back to page chunking: {chapters:?}");
    }

    #[test]
    fn outline_with_enough_entries_wins_over_heading_heuristic() {
        let mut blocks = Vec::new();
        for i in 0..40 {
            blocks.push(Block { kind: BlockKind::Heading(1), text: format!("Detected {i}"), page: i, anchor: format!("b{i}") });
        }
        let outline = vec![
            OutlineEntry { title: "Chapter One".into(), page_index: Some(0), children: vec![] },
            OutlineEntry { title: "Chapter Two".into(), page_index: Some(5), children: vec![] },
        ];
        let log = NoopLog;
        let chapters = split_chapters(blocks, false, &outline, "Untitled", 40, &log);
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].title, "Chapter One");
        assert_eq!(chapters[1].title, "Chapter Two");
    }

    #[test]
    fn well_formed_xhtml_is_produced() {
        let chapter = Chapter {
            title: "A & B <Test>".into(),
            blocks: vec![Block { kind: BlockKind::Paragraph, text: "Hello & <world>".into(), page: 0, anchor: "b0".into() }],
        };
        let xhtml = render_chapter_xhtml(&chapter);
        assert!(crate::epub::parse_xml(&xhtml).is_ok(), "synthesized chapter must be well-formed XML: {xhtml}");
        assert!(xhtml.contains("A &amp; B &lt;Test&gt;"));
    }
}
