//! Drives the bundled headless Chromium: launches it, opens locked-down
//! tabs, and prints each chapter (and the cover) to its own PDF.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use anyform_core::{ConvError, Log};
use headless_chrome::browser::tab::RequestPausedDecision;
use headless_chrome::protocol::cdp::{Emulation, Fetch, Network};
use headless_chrome::types::PrintToPdfOptions;
use headless_chrome::{Browser, LaunchOptions, Tab};
use lopdf::{Dictionary, Object};

use crate::href;
use crate::ir::SpineItem;

use super::chapter_html::prepare_chapter_html;
use super::{RenderOptions, sniff_image_mime};

/// Renders `indices` (spine positions) concurrently across a small pool of
/// Chrome tabs on the same browser instance, sized to the CPU count like
/// calibre's own PDF-output worker pool (`detect_ncpus()` in
/// `html_writer.py`) - this was the actual source of calibre's conversion
/// speed advantage found earlier this session, not a different rendering
/// approach. Work is split into contiguous chunks up front (not a
/// work-stealing queue) since chapters are usually similar in size, so
/// static chunking is simple and good enough. Results are returned in
/// original spine order regardless of which worker finished first or
/// slowest, since the merge step requires spine order. `done_count` is a
/// single shared counter so progress reported to `log` stays monotonic
/// even though completion order across workers is not spine order.
#[allow(clippy::too_many_arguments)]
pub(super) fn render_chapters_parallel(
    browser: &Browser,
    content_dir: &Path,
    spine: &[SpineItem],
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
                    let tab = open_locked_down_tab(browser, content_dir)?;
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
                        let item = &spine[i];
                        let original_path = content_dir.join(&item.href);
                        let fragments = toc_fragments.get(&item.href).map(Vec::as_slice).unwrap_or_default();
                        let original_bytes = prepare_chapter_html(&original_path, render_opts, fragments)?;
                        let doc = render_one_chapter(&tab, &original_path, &item.href, render_opts);
                        let _ = std::fs::write(&original_path, &original_bytes);
                        let mut doc = doc?;
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

fn render_one_chapter(tab: &Tab, render_path: &Path, href: &str, opts: &RenderOptions) -> Result<lopdf::Document, ConvError> {
    let url = format!("file://{}", render_path.display());
    tab.navigate_to(&url)
        .map_err(|e| ConvError::Other(format!("failed to load {href}: {e}")))?;
    tab.wait_until_navigated()
        .map_err(|e| ConvError::Other(format!("failed to render {href}: {e}")))?;
    wait_for_fonts_ready(tab);
    let bytes = tab
        .print_to_pdf(Some(chapter_print_options(opts)))
        .map_err(|e| ConvError::Other(format!("print_to_pdf failed for {href}: {e}")))?;
    lopdf::Document::load_mem(&bytes).map_err(|e| ConvError::Other(format!("failed to parse rendered PDF for {href}: {e}")))
}

/// Chrome's print_to_pdf turns a same-document fragment link (`<a
/// href="thisfile.html#note3">`, which is how EPUB footnote/cross-
/// reference links are written even when the target is in the very file
/// being printed) into a working `/Dest /note3` link annotation plus a
/// `/Dests` name dictionary on that chapter's own catalog mapping
/// `note3` to an exact page/position - confirmed empirically before
/// writing this. Since every chapter is printed as an independent
/// single-file PDF, each chapter's dest names live in their own
/// namespace and would collide across chapters once merged (e.g. two
/// different chapters both defining `fn1`). This renames every entry in
/// this chapter's own Dests dictionary to `{prefix}::{name}`, and
/// rewrites this chapter's own Link annotations to match, so the
/// destinations survive `merge_chapter_pdfs` consolidating every
/// chapter's Dests dictionary into one on the merged document without
/// colliding.
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
/// only guarantees the DOM load event fired — it says nothing about
/// `@font-face` fonts (this book declares six, loaded from local .otf
/// files) finishing their *own* async load, which races independently of
/// page load. Printing before a font swap finishes is a plausible source
/// of corrupted-looking text (values mid-reflow) — matches why calibre's
/// own Chromium-based PDF renderer explicitly waits a settle period after
/// load before printing (see `html_writer.py`'s `settle_time`). Best-effort:
/// a failure here shouldn't abort the whole conversion over one chapter.
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
pub(super) fn open_locked_down_tab(browser: &Browser, content_dir: &Path) -> Result<Arc<Tab>, ConvError> {
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
    tab.enable_request_interception(Arc::new(move |_transport, _session, event: Fetch::events::RequestPausedEvent| {
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
    Ok(tab)
}

fn url_is_inside_book(url: &str, book_root: &Path) -> bool {
    if url.starts_with("data:") {
        return true;
    }
    let Some(rest) = url.strip_prefix("file://") else { return false };
    let path = rest.split(['?', '#']).next().unwrap_or("");
    let Some(decoded) = href::percent_decode(path) else { return false };
    Path::new(&decoded).canonicalize().is_ok_and(|p| p.starts_with(book_root))
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
    tab: &Tab,
    cover_bytes: &[u8],
    content_dir: &Path,
    render_opts: &RenderOptions,
) -> Result<lopdf::Document, ConvError> {
    use base64::Engine;
    let mime = sniff_image_mime(cover_bytes);
    let encoded = base64::engine::general_purpose::STANDARD.encode(cover_bytes);
    let html = format!(
        "<html><body style=\"margin:0;display:flex;align-items:center;justify-content:center;height:100vh\">\
         <img src=\"data:{mime};base64,{encoded}\" style=\"max-width:100%;max-height:100%\"/></body></html>"
    );
    let cover_path = content_dir.join("__anyform_cover.html");
    std::fs::write(&cover_path, html)?;
    let url = format!("file://{}", cover_path.display());
    let result = (|| -> Result<lopdf::Document, ConvError> {
        tab.navigate_to(&url)
            .map_err(|e| ConvError::Other(format!("failed to load cover page: {e}")))?;
        tab.wait_until_navigated()
            .map_err(|e| ConvError::Other(format!("failed to render cover page: {e}")))?;
        let bytes = tab
            .print_to_pdf(Some(cover_print_options(render_opts)))
            .map_err(|e| ConvError::Other(format!("print_to_pdf failed for cover: {e}")))?;
        lopdf::Document::load_mem(&bytes)
            .map_err(|e| ConvError::Other(format!("failed to parse cover PDF: {e}")))
    })();
    let _ = std::fs::remove_file(&cover_path);
    result
}
