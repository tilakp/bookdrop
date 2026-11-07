//! Splits the blocks into chapters: by the PDF outline, by headings, or by page count.

use super::*;

const MAX_PLAUSIBLE_CHAPTERS: usize = 200;
const MIN_MEDIAN_BLOCKS_PER_CHAPTER: usize = 3;
const MAX_CHAPTER_CHARS: usize = 500_000;
const PAGES_PER_FALLBACK_CHUNK: usize = 10;
const MIN_OUTLINE_ENTRIES: usize = 2;

pub(super) fn split_chapters(
    all_blocks: Vec<Block>,
    low_confidence: bool,
    outline: &[OutlineEntry],
    fallback_title: &str,
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
        return chunk_by_pages(all_blocks);
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

fn chunk_by_pages(blocks: Vec<Block>) -> Vec<Chapter> {
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
