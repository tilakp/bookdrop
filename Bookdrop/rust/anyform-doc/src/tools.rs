//! Finds the bundled helper programs: Chromium (PDF output), pdfium (PDF
//! input) and boko (Kindle input).

use std::path::PathBuf;

use anyform_core::{ConvError, Options};

use crate::options;

pub(crate) struct Tool {
    /// What the error message calls the tool.
    description: &'static str,
    option: &'static str,
    env_var: &'static str,
    /// Path under `rust/vendor/`, with `{platform}` for "mac-arm64" or
    /// "mac-x64".
    vendor_path: &'static str,
    fetch_script: &'static str,
}

pub(crate) const CHROMIUM: Tool = Tool {
    description: "Chromium binary",
    option: options::CHROMIUM_PATH,
    env_var: "ANYFORM_CHROMIUM_PATH",
    vendor_path: "chromium/{platform}/chrome-headless-shell-{platform}/chrome-headless-shell",
    fetch_script: "fetch-chromium.sh",
};

pub(crate) const PDFIUM: Tool = Tool {
    description: "PDF reader",
    option: options::PDFIUM_PATH,
    env_var: "ANYFORM_PDFIUM_PATH",
    vendor_path: "pdfium/{platform}/libpdfium.dylib",
    fetch_script: "fetch-pdfium.sh",
};

pub(crate) const BOKO: Tool = Tool {
    description: "ebook converter",
    option: options::BOKO_PATH,
    env_var: "ANYFORM_BOKO_PATH",
    vendor_path: "boko/{platform}/boko",
    fetch_script: "fetch-boko.sh",
};

impl Tool {
    /// The first path that exists: the option, then the environment
    /// variable, then the copy that `scripts/<fetch_script>` vendors, so
    /// `cargo test` and `anyform-cli` work without the app. The app always
    /// sets the option to the copy in its bundle.
    pub(crate) fn resolve(&self, opts: &Options) -> Result<PathBuf, ConvError> {
        let platform = if cfg!(target_arch = "aarch64") { "mac-arm64" } else { "mac-x64" };
        let vendored = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../vendor")
            .join(self.vendor_path.replace("{platform}", platform));
        let candidates = [
            opts.get_str(self.option).map(PathBuf::from),
            std::env::var(self.env_var).ok().map(PathBuf::from),
            Some(vendored),
        ];
        candidates.into_iter().flatten().find(|p| p.exists()).ok_or_else(|| {
            ConvError::ToolMissing(format!(
                "no bundled {} found: set the \"{}\" option or {}, or run scripts/{}",
                self.description, self.option, self.env_var, self.fetch_script
            ))
        })
    }
}
