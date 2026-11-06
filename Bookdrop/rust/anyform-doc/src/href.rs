//! EPUB hrefs are relative URLs, not file paths. This module turns them into
//! the one canonical form every `DocumentIR` href uses: a percent-decoded,
//! `/`-separated path relative to `content_dir`, with no `.`/`..` segments.
//!
//! `resolve` is the only door from book-supplied text to a filesystem path.
//! It rejects anything that could leave the book: absolute paths, URLs with
//! a scheme (`file:`, `http:`), and `..` segments that climb above the root.

/// Resolves `href` against `base_dir` (a canonical directory, "" for the
/// content root). Returns the canonical path and the fragment, if any.
/// Returns `None` for hrefs that point outside the book or have no path.
pub(crate) fn resolve(base_dir: &str, href: &str) -> Option<(String, Option<String>)> {
    let (rest, fragment) = match href.split_once('#') {
        Some((p, f)) => (p, Some(f.to_string()).filter(|f| !f.is_empty())),
        None => (href, None),
    };
    let path = rest.split_once('?').map_or(rest, |(p, _)| p);
    if path.is_empty() || path.starts_with('/') || has_scheme(path) {
        return None;
    }
    let decoded = percent_decode(path)?;
    if decoded.contains('\0') {
        return None;
    }

    let mut parts: Vec<&str> = base_dir.split('/').filter(|s| !s.is_empty()).collect();
    for segment in decoded.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    if parts.is_empty() {
        return None;
    }
    Some((parts.join("/"), fragment))
}

/// The directory part of a canonical path ("text/ch1.xhtml" -> "text").
pub(crate) fn parent_dir(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// The relative URL that reaches canonical `target` from a document in
/// canonical directory `from_dir`, percent-encoded for use in markup.
pub(crate) fn relative_url(from_dir: &str, target: &str) -> String {
    let from: Vec<&str> = from_dir.split('/').filter(|s| !s.is_empty()).collect();
    let to: Vec<&str> = target.split('/').collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut segments: Vec<String> = vec!["..".to_string(); from.len() - common];
    segments.extend(to[common..].iter().map(|s| percent_encode(s)));
    segments.join("/")
}

/// Encodes the characters that would change a path segment's meaning in a
/// URL. Non-ASCII text stays as is: EPUB allows IRIs.
pub(crate) fn percent_encode(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for c in segment.chars() {
        match c {
            ' ' | '%' | '#' | '?' | '"' | '<' | '>' | '\\' => out.push_str(&format!("%{:02X}", c as u32)),
            _ => out.push(c),
        }
    }
    out
}

fn has_scheme(path: &str) -> bool {
    let before_slash = path.split('/').next().unwrap_or("");
    before_slash.contains(':')
}

/// Decodes `%XX` escapes. A `%` that does not start a valid escape stays
/// literal, as browsers do. Returns `None` if the bytes are not UTF-8.
pub(crate) fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_value(bytes[i + 1]), hex_value(bytes[i + 2])) {
                out.push(hi << 4 | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).ok()
}

fn hex_value(b: u8) -> Option<u8> {
    (b as char).to_digit(16).map(|d| d as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_relative_paths_and_fragments() {
        assert_eq!(resolve("", "ch1.xhtml"), Some(("ch1.xhtml".into(), None)));
        assert_eq!(resolve("text", "ch1.xhtml#p3"), Some(("text/ch1.xhtml".into(), Some("p3".into()))));
        assert_eq!(resolve("text", "../images/a.png"), Some(("images/a.png".into(), None)));
        assert_eq!(resolve("", "./a/./b.xhtml"), Some(("a/b.xhtml".into(), None)));
    }

    #[test]
    fn decodes_percent_escapes() {
        assert_eq!(resolve("", "chapter%201.xhtml"), Some(("chapter 1.xhtml".into(), None)));
        assert_eq!(resolve("", "caf%C3%A9.xhtml"), Some(("café.xhtml".into(), None)));
        assert_eq!(resolve("", "100%.xhtml"), Some(("100%.xhtml".into(), None)));
        assert_eq!(resolve("", "é%"), Some(("é%".into(), None)));
    }

    #[test]
    fn rejects_hrefs_that_leave_the_book() {
        assert_eq!(resolve("", "/etc/passwd"), None);
        assert_eq!(resolve("", "../outside.xhtml"), None);
        assert_eq!(resolve("text", "../../outside.xhtml"), None);
        assert_eq!(resolve("", "file:///etc/passwd"), None);
        assert_eq!(resolve("", "http://example.com/a.css"), None);
        assert_eq!(resolve("", "%2Fetc%2Fpasswd").map(|r| r.0), Some("etc/passwd".into()));
        assert_eq!(resolve("", "%2E%2E/outside.xhtml"), None);
        assert_eq!(resolve("", "#only-a-fragment"), None);
    }

    #[test]
    fn builds_relative_urls() {
        assert_eq!(relative_url("text", "text/ch1.xhtml"), "ch1.xhtml");
        assert_eq!(relative_url("", "text/ch 1.xhtml"), "text/ch%201.xhtml");
        assert_eq!(relative_url("nav", "text/ch1.xhtml"), "../text/ch1.xhtml");
        assert_eq!(parent_dir("text/ch1.xhtml"), "text");
        assert_eq!(parent_dir("ch1.xhtml"), "");
    }
}
