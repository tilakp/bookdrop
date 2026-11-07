//! Groups positioned glyphs into text lines, with word gaps found per page.

use super::*;

const LINE_BREAK_FRACTION: f32 = 0.5; // x median glyph height
const WORD_GAP_FRACTION: f32 = 0.25; // x current font size

pub(crate) fn group_lines(page: &PageText) -> Vec<Line> {
    if page.glyphs.is_empty() {
        return Vec::new();
    }

    // Glyphs are NOT sorted by position. pdfium's `chars()` order is the
    // content stream's drawing order, which is reading order. Bounding
    // boxes are less reliable: some producers (calibre's PDF writer among
    // them) emit word spaces with a zero-width box placed slightly after
    // the next glyph, and a sort by x0 turned "put them through" into
    // "put them t hrough". Boxes are used only to cluster glyphs into
    // lines, where an error of a fraction of a glyph height does no harm.
    //
    // Each line grows a [y_min, y_max] window, not a fixed anchor from its
    // first glyph. Ascenders and descenders move y1 and y0, and a fixed
    // anchor put the 'p' of a "Chapter One" heading on a line of its own.
    // The threshold scales with each glyph's own height, so large headings
    // and small body text both get a fitting tolerance.
    let mut clusters: Vec<Vec<&Glyph>> = Vec::new();
    let mut cluster_y_min = f32::MAX;
    let mut cluster_y_max = f32::MIN;
    for g in &page.glyphs {
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

    // Word-gap thresholds are found per font size across the whole page,
    // not per line: one short line has too few gap samples, and the
    // per-line version split nearly every letter of "dependable,
    // no-nonsense". Buckets by font size keep a heading's spacing apart
    // from body text. See `page_word_gap_thresholds`.
    let thresholds = page_word_gap_thresholds(&clusters);
    clusters.into_iter().map(|cluster| build_line(&cluster, page.index, &thresholds)).collect()
}

/// Buckets every line's kerning-candidate gaps by rounded font size (to
/// the nearest point - fine enough to separate a 25pt heading from 11pt
/// body text, coarse enough that ordinary sub-point rendering jitter
/// within "the same font" doesn't fragment one real distribution into
/// several sparse ones) and finds one `adaptive_word_gap_threshold` per
/// bucket from the pooled page-wide samples.
fn page_word_gap_thresholds(clusters: &[Vec<&Glyph>]) -> HashMap<i32, f32> {
    let mut samples_by_size: HashMap<i32, Vec<f32>> = HashMap::new();
    for cluster in clusters {
        for i in 1..cluster.len() {
            if cluster[i].ch != ' ' && cluster[i - 1].ch != ' ' {
                let gap = cluster[i].x0 - cluster[i - 1].x1;
                if gap > 0.0 {
                    let bucket = cluster[i].font_size.round() as i32;
                    samples_by_size.entry(bucket).or_default().push(gap);
                }
            }
        }
    }
    samples_by_size
        .into_iter()
        .map(|(bucket, mut samples)| (bucket, adaptive_word_gap_threshold(&mut samples, bucket as f32)))
        .collect()
}

// No fixed rule fits every typeface. In one real book, a condensed heading
// face had intra-word gaps of 0.15-1.8pt and word gaps of 3.3-4.0pt, while
// a tighter body face had intra-word gaps up to 2.0pt and word gaps from
// 2.0pt. A fixed fraction of the font size, or a multiple of the median
// gap, fixed one face and broke the other ("the" -> "th e").
//
// What both faces share is a clear jump in the sorted gap list between
// kerning and word spacing. This finds that jump directly (the largest
// step between consecutive sorted gaps). It needs many samples to be
// reliable, so `page_word_gap_thresholds` pools a whole page's lines per
// font-size bucket before it calls this.
const WORD_GAP_MIN_SAMPLES: usize = 20;
const WORD_GAP_MIN_JUMP: f32 = 0.5;

/// Returns the midpoint of the largest jump between consecutive sorted
/// gap values - the pooled samples' own natural boundary between
/// intra-word kerning and real word spacing. Falls back to a fixed
/// fraction of font size when there isn't enough data to find one
/// reliably, or when the largest jump found is too small to be a real
/// signal rather than noise within a single cluster (e.g. a short page
/// whose word boundaries all happened to be marked by real space
/// characters, so every sampled gap here is genuinely just kerning).
fn adaptive_word_gap_threshold(kerning_samples: &mut [f32], mean_font_size: f32) -> f32 {
    let fallback = WORD_GAP_FRACTION * mean_font_size;
    if kerning_samples.len() < WORD_GAP_MIN_SAMPLES {
        return fallback;
    }
    kerning_samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mut best_jump = 0.0f32;
    let mut best_threshold = fallback;
    for w in kerning_samples.windows(2) {
        let jump = w[1] - w[0];
        if jump > best_jump {
            best_jump = jump;
            best_threshold = (w[0] + w[1]) / 2.0;
        }
    }
    if best_jump < WORD_GAP_MIN_JUMP {
        return fallback;
    }
    best_threshold
}

fn build_line(glyphs: &[&Glyph], page: usize, thresholds: &HashMap<i32, f32>) -> Line {
    let mut text = String::new();
    let mut total_size = 0.0f32;
    let mut bold_chars = 0u32;
    let count = glyphs.len() as f32;
    // min/max fold rather than first()/last(): natural emission order is
    // left-to-right but not guaranteed jitter-free at the extremes (the
    // same degenerate-bbox glyphs that motivated dropping the x0 sort
    // above), and the line's bounding box feeds column detection, so it's
    // worth being robust here even though text order no longer depends on it.
    let x0 = glyphs.iter().map(|g| g.x0).fold(f32::MAX, f32::min);
    let x1 = glyphs.iter().map(|g| g.x1).fold(f32::MIN, f32::max);
    let y_top = glyphs.iter().map(|g| g.y1).fold(f32::MIN, f32::max);
    let y_bottom = glyphs.iter().map(|g| g.y0).fold(f32::MAX, f32::min);
    let mean_font_size = if count > 0.0 { glyphs.iter().map(|g| g.font_size).sum::<f32>() / count } else { 12.0 };
    // Looked up from the page-wide pool (see `page_word_gap_thresholds`),
    // not recomputed per line - a single line rarely has enough samples
    // of its own to calibrate reliably.
    let word_gap_threshold = thresholds
        .get(&(mean_font_size.round() as i32))
        .copied()
        .unwrap_or(WORD_GAP_FRACTION * mean_font_size);

    for (i, g) in glyphs.iter().enumerate() {
        // Only synthesize a space from the x-gap when neither glyph is
        // already whitespace - PDFium does yield real ' ' glyphs with real
        // bounds (not every space is inkless/omitted), and real books with
        // slightly generous word spacing (seen live on a real government
        // PDF fixture) exceed the threshold on gaps that already have an
        // actual space character - double-counting into "word  gap"
        // otherwise.
        if i > 0 && g.ch != ' ' {
            let prev = glyphs[i - 1];
            let gap = g.x0 - prev.x1;
            if prev.ch != ' ' && gap > word_gap_threshold && !text.ends_with(' ') && !text.is_empty() {
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

