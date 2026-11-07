mod chapter_html;
mod decorate;
mod merge;
mod render;

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::AtomicUsize;

use anyform_core::{ConvError, Log, Options, OutputPlugin};

use crate::ir::DocumentIR;
use crate::options::{self, BookOptions};
use crate::tools;

use self::render::{render_chapters_parallel, open_locked_down_tab, launch_browser, render_cover_page};
use self::chapter_html::{toc_fragments_by_chapter, spine_item_is_cover_page};
use self::merge::merge_chapter_pdfs;
use self::decorate::{add_page_decorations, set_metadata};

/// PDF output: the bundled headless Chromium (`chrome-headless-shell`,
/// from `scripts/fetch-chromium.sh`) prints each spine chapter with
/// `Page.printToPDF`. The chapter PDFs are then merged into one document
/// with an outline from the TOC. The options (page size, margins,
/// typography, headers, footers, page numbers) match the app's
/// `PDFOptions`; see `RenderOptions::from_options`.
pub struct PdfOutput;

impl OutputPlugin<DocumentIR> for PdfOutput {
    fn name(&self) -> &'static str {
        "pdf"
    }

    fn extension(&self) -> &'static str {
        "pdf"
    }

    fn options(&self) -> &'static [&'static str] {
        &[
            options::INCLUDE_COVER,
            options::GENERATE_TOC,
            options::PAGE_WIDTH_IN,
            options::PAGE_HEIGHT_IN,
            options::MARGIN_IN,
            options::FONT_FAMILY,
            options::FONT_SIZE_PT,
            options::LINE_SPACING,
            options::PRESERVE_EPUB_STYLING,
            options::REMOVE_PUBLISHER_STYLING,
            options::SHOW_PAGE_NUMBERS,
            options::INCLUDE_HEADERS,
            options::INCLUDE_FOOTERS,
            options::CHROMIUM_PATH,
        ]
    }

    fn convert(&self, ir: &DocumentIR, output: &Path, opts: &Options, log: &dyn Log) -> Result<(), ConvError> {
        if ir.spine.is_empty() {
            return Err(ConvError::NoChapters);
        }

        let render_opts = RenderOptions::from_options(opts);

        let chromium_path = tools::CHROMIUM.resolve(opts)?;
        log.info(&format!("launching {}", chromium_path.display()));
        let browser = launch_browser(&chromium_path)?;
        let tab = open_locked_down_tab(&browser, &ir.content_dir)?;

        // Each rendered PDF with the href of its spine item (None for the
        // cover page). The merge uses the href to point bookmarks at pages.
        let mut chunks: Vec<(Option<String>, lopdf::Document)> = Vec::new();

        let include_synthetic_cover = render_opts.include_cover && ir.metadata.cover.is_some();

        // Many EPUBs also have a cover page in the spine, usually first: an
        // XHTML file with only an <img> of the cover. With our own cover
        // page, rendering that one too would show the cover twice. It is
        // found by its markup referring to the cover image's file name, not
        // by its position alone.
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
                ir,
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

        merged
            .save(output)
            .map_err(|e| ConvError::Other(format!("failed to save PDF: {e}")))?;
        Ok(())
    }
}

/// The PDF options. With no options set, the result matches the defaults
/// of the app's `PDFOptions`: US Letter, 1 in margins, 15 pt type with 1.2
/// line spacing, cover and outline on, page numbers on, headers and
/// footers off. These are also calibre's PDF output defaults.
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
            .get_str(options::FONT_FAMILY)
            .map(str::trim)
            .filter(|s| !s.is_empty() && *s != "Original")
            .map(str::to_string);
        let BookOptions { include_cover, generate_toc } = BookOptions::from_options(opts);
        RenderOptions {
            include_cover,
            generate_toc,
            page_width_in: opts.get_f64(options::PAGE_WIDTH_IN, 8.5),
            page_height_in: opts.get_f64(options::PAGE_HEIGHT_IN, 11.0),
            margin_in: opts.get_f64(options::MARGIN_IN, 1.0),
            font_family,
            font_size_pt: opts.get_f64(options::FONT_SIZE_PT, 15.0),
            line_spacing: opts.get_f64(options::LINE_SPACING, 1.2),
            preserve_epub_styling: opts.get_bool(options::PRESERVE_EPUB_STYLING, true),
            remove_publisher_styling: opts.get_bool(options::REMOVE_PUBLISHER_STYLING, false),
            show_page_numbers: opts.get_bool(options::SHOW_PAGE_NUMBERS, true),
            include_headers: opts.get_bool(options::INCLUDE_HEADERS, false),
            include_footers: opts.get_bool(options::INCLUDE_FOOTERS, false),
        }
    }

    fn page_width_pt(&self) -> f64 {
        self.page_width_in * 72.0
    }

    fn page_height_pt(&self) -> f64 {
        self.page_height_in * 72.0
    }
}
