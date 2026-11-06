use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// Errors from any input, output or transform plugin. One flat enum,
/// because every plugin returns through `Registry::convert` and callers
/// need one type to match on.
///
/// The `Display` text says what went wrong. What the user can do about it
/// is the host's job: the app maps `code()` to its own advice.
#[derive(thiserror::Error, Debug)]
pub enum ConvError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("no input plugin registered for extension \"{0}\"")]
    NoInputPlugin(String),
    #[error("no output plugin registered for extension \"{0}\"")]
    NoOutputPlugin(String),
    #[error("this file isn't a valid archive")]
    InvalidArchive,
    #[error("missing required file: {0}")]
    MissingFile(String),
    #[error("malformed document: {0}")]
    Malformed(String),
    #[error("conversion was cancelled")]
    Cancelled,
    #[error("This book is DRM-protected, so it can't be converted.")]
    DrmProtected,
    #[error("This PDF is password-protected, so it can't be converted.")]
    PasswordProtected,
    #[error("This PDF has no selectable text. It is a scan or a photo of pages.")]
    ScannedPdf,
    #[error("This PDF has a multi-column layout, which Bookdrop can't reflow yet.")]
    MultiColumnPdf,
    #[error("This book has no readable chapters.")]
    NoChapters,
    /// A bundled helper program (Chromium, pdfium, boko) was not found.
    #[error("{0}")]
    ToolMissing(String),
    #[error("{0}")]
    Other(String),
}

impl ConvError {
    /// A stable identifier for hosts. It crosses the FFI as `"code"`, so
    /// renaming one is a breaking change for the app.
    pub fn code(&self) -> &'static str {
        match self {
            ConvError::Io(_) => "io",
            ConvError::NoInputPlugin(_) => "unsupported_input",
            ConvError::NoOutputPlugin(_) => "unsupported_output",
            ConvError::InvalidArchive => "invalid_archive",
            ConvError::MissingFile(_) => "missing_file",
            ConvError::Malformed(_) => "malformed",
            ConvError::Cancelled => "cancelled",
            ConvError::DrmProtected => "drm_protected",
            ConvError::PasswordProtected => "password_protected",
            ConvError::ScannedPdf => "scanned_pdf",
            ConvError::MultiColumnPdf => "multi_column_pdf",
            ConvError::NoChapters => "no_chapters",
            ConvError::ToolMissing(_) => "tool_missing",
            ConvError::Other(_) => "other",
        }
    }
}

/// Doubles as the plugin↔host callback channel: logging, fractional
/// progress, and cooperative cancellation all flow through the one
/// `&dyn Log` every plugin call already threads through, rather than
/// growing `InputPlugin`/`OutputPlugin`/`Registry` signatures for each.
pub trait Log: Send + Sync {
    fn info(&self, msg: &str);
    /// Fractional progress (0.0..=1.0) plus a short human-readable stage
    /// description. Default no-op — most hosts (the CLI, tests) don't need it.
    fn progress(&self, _fraction: f64, _stage: &str) {}
    /// Plugins should poll this between chapters/pages and bail out with
    /// `ConvError::Cancelled` if true. Default: never cancelled.
    fn is_cancelled(&self) -> bool {
        false
    }
}

pub struct StdLog;
impl Log for StdLog {
    fn info(&self, msg: &str) {
        eprintln!("{msg}");
    }
}

#[derive(Debug, Clone)]
pub enum Value {
    Bool(bool),
    Float(f64),
    Str(String),
}

/// Named option values from the host. Each plugin reads the keys it knows,
/// with its own default, and ignores the rest.
#[derive(Default)]
pub struct Options {
    values: HashMap<String, Value>,
}

impl Options {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, name: &str, value: Value) {
        self.values.insert(name.to_string(), value);
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        self.values.get(name)
    }

    pub fn get_str(&self, name: &str) -> Option<&str> {
        match self.get(name) {
            Some(Value::Str(s)) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn get_bool(&self, name: &str, default: bool) -> bool {
        match self.get(name) {
            Some(Value::Bool(b)) => *b,
            _ => default,
        }
    }

    pub fn get_f64(&self, name: &str, default: f64) -> f64 {
        match self.get(name) {
            Some(Value::Float(f)) => *f,
            _ => default,
        }
    }
}

pub trait InputPlugin<IR>: Send + Sync {
    fn name(&self) -> &'static str;
    fn extensions(&self) -> &'static [&'static str];
    fn convert(&self, input: &Path, opts: &Options, log: &dyn Log) -> Result<IR, ConvError>;
}

