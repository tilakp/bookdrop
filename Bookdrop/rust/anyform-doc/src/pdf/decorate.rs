//! Draws page numbers, running headers and footers over the merged
//! document, and writes the PDF Info metadata.

use anyform_core::ConvError;
use lopdf::content::{Content, Operation};
use lopdf::{dictionary, Dictionary, Object, ObjectId, Stream};

use crate::ir::DocumentIR;

use super::RenderOptions;

/// Post-merge overlay pass for page numbers / running header / running
/// footer — done here (rather than via Chrome's own per-chapter
/// `headerTemplate`/`footerTemplate`) because each chapter is rendered as
/// a *separate* `print_to_pdf` call: Chrome's built-in page-number
/// substitution would restart at 1 for every chapter instead of counting
/// across the whole merged book. Mirrors Swift `PdfConverter.decoratePages`
/// (draw over each final page, book title top/bottom-center, page number
/// bottom-center) but as PDF content-stream operators instead of Core
/// Graphics, and with an approximate (not glyph-metric-exact) text width
/// for centering — close enough for short header/footer/page-number
/// strings at 8pt.
pub(super) fn add_page_decorations(doc: &mut lopdf::Document, opts: &RenderOptions, book_title: &str) -> Result<(), ConvError> {
    if !(opts.show_page_numbers || opts.include_headers || opts.include_footers) {
        return Ok(());
    }

    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });

    let page_width_pt = opts.page_width_pt();
    let page_height_pt = opts.page_height_pt();
    let page_ids: Vec<ObjectId> = doc.get_pages().into_values().collect();

    for (i, page_id) in page_ids.iter().enumerate() {
        let mut operations = Vec::new();
        if opts.include_headers {
            operations.extend(centered_text_ops(book_title, 8.0, page_width_pt, page_height_pt - 20.0));
        }
        if opts.include_footers {
            operations.extend(centered_text_ops(book_title, 8.0, page_width_pt, 16.0));
        }
        if opts.show_page_numbers {
            let y = if opts.include_footers { 4.0 } else { 16.0 };
            operations.extend(centered_text_ops(&(i + 1).to_string(), 8.0, page_width_pt, y));
        }
        if operations.is_empty() {
            continue;
        }

        let encoded = Content { operations }
            .encode()
            .map_err(|e| ConvError::Other(format!("failed to encode page decoration: {e}")))?;
        let stream_id = doc.add_object(Stream::new(dictionary! {}, encoded));

        let (mut contents, resources_obj) = {
            let page_dict = doc.get_object(*page_id).ok().and_then(|o| o.as_dict().ok());
            let contents = match page_dict.and_then(|d| d.get(b"Contents").ok()) {
                Some(Object::Array(arr)) => arr.clone(),
                Some(Object::Reference(r)) => vec![Object::Reference(*r)],
                _ => Vec::new(),
            };
            let resources_obj = page_dict.and_then(|d| d.get(b"Resources").ok()).cloned();
            (contents, resources_obj)
        };
        contents.push(Object::Reference(stream_id));

        let mut resources_dict = match resources_obj {
            Some(Object::Reference(res_id)) => doc
                .get_object(res_id)
                .ok()
                .and_then(|o| o.as_dict().ok())
                .cloned()
                .unwrap_or_default(),
            Some(Object::Dictionary(d)) => d,
            _ => Dictionary::new(),
        };
        add_font_to_resources(&mut resources_dict, font_id);

        if let Some(page_dict) = doc.get_object_mut(*page_id).ok().and_then(|o| o.as_dict_mut().ok()) {
            page_dict.set("Contents", Object::Array(contents));
            page_dict.set("Resources", Object::Dictionary(resources_dict));
        }
    }

    Ok(())
}

fn add_font_to_resources(resources: &mut Dictionary, font_id: ObjectId) {
    let mut fonts = match resources.get(b"Font") {
        Ok(Object::Dictionary(d)) => d.clone(),
        _ => Dictionary::new(),
    };
    fonts.set("F_Anyform", Object::Reference(font_id));
    resources.set("Font", Object::Dictionary(fonts));
}

fn centered_text_ops(text: &str, font_size: f64, page_width_pt: f64, y: f64) -> Vec<Operation> {
    // Rough Helvetica average-advance-width estimate (not real glyph
    // metrics) — fine for centering a short title/page-number string.
    let approx_width = text.chars().count() as f64 * font_size * 0.5;
    let x = ((page_width_pt - approx_width) / 2.0).max(0.0);
    vec![
        Operation::new("q", vec![]),
        Operation::new("BT", vec![]),
        Operation::new("g", vec![Object::Real(0.35_f32)]),
        Operation::new("Tf", vec![Object::Name(b"F_Anyform".to_vec()), Object::Real(font_size as f32)]),
        Operation::new("Td", vec![Object::Real(x as f32), Object::Real(y as f32)]),
        Operation::new("Tj", vec![Object::string_literal(win_ansi_bytes(text))]),
        Operation::new("ET", vec![]),
        Operation::new("Q", vec![]),
    ]
}

/// A PDF "text string" (Info dictionary values). Raw UTF-8 bytes are wrong
/// here: readers decode them as PDFDocEncoding, so "é" shows as "Ã©".
/// Non-ASCII text must be UTF-16BE with a byte order mark, the same rule
/// lopdf applies to bookmark titles.
fn pdf_text_string(text: &str) -> Object {
    if text.is_ascii() {
        return Object::string_literal(text);
    }
    let mut bytes = vec![0xFE, 0xFF];
    bytes.extend(text.encode_utf16().flat_map(u16::to_be_bytes));
    Object::String(bytes, lopdf::StringFormat::Hexadecimal)
}

/// Encodes `text` for the standard Helvetica font with WinAnsiEncoding.
/// That covers Latin-script languages. Characters outside it (CJK, for
/// example) become "?": showing them would need an embedded font.
fn win_ansi_bytes(text: &str) -> Vec<u8> {
    const CP1252_HIGH: [(char, u8); 27] = [
        ('€', 0x80), ('‚', 0x82), ('ƒ', 0x83), ('„', 0x84), ('…', 0x85), ('†', 0x86), ('‡', 0x87),
        ('ˆ', 0x88), ('‰', 0x89), ('Š', 0x8A), ('‹', 0x8B), ('Œ', 0x8C), ('Ž', 0x8E), ('‘', 0x91),
        ('’', 0x92), ('“', 0x93), ('”', 0x94), ('•', 0x95), ('–', 0x96), ('—', 0x97), ('˜', 0x98),
        ('™', 0x99), ('š', 0x9A), ('›', 0x9B), ('œ', 0x9C), ('ž', 0x9E), ('Ÿ', 0x9F),
    ];
    text.chars()
        .map(|c| match c as u32 {
            0x20..=0x7E | 0xA0..=0xFF => c as u8,
            _ => CP1252_HIGH.iter().find(|(ch, _)| *ch == c).map_or(b'?', |(_, b)| *b),
        })
        .collect()
}

pub(super) fn set_metadata(doc: &mut lopdf::Document, ir: &DocumentIR) {
    let mut info = dictionary! {
        "Title" => pdf_text_string(&ir.metadata.title),
        "Producer" => pdf_text_string("Bookdrop (anyform engine)"),
    };
    if let Some(author) = &ir.metadata.author {
        info.set("Author", pdf_text_string(author));
    }
    let info_id = doc.add_object(info);
    doc.trailer.set("Info", info_id);
}
