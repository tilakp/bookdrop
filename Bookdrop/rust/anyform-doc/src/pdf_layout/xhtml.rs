//! Writes one chapter as an XHTML file for the IR.

use super::*;
use crate::markup::xml_escape;

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
