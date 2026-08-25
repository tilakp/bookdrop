//! C ABI surface for the `anyform` engine — the boundary Bookdrop's Swift
//! app calls into (plan Phase 3/4). Uses JSON for all structured data
//! rather than mirrored C structs: far less ABI-fragile, and keeps Swift's
//! existing `Book`/`PDFOptions` types as the source of truth (Swift decodes
//! JSON into its own types; Rust never needs to know their exact layout).
//!
//! Header is generated with `cbindgen --crate anyform-ffi --output
//! include/anyform.h` (run manually, not on every build — see
//! `rust/anyform-ffi/README.md`).

use std::ffi::{c_char, c_void, CStr, CString};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyform_core::{Log, Options, Priority, Value};
use anyform_doc::{DocumentIR, TocNode};
use serde::Serialize;

/// Opaque cancellation handle for an in-flight `anyform_convert_start` call.
pub struct AnyformCancelToken {
    cancelled: Arc<AtomicBool>,
}

pub type ProgressCallback = extern "C" fn(fraction: f64, stage: *const c_char, ctx: *mut c_void);
pub type CompleteCallback = extern "C" fn(success: i32, error_json: *const c_char, ctx: *mut c_void);

/// Wraps the C callbacks as an `anyform_core::Log` so plugins can report
/// progress/cancellation without knowing FFI exists. Raw pointers aren't
/// `Send`/`Sync` by default; the caller (Swift) is responsible for the
/// `ctx` pointer staying valid until `on_complete` fires, same contract as
/// any C callback API.
struct FfiLog {
    cancelled: Arc<AtomicBool>,
    on_progress: Option<ProgressCallback>,
    progress_ctx: usize,
}
unsafe impl Send for FfiLog {}
unsafe impl Sync for FfiLog {}

impl Log for FfiLog {
    fn info(&self, msg: &str) {
        eprintln!("[anyform] {msg}");
    }

