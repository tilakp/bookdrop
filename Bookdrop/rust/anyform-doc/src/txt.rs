use std::path::Path;

use anyform_core::{ConvError, Log, Options, OutputPlugin};

use crate::htmltext;
use crate::ir::DocumentIR;

/// Plain-text output, from `htmltext::extract_text`. The extraction is not
/// layout-aware, so whitespace can differ from what a browser shows. Tests
/// check the words and that no markup remains, not exact bytes.
pub struct TxtOutput;

impl OutputPlugin<DocumentIR> for TxtOutput {
    fn name(&self) -> &'static str {
        "txt"
    }

    fn extension(&self) -> &'static str {
        "txt"
    }

    fn options(&self) -> &'static [&'static str] {
        &[]
    }

    fn convert(&self, ir: &DocumentIR, output: &Path, _opts: &Options, log: &dyn Log) -> Result<(), ConvError> {
        if ir.spine.is_empty() {
            return Err(ConvError::NoChapters);
        }

        let total = ir.spine.len();
        let mut chapters = Vec::with_capacity(total);
        for (i, item) in ir.spine.iter().enumerate() {
            if log.is_cancelled() {
                return Err(ConvError::Cancelled);
            }
            log.progress(i as f64 / total as f64, &format!("Extracting chapter {} of {}", i + 1, total));

            let text = htmltext::extract_text(&ir.read_chapter(item)?);
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

        std::fs::write(output, body)?;
        log.progress(1.0, "Done");
        Ok(())
    }
}
