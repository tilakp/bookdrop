mod chapter_html;
mod decorate;
mod merge;
mod render;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicUsize;

use anyform_core::{ConvError, Log, Options, OutputPlugin};

use crate::ir::DocumentIR;

use self::render::{render_chapters_parallel, open_locked_down_tab, launch_browser, render_cover_page};
use self::chapter_html::{toc_fragments_by_chapter, spine_item_is_cover_page};
use self::merge::merge_chapter_pdfs;
use self::decorate::{add_page_decorations, set_metadata};

/// Renders each spine chapter through a bundled headless-Chromium binary
/// (chrome-headless-shell, fetched by `scripts/fetch-chromium.sh` — see
/// plan Phase 2) via `Page.printToPDF`, then merges the resulting
/// single-format PDFs into one document with an outline built from the
/// EPUB's TOC — mirrors the shape of Bookdrop's Swift `PdfConverter`
/// (chapter-by-chapter render + merge) but with Chromium doing the
/// HTML/CSS layout instead of WKWebView. Full option surface (page size,
/// margins, typography, headers/footers/page numbers) mirrors Bookdrop's
/// `PDFOptions` — see `RenderOptions::from_options`.
pub struct PdfOutput;

impl OutputPlugin<DocumentIR> for PdfOutput {
    fn name(&self) -> &'static str {
        "pdf"
    }

    fn extension(&self) -> &'static str {
        "pdf"
    }

    fn convert(&self, ir: &DocumentIR, output: &Path, opts: &Options, log: &dyn Log) -> Result<(), ConvError> {
        if ir.spine.is_empty() {
            return Err(ConvError::NoChapters);
        }

        let render_opts = RenderOptions::from_options(opts);

        let chromium_path = resolve_chromium_path(opts)?;
        log.info(&format!("launching {}", chromium_path.display()));
        let browser = launch_browser(&chromium_path)?;
        let tab = open_locked_down_tab(&browser, &ir.content_dir)?;

        // (href of the spine item this PDF chunk came from, None for the
        // cover page — used afterwards to point TOC bookmarks at the right
        // page) paired with the rendered single-chapter PDF.
        let mut chunks: Vec<(Option<String>, lopdf::Document)> = Vec::new();

        let include_synthetic_cover = render_opts.include_cover && ir.metadata.cover.is_some();

        // Many EPUBs *also* have a dedicated cover page in the spine (a
        // separate XHTML file that's just an <img> of the same cover
        // image) — conventionally the first spine item. If we've already
        // inserted our own synthetic cover page above, rendering that one
        // too would duplicate the cover as page 2. Detected by checking
        // whether the first spine item's markup references the cover
        // image's filename, rather than assuming by position alone.
        let skip_spine_index = if include_synthetic_cover {
            ir.metadata.cover_href.as_deref().and_then(|cover_href| {
                let first = ir.spine.first()?;
                let first_path = ir.content_dir.join(&first.href);
                spine_item_is_cover_page(&first_path, cover_href).then_some(0usize)
            })
        } else {
            None
        };

        let render_count = ir.spine.len() - if skip_spine_index.is_some() { 1 } else { 0 };
        let total_steps = render_count + if include_synthetic_cover { 1 } else { 0 };

        if include_synthetic_cover {
            let cover_bytes = ir.metadata.cover.as_ref().unwrap();
            log.info("rendering cover page");
            log.progress(0.0, "Rendering cover");
            let doc = render_cover_page(&tab, cover_bytes, &ir.content_dir, &render_opts)?;
            chunks.push((None, doc));
        }

        let render_indices: Vec<usize> =
            (0..ir.spine.len()).filter(|i| skip_spine_index != Some(*i)).collect();
        let toc_fragments = toc_fragments_by_chapter(&ir.toc);
        if !render_indices.is_empty() {
            let done_count = AtomicUsize::new(if include_synthetic_cover { 1 } else { 0 });
            let rendered = render_chapters_parallel(
                &browser,
                &ir.content_dir,
                &ir.spine,
                &render_indices,
                &render_opts,
                &toc_fragments,
                log,
                &done_count,
                total_steps,
            )?;
            chunks.extend(rendered.into_iter().map(|(href, doc)| (Some(href), doc)));
        }

        if log.is_cancelled() {
            return Err(ConvError::Cancelled);
        }
        log.progress(0.92, "Merging pages");
        let href_to_prefix: HashMap<String, String> =
            ir.spine.iter().enumerate().map(|(i, item)| (item.href.clone(), format!("c{i}"))).collect();
        let mut merged =
            merge_chapter_pdfs(chunks, &ir.toc, render_opts.generate_toc, &ir.content_dir, &href_to_prefix)?;

        log.progress(0.96, "Adding page decorations");
        add_page_decorations(&mut merged, &render_opts, &ir.metadata.title)?;
        set_metadata(&mut merged, ir);

        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent)?;
        }
        merged
            .save(output)
            .map_err(|e| ConvError::Other(format!("failed to save PDF: {e}")))?;
        Ok(())
    }
}