    fn progress(&self, fraction: f64, stage: &str) {
        if let Some(cb) = self.on_progress {
            if let Ok(c_stage) = CString::new(stage) {
                cb(fraction, c_stage.as_ptr(), self.progress_ctx as *mut c_void);
            }
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

// ---- Book-info JSON (for `anyform_parse_epub`) ----

#[derive(Serialize)]
struct SpineItemWire {
    id: String,
    href: String,
}

#[derive(Serialize)]
struct TocNodeWire {
    title: String,
    href: Option<String>,
    children: Vec<TocNodeWire>,
}

impl From<&TocNode> for TocNodeWire {
    fn from(n: &TocNode) -> Self {
        TocNodeWire {
            title: n.title.clone(),
            href: n.href.clone(),
            children: n.children.iter().map(TocNodeWire::from).collect(),
        }
    }
}

#[derive(Serialize)]
struct BookInfo {
    title: String,
    author: Option<String>,
    cover_base64: Option<String>,
    file_size_bytes: u64,
    spine: Vec<SpineItemWire>,
    toc: Vec<TocNodeWire>,
}

impl From<&DocumentIR> for BookInfo {
    fn from(ir: &DocumentIR) -> Self {
        BookInfo {
            title: ir.metadata.title.clone(),
            author: ir.metadata.author.clone(),
            cover_base64: ir.metadata.cover.as_ref().map(|c| base64_encode(c)),
            file_size_bytes: ir.file_size_bytes,
            spine: ir
                .spine
                .iter()
                .map(|s| SpineItemWire {
                    id: s.id.clone(),
                    href: s.href.clone(),
                })
                .collect(),
            toc: ir.toc.iter().map(TocNodeWire::from).collect(),
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
enum ParseResult {
    Ok { book: BookInfo },
    Error { message: String },
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
enum ErrorResult {
    Error { message: String },
}

/// Parses an EPUB and returns Book-info JSON (`{"status":"ok","book":{...}}`
/// or `{"status":"error","message":"..."}`) for the UI's file-loaded panel.
/// Caller must free the returned pointer with `anyform_free_string`.
///
/// # Safety
/// `path` must be a valid, NUL-terminated UTF-8 C string.
#[no_mangle]
pub unsafe extern "C" fn anyform_parse_epub(path: *const c_char) -> *mut c_char {
    let result: Result<BookInfo, String> = (|| {
        let path = c_str_to_pathbuf(path)?;
        let registry = anyform_doc::document_registry();
        let ir = registry
            .parse(&path, &Options::new(), &anyform_core::StdLog)
            .map_err(|e| e.to_string())?;
        Ok(BookInfo::from(&ir))
    })();

    let payload = match result {
        Ok(book) => serde_json::to_string(&ParseResult::Ok { book }),
        Err(message) => serde_json::to_string(&ParseResult::Error { message }),
    }
    .unwrap_or_else(|e| format!("{{\"status\":\"error\",\"message\":{:?}}}", e.to_string()));

    string_to_c(payload)
}

/// Starts an async conversion on a background thread. `on_progress` may be
/// called zero or more times before `on_complete` fires exactly once.
/// Returns an owned `AnyformCancelToken*` the caller must eventually free
/// with `anyform_free_cancel_token` (after `on_complete` has fired, or
/// after cancelling).
///
/// # Safety
/// `input_path`/`output_path`/`options_json` must be valid NUL-terminated
/// UTF-8 C strings. `progress_ctx`/`complete_ctx` are passed back verbatim
/// to the callbacks on whatever thread the conversion runs on — the caller
/// must ensure they stay valid and are safe to use from that thread until
/// `on_complete` fires.
#[no_mangle]
pub unsafe extern "C" fn anyform_convert_start(
    input_path: *const c_char,
    output_path: *const c_char,
    options_json: *const c_char,
    on_progress: Option<extern "C" fn(fraction: f64, stage: *const c_char, ctx: *mut c_void)>,
    progress_ctx: *mut c_void,
    on_complete: Option<extern "C" fn(success: i32, error_json: *const c_char, ctx: *mut c_void)>,
    complete_ctx: *mut c_void,
) -> *mut AnyformCancelToken {
    let cancelled = Arc::new(AtomicBool::new(false));
    let token = Box::into_raw(Box::new(AnyformCancelToken {
        cancelled: cancelled.clone(),
    }));

    let setup: Result<(PathBuf, PathBuf, Options), String> = (|| {
        let input = c_str_to_pathbuf(input_path)?;
        let output = c_str_to_pathbuf(output_path)?;
        let opts = options_from_json(options_json)?;
        Ok((input, output, opts))
    })();

    let (input, output, opts) = match setup {
        Ok(v) => v,
        Err(message) => {
            complete_with_error(on_complete, complete_ctx, message);
            return token;
        }
    };

    let progress_ctx_addr = progress_ctx as usize;
    let complete_ctx_addr = complete_ctx as usize;

    std::thread::spawn(move || {
        let log = FfiLog {
            cancelled,
            on_progress,
            progress_ctx: progress_ctx_addr,
        };
        let registry = anyform_doc::document_registry();
        let result = registry.convert(&input, &output, &opts, &log);

        let (success, error_json) = match result {
            Ok(()) => (1, None),
            Err(e) => (
                0,
                Some(
                    serde_json::to_string(&ErrorResult::Error { message: e.to_string() })
                        .unwrap_or_else(|_| "{\"status\":\"error\",\"message\":\"unknown error\"}".into()),
                ),
            ),
        };

        if let Some(cb) = on_complete {
            match &error_json {
                Some(json) => {
                    if let Ok(c_json) = CString::new(json.as_str()) {
                        cb(success, c_json.as_ptr(), complete_ctx_addr as *mut c_void);
                    }
                }
                None => cb(success, std::ptr::null(), complete_ctx_addr as *mut c_void),
            }
        }
    });

    token
}

fn complete_with_error(on_complete: Option<CompleteCallback>, ctx: *mut c_void, message: String) {
    if let Some(cb) = on_complete {
        let json = serde_json::to_string(&ErrorResult::Error { message }).unwrap_or_default();
        if let Ok(c_json) = CString::new(json) {
            cb(0, c_json.as_ptr(), ctx);
        }
    }
}

/// # Safety
/// `token` must be a pointer returned by `anyform_convert_start` that has
/// not yet been freed.
#[no_mangle]
pub unsafe extern "C" fn anyform_cancel(token: *mut AnyformCancelToken) {
    if token.is_null() {
        return;
    }
    (*token).cancelled.store(true, Ordering::Relaxed);
}

/// # Safety
/// `token` must be a pointer returned by `anyform_convert_start`, not
/// already freed, and not used again afterward.
#[no_mangle]
pub unsafe extern "C" fn anyform_free_cancel_token(token: *mut AnyformCancelToken) {
    if token.is_null() {
        return;
    }
    drop(Box::from_raw(token));
}

/// # Safety
/// `s` must be a pointer returned by one of this crate's functions, not
/// already freed, and not used again afterward.
#[no_mangle]
pub unsafe extern "C" fn anyform_free_string(s: *mut c_char) {
    if s.is_null() {
        return;
    }
    drop(CString::from_raw(s));
}

// ---- helpers ----

unsafe fn c_str_to_pathbuf(s: *const c_char) -> Result<PathBuf, String> {
    if s.is_null() {
        return Err("null path".to_string());
    }
    CStr::from_ptr(s)
        .to_str()
        .map(|s| PathBuf::from(s.to_string()))
        .map_err(|e| format!("invalid UTF-8 path: {e}"))
}

unsafe fn options_from_json(s: *const c_char) -> Result<Options, String> {
    let mut opts = Options::new();
    if s.is_null() {
        return Ok(opts);
    }
    let text = CStr::from_ptr(s).to_str().map_err(|e| e.to_string())?;
    if text.trim().is_empty() {
        return Ok(opts);
    }
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("invalid options JSON: {e}"))?;
    let serde_json::Value::Object(map) = value else {
        return Ok(opts);
    };
    for (key, v) in map {
        let value = match v {
            serde_json::Value::Bool(b) => Value::Bool(b),
            serde_json::Value::Number(n) => Value::Float(n.as_f64().unwrap_or(0.0)),
            serde_json::Value::String(s) => Value::Str(s),
            _ => continue,
        };
        opts.set(&key, value, Priority::UserSet);
    }
    Ok(opts)
}

fn string_to_c(s: String) -> *mut c_char {
    CString::new(s)
        .unwrap_or_else(|_| CString::new("{\"status\":\"error\",\"message\":\"internal: NUL in output\"}").unwrap())
        .into_raw()
}

fn base64_encode(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}
