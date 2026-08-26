use std::path::Path;

use anyform_core::{ConvError, Log, Options, OutputPlugin};
use docx_rs::{Docx, Paragraph, Pic, Run, RunFonts, Style, StyleType};

use crate::htmltext::{self, BlockKind};
use crate::ir::DocumentIR;

/// Word (.docx) output - a Rust port of Bookdrop's Swift `DocxConverter`.
/// The Swift original used `NSAttributedString`'s native `.officeOpenXML`
/// document type (the same mechanism TextEdit's "Save as Word Document"
/// uses) rather than hand-rolling OOXML; that's an AppKit-only API, so this
/// builds the document structure directly via `docx-rs` instead, driven by
/// `htmltext::extract_blocks` (shared with `TxtOutput`).
///
/// `docx-rs` ships with its `image` feature (auto-detecting an embedded
/// image's format/dimensions via the `image` crate) on by default; this
/// crate depends on it with `default-features = false` instead; and
/// `image_dimensions` below hand-rolls PNG/JPEG dimension reading, since
/// the underlying `Pic::new_with_dimensions` API doesn't actually need the
/// `image` crate at all - only `Pic::new`'s auto-detection convenience
/// does. Checked with `cargo tree` before committing to this: `image`
/// pulls in its own decoders for a dozen formats plus a *second*,
/// independent copy of the `zip` crate (docx-rs's own container writer
/// already needs one) - meaningful build-time and static-lib-size weight
/// for a feature this plugin only needs for two formats.
///
/// No TOC concept, matching `DocxConverter.swift` exactly - it never had a
/// `generateTOC` parameter, so `DocxOutput` reads `include_cover` and
/// ignores `generate_table_of_contents` entirely. And, carried over
/// verbatim from the Swift original's own documented decision (verified
/// working correctly in real Word before this port, not a gap): no
/// synthetic chapter-title paragraph is added from the TOC, because each
/// chapter's own XHTML `<h1>` already becomes a heading via
/// `htmltext::extract_blocks`, and adding a second TOC-derived title
/// produced a visibly duplicated heading.
pub struct DocxOutput;

impl OutputPlugin<DocumentIR> for DocxOutput {
    fn name(&self) -> &'static str {
        "docx"
    }

    fn extension(&self) -> &'static str {
        "docx"
    }

    fn convert(&self, ir: &DocumentIR, output: &Path, opts: &Options, log: &dyn Log) -> Result<(), ConvError> {
        if ir.spine.is_empty() {
            return Err(ConvError::Other("This book has no readable chapters.".into()));
        }
        let include_cover = opts.get_bool("include_cover", true);

        let mut docx = Docx::new();
        for level in 1..=6u8 {
            docx = docx.add_style(
                Style::new(format!("Heading{level}"), StyleType::Paragraph)
                    .name(format!("heading {level}"))
                    .bold()
                    .size(heading_half_points(level)),
            );
        }

        docx = docx.add_paragraph(Paragraph::new().add_run(Run::new().add_text(&ir.metadata.title).bold().size(48)));
        if let Some(author) = &ir.metadata.author {
            docx = docx.add_paragraph(Paragraph::new().add_run(Run::new().add_text(author).size(28)));
        }
        docx = docx.add_paragraph(Paragraph::new());

        if include_cover {
            if let Some(cover) = &ir.metadata.cover {
                match image_dimensions(cover) {
                    Some((w, h)) => {
                        let pic = Pic::new_with_dimensions(cover.clone(), w, h);
                        docx = docx.add_paragraph(Paragraph::new().add_run(Run::new().add_image(pic)));
                        docx = docx.add_paragraph(Paragraph::new());
                    }
                    None => log.info("skipping cover image: couldn't determine its pixel dimensions"),
                }
            }
        }

        let total = ir.spine.len();
        for (i, item) in ir.spine.iter().enumerate() {
            if log.is_cancelled() {
                return Err(ConvError::Cancelled);
            }
            log.progress(i as f64 / total as f64, &format!("Rendering chapter {} of {}", i + 1, total));

            let path = ir.content_dir.join(&item.href);
            let raw = std::fs::read_to_string(&path).map_err(|_| ConvError::MissingFile(item.href.clone()))?;
            for block in htmltext::extract_blocks(&raw) {
                docx = docx.add_paragraph(paragraph_for_block(&block));
            }
            docx = docx.add_paragraph(Paragraph::new());
            docx = docx.add_paragraph(Paragraph::new());
        }

        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = std::fs::File::create(output)?;
        docx.pack(file).map_err(|e| ConvError::Other(format!("failed to write DOCX: {e}")))?;
        log.progress(1.0, "Done");
        Ok(())
    }
}

