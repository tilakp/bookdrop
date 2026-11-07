use std::path::PathBuf;

use anyform_core::{Options, OutputPlugin, StdLog};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn convert(name: &str, epub: &str) -> String {
    let output = std::env::temp_dir().join(format!("anyform-txt-test-{name}-{}.txt", std::process::id()));
    let registry = anyform_doc::document_registry();
    registry
        .convert(&fixtures_dir().join(epub), &output, &Options::new(), &StdLog)
        .unwrap_or_else(|e| panic!("{epub} should convert to TXT: {e}"));
    let text = std::fs::read_to_string(&output).expect("output should be valid UTF-8");
    let _ = std::fs::remove_file(&output);
    text
}

#[test]
fn includes_title_author_and_all_chapters() {
    let text = convert("basic", "minimal.epub");
    assert!(text.contains("Minimal Fixture Book"));
    assert!(text.contains("Test Author"));
    assert!(text.contains("Chapter One"));
    assert!(text.contains("Chapter Two"));
    assert!(text.contains("first chapter of the minimal fixture book"));
}

#[test]
fn strips_all_markup() {
    // The main promise of TXT output: no markup survives, not only that
    // the body text is there.
    let text = convert("no-markup", "minimal.epub");
    assert!(!text.contains('<') && !text.contains('>'));
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
        work_dir: None,
    };
    let output = std::env::temp_dir().join(format!("anyform-txt-test-empty-{}.txt", std::process::id()));
    let result = anyform_doc::TxtOutput.convert(&ir, &output, &Options::new(), &StdLog);
    let _ = std::fs::remove_file(&output);
    assert!(result.is_err());
}

#[test]
fn decodes_entities_and_drops_script_and_style() {
    // Forces the htmltext.rs regex fallback (the chapter is deliberately
    // non-well-formed XML - an unclosed <br> and undeclared HTML named
    // entities) - the only test exercising that path end to end rather
    // than the roxmltree fast path.
    let text = convert("entities", "text-entities.epub");
    assert!(text.contains('\u{2014}'), "em-dash entities (&mdash;/&#8212;/&#x2014;) should all decode");
    assert!(text.contains('\u{00A0}') || text.contains(' '), "&nbsp; should decode to some form of space");
    assert!(!text.contains("shouldNotAppear"), "script content must not leak into the output");
    assert!(!text.contains("font-size: 999px"), "style content must not leak into the output");
    assert!(!text.to_lowercase().contains("<title>"), "head/title content must not leak into the output");
}

#[test]
fn real_book_converts_to_plausible_plain_text() {
    // Loose structural bound calibrated from an actual run (744,619 bytes
    // measured directly), not guessed, following real_book_tests.rs's
    // established pattern - plus tight content spot-checks.
    let text = convert("real-book", "pride-and-prejudice.epub");
    assert!((500_000..=900_000).contains(&text.len()), "expected a plausible plain-text length, got {}", text.len());
    assert!(text.contains("Mr. Darcy"));
    assert!(text.contains("Elizabeth"));
    assert!(!text.contains('<') && !text.contains('>'));
}

#[test]
fn a_misspelled_option_is_refused_instead_of_ignored() {
    let registry = anyform_doc::document_registry();
    let mut opts = Options::new();
    opts.set("include_covr", anyform_core::Value::Bool(false));
    let output = std::env::temp_dir().join(format!("anyform-txt-test-unknown-option-{}.txt", std::process::id()));
    let err = registry.convert(&fixtures_dir().join("minimal.epub"), &output, &opts, &StdLog).unwrap_err();
    assert_eq!(err.code(), "unknown_option");
    assert!(err.to_string().contains("include_covr"));
    assert!(!output.exists());
}

#[test]
fn every_plugin_declares_its_options_and_plugins_are_listed_once() {
    let registry = anyform_doc::document_registry();
    let inputs: Vec<_> = registry.input_plugins().iter().map(|p| p.name()).collect();
    assert_eq!(inputs, ["epub", "kindle", "pdf"]);
    let outputs: Vec<_> = registry.output_plugins().iter().map(|p| p.name()).collect();
    assert_eq!(outputs, ["docx", "epub", "html", "markdown", "pdf", "txt"]);
    let html = registry.output_plugins().into_iter().find(|p| p.name() == "html").unwrap();
    assert_eq!(html.options(), ["include_cover", "generate_table_of_contents"]);
}