pub trait OutputPlugin<IR>: Send + Sync {
    fn name(&self) -> &'static str;
    fn extension(&self) -> &'static str;
    fn convert(&self, ir: &IR, output: &Path, opts: &Options, log: &dyn Log) -> Result<(), ConvError>;
}

pub struct Registry<IR> {
    inputs: HashMap<&'static str, Arc<dyn InputPlugin<IR>>>,
    outputs: HashMap<&'static str, Arc<dyn OutputPlugin<IR>>>,
}

impl<IR> Registry<IR> {
    pub fn new() -> Self {
        Self {
            inputs: HashMap::new(),
            outputs: HashMap::new(),
        }
    }

    pub fn add_input(&mut self, plugin: Arc<dyn InputPlugin<IR>>) {
        for ext in plugin.extensions() {
            self.inputs.insert(ext, plugin.clone());
        }
    }

    pub fn add_output(&mut self, plugin: Arc<dyn OutputPlugin<IR>>) {
        self.outputs.insert(plugin.extension(), plugin);
    }

    pub fn parse(&self, input: &Path, opts: &Options, log: &dyn Log) -> Result<IR, ConvError> {
        let ext = ext_of(input)?;
        let plugin = self
            .inputs
            .get(ext.as_str())
            .ok_or_else(|| ConvError::NoInputPlugin(ext.clone()))?;
        plugin.convert(input, opts, log)
    }

    /// Parses `input` and writes it to `output`. Both plugins are picked by
    /// file extension. The output plugin is checked first, so an
    /// unsupported output format fails before any parsing work.
    pub fn convert(&self, input: &Path, output: &Path, opts: &Options, log: &dyn Log) -> Result<(), ConvError> {
        self.output_plugin(output)?;
        let ir = self.parse(input, opts, log)?;
        self.write(&ir, output, opts, log)
    }

    /// Writes an already parsed document to `output`, with the output
    /// plugin for its extension. The plugin writes to a temp file next to
    /// `output`, which is renamed into place only on success: a failed or
    /// cancelled conversion never leaves a broken file, and never destroys
    /// the file it was to replace.
    pub fn write(&self, ir: &IR, output: &Path, opts: &Options, log: &dyn Log) -> Result<(), ConvError> {
        let (plugin, ext) = self.output_plugin(output)?;
        let parent = output.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        std::fs::create_dir_all(parent)?;
        let staging = tempfile::Builder::new()
            .prefix(".bookdrop-")
            .suffix(&format!(".{ext}"))
            .tempfile_in(parent)?
            .into_temp_path();
        plugin.convert(ir, &staging, opts, log)?;
        set_default_file_mode(&staging)?;
        staging.persist(output).map_err(|e| ConvError::Io(e.error))?;
        Ok(())
    }

    fn output_plugin(&self, output: &Path) -> Result<(&Arc<dyn OutputPlugin<IR>>, String), ConvError> {
        let ext = ext_of(output)?;
        let plugin = self.outputs.get(ext.as_str()).ok_or_else(|| ConvError::NoOutputPlugin(ext.clone()))?;
        Ok((plugin, ext))
    }
}

impl<IR> Default for Registry<IR> {
    fn default() -> Self {
        Self::new()
    }
}

pub fn ext_of(path: &Path) -> Result<String, ConvError> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .ok_or_else(|| ConvError::Other(format!("no file extension on {}", path.display())))
}

/// A fresh, uniquely named working directory under
/// `$TMPDIR/Bookdrop/`. It is deleted when the returned value is dropped.
/// The shared parent folder lets the app's "Clear Temporary Files" remove
/// whatever a crash left behind.
pub fn fresh_work_dir(prefix: &str) -> std::io::Result<tempfile::TempDir> {
    let root = std::env::temp_dir().join("Bookdrop");
    std::fs::create_dir_all(&root)?;
    tempfile::Builder::new().prefix(prefix).tempdir_in(root)
}

/// `tempfile` creates files readable only by their owner (0600). A converted
/// book is a normal document, so give it the usual 0644.
fn set_default_file_mode(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644))?;
    }
    Ok(())
}

pub use tempfile::TempDir;
