//! Detects multi-column pages, which this input cannot reflow.

use super::*;

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
pub(super) fn detect_columns(pages: &[PageText]) -> ColumnVerdict {
    let mut two_column_pages = 0usize;
    let mut multi_column_pages = 0usize;
    let mut total_pages_with_text = 0usize;

    for page in pages {
        if page.glyphs.is_empty() {
            continue;
        }
        total_pages_with_text += 1;
        match detect_page_columns(page) {
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
            if let ColumnVerdict::TwoColumn { gutter_x } = detect_page_columns(page) {
                return ColumnVerdict::TwoColumn { gutter_x };
            }
        }
    }
    ColumnVerdict::SingleColumn
}

fn detect_page_columns(page: &PageText) -> ColumnVerdict {
    let glyphs = &page.glyphs;
    // Clamped to the page: a hostile PDF can report glyph boxes millions of
    // points wide, and the coverage histogram below has one bin per 2pt.
    let min_x = glyphs.iter().map(|g| g.x0).fold(f32::MAX, f32::min).max(0.0);
    let max_x = glyphs.iter().map(|g| g.x1).fold(f32::MIN, f32::max).min(page.width.max(1.0));
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
