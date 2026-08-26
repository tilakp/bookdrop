use std::collections::HashMap;
use std::path::Path;

use anyform_core::{ConvError, Log, Options, OutputPlugin};

use crate::ir::{DocumentIR, TocNode};
use crate::pdf::sniff_image_mime;

/// Self-contained HTML output - a Rust port of Bookdrop's Swift
/// `HtmlConverter`: CSS inlined into a `<style>` block, images embedded as
/// base64 `data:` URIs, so the output has zero external dependencies and
/// works with the network off.
///
/// One deliberate divergence from the Swift original: resource iteration
/// (images for data-URI inlining, CSS files for `<style>` concatenation,
/// and reference-rewrite candidates) is sorted rather than iterated in
/// `HashMap` order, and reference rewriting runs in two passes (see
/// `rewrite_references`'s own doc comment for why). The Swift version
/// iterated an unordered dictionary in a single combined pass, which was
/// both non-deterministic *and* could let one resource's bare-filename
/// fallback shadow a *different* resource's own exact short-href match.
///
/// Everything else - the TOC's flat-by-spine-index shape (not nested, even
/// though `ir.toc` is a tree), the exact class names/style rules, the
/// error policy (only an empty spine throws; unreadable images/CSS/
/// chapters are silently skipped) - matches the Swift original exactly on
/// purpose, not "improved."
pub struct HtmlOutput;

impl OutputPlugin<DocumentIR> for HtmlOutput {
    fn name(&self) -> &'static str {
        "html"
    }

    fn extension(&self) -> &'static str {
        "html"
    }

    fn convert(&self, ir: &DocumentIR, output: &Path, opts: &Options, log: &dyn Log) -> Result<(), ConvError> {
        if ir.spine.is_empty() {
            return Err(ConvError::Other("This book has no readable chapters.".into()));
        }
        let include_cover = opts.get_bool("include_cover", true);
        let generate_toc = opts.get_bool("generate_table_of_contents", true);

        let mut image_items: Vec<_> = ir.manifest.values().filter(|r| r.media_type.starts_with("image/")).collect();
        image_items.sort_by(|a, b| a.href.cmp(&b.href));
        let mut data_uris: HashMap<String, String> = HashMap::new();
        for item in &image_items {
            let Ok(bytes) = std::fs::read(ir.content_dir.join(&item.href)) else {
                log.info(&format!("skipping unreadable image: {}", item.href));
                continue;
            };
            use base64::Engine;
            let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
            data_uris.insert(item.href.clone(), format!("data:{};base64,{}", item.media_type, encoded));
        }

        let mut css_items: Vec<_> = ir.manifest.values().filter(|r| r.media_type == "text/css").collect();
        css_items.sort_by(|a, b| a.href.cmp(&b.href));
        let mut css = String::new();
        for item in &css_items {
            let Ok(bytes) = std::fs::read(ir.content_dir.join(&item.href)) else {
                log.info(&format!("skipping unreadable stylesheet: {}", item.href));
                continue;
            };
            let Ok(text) = String::from_utf8(bytes) else {
                log.info(&format!("skipping non-UTF8 stylesheet: {}", item.href));
                continue;
            };
            css.push_str(&rewrite_references(&text, &data_uris));
            css.push('\n');
        }

        let mut body = String::new();

        if include_cover {
            if let Some(cover) = &ir.metadata.cover {
                use base64::Engine;
                let encoded = base64::engine::general_purpose::STANDARD.encode(cover);
                let mime = sniff_image_mime(cover);
                body.push_str(&format!("<img class=\"bookdrop-cover\" src=\"data:{mime};base64,{encoded}\" alt=\"Cover\"/>\n"));
            }
        }

        if generate_toc {
            body.push_str("<nav class=\"bookdrop-toc\"><h2>Contents</h2><ul>\n");
            for (index, item) in ir.spine.iter().enumerate() {
                let title = toc_title(&item.href, &ir.toc).map(str::to_string).unwrap_or_else(|| format!("Chapter {}", index + 1));
                body.push_str(&format!("<li><a href=\"#chapter-{index}\">{}</a></li>\n", xml_escape(&title)));
            }
            body.push_str("</ul></nav>\n");
        }

        let total = ir.spine.len();
        for (index, item) in ir.spine.iter().enumerate() {
            if log.is_cancelled() {
                return Err(ConvError::Cancelled);
            }
            log.progress(index as f64 / total as f64, &format!("Rendering chapter {}/{}", index + 1, total));

            let Ok(bytes) = std::fs::read(ir.content_dir.join(&item.href)) else {
                log.info(&format!("skipping unreadable chapter: {}", item.href));
                continue;
            };
            let Ok(raw) = String::from_utf8(bytes) else {
                log.info(&format!("skipping non-UTF8 chapter: {}", item.href));
                continue;
            };
            let inner = extract_body(&raw);
            body.push_str(&format!("<section id=\"chapter-{index}\">\n{}\n</section>\n", rewrite_references(inner, &data_uris)));
        }

        let html = format!(
            "<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n<title>{title}</title>\n<style>\n{css}\n\
             .bookdrop-cover {{ max-width: 100%; display: block; margin: 0 auto 2em; }}\n\
             .bookdrop-toc {{ margin-bottom: 2em; }}\n</style>\n</head>\n<body>\n{body}\n</body>\n</html>\n",
            title = xml_escape(&ir.metadata.title),
        );

        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(output, html)?;
        log.progress(1.0, "Done");
        Ok(())
    }
}

