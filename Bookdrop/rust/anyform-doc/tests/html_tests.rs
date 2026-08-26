use std::path::PathBuf;

use anyform_core::{Options, Priority, StdLog, Value};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn convert(name: &str, epub: &str) -> String {
    convert_with_options(name, epub, Options::new())
}

fn convert_with_options(name: &str, epub: &str, opts: Options) -> String {
    let output = std::env::temp_dir().join(format!("anyform-html-test-{name}-{}.html", std::process::id()));
    let registry = anyform_doc::document_registry();
    registry
        .convert(&fixtures_dir().join(epub), &output, &opts, &StdLog)
        .unwrap_or_else(|e| panic!("{epub} should convert to HTML: {e}"));
    let html = std::fs::read_to_string(&output).expect("output should be valid UTF-8");
    let _ = std::fs::remove_file(&output);
    html
}

fn opts(pairs: &[(&str, Value)]) -> Options {
    let mut opts = Options::new();
    for (name, value) in pairs {
        opts.set(name, value.clone(), Priority::UserSet);
    }
    opts
}

#[test]
fn produces_self_contained_document() {
    let html = convert("self-contained", "minimal.epub");
    assert!(html.contains("<!DOCTYPE html>"));
    assert!(html.contains("Chapter One"));
    assert!(html.contains("Chapter Two"));
    assert!(html.contains("id=\"chapter-0\""));
    assert!(html.contains("id=\"chapter-1\""));
    assert!(html.contains("data:image/"));
    assert!(html.contains("font-family: serif"), "expected style.css's rule to be inlined verbatim");
}

#[test]
fn rewrites_external_image_references() {
    let html = convert("rewrite", "minimal.epub");
    assert!(!html.contains("src=\"cover.jpg\""), "the external reference must be fully rewritten to a data URI");
}

#[test]
fn omits_cover_and_toc_when_disabled() {
    let html = convert_with_options(
        "omit",
        "minimal.epub",
        opts(&[("include_cover", Value::Bool(false)), ("generate_table_of_contents", Value::Bool(false))]),
    );
    assert!(!html.contains("<img class=\"bookdrop-cover\""), "cover element should be absent, not just unstyled");
    assert!(!html.contains("<nav class=\"bookdrop-toc\""), "TOC element should be absent, not just unstyled");
}

#[test]
fn empty_spine_is_rejected() {
    let ir = anyform_doc::DocumentIR {
        metadata: anyform_doc::Metadata { title: "Empty".into(), author: None, language: None, cover: None, cover_href: None },
        manifest: std::collections::HashMap::new(),
        spine: Vec::new(),
        toc: Vec::new(),
        file_size_bytes: 0,
        content_dir: std::env::temp_dir(),
    };
    let output = std::env::temp_dir().join(format!("anyform-html-test-empty-{}.html", std::process::id()));
    use anyform_core::OutputPlugin;
    let result = anyform_doc::HtmlOutput.convert(&ir, &output, &Options::new(), &StdLog);
    let _ = std::fs::remove_file(&output);
    assert!(result.is_err());
}

#[test]
fn output_is_deterministic() {
    // doctor-dolittle.epub has 3 stylesheets and 55+ images - multiple
    // resources of both kinds, unlike css-edge-cases.epub (verified: only
    // 1 CSS file and 0 images, which would make this assertion pass
    // trivially regardless of whether determinism was fixed at all).
    // Directly confirmed real HashMap iteration nondeterminism across
    // separate parses of this exact fixture (3 different manifest
    // orderings observed across 3 runs) before writing this test, so this
    // is pinning a real, previously-observed hazard, not a hypothetical
    // one. Depends specifically on css_items/image_items being sorted
    // before concatenation/data-URI-map construction - see html.rs's own
    // inline unit tests for the *other* half of D3 (the rewrite_references
    // candidate sort's effect on same-bare-filename collisions), which
    // this fixture doesn't exercise (no filename collisions here).
    let a = convert("determinism-a", "doctor-dolittle.epub");
    let b = convert("determinism-b", "doctor-dolittle.epub");
    assert_eq!(a, b, "HtmlOutput should produce byte-identical output across runs");
}

#[test]
fn real_book_converts_with_all_images_inlined() {
    let html = convert("real-book", "doctor-dolittle.epub");
    let inlined = html.matches("data:image/").count();
    assert!(inlined >= 10, "expected at least 10 inlined images, got {inlined}");
    // Some source markup carries id="foo.jpg"-shaped attributes unrelated
    // to src= (found empirically on this exact book), so check src=
    // specifically rather than any occurrence of the extension.
    assert!(!has_unrewritten_src(&html, "jpg"), "no residual external src=\"...jpg\" should remain");
    assert!(!has_unrewritten_src(&html, "png"), "no residual external src=\"...png\" should remain");
}

fn has_unrewritten_src(html: &str, ext: &str) -> bool {
    let needle_dq = format!(".{ext}\"");
    let needle_sq = format!(".{ext}'");
    for (i, _) in html.match_indices("src=") {
        let after = &html[i + 4..];
        let tail = after.get(..200).unwrap_or(after);
        if tail.starts_with('"') && tail.contains(&needle_dq) && !tail.starts_with("\"data:") {
            return true;
        }
        if tail.starts_with('\'') && tail.contains(&needle_sq) && !tail.starts_with("'data:") {
            return true;
        }
    }
    false
}