fn heading_half_points(level: u8) -> usize {
    // 6 levels, stepping down from 32pt to 14pt - half-points per docx-rs's
    // Style::size/Run::size convention (48 half-points = 24pt, matching
    // the Swift original's title styling).
    match level {
        1 => 64,
        2 => 56,
        3 => 48,
        4 => 40,
        5 => 32,
        _ => 28,
    }
}

fn paragraph_for_block(block: &htmltext::Block) -> Paragraph {
    let mut paragraph = match block.kind {
        BlockKind::Heading(level) => Paragraph::new().style(&format!("Heading{}", level.clamp(1, 6))),
        BlockKind::Preformatted => Paragraph::new(),
        _ => Paragraph::new(),
    };

    let prefix = matches!(block.kind, BlockKind::ListItem).then_some("\u{2022} ");
    let monospace = matches!(block.kind, BlockKind::Preformatted);

    let mut first = true;
    for run in &block.runs {
        let text = if first { format!("{}{}", prefix.unwrap_or(""), run.text) } else { run.text.clone() };
        first = false;
        if text.is_empty() {
            continue;
        }
        let mut r = Run::new().add_text(text);
        if run.bold {
            r = r.bold();
        }
        if run.italic {
            r = r.italic();
        }
        if monospace {
            r = r.fonts(RunFonts::new().ascii("Courier New"));
        }
        paragraph = paragraph.add_run(r);
    }
    paragraph
}

/// Hand-rolled PNG/JPEG pixel-dimension readers - see this module's doc
/// comment for why this avoids depending on the `image` crate. Returns
/// `None` for anything else (GIF covers are rare in practice), in which
/// case the caller skips embedding a cover entirely rather than guessing.
fn image_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    png_dimensions(bytes).or_else(|| jpeg_dimensions(bytes))
}

fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    if bytes.len() < 24 || bytes[..8] != SIGNATURE {
        return None;
    }
    let w = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let h = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    Some((w, h))
}

/// Scans JPEG segment markers for the first SOF (Start Of Frame) marker,
/// which carries the image's real pixel dimensions - height before width,
/// unlike PNG. Handles baseline and progressive SOF variants (0xC0-0xC3,
/// 0xC5-0xC7, 0xC9-0xCB, 0xCD-0xCF; 0xC4/0xC8/0xCC are DHT/JPG/DAC, not
/// SOF, and deliberately excluded).
fn jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return None;
    }
    let mut i = 2;
    while i + 1 < bytes.len() {
        if bytes[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = bytes[i + 1];
        if marker == 0xFF {
            i += 1;
            continue;
        }
        // Markers with no payload/length field: RST0-7, TEM.
        if (0xD0..=0xD9).contains(&marker) || marker == 0x01 {
            i += 2;
            if marker == 0xD9 {
                return None; // EOI reached with no SOF found
            }
            continue;
        }
        if i + 4 > bytes.len() {
            return None;
        }
        let seg_len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        let is_sof = matches!(marker, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF);
        if is_sof {
            if i + 9 > bytes.len() {
                return None;
            }
            let height = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as u32;
            let width = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]) as u32;
            return Some((width, height));
        }
        if seg_len < 2 {
            return None;
        }
        i += 2 + seg_len;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_png_dimensions() {
        // A real 1x1 PNG (signature + IHDR chunk declaring width=1, height=1).
        let bytes: &[u8] = &[
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00,
            0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00,
        ];
        assert_eq!(image_dimensions(bytes), Some((1, 1)));
    }

    #[test]
    fn rejects_non_image_bytes() {
        assert_eq!(image_dimensions(b"not an image"), None);
    }
}
