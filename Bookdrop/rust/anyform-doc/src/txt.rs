use std::path::Path;

use anyform_core::{ConvError, Log, Options, OutputPlugin};

use crate::htmltext;
use crate::ir::DocumentIR;

/// Plain-text output - the Rust port of Bookdrop's Swift `TxtConverter`.
/// Behavioral note carried over from the Swift original (see its own
/// implementation plan, Risk 5): `NSAttributedString`'s HTML import did
/// full layout-aware text extraction; `htmltext::extract_text` won't match
/// it whitespace-for-whitespace. The Swift tests never asserted exact
/// whitespace either, only presence of the right text plus "no `<`/`>`
/// characters anywhere in the output" - match those assertions, not exact
/// bytes.
pub struct TxtOutput;

impl OutputPlugin<DocumentIR> for TxtOutput {
    fn name(&self) -> &'static str {
        "txt"
    }

    fn extension(&self) -> &'static str {
        "txt"
    }

    fn convert(&self, ir: &DocumentIR, output: &Path, _opts: &Options, log: &dyn Log) -> Result<(), ConvError> {
        if ir.spine.is_empty() {
            return Err(ConvError::Other("This book has no readable chapters.".into()));
        }

        let total = ir.spine.len();
        let mut chapters = Vec::with_capacity(total);
        for (i, item) in ir.spine.iter().enumerate() {
            if log.is_cancelled() {
                return Err(ConvError::Cancelled);
            }
            log.progress(i as f64 / total as f64, &format!("Extracting chapter {} of {}", i + 1, total));

            let path = ir.content_dir.join(&item.href);
            let raw = std::fs::read_to_string(&path).map_err(|_| ConvError::MissingFile(item.href.clone()))?;
            let text = htmltext::extract_text(&raw);
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                chapters.push(trimmed.to_string());
            }
        }

        let mut body = format!("{}\n", ir.metadata.title);
        if let Some(author) = &ir.metadata.author {
            body.push_str(author);
            body.push('\n');
        }
        body.push_str("\n\n");
        body.push_str(&chapters.join("\n\n\n"));
        body.push('\n');

        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(output, body)?;
        log.progress(1.0, "Done");
        Ok(())
    }
}
