//! Books are untrusted input. These tests build small hostile EPUBs at run
//! time and check that no conversion reads, writes or loads anything
//! outside the book, and that failures never leave broken files behind.
//! The PDF tests need the vendored Chromium (`scripts/fetch-chromium.sh`).

use std::io::Write;
use std::path::{Path, PathBuf};

use anyform_core::{InputPlugin, Options, StdLog};
use anyform_doc::EpubInput;

const XHTML: &str = "application/xhtml+xml";

/// A unique scratch folder for one test, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("anyform-hostile-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir.canonicalize().unwrap())
    }

    fn path(&self, file: &str) -> PathBuf {
        self.0.join(file)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn chapter(body: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><html xmlns=\"http://www.w3.org/1999/xhtml\">\
         <head><title>c</title></head><body>{body}</body></html>"
    )
}

/// Writes an EPUB whose OPF sits at `OEBPS/content.opf`. `manifest` is
/// (id, href as written in the OPF); every item is a spine chapter.
fn write_epub(path: &Path, title: &str, manifest: &[(&str, &str)], files: &[(&str, String)]) {
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    let stored = zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file("mimetype", stored).unwrap();
    zip.write_all(b"application/epub+zip").unwrap();
    zip.start_file("META-INF/container.xml", stored).unwrap();
    zip.write_all(
        br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
    )
    .unwrap();
    let items: String = manifest.iter().map(|(id, href)| format!(r#"<item id="{id}" href="{href}" media-type="{XHTML}"/>"#)).collect();
    let refs: String = manifest.iter().map(|(id, _)| format!(r#"<itemref idref="{id}"/>"#)).collect();
    zip.start_file("OEBPS/content.opf", stored).unwrap();
    write!(
        zip,
        r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>{title}</dc:title></metadata><manifest>{items}</manifest><spine>{refs}</spine></package>"#
    )
    .unwrap();
    for (name, content) in files {
        zip.start_file(*name, stored).unwrap();
        zip.write_all(content.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
}

fn convert(input: &Path, output: &Path) -> Result<(), anyform_core::ConvError> {
    anyform_doc::document_registry().convert(input, output, &Options::new(), &StdLog)
}

/// Text of a PDF, read back through the engine's own PDF input.
fn pdf_text(pdf: &Path, scratch: &Scratch) -> String {
    let txt = scratch.path("readback.txt");
    convert(pdf, &txt).expect("rendered PDF should read back");
    std::fs::read_to_string(txt).unwrap()
}

#[test]
fn manifest_hrefs_outside_the_book_are_dropped() {
    let scratch = Scratch::new("outside-href");
    let secret = scratch.path("secret.html");
    std::fs::write(&secret, chapter("<p>SECRET-OUTSIDE-FILE</p>")).unwrap();
    let book = scratch.path("book.epub");
    let absolute = secret.display().to_string();
    write_epub(
        &book,
        "Hostile",
        &[("c1", "ch1.xhtml"), ("abs", &absolute), ("up", "../../secret.html"), ("url", "file:///etc/hosts")],
        &[("OEBPS/ch1.xhtml", chapter("<p>Real chapter.</p>"))],
    );

    let ir = EpubInput.convert(&book, &Options::new(), &StdLog).unwrap();
    let hrefs: Vec<&str> = ir.spine.iter().map(|s| s.href.as_str()).collect();
    assert_eq!(hrefs, ["ch1.xhtml"], "only the in-book chapter may survive parsing");

    for ext in ["txt", "epub", "html"] {
        let out = scratch.path(&format!("out.{ext}"));
        convert(&book, &out).unwrap();
        let bytes = std::fs::read(&out).unwrap();
        let leaked = String::from_utf8_lossy(&bytes).contains("SECRET-OUTSIDE-FILE");
        assert!(!leaked, "{ext} output must not contain a file from outside the book");
    }
}

#[test]
fn percent_encoded_hrefs_resolve() {
    let scratch = Scratch::new("pct-href");
    let book = scratch.path("book.epub");
    write_epub(
        &book,
        "Encoded",
        &[("c1", "chapter%201.xhtml")],
        &[("OEBPS/chapter 1.xhtml", chapter("<p>Space in the file name.</p>"))],
    );
    let txt = scratch.path("out.txt");
    convert(&book, &txt).expect("an encoded href must find its file");
    assert!(std::fs::read_to_string(&txt).unwrap().contains("Space in the file name."));

    // The EPUB round trip must keep the reference valid in the OPF.
    let epub = scratch.path("out.epub");
    convert(&book, &epub).unwrap();
    let again = EpubInput.convert(&epub, &Options::new(), &StdLog).unwrap();
    assert_eq!(again.spine[0].href, "chapter 1.xhtml");
}

#[test]
fn work_directory_is_deleted_when_the_ir_is_dropped() {
    let book = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/minimal.epub");
    let ir = EpubInput.convert(&book, &Options::new(), &StdLog).unwrap();
    let work_dir = ir.work_dir.as_ref().expect("EpubInput owns its work dir").path().to_path_buf();
    assert!(work_dir.exists());
    drop(ir);
    assert!(!work_dir.exists(), "dropping the IR must remove the extracted book");
}

#[test]
fn failed_conversion_keeps_the_existing_output_and_leaves_no_temp_file() {
    let scratch = Scratch::new("atomic");
    let book = scratch.path("book.epub");
    // The chapter file is missing from the archive, so EpubOutput fails
    // after it has started writing.
    write_epub(&book, "Broken", &[("c1", "ch1.xhtml")], &[]);
    let output = scratch.path("out.epub");
    std::fs::write(&output, b"previous conversion").unwrap();

    assert!(convert(&book, &output).is_err());
    assert_eq!(std::fs::read(&output).unwrap(), b"previous conversion", "a failure must not destroy the old file");
    let leftovers: Vec<_> = std::fs::read_dir(&scratch.0)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with(".bookdrop-"))
        .collect();
    assert!(leftovers.is_empty(), "no staging file may be left behind");
}

#[test]
fn successful_output_is_world_readable() {
    let scratch = Scratch::new("mode");
    let book = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/minimal.epub");
    let out = scratch.path("out.txt");
    convert(&book, &out).unwrap();
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(&out).unwrap().permissions().mode() & 0o777, 0o644);
}

#[test]
fn pdf_rendering_blocks_local_files_and_scripts() {
    let scratch = Scratch::new("pdf-sandbox");
    let secret = scratch.path("secret.txt");
    std::fs::write(&secret, "SECRET-LOCAL-FILE").unwrap();
    let book = scratch.path("book.epub");
    let body = format!(
        r#"<p>Intro.</p><iframe src="file://{}" width="400" height="80"></iframe>
           <img src="https://example.invalid/track.png"/>
           <p id="js">SCRIPTS-OFF</p><script>document.getElementById("js").textContent = "SCRIPTS-RAN";</script>"#,
        secret.display()
    );
    write_epub(&book, "Hostile PDF", &[("c1", "ch1.xhtml")], &[("OEBPS/ch1.xhtml", chapter(&body))]);

    let pdf = scratch.path("out.pdf");
    convert(&book, &pdf).unwrap();
    let text = pdf_text(&pdf, &scratch);
    assert!(text.contains("Intro."), "the chapter itself must still render: {text}");
    assert!(!text.contains("SECRET-LOCAL-FILE"), "a local file outside the book must not be loaded");
    assert!(text.contains("SCRIPTS-OFF") && !text.contains("SCRIPTS-RAN"), "book scripts must not run");
}
