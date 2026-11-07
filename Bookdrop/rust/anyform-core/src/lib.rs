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
    /// The output's companion folder already exists, but the output file
    /// does not, so the folder is not from an earlier conversion.
    #[error("A folder named \"{0}\" is already there.")]
    OutputExists(String),
    /// An option no registered plugin reads: usually a misspelled key.
    #[error("unknown option \"{0}\"")]
    UnknownOption(String),
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
            ConvError::OutputExists(_) => "output_exists",
            ConvError::UnknownOption(_) => "unknown_option",
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
    /// description. Default: no-op. Most hosts (the CLI, tests) do not need it.
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

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.values.keys().map(String::as_str)
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
    /// Every option name this plugin reads. The registry refuses options
    /// that no plugin declares.
    fn options(&self) -> &'static [&'static str];
    fn convert(&self, input: &Path, opts: &Options, log: &dyn Log) -> Result<IR, ConvError>;
}

pub trait OutputPlugin<IR>: Send + Sync {
    fn name(&self) -> &'static str;
    fn extension(&self) -> &'static str;
    /// Every option name this plugin reads. The registry refuses options
    /// that no plugin declares.
    fn options(&self) -> &'static [&'static str];
    /// A plugin that writes more than its one output file names the one
    /// folder it may also write: `companion_dir(output, suffix)`, next to
    /// the output. The registry moves it into place with the file, and
    /// replaces it when it replaces the file.
    fn companion_suffix(&self) -> Option<&'static str> {
        None
    }
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

    /// The registered input plugins, each once, sorted by name.
    pub fn input_plugins(&self) -> Vec<&Arc<dyn InputPlugin<IR>>> {
        let mut plugins: Vec<_> = self.inputs.values().collect();
        plugins.sort_by_key(|p| p.name());
        plugins.dedup_by_key(|p| p.name());
        plugins
    }

    /// The registered output plugins, sorted by name.
    pub fn output_plugins(&self) -> Vec<&Arc<dyn OutputPlugin<IR>>> {
        let mut plugins: Vec<_> = self.outputs.values().collect();
        plugins.sort_by_key(|p| p.name());
        plugins
    }

    /// A host sends one set of options to every call, so a name is valid
    /// if any registered plugin reads it.
    fn check_options(&self, opts: &Options) -> Result<(), ConvError> {
        let known = |name: &str| {
            self.inputs.values().any(|p| p.options().contains(&name))
                || self.outputs.values().any(|p| p.options().contains(&name))
        };
        match opts.names().find(|name| !known(name)) {
            Some(name) => Err(ConvError::UnknownOption(name.to_string())),
            None => Ok(()),
        }
    }

    pub fn parse(&self, input: &Path, opts: &Options, log: &dyn Log) -> Result<IR, ConvError> {
        self.check_options(opts)?;
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
    /// plugin for its extension. The plugin writes into a staging folder
    /// next to `output`, under the output's real file name. Only on
    /// success are the file, and its companion folder if any, moved into
    /// place: a failed or cancelled conversion never leaves a broken file,
    /// and never destroys the file it was to replace.
    pub fn write(&self, ir: &IR, output: &Path, opts: &Options, log: &dyn Log) -> Result<(), ConvError> {
        self.check_options(opts)?;
        let (plugin, _) = self.output_plugin(output)?;
        let parent = output.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        let file_name = output
            .file_name()
            .ok_or_else(|| ConvError::Other(format!("no file name in {}", output.display())))?;
        let companion = plugin.companion_suffix().map(|suffix| companion_dir(output, suffix));
        // A companion folder without its file is not ours to replace.
        if let Some(dir) = &companion {
            if dir.exists() && !output.exists() {
                return Err(ConvError::OutputExists(dir.file_name().unwrap_or_default().to_string_lossy().into_owned()));
            }
        }

        std::fs::create_dir_all(parent)?;
        let staging = tempfile::Builder::new().prefix(".bookdrop-").tempdir_in(parent)?;
        let staged_output = staging.path().join(file_name);
        plugin.convert(ir, &staged_output, opts, log)?;
        set_default_file_mode(&staged_output)?;

        // The folder goes first, so the new file never points at missing
        // images. An old folder is moved into the staging folder, which is
        // deleted when `staging` is dropped.
        if let Some(dir) = companion {
            let name = dir.file_name().expect("companion_dir always has a file name");
            if dir.exists() {
                std::fs::rename(&dir, staging.path().join(".replaced"))?;
            }
            let staged_dir = staging.path().join(name);
            if staged_dir.exists() {
                std::fs::rename(&staged_dir, &dir)?;
            }
        }
        std::fs::rename(&staged_output, output)?;
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

/// The companion folder of `output`: its file stem plus `suffix`, in the
/// same folder ("Book.md" with "_images" -> "Book_images").
pub fn companion_dir(output: &Path, suffix: &str) -> std::path::PathBuf {
    let mut name = output.file_stem().unwrap_or_default().to_os_string();
    name.push(suffix);
    output.with_file_name(name)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes "file:<tag>" to the output and "image:<tag>" into its
    /// companion folder, or fails after writing both.
    struct Fake {
        tag: &'static str,
        fail: bool,
    }

    impl OutputPlugin<()> for Fake {
        fn name(&self) -> &'static str {
            "fake"
        }
        fn extension(&self) -> &'static str {
            "fake"
        }
        fn options(&self) -> &'static [&'static str] {
            &[]
        }
        fn companion_suffix(&self) -> Option<&'static str> {
            Some("_files")
        }
        fn convert(&self, _ir: &(), output: &Path, _opts: &Options, _log: &dyn Log) -> Result<(), ConvError> {
            let dir = companion_dir(output, "_files");
            std::fs::create_dir_all(&dir)?;
            std::fs::write(dir.join("image.png"), format!("image:{}", self.tag))?;
            std::fs::write(output, format!("file:{}", self.tag))?;
            if self.fail {
                return Err(ConvError::Cancelled);
            }
            Ok(())
        }
    }

    fn registry(tag: &'static str, fail: bool) -> Registry<()> {
        let mut r = Registry::new();
        r.add_output(Arc::new(Fake { tag, fail }));
        r
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap()
    }

    fn staging_leftovers(dir: &Path) -> usize {
        std::fs::read_dir(dir).unwrap().filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().starts_with(".bookdrop-")).count()
    }

    #[test]
    fn the_file_and_its_companion_folder_are_moved_into_place() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("Book.fake");
        registry("new", false).write(&(), &out, &Options::new(), &StdLog).unwrap();
        assert_eq!(read(&out), "file:new");
        assert_eq!(read(&dir.path().join("Book_files/image.png")), "image:new");
        assert_eq!(staging_leftovers(dir.path()), 0);
    }

    #[test]
    fn replacing_the_file_replaces_its_companion_folder() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("Book.fake");
        registry("old", false).write(&(), &out, &Options::new(), &StdLog).unwrap();
        std::fs::write(dir.path().join("Book_files/stale.png"), "stale").unwrap();
        registry("new", false).write(&(), &out, &Options::new(), &StdLog).unwrap();
        assert_eq!(read(&out), "file:new");
        assert_eq!(read(&dir.path().join("Book_files/image.png")), "image:new");
        assert!(!dir.path().join("Book_files/stale.png").exists(), "the old folder's files must go with it");
        assert_eq!(staging_leftovers(dir.path()), 0);
    }

    #[test]
    fn a_failure_keeps_the_old_file_and_folder() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("Book.fake");
        registry("old", false).write(&(), &out, &Options::new(), &StdLog).unwrap();
        assert!(registry("new", true).write(&(), &out, &Options::new(), &StdLog).is_err());
        assert_eq!(read(&out), "file:old");
        assert_eq!(read(&dir.path().join("Book_files/image.png")), "image:old");
        assert_eq!(staging_leftovers(dir.path()), 0);
    }

    #[test]
    fn a_folder_without_its_file_is_never_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("Book.fake");
        std::fs::create_dir(dir.path().join("Book_files")).unwrap();
        std::fs::write(dir.path().join("Book_files/mine.txt"), "user data").unwrap();
        let err = registry("new", false).write(&(), &out, &Options::new(), &StdLog).unwrap_err();
        assert_eq!(err.code(), "output_exists");
        assert_eq!(read(&dir.path().join("Book_files/mine.txt")), "user data");
        assert!(!out.exists());
    }
}