/// Mirrors Bookdrop's Swift `PDFOptions` (`Sources/Bookdrop/Models/PDFOptions.swift`)
/// — field names/defaults match so an unset `Options` behaves like the
/// Swift struct's defaults (US Letter, normal margins, 11pt/1.2 line
/// spacing, cover+TOC on, page numbers on, headers/footers off).
struct RenderOptions {
    include_cover: bool,
    generate_toc: bool,
    page_width_in: f64,
    page_height_in: f64,
    margin_in: f64,
    font_family: Option<String>,
    font_size_pt: f64,
    line_spacing: f64,
    preserve_epub_styling: bool,
    remove_publisher_styling: bool,
    show_page_numbers: bool,
    include_headers: bool,
    include_footers: bool,
}

impl RenderOptions {
    fn from_options(opts: &Options) -> Self {
        let font_family = opts
            .get_str("font_family")
            .map(str::trim)
            .filter(|s| !s.is_empty() && *s != "Original")
            .map(str::to_string);
        RenderOptions {
            include_cover: opts.get_bool("include_cover", true),
            generate_toc: opts.get_bool("generate_table_of_contents", true),
            page_width_in: opts.get_f64("page_width_in", 8.5),
            page_height_in: opts.get_f64("page_height_in", 11.0),
            // Fallback defaults match calibre's PDF Output plugin defaults
            // (1in margins, 15pt base font — see PDFOptions.swift for the
            // verification) since Swift always sends these explicitly; these
            // only apply to callers that don't (anyform-cli, bare Options::new()).
            margin_in: opts.get_f64("margin_in", 1.0),
            font_family,
            font_size_pt: opts.get_f64("font_size_pt", 15.0),
            line_spacing: opts.get_f64("line_spacing", 1.2),
            preserve_epub_styling: opts.get_bool("preserve_epub_styling", true),
            remove_publisher_styling: opts.get_bool("remove_publisher_styling", false),
            show_page_numbers: opts.get_bool("show_page_numbers", true),
            include_headers: opts.get_bool("include_headers", false),
            include_footers: opts.get_bool("include_footers", false),
        }
    }

    fn page_width_pt(&self) -> f64 {
        self.page_width_in * 72.0
    }

    fn page_height_pt(&self) -> f64 {
        self.page_height_in * 72.0
    }
}

fn resolve_chromium_path(opts: &Options) -> Result<PathBuf, ConvError> {
    if let Some(p) = opts.get_str("chromium_path") {
        let path = PathBuf::from(p);
        if path.exists() {
            return Ok(path);
        }
    }
    if let Ok(p) = std::env::var("ANYFORM_CHROMIUM_PATH") {
        let path = PathBuf::from(p);
        if path.exists() {
            return Ok(path);
        }
    }
    // Dev-time convenience: `scripts/fetch-chromium.sh` vendors the binary
    // under `rust/vendor/chromium/<platform>/...` relative to this crate,
    // so `cargo test`/`anyform-cli` work without extra plumbing. The FFI
    // layer always passes an explicit `chromium_path` pointing at the
    // app-bundle resource instead of relying on this fallback.
    let platform = if cfg!(target_arch = "aarch64") {
        "mac-arm64"
    } else {
        "mac-x64"
    };
    let dev_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../vendor/chromium")
        .join(platform)
        .join(format!("chrome-headless-shell-{platform}"))
        .join("chrome-headless-shell");
    if dev_path.exists() {
        return Ok(dev_path);
    }

    Err(ConvError::ToolMissing(
        "no bundled Chromium binary found — set the \"chromium_path\" option, \
         ANYFORM_CHROMIUM_PATH, or run scripts/fetch-chromium.sh"
            .into(),
    ))
}

pub(crate) fn sniff_image_mime(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        "image/png"
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF8") {
        "image/gif"
    } else {
        "image/jpeg"
    }
}
