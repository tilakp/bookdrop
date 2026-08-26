use std::path::{Path, PathBuf};
use std::process::Command;

use anyform_core::{ConvError, InputPlugin, Log, Options};

use crate::epub::EpubInput;
use crate::ir::DocumentIR;

/// Reads Kindle-family ebook formats (AZW3, KFX, MOBI) by normalizing them
/// to EPUB with the bundled `boko` binary and then handing the result to
/// the existing [`EpubInput`] parser. One conversion step therefore unlocks
/// every output format the engine already supports, rather than needing a
/// bespoke path per input/output pair.
///
/// `boko` is invoked as a **separate process**, not linked as a crate: it
/// is GPL-3.0-or-later and Bookdrop is MIT, and exec'ing a separate program
/// does not trigger the GPL's linking clause. `rust/scripts/fetch-boko.sh`
/// vendors the binary (and its source tarball, per GPL-3 §6) the same way
/// `fetch-chromium.sh` vendors the headless renderer.
///
/// DRM is explicitly out of scope. Books bought from Amazon are encrypted,
/// and this deliberately makes no attempt to decrypt them — it detects the
/// failure and reports it plainly (see [`describe_boko_failure`]) so the
/// user gets a clear "this file is DRM-protected" message instead of a raw
/// subprocess error. That is a different thing from the EPUB font
/// *obfuscation* handled in `epub.rs`, which is a documented, reversible
/// scrambling scheme rather than encryption.
pub struct KindleInput;

impl InputPlugin<DocumentIR> for KindleInput {
    fn name(&self) -> &'static str {
        "kindle"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["azw3", "azw", "kfx", "mobi"]
    }

    fn convert(&self, input: &Path, opts: &Options, log: &dyn Log) -> Result<DocumentIR, ConvError> {
        let boko = resolve_boko_path(opts)?;
        let work_dir = anyform_core::fresh_work_dir("Bookdrop-kindle")?;
        let staged_epub = work_dir.join("normalized.epub");

        let label = input.file_name().and_then(|n| n.to_str()).unwrap_or("book");
        log.info(&format!("converting {label} to EPUB via bundled boko"));
        log.progress(0.0, "Reading Kindle file");

        let output = Command::new(&boko)
            .arg("convert")
            .arg(input)
            .arg(&staged_epub)
            .output()
            .map_err(|e| ConvError::Other(format!("failed to run the bundled ebook converter: {e}")))?;

        if !output.status.success() || !staged_epub.exists() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ConvError::Other(describe_boko_failure(&stderr)));
        }

        // The staged EPUB is a normal EPUB in every respect, so the whole
        // existing pipeline (font deobfuscation, TOC parsing, cover
        // detection) applies unchanged from here.
        log.progress(0.10, "Reading book structure");
        let mut ir = EpubInput.convert(&staged_epub, opts, log)?;

        // Report the *original* file's size, not the intermediate EPUB's —
        // the size shown in the UI should describe the file the user
        // actually picked.
        ir.file_size_bytes = std::fs::metadata(input).map(|m| m.len()).unwrap_or(ir.file_size_bytes);
        Ok(ir)
    }
}

/// Turns boko's stderr into something worth showing a user. DRM is the one
/// failure that is both common and genuinely unfixable, so it gets its own
/// message instead of being lumped in with malformed-file errors.
fn describe_boko_failure(stderr: &str) -> String {
    let lower = stderr.to_lowercase();
    if lower.contains("drm") || lower.contains("encrypted") || lower.contains("decrypt") {
        return "This book is DRM-protected, so it can't be converted. \
                Kindle books bought from Amazon are encrypted."
            .into();
    }
    let detail = stderr.lines().rfind(|l| !l.trim().is_empty()).unwrap_or("").trim();
    if detail.is_empty() {
        "This Kindle file couldn't be read.".into()
    } else {
        format!("This Kindle file couldn't be read: {detail}")
    }
}

/// Mirrors `resolve_chromium_path` in `pdf.rs`: an explicit option wins, then
/// an env var, then the vendored dev-tree copy so `cargo test` and
/// `anyform-cli` work without going through `build-app.sh`. The FFI layer
/// always passes an explicit path pointing at the app-bundle resource.
fn resolve_boko_path(opts: &Options) -> Result<PathBuf, ConvError> {
    if let Some(p) = opts.get_str("boko_path") {
        let path = PathBuf::from(p);
        if path.exists() {
            return Ok(path);
        }
    }
    if let Ok(p) = std::env::var("ANYFORM_BOKO_PATH") {
        let path = PathBuf::from(p);
        if path.exists() {
            return Ok(path);
        }
    }
    let platform = if cfg!(target_arch = "aarch64") { "mac-arm64" } else { "mac-x64" };
    let dev_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../vendor/boko")
        .join(platform)
        .join("boko");
    if dev_path.exists() {
        return Ok(dev_path);
    }

    Err(ConvError::Other(
        "no bundled ebook converter found — set the \"boko_path\" option, \
         ANYFORM_BOKO_PATH, or run scripts/fetch-boko.sh"
            .into(),
    ))
}
