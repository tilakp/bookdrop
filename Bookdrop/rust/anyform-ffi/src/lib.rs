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
            let c_stage = c_string_lossy(stage.to_string());
            cb(fraction, c_stage.as_ptr(), self.progress_ctx as *mut c_void);
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

// ---- Book-info JSON (for `anyform_parse_book`) ----

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

/// An error as the host sees it: `code` is `ConvError::code()` (or
/// "invalid_argument" / "internal" for failures in this layer), `message`
/// is the text to show.
#[derive(Serialize)]
struct WireError {
    code: &'static str,
    message: String,
}

impl From<anyform_core::ConvError> for WireError {
    fn from(e: anyform_core::ConvError) -> Self {
        WireError { code: e.code(), message: e.to_string() }
    }
}

impl WireError {
    fn invalid_argument(message: String) -> Self {
        WireError { code: "invalid_argument", message }
    }
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
enum ParseResult {
    Ok { book: BookInfo },
    Error(WireError),
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
enum ErrorResult {
    Error(WireError),
}

/// Parses any registered input format (EPUB, Kindle, PDF) and returns
/// `{"status":"ok","book":{...}}` or
/// `{"status":"error","code":"...","message":"..."}`.
/// Caller must free the returned pointer with `anyform_free_string`.
///
/// `options_json` may be null; it carries the same tool paths
/// (`boko_path`, `pdfium_path`) as `anyform_convert_start`.
///
/// # Safety
/// `path` must be a valid, NUL-terminated UTF-8 C string. `options_json`
/// must be null or a valid, NUL-terminated UTF-8 C string.
#[no_mangle]
pub unsafe extern "C" fn anyform_parse_book(path: *const c_char, options_json: *const c_char) -> *mut c_char {
    let result: Result<BookInfo, WireError> = catch_panic(|| {
        let path = c_str_to_pathbuf(path).map_err(WireError::invalid_argument)?;
        let opts = options_from_json(options_json).map_err(WireError::invalid_argument)?;
        let registry = anyform_doc::document_registry();
        let ir = registry.parse(&path, &opts, &anyform_core::StdLog)?;
        Ok(BookInfo::from(&ir))
    });

    let payload = match result {
        Ok(book) => serde_json::to_string(&ParseResult::Ok { book }),
        Err(error) => serde_json::to_string(&ParseResult::Error(error)),
    }
    .unwrap_or_else(|_| INTERNAL_ERROR_JSON.into());

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
            complete_with_error(on_complete, complete_ctx, WireError::invalid_argument(message));
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
        let result = catch_panic(|| {
            let registry = anyform_doc::document_registry();
            Ok(registry.convert(&input, &output, &opts, &log)?)
        });

        // `on_complete` must fire exactly once whatever happened above: the
        // Swift side waits on it, and a missed call would hang the UI.
        match (on_complete, result) {
            (Some(cb), Ok(())) => cb(1, std::ptr::null(), complete_ctx_addr as *mut c_void),
            (Some(_), Err(error)) => complete_with_error(on_complete, complete_ctx_addr as *mut c_void, error),
            (None, _) => {}
        }
    });

    token
}

const INTERNAL_ERROR_JSON: &str = r#"{"status":"error","code":"internal","message":"unknown error"}"#;

fn complete_with_error(on_complete: Option<CompleteCallback>, ctx: *mut c_void, error: WireError) {
    if let Some(cb) = on_complete {
        let json = serde_json::to_string(&ErrorResult::Error(error)).unwrap_or_else(|_| INTERNAL_ERROR_JSON.into());
        let c_json = c_string_lossy(json);
        cb(0, c_json.as_ptr(), ctx);
    }
}

/// Runs `f`, turning a panic into an error. A panic must never unwind out
/// of an `extern "C"` function (that aborts the whole app), and must never
/// skip the completion callback (that hangs it).
fn catch_panic<T>(f: impl FnOnce() -> Result<T, WireError>) -> Result<T, WireError> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|_| {
        Err(WireError { code: "internal", message: "Bookdrop hit an internal error while reading this book.".into() })
    })
}

/// `CString::new` fails on an interior NUL (possible in text from a
/// subprocess's stderr). Drop NULs instead of losing the whole message.
fn c_string_lossy(s: String) -> CString {
    CString::new(s.replace('\0', "")).expect("NULs were removed")
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
    c_string_lossy(s).into_raw()
}

fn base64_encode(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(path: &str) -> serde_json::Value {
        let path = CString::new(path).unwrap();
        let raw = unsafe { anyform_parse_book(path.as_ptr(), std::ptr::null()) };
        let json = unsafe { CStr::from_ptr(raw) }.to_str().unwrap().to_string();
        unsafe { anyform_free_string(raw) };
        serde_json::from_str(&json).unwrap()
    }

    #[test]
    fn parse_errors_carry_a_code_and_a_message() {
        let fixtures = concat!(env!("CARGO_MANIFEST_DIR"), "/../anyform-doc/tests/fixtures");
        let encrypted = parse(&format!("{fixtures}/encrypted.pdf"));
        assert_eq!(encrypted["status"], "error");
        assert_eq!(encrypted["code"], "password_protected");
        assert!(encrypted["message"].as_str().unwrap().contains("password-protected"));

        let unknown = parse("/tmp/book.unknown-extension");
        assert_eq!(unknown["code"], "unsupported_input");
    }

    #[test]
    fn parse_success_returns_the_book() {
        let book = parse(concat!(env!("CARGO_MANIFEST_DIR"), "/../anyform-doc/tests/fixtures/minimal.epub"));
        assert_eq!(book["status"], "ok");
        assert_eq!(book["book"]["title"], "Minimal Fixture Book");
    }
}