/// Best-effort: matches manifest hrefs (and their bare filename) verbatim
/// against quoted attribute values and CSS `url(...)` references. Doesn't
/// resolve arbitrary relative-path forms (`../images/x.jpg` from a nested
/// chapter file won't match a manifest href of `images/x.jpg`) - matches
/// the Swift original's own documented limitation.
///
/// Two passes, deliberately: every candidate's *exact* href match first
/// (quoted forms and CSS `url(...)`), then every candidate's bare-filename
/// fallback second. A single combined pass (tried first, then rejected)
/// could let a *different* resource's bare-filename fallback consume a
/// short href's own exact match before the short href got its turn - e.g.
/// "logo.png" and "images/logo.png" share the filename "logo.png", so a
/// chapter correctly referencing its own root-level "logo.png" would get
/// silently overwritten by "images/logo.png"'s bare-filename branch if
/// that ran first. Doing every exact match first avoids this: manifest
/// hrefs are unique keys, so exact matches can never collide with each
/// other regardless of order, and by the time bare-filename fallback runs,
/// every legitimately-exact reference has already been resolved and can
/// no longer be matched by a stray bare-filename search.
///
/// This does *not* fully resolve the genuinely ambiguous case of two
/// *different* images sharing the same bare filename in different
/// directories, both referenced only by that bare filename (no realistic
/// way to know which chapter "meant" which from string content alone) -
/// but it does make that case deterministic (same resolution every run,
/// based on the sort below) instead of dependent on `HashMap` iteration
/// order, which is what actually varied between runs before this fix.
fn rewrite_references(text: &str, data_uris: &HashMap<String, String>) -> String {
    let mut candidates: Vec<(&String, &String)> = data_uris.iter().collect();
    candidates.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(b.0)));

    let mut result = text.to_string();
    for (href, data_uri) in &candidates {
        result = result.replace(&format!("\"{href}\""), &format!("\"{data_uri}\""));
        result = result.replace(&format!("'{href}'"), &format!("'{data_uri}'"));
        result = result.replace(&format!("({href})"), &format!("({data_uri})"));
    }
    for (href, data_uri) in &candidates {
        if let Some(filename) = href.rsplit('/').next() {
            if filename != href.as_str() {
                result = result.replace(&format!("\"{filename}\""), &format!("\"{data_uri}\""));
                result = result.replace(&format!("'{filename}'"), &format!("'{data_uri}'"));
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_short_href_is_not_shadowed_by_a_longer_hrefs_bare_filename_fallback() {
        // "logo.png" (root) and "images/logo.png" share the bare filename
        // "logo.png". A chapter correctly referencing its own root-level
        // "logo.png" by its exact href must get *its own* image, not the
        // nested one's, regardless of which order the two candidates are
        // considered in.
        let mut data_uris = HashMap::new();
        data_uris.insert("logo.png".to_string(), "data:image/png;base64,ROOT".to_string());
        data_uris.insert("images/logo.png".to_string(), "data:image/png;base64,NESTED".to_string());

        let result = rewrite_references(r#"<img src="logo.png"/>"#, &data_uris);
        assert_eq!(result, r#"<img src="data:image/png;base64,ROOT"/>"#);
    }

    #[test]
    fn bare_filename_collision_between_two_different_images_is_deterministic() {
        // Two different images, both named "logo.png" in different
        // directories, both referenced only by bare filename - genuinely
        // ambiguous from string content alone (see this function's doc
        // comment), so this only asserts the resolution is *stable*
        // across repeated calls with the same input, not "correct" for
        // both chapters simultaneously.
        let mut data_uris = HashMap::new();
        data_uris.insert("a/logo.png".to_string(), "data:image/png;base64,AAAA".to_string());
        data_uris.insert("b/logo.png".to_string(), "data:image/png;base64,BBBB".to_string());

        let first = rewrite_references(r#"<img src="logo.png"/>"#, &data_uris);
        for _ in 0..10 {
            assert_eq!(rewrite_references(r#"<img src="logo.png"/>"#, &data_uris), first);
        }
    }
}

fn extract_body(html: &str) -> &str {
    let bytes = html.as_bytes();
    let Some(body_start) = find_ascii_ci(bytes, b"<body") else { return html };
    let Some(gt_offset) = bytes[body_start..].iter().position(|&b| b == b'>') else { return html };
    let open_end = body_start + gt_offset + 1;
    let Some(close_offset) = rfind_ascii_ci(&bytes[open_end..], b"</body>") else { return html };
    let close_start = open_end + close_offset;
    std::str::from_utf8(&bytes[open_end..close_start]).unwrap_or(html)
}

fn find_ascii_ci(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    (0..=haystack.len() - needle.len()).find(|&i| haystack[i..i + needle.len()].eq_ignore_ascii_case(needle))
}

fn rfind_ascii_ci(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    (0..=haystack.len() - needle.len()).rev().find(|&i| haystack[i..i + needle.len()].eq_ignore_ascii_case(needle))
}

fn toc_title<'a>(href: &str, nodes: &'a [TocNode]) -> Option<&'a str> {
    for node in nodes {
        if let Some(node_href) = &node.href {
            if node_href.split('#').next() == Some(href) {
                return Some(&node.title);
            }
        }
        if let Some(found) = toc_title(href, &node.children) {
            return Some(found);
        }
    }
    None
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}
