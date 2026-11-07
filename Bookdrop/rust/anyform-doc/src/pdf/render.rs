//! Drives the bundled headless Chromium: launches it, opens locked-down
//! tabs, and prints each chapter (and the cover) to its own PDF.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use anyform_core::{ConvError, Log};
use headless_chrome::browser::tab::RequestPausedDecision;
use headless_chrome::protocol::cdp::{Emulation, Fetch, Network};
use headless_chrome::types::PrintToPdfOptions;
use headless_chrome::{Browser, LaunchOptions, Tab};
use lopdf::{Dictionary, Object};

use crate::href;
use crate::ir::DocumentIR;

use super::chapter_html::prepare_chapter_html;
use super::RenderOptions;
use crate::markup::{data_uri, sniff_image_mime};

/// Renders `indices` (spine positions) in parallel on a small pool of
/// Chrome tabs in one browser, sized to the CPU count (at most 8), as
/// calibre's PDF output does. This parallelism, not a different renderer,
/// is what made calibre faster. Work is split into contiguous chunks up
/// front, because chapters are usually of similar size. Results come back
/// in spine order, which the merge needs. `done_count` is shared, so the
/// progress stays monotonic although workers finish out of order.
#[allow(clippy::too_many_arguments)]
pub(super) fn render_chapters_parallel(
    browser: &Browser,
    ir: &DocumentIR,
    indices: &[usize],
    render_opts: &RenderOptions,
    toc_fragments: &HashMap<String, Vec<String>>,
    log: &dyn Log,
    done_count: &AtomicUsize,
    total_steps: usize,
) -> Result<Vec<(String, lopdf::Document)>, ConvError> {
    let worker_count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(8)
        .min(indices.len())
        .max(1);
    let chunk_size = indices.len().div_ceil(worker_count).max(1);

    type ChunkResult = Result<Vec<(usize, String, lopdf::Document)>, ConvError>;
    let results: Vec<ChunkResult> = std::thread::scope(|scope| {
        let handles: Vec<_> = indices
            .chunks(chunk_size)
            .map(|chunk| {
                scope.spawn(move || -> ChunkResult {
                    let tab = open_locked_down_tab(browser, &ir.content_dir)?;
                    let mut out = Vec::with_capacity(chunk.len());
                    for &i in chunk {
                        // Each worker polls the same shared cancellation flag
                        // independently, so a cancellation request is picked
                        // up by every in-flight tab as soon as it finishes
                        // its current chapter, not just the one that noticed
                        // it first.
                        if log.is_cancelled() {
                            return Err(ConvError::Cancelled);
                        }
                        let item = &ir.spine[i];
                        let fragments = toc_fragments.get(&item.href).map(Vec::as_slice).unwrap_or_default();
                        let html = prepare_chapter_html(ir.read_chapter(item)?, render_opts, fragments);
                        let path = ir.content_dir.join(&item.href);
                        let mut doc = tab.print(&path, &html, chapter_print_options(render_opts), &item.href)?;
                        namespace_chapter_dests(&mut doc, &format!("c{i}"));

                        let done = done_count.fetch_add(1, Ordering::SeqCst) + 1;
                        log.info(&format!("rendering {} ({done}/{total_steps})", item.href));
                        log.progress(done as f64 / total_steps as f64, &format!("Rendering chapter {done}/{total_steps}"));
                        out.push((i, item.href.clone(), doc));
                    }
                    Ok(out)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or_else(|_| Err(ConvError::Other("a chapter failed to render (internal error)".into()))))
            .collect()
    });

    let mut ordered: BTreeMap<usize, (String, lopdf::Document)> = BTreeMap::new();
    let mut first_error: Option<ConvError> = None;
    for result in results {
        match result {
            Ok(items) => ordered.extend(items.into_iter().map(|(i, href, doc)| (i, (href, doc)))),
            Err(e) => {
                let is_cancel = matches!(e, ConvError::Cancelled);
                if first_error.is_none() || is_cancel {
                    first_error = Some(e);
                }
            }
        }
    }
    if let Some(e) = first_error {
        return Err(e);
    }
    Ok(ordered.into_values().collect())
}

/// A Chrome tab that is locked down for untrusted book markup (see
/// `open_locked_down_tab`) and can serve one page from memory.
pub(super) struct BookTab {
    tab: Arc<Tab>,
    served: Arc<Mutex<Option<ServedPage>>>,
}

struct ServedPage {
    path: PathBuf,
    content_type: &'static str,
    body_base64: String,
}

impl BookTab {
    /// Prints `html` as the page at `path`. Chrome loads it from the URL of
    /// `path`, so relative images and stylesheets resolve as for the file,
    /// and a footnote link such as `thisfile.xhtml#note3` stays a link in
    /// the same document, which Chrome needs to make a working `/Dest`. The
    /// file on disk is not read or changed.
    pub(super) fn print(&self, path: &Path, html: &str, print_opts: PrintToPdfOptions, label: &str) -> Result<lopdf::Document, ConvError> {
        use base64::Engine;
        *lock(&self.served) = Some(ServedPage {
            path: path.to_path_buf(),
            content_type: content_type_for(path),
            body_base64: base64::engine::general_purpose::STANDARD.encode(html),
        });
        let result = self.navigate_and_print(path, print_opts, label);
        *lock(&self.served) = None;
        result
    }

    fn navigate_and_print(&self, path: &Path, print_opts: PrintToPdfOptions, label: &str) -> Result<lopdf::Document, ConvError> {
        let url = format!("file://{}", path.display());
        self.tab
            .navigate_to(&url)
            .map_err(|e| ConvError::Other(format!("failed to load {label}: {e}")))?;
        self.tab
            .wait_until_navigated()
            .map_err(|e| ConvError::Other(format!("failed to render {label}: {e}")))?;
        wait_for_fonts_ready(&self.tab);
        let bytes = self
            .tab
            .print_to_pdf(Some(print_opts))
            .map_err(|e| ConvError::Other(format!("print_to_pdf failed for {label}: {e}")))?;
        lopdf::Document::load_mem(&bytes).map_err(|e| ConvError::Other(format!("failed to parse rendered PDF for {label}: {e}")))
    }
}

/// Chrome picks the parser for a `file://` page from its extension. A
/// served page must get the same parser, because the strict XHTML parser
/// refuses markup that the HTML parser accepts.
fn content_type_for(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("xhtml" | "xht" | "xml") => "application/xhtml+xml; charset=utf-8",
        _ => "text/html; charset=utf-8",
    }
}

/// A poisoned lock only means another render panicked. The served page is
/// replaced before each use, so its value is still safe to use.
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Chrome's `print_to_pdf` turns a same-document fragment link (`<a
/// href="thisfile.html#note3">`, the usual EPUB footnote form) into a
/// working `/Dest /note3` link plus a `/Dests` entry on that chapter's
/// catalog. Each chapter is a separate PDF, so names such as `fn1` would
/// collide after the merge. This renames every entry in this chapter's
/// Dests dictionary to `{prefix}::{name}` and updates its Link annotations
/// to match, so `merge_chapter_pdfs` can combine all chapters' Dests.
fn namespace_chapter_dests(doc: &mut lopdf::Document, prefix: &str) {
    let dests_id = doc
        .objects
        .values()
        .find(|o| o.type_name().unwrap_or(b"") == b"Catalog")
        .and_then(|cat| cat.as_dict().ok())
        .and_then(|d| d.get(b"Dests").ok())
        .and_then(|o| o.as_reference().ok());
    let Some(dests_id) = dests_id else { return };

    let old_names: Vec<Vec<u8>> = match doc.get_object(dests_id).and_then(|o| o.as_dict()) {
        Ok(d) => d.iter().map(|(k, _)| k.clone()).collect(),
        Err(_) => return,
    };
    if old_names.is_empty() {
        return;
    }

    if let Ok(dict) = doc.get_object_mut(dests_id).and_then(|o| o.as_dict_mut()) {
        let mut renamed = Dictionary::new();
        for (key, value) in dict.iter() {
            renamed.set(format!("{prefix}::{}", String::from_utf8_lossy(key)), value.clone());
        }
        *dict = renamed;
    }

    let old_name_set: std::collections::HashSet<Vec<u8>> = old_names.into_iter().collect();
    for object in doc.objects.values_mut() {
        let Ok(dict) = object.as_dict_mut() else { continue };
        if dict.get(b"Subtype").and_then(|o| o.as_name()).unwrap_or(b"") != b"Link" {
            continue;
        }
        if let Ok(Object::Name(name)) = dict.get_mut(b"Dest") {
            if old_name_set.contains(name.as_slice()) {
                let mut renamed = format!("{prefix}::").into_bytes();
                renamed.extend_from_slice(name);
                *name = renamed;
            }
        }
    }
}

fn chapter_print_options(opts: &RenderOptions) -> PrintToPdfOptions {
    PrintToPdfOptions {
        print_background: Some(true),
        paper_width: Some(opts.page_width_in),
        paper_height: Some(opts.page_height_in),
        margin_top: Some(opts.margin_in),
        margin_bottom: Some(opts.margin_in),
        margin_left: Some(opts.margin_in),
        margin_right: Some(opts.margin_in),
        prefer_css_page_size: Some(false),
        ..Default::default()
    }
}

fn cover_print_options(opts: &RenderOptions) -> PrintToPdfOptions {
    PrintToPdfOptions {
        print_background: Some(true),
        paper_width: Some(opts.page_width_in),
        paper_height: Some(opts.page_height_in),
        margin_top: Some(0.0),
        margin_bottom: Some(0.0),
        margin_left: Some(0.0),
        margin_right: Some(0.0),
        prefer_css_page_size: Some(false),
        ..Default::default()
    }
}

/// Waits for `document.fonts.ready` before printing. `wait_until_navigated`
/// waits only for the load event, and `@font-face` fonts can finish later.
/// Printing during a font swap can give corrupted text. calibre waits for
/// the same reason (`settle_time` in `html_writer.py`). A failure here is
/// ignored: one chapter must not stop the whole conversion.
fn wait_for_fonts_ready(tab: &Tab) {
    let _ = tab.evaluate(
        "(document.fonts ? document.fonts.ready : Promise.resolve()).then(() => true)",
        true,
    );
}

/// Book markup is untrusted input. Each tab runs with page scripts off and
/// may load only files inside the book (and `data:` URIs): no network, and
/// no other local files, which an `<iframe src="file:///...">` could
/// otherwise print into the PDF.
pub(super) fn open_locked_down_tab(browser: &Browser, content_dir: &Path) -> Result<BookTab, ConvError> {
    fn fail(what: &str, e: impl std::fmt::Display) -> ConvError {
        ConvError::Other(format!("failed to {what}: {e}"))
    }
    let tab = browser.new_tab().map_err(|e| fail("open a tab", e))?;
    tab.call_method(Emulation::SetScriptExecutionDisabled { value: true })
        .map_err(|e| fail("disable scripts", e))?;

    let book_root = content_dir.canonicalize()?;
    let every_request =
        Fetch::RequestPattern { url_pattern: Some("*".into()), resource_Type: None, request_stage: Some(Fetch::RequestStage::Request) };
    tab.enable_fetch(Some(&[every_request]), None).map_err(|e| fail("filter requests", e))?;
    let served: Arc<Mutex<Option<ServedPage>>> = Arc::default();
    let served_in_handler = served.clone();
    tab.enable_request_interception(Arc::new(move |_transport, _session, event: Fetch::events::RequestPausedEvent| {
        let request_path = file_url_path(&event.params.request.url);
        if let Some(page) = lock(&served_in_handler).as_ref().filter(|p| request_path.as_deref() == Some(p.path.as_path())) {
            return RequestPausedDecision::Fulfill(Fetch::FulfillRequest {
                request_id: event.params.request_id,
                response_code: 200,
                response_headers: Some(vec![Fetch::HeaderEntry {
                    name: "Content-Type".into(),
                    value: page.content_type.into(),
                }]),
                binary_response_headers: None,
                body: Some(page.body_base64.clone()),
                response_phrase: None,
            });
        }
        if url_is_inside_book(&event.params.request.url, &book_root) {
            RequestPausedDecision::Continue(None)
        } else {
            RequestPausedDecision::Fail(Fetch::FailRequest {
                request_id: event.params.request_id,
                error_reason: Network::ErrorReason::BlockedByClient,
            })
        }
    }))
    .map_err(|e| fail("filter requests", e))?;
    Ok(BookTab { tab, served })
}

fn url_is_inside_book(url: &str, book_root: &Path) -> bool {
    if url.starts_with("data:") {
        return true;
    }
    file_url_path(url).and_then(|p| p.canonicalize().ok()).is_some_and(|p| p.starts_with(book_root))
}

/// The local path a `file://` URL names, without its query or fragment.
fn file_url_path(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    let path = rest.split(['?', '#']).next().unwrap_or("");
    href::percent_decode(path).map(PathBuf::from)
}

pub(super) fn launch_browser(path: &Path) -> Result<Browser, ConvError> {
    let options = LaunchOptions::default_builder()
        .path(Some(path.to_path_buf()))
        .headless(true)
        .sandbox(true)
        .build()
        .map_err(|e| ConvError::Other(format!("invalid chromium launch options: {e}")))?;
    Browser::new(options).map_err(|e| ConvError::Other(format!("failed to launch chromium: {e}")))
}

pub(super) fn render_cover_page(
    tab: &BookTab,
    cover_bytes: &[u8],
    content_dir: &Path,
    render_opts: &RenderOptions,
) -> Result<lopdf::Document, ConvError> {
    let src = data_uri(sniff_image_mime(cover_bytes), cover_bytes);
    let html = format!(
        "<html><body style=\"margin:0;display:flex;align-items:center;justify-content:center;height:100vh\">\
         <img src=\"{src}\" style=\"max-width:100%;max-height:100%\"/></body></html>"
    );
    // No file exists at this path: the tab serves the page from memory.
    let cover_path = content_dir.join("__anyform_cover.html");
    tab.print(&cover_path, &html, cover_print_options(render_opts), "cover page")
}
