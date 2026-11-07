//! Removes running heads, running feet and page numbers that repeat across pages.

use super::*;

const RUNNING_HEAD_MIN_PAGES: usize = 4;
const RUNNING_HEAD_BAND_FRACTION: f32 = 0.12;
const RUNNING_HEAD_REPETITION_THRESHOLD: f32 = 0.60;
const RUNNING_HEAD_Y_BUCKET_PT: f32 = 5.0;
const BARE_NUMBER_PAGE_FRACTION: f32 = 0.5;

pub(super) fn strip_running_heads(pages: &mut [Vec<Line>], heights: &[f32], log: &dyn Log) {
    if pages.len() < RUNNING_HEAD_MIN_PAGES {
        return;
    }

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
