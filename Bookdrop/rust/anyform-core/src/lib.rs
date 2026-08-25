use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Errors surfaced anywhere in an input/output/transform pipeline.
///
/// Kept as one flat enum (rather than per-plugin error types) since every
/// plugin funnels into the same `Registry::convert` call site, and callers
/// (the CLI today, the FFI layer later) need one thing to match on.
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
    #[error("{0}")]
    Other(String),
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

/// Mirrors calibre's `OptionRecommendation` levels: a plugin default can be
/// overridden by a device/output profile, which can in turn be overridden by
/// an explicit user setting, but never the other way around.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    PluginDefault = 0,
    Profile = 1,
    UserSet = 2,
}

#[derive(Debug, Clone)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
}

pub struct OptionSpec {
    pub name: &'static str,
    pub default: Value,
    pub help: &'static str,
}

/// A set of option values, each tagged with the priority it was set at — a
/// lower-priority write never clobbers a higher-priority one already present.
#[derive(Default)]
pub struct Options {
    values: HashMap<String, (Value, Priority)>,
}

impl Options {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, name: &str, value: Value, priority: Priority) {
        if let Some((_, existing)) = self.values.get(name) {
            if *existing > priority {
                return;
            }
        }
        self.values.insert(name.to_string(), (value, priority));
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        self.values.get(name).map(|(v, _)| v)
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
            Some(Value::Int(i)) => *i as f64,
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
    fn options(&self) -> &'static [OptionSpec] {
        &[]
    }
    fn convert(&self, ir: &IR, output: &Path, opts: &Options, log: &dyn Log) -> Result<(), ConvError>;
}

/// Optional IR-level passes shared across output formats (hyphenation, font
/// subsetting, link rewriting) — analogous to calibre's `oeb/polish/*`.
pub trait Transform<IR>: Send + Sync {
    fn apply(&self, ir: &mut IR, opts: &Options) -> Result<(), ConvError>;
}

pub struct Registry<IR> {
    inputs: HashMap<&'static str, Arc<dyn InputPlugin<IR>>>,
    outputs: HashMap<&'static str, Arc<dyn OutputPlugin<IR>>>,
    transforms: Vec<Arc<dyn Transform<IR>>>,
}

impl<IR> Registry<IR> {
    pub fn new() -> Self {
        Self {
            inputs: HashMap::new(),
            outputs: HashMap::new(),
            transforms: Vec::new(),
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

    pub fn add_transform(&mut self, transform: Arc<dyn Transform<IR>>) {
        self.transforms.push(transform);
    }

    pub fn parse(&self, input: &Path, opts: &Options, log: &dyn Log) -> Result<IR, ConvError> {
        let ext = ext_of(input)?;
        let plugin = self
            .inputs
            .get(ext.as_str())
            .ok_or_else(|| ConvError::NoInputPlugin(ext.clone()))?;
        plugin.convert(input, opts, log)
    }

    pub fn convert(
        &self,
        input: &Path,
        output: &Path,
        opts: &Options,
        log: &dyn Log,
    ) -> Result<(), ConvError> {
        let out_ext = ext_of(output)?;
        let out_plugin = self
            .outputs
            .get(out_ext.as_str())
            .ok_or_else(|| ConvError::NoOutputPlugin(out_ext.clone()))?;

        let mut ir = self.parse(input, opts, log)?;
        for t in &self.transforms {
            t.apply(&mut ir, opts)?;
        }
        out_plugin.convert(&ir, output, opts, log)
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

/// Extracts a fresh unique working directory under the system temp dir,
/// mirroring Bookdrop's Swift `EpubParser` behavior of extracting each
/// EPUB into `NSTemporaryDirectory()/Bookdrop/<uuid>/`.
pub fn fresh_work_dir(namespace: &str) -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir()
        .join(namespace)
        .join(format!("{:x}", fastrand_u128()));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Tiny dependency-free random u128 for work-dir naming — not
/// cryptographic, just needs to not collide across concurrent conversions.
fn fastrand_u128() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let pid = std::process::id() as u128;
    nanos ^ (pid << 64)
}
