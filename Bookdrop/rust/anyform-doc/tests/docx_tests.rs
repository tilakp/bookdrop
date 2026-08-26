use std::path::PathBuf;

use anyform_core::{Options, Priority, StdLog, Value};
use docx_rs::{read_docx, DocumentChild, ParagraphChild, RunChild};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn convert(name: &str, epub: &str) -> Vec<u8> {
    convert_with_options(name, epub, Options::new())
}

fn convert_with_options(name: &str, epub: &str, opts: Options) -> Vec<u8> {
    let output = std::env::temp_dir().join(format!("anyform-docx-test-{name}-{}.docx", std::process::id()));
    let registry = anyform_doc::document_registry();
    registry
        .convert(&fixtures_dir().join(epub), &output, &opts, &StdLog)
        .unwrap_or_else(|e| panic!("{epub} should convert to DOCX: {e}"));
    let bytes = std::fs::read(&output).expect("output should be readable");
    let _ = std::fs::remove_file(&output);
    bytes
}

fn opts(pairs: &[(&str, Value)]) -> Options {
    let mut opts = Options::new();
    for (name, value) in pairs {
        opts.set(name, value.clone(), Priority::UserSet);
    }
    opts
}

/// Flattens every run's text across the whole document, in order - the
/// same acceptance bar as the Swift original's own round-trip test: what
/// matters is that a real reader (here, docx-rs's own `read_docx`, the
/// same model used by both its reader and writer) gets the right content
/// back, not just that bytes were written.
fn full_text(docx: &docx_rs::Docx) -> String {
    let mut text = String::new();
    for child in &docx.document.children {
        if let DocumentChild::Paragraph(p) = child {
            for pc in &p.children {
                if let ParagraphChild::Run(r) = pc {
                    for rc in &r.children {
                        if let RunChild::Text(t) = rc {
                            text.push_str(&t.text);
                        }
                    }
                }
            }
            text.push('\n');
        }
    }
    text
}

#[test]
fn produces_docx_that_reads_back() {
    let bytes = convert("readback", "minimal.epub");
    let docx = read_docx(&bytes).expect("output should be a valid, readable DOCX");
    let text = full_text(&docx);
    assert!(text.contains("Minimal Fixture Book"));
    assert!(text.contains("Test Author"));
    assert!(text.contains("Chapter One"));
    assert!(text.contains("Chapter Two"));
    assert!(text.contains("first chapter of the minimal fixture book"));
}

#[test]
fn preserves_heading_structure() {
    let bytes = convert("headings", "minimal.epub");
    let docx = read_docx(&bytes).unwrap();
    let has_heading1 = docx.document.children.iter().any(|c| {
        matches!(c, DocumentChild::Paragraph(p) if p.property.style.as_ref().is_some_and(|s| s.val == "Heading1"))
    });
    assert!(has_heading1, "expected at least one Heading1-styled paragraph (from each chapter's <h1>)");
}

#[test]
fn includes_cover_when_enabled() {
    let bytes = convert_with_options("cover-on", "minimal.epub", opts(&[("include_cover", Value::Bool(true))]));
    let docx = read_docx(&bytes).unwrap();
    let has_drawing = docx.document.children.iter().any(|c| {
        matches!(c, DocumentChild::Paragraph(p) if p.children.iter().any(|pc| {
            matches!(pc, ParagraphChild::Run(r) if r.children.iter().any(|rc| matches!(rc, RunChild::Drawing(_))))
        }))
    });
    assert!(has_drawing, "expected an embedded image when include_cover is true");
}

#[test]
fn omits_cover_when_disabled() {
    let bytes = convert_with_options("cover-off", "minimal.epub", opts(&[("include_cover", Value::Bool(false))]));
    let docx = read_docx(&bytes).unwrap();
    let has_drawing = docx.document.children.iter().any(|c| {
        matches!(c, DocumentChild::Paragraph(p) if p.children.iter().any(|pc| {
            matches!(pc, ParagraphChild::Run(r) if r.children.iter().any(|rc| matches!(rc, RunChild::Drawing(_))))
        }))
    });
    assert!(!has_drawing, "expected no embedded image when include_cover is false");
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
    let output = std::env::temp_dir().join(format!("anyform-docx-test-empty-{}.docx", std::process::id()));
    use anyform_core::OutputPlugin;
    let result = anyform_doc::DocxOutput.convert(&ir, &output, &Options::new(), &StdLog);
    let _ = std::fs::remove_file(&output);
    assert!(result.is_err());
}

#[test]
fn real_book_converts_with_plausible_content() {
    // Loose bound calibrated from an actual run (4,481 paragraphs
    // measured directly), not guessed, following this session's
    // established real-book-test pattern.
    let bytes = convert("real-book", "pride-and-prejudice.epub");
    let docx = read_docx(&bytes).expect("real book output should be a valid, readable DOCX");
    let para_count = docx.document.children.iter().filter(|c| matches!(c, DocumentChild::Paragraph(_))).count();
    assert!((3_000..=6_000).contains(&para_count), "expected a plausible paragraph count, got {para_count}");
    let text = full_text(&docx);
    assert!(text.contains("Mr. Darcy"));
    assert!(text.contains("Elizabeth"));
}
