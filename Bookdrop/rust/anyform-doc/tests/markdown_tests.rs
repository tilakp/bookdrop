use std::path::{Path, PathBuf};

use anyform_core::{Options, StdLog, Value};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

/// A fresh folder for one test's output, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("anyform-md-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn convert(book: &str, output: &Path, opts: &Options) -> String {
    anyform_doc::document_registry()
        .convert(&fixture(book), output, opts, &StdLog)
        .unwrap_or_else(|e| panic!("{book} should convert to Markdown: {e}"));
    std::fs::read_to_string(output).unwrap()
}

/// The decoded targets of every `![alt](target)` in `md`.
fn image_targets(md: &str) -> Vec<String> {
    md.match_indices("](")
        .filter(|(i, _)| md[..*i].rfind("![").is_some_and(|open| !md[open..*i].contains('\n')))
        .filter_map(|(i, _)| md[i + 2..].split_once(')').map(|(t, _)| t.replace("%20", " ").replace("%28", "(").replace("%29", ")")))
        .collect()
}

#[test]
fn writes_title_author_headings_and_text() {
    let scratch = Scratch::new("basic");
    let md = convert("minimal.epub", &scratch.0.join("Book.md"), &Options::new());
    assert!(md.starts_with("# Minimal Fixture Book\n\nTest Author\n"), "{md}");
    assert!(md.contains("Chapter One"));
    assert!(md.contains("Chapter Two"));
    assert!(!md.contains("<p>") && !md.contains("</"), "no markup may remain: {md}");
}

#[test]
fn every_image_link_points_at_a_copied_file() {
    let scratch = Scratch::new("dolittle");
    let output = scratch.0.join("Doctor Dolittle (1).md");
    let md = convert("doctor-dolittle.epub", &output, &Options::new());
    let targets = image_targets(&md);
    assert!(targets.len() > 10, "an image-heavy book should keep its images, got {}", targets.len());
    for target in &targets {
        assert!(target.starts_with("Doctor Dolittle (1)_images/"), "{target}");
        assert!(scratch.0.join(target).is_file(), "missing image file for link {target}");
    }
    assert!(md.starts_with("# "), "{}", &md[..200.min(md.len())]);
    let covers = md.matches("![Cover]").count();
    assert_eq!(covers, 1, "the cover must be shown once");
}

#[test]
fn the_cover_can_be_left_out() {
    let scratch = Scratch::new("no-cover");
    let mut opts = Options::new();
    opts.set("include_cover", Value::Bool(false));
    let md = convert("doctor-dolittle.epub", &scratch.0.join("Book.md"), &opts);
    assert!(!md.contains("![Cover]"));
}

#[test]
fn a_book_without_images_writes_no_images_folder() {
    let scratch = Scratch::new("no-images");
    convert("heading-tiers.epub", &scratch.0.join("Book.md"), &Options::new());
    let entries: Vec<_> = std::fs::read_dir(&scratch.0).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(entries, ["Book.md"], "only the Markdown file, no empty folder and no staging leftovers");
}
