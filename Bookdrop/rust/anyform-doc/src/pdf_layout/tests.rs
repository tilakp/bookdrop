use super::*;
use super::blocks::{body_font_size, heading_tiers, join_paragraph_lines};

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

/// Builds one line's glyphs from `(char, gap_before)` pairs. `gap_before`
/// is the horizontal gap between this glyph's x0 and the previous
/// glyph's x1 (ignored for the first char). Lets a test hand-pick
/// exactly which gaps are "kerning" and which are "real word gaps"
/// without needing real font metrics.
fn glyphs_from_gaps(spec: &[(char, f32)], x0: f32, y: f32, size: f32) -> Vec<Glyph> {
    let mut out = Vec::new();
    let mut x = x0;
    let w = size * 0.6;
    for (i, (ch, gap_before)) in spec.iter().enumerate() {
        if i > 0 {
            x += gap_before;
        }
        out.push(glyph(*ch, x, y, x + w, y + size, size));
        x += w;
    }
    out
}

#[test]
fn page_level_pooling_splits_a_heading_too_short_to_calibrate_on_its_own() {
    // Seen in a real book: "Marcus Was a Victim of His
    // Blindspots" collapsed to "MarcusWasaVictimofHisBlindspots"
    // because a single heading line rarely carries enough intra-word
    // gap samples to find its own reliable split between kerning and
    // real word gaps - see `page_word_gap_thresholds`'s doc comment.
    //
    // This fixture reproduces that shape: four short heading-style
    // lines (24pt), each with only 6 kerning samples of its own -
    // below WORD_GAP_MIN_SAMPLES (20), so any one of them judged
    // alone falls back to the fixed-fraction heuristic
    // (0.25 x 24pt = 6.0pt), well above this heading's own real word
    // gap (3.5pt) - it would wrongly run every word together. Pooled
    // across all four lines (24 kerning + 8 word-gap samples), there's
    // enough data to find the real elbow instead (checked: with
    // `page_word_gap_thresholds` stubbed to return an empty map -
    // simulating "no pooling" - this test fails exactly as described,
    // and passes again once pooling is restored).
    //
    // A differently-kerned body font (11pt) on the same page proves
    // the two don't contaminate each other - they land in separate
    // buckets by rounded font size, each finding its own elbow (body:
    // kerning up to 1.0pt vs. word gaps of 2.4pt, threshold ~1.7pt -
    // deliberately a much narrower kerning/word-gap separation than
    // the heading's, so a shared/contaminated threshold would break
    // one font or the other, same as the real book's heading vs.
    // checklist-section regression).
    let heading_spec = [
        ('A', 0.0), ('B', 0.4), ('C', 0.4), ('D', 3.5), ('E', 0.4), ('F', 0.4), ('G', 3.5), ('H', 0.4), ('I', 0.4),
    ];
    let body_spec = [
        ('X', 0.0), ('Y', 0.8), ('Z', 1.0), ('U', 2.4), ('V', 0.8), ('W', 1.0), ('R', 2.4), ('S', 0.8), ('T', 1.0),
    ];

    let mut glyphs = Vec::new();
    for i in 0..4 {
        glyphs.extend(glyphs_from_gaps(&heading_spec, 72.0, 700.0 - i as f32 * 40.0, 24.0));
    }
    for i in 0..4 {
        glyphs.extend(glyphs_from_gaps(&body_spec, 72.0, 500.0 - i as f32 * 20.0, 11.0));
    }

    let page = PageText { index: 0, width: 612.0, height: 792.0, glyphs, image_area_ratio: 0.0 };
    let lines = group_lines(&page);
    assert_eq!(lines.len(), 8, "expected 4 heading lines + 4 body lines: {lines:?}");

    for line in &lines[0..4] {
        assert_eq!(line.text, "ABC DEF GHI", "a short heading line should be split correctly once pooled with its page: {line:?}");
    }
    for line in &lines[4..8] {
        assert_eq!(line.text, "XYZ UVW RST", "a differently-kerned body font on the same page must keep its own threshold: {line:?}");
    }
}

#[test]
fn degenerate_zero_width_space_does_not_reorder_the_following_letter() {
    // Seen in a commercially typeset PDF (made by calibre's PDF
    // writer): a word-space glyph can carry a
    // zero-width bounding box (x0 == x1) whose reported x-position
    // falls *after* the following real letter's x0 - e.g. observed
    // "put them|<space x0=138.71>|<'t' of through, x0=138.04>", i.e.
    // the space's own x0 is numerically greater than the very next
    // glyph's x0. pdfium's emission order is still correct (space,
    // then 't') - a naive sort-by-x0 would reorder them, producing
    // "put themt hrough" instead of "put them through". This
    // hand-built fixture reproduces that exact relative-position
    // inversion.
    let mut glyphs = text_line("put them", 72.0, 12.0, 0);
    let last = glyphs.last().unwrap().clone();
    // A degenerate space: zero-width, positioned *after* the 't' that
    // logically follows it - the inversion that broke a naive x0 sort.
    glyphs.push(Glyph { ch: ' ', x0: last.x1 + 6.0, y0: last.y0, x1: last.x1 + 6.0, y1: last.y1, font_size: 1.0, font_name: "".into(), bold: false, italic: false });
    let mut rest = text_line("through", last.x1 + 5.3, 12.0, 0); // starts slightly BEFORE the space's own x0
    glyphs.append(&mut rest);

    let page = PageText { index: 0, width: 612.0, height: 792.0, glyphs, image_area_ratio: 0.0 };
    let lines = group_lines(&page);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].text, "put them through", "emission order must win over a misleading bounding box");
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
    let chapters = split_chapters(blocks, false, &[], "Untitled", &log);
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
    let chapters = split_chapters(blocks, false, &outline, "Untitled", &log);
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
