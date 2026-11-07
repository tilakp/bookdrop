//! Small helpers for the markup that plugins write.

/// Escapes text for XML or HTML, in element content and in quoted
/// attribute values.
pub(crate) fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// A `data:` URI that holds `bytes`.
pub(crate) fn data_uri(media_type: &str, bytes: &[u8]) -> String {
    use base64::Engine;
    format!("data:{media_type};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
}

/// The image type from the first bytes of the file. JPEG if not known.
pub(crate) fn sniff_image_mime(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        "image/png"
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF8") {
        "image/gif"
    } else {
        "image/jpeg"
    }
}
