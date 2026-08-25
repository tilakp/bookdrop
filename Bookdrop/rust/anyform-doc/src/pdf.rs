use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyform_core::{ConvError, Log, OptionSpec, Options, OutputPlugin};
use headless_chrome::types::PrintToPdfOptions;
use headless_chrome::{Browser, LaunchOptions, Tab};
use lopdf::content::{Content, Operation};
use lopdf::{dictionary, Bookmark, Dictionary, Object, ObjectId, Stream};
use regex::Regex;

use crate::ir::{DocumentIR, TocNode};

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

    fn options(&self) -> &'static [OptionSpec] {
        &[]
    }

    fn convert(&self, ir: &DocumentIR, output: &Path, opts: &Options, log: &dyn Log) -> Result<(), ConvError> {
        if ir.spine.is_empty() {
            return Err(ConvError::Other("book has no chapters to render".into()));
        }

        let render_opts = RenderOptions::from_options(opts);

        let chromium_path = resolve_chromium_path(opts)?;
        log.info(&format!("launching {}", chromium_path.display()));
        let browser = launch_browser(&chromium_path)?;
        let tab = browser
            .new_tab()
            .map_err(|e| ConvError::Other(format!("failed to open a tab: {e}")))?;

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
        let mut step = 0usize;

        if include_synthetic_cover {
            let cover_bytes = ir.metadata.cover.as_ref().unwrap();
            log.info("rendering cover page");
            log.progress(step as f64 / total_steps as f64, "Rendering cover");
            let doc = render_cover_page(&tab, cover_bytes, &ir.content_dir, &render_opts)?;
            chunks.push((None, doc));
            step += 1;
        }

        for (i, item) in ir.spine.iter().enumerate() {
            if skip_spine_index == Some(i) {
                continue;
            }
            if log.is_cancelled() {
                return Err(ConvError::Cancelled);
            }
            log.info(&format!("rendering {} ({}/{})", item.href, step + 1, total_steps));
            log.progress(step as f64 / total_steps as f64, &format!("Rendering chapter {}/{}", step + 1, total_steps));
            step += 1;

            let original_path = ir.content_dir.join(&item.href);
            let render_path = prepare_chapter_html(&original_path, i, &render_opts)?;
            let url = format!("file://{}", render_path.display());
            let render_result = (|| -> Result<lopdf::Document, ConvError> {
                tab.navigate_to(&url)
                    .map_err(|e| ConvError::Other(format!("failed to load {}: {e}", item.href)))?;
                tab.wait_until_navigated()
                    .map_err(|e| ConvError::Other(format!("failed to render {}: {e}", item.href)))?;
                let bytes = tab
                    .print_to_pdf(Some(chapter_print_options(&render_opts)))
                    .map_err(|e| ConvError::Other(format!("print_to_pdf failed for {}: {e}", item.href)))?;
                lopdf::Document::load_mem(&bytes)
                    .map_err(|e| ConvError::Other(format!("failed to parse rendered PDF for {}: {e}", item.href)))
            })();
            if render_path != original_path {
                let _ = std::fs::remove_file(&render_path);
            }
            chunks.push((Some(item.href.clone()), render_result?));
        }

        if log.is_cancelled() {
            return Err(ConvError::Cancelled);
        }
        log.progress(0.92, "Merging pages");
        let mut merged = merge_chapter_pdfs(chunks, &ir.toc, render_opts.generate_toc)?;

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
            margin_in: opts.get_f64("margin_in", 48.0 / 72.0),
            font_family,
            font_size_pt: opts.get_f64("font_size_pt", 11.0),
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

/// Regexes used to strip the EPUB's own CSS when `!preserve_epub_styling ||
/// remove_publisher_styling` — mirrors the DOM-query removal Swift's
/// `PdfConverter.typographyInjectionScript` does via injected JS, just as
/// text surgery on the raw (XHTML-so-lowercase-tag) markup instead of a
/// live DOM.
struct StyleStripRegexes {
    style_tag: Regex,
    link_tag: Regex,
    style_attr_dq: Regex,
    style_attr_sq: Regex,
}

fn style_strip_regexes() -> &'static StyleStripRegexes {
    static REGEXES: OnceLock<StyleStripRegexes> = OnceLock::new();
    REGEXES.get_or_init(|| StyleStripRegexes {
        style_tag: Regex::new(r"(?is)<style\b[^>]*>.*?</style>").unwrap(),
        link_tag: Regex::new(r#"(?i)<link[^>]*\brel\s*=\s*['"]stylesheet['"][^>]*/?>"#).unwrap(),
        style_attr_dq: Regex::new(r#"\sstyle\s*=\s*"[^"]*""#).unwrap(),
        style_attr_sq: Regex::new(r"\sstyle\s*=\s*'[^']*'").unwrap(),
    })
}

/// Writes a version of `original` with typography overrides injected (and,
/// if requested, the EPUB's own styling stripped) to a sibling temp file,
/// returning its path — or `original` unchanged if no overrides apply, so
/// callers can skip cleanup in that case.
fn prepare_chapter_html(original: &Path, index: usize, opts: &RenderOptions) -> Result<PathBuf, ConvError> {
    let needs_strip = !opts.preserve_epub_styling || opts.remove_publisher_styling;
    let mut html = std::fs::read_to_string(original)
        .map_err(|e| ConvError::Other(format!("failed to read {}: {e}", original.display())))?;

    if needs_strip {
        let re = style_strip_regexes();
        html = re.style_tag.replace_all(&html, "").into_owned();
        html = re.link_tag.replace_all(&html, "").into_owned();
        html = re.style_attr_dq.replace_all(&html, "").into_owned();
        html = re.style_attr_sq.replace_all(&html, "").into_owned();
    }

    let css = typography_css(opts);
    html = inject_style(&html, &css);

    // Keep the original extension (almost always .xhtml): renaming to
    // .html changes how Chrome parses the file — lenient HTML5 tag-soup
    // parsing instead of strict XHTML/XML parsing — which produced a
    // *different DOM* for real book markup (dropcap spans, epub:-namespaced
    // attributes, etc.) and silently collapsed pagination to 1-3 pages
    // regardless of actual content length. A 2-chapter test fixture never
    // exercised markup complex enough to trip this; a real book did.
    let ext = original.extension().and_then(|e| e.to_str()).unwrap_or("xhtml");
    let render_path = original
        .parent()
        .unwrap_or(original)
        .join(format!("__anyform_render_{index}.{ext}"));
    std::fs::write(&render_path, html)?;
    Ok(render_path)
}

fn typography_css(opts: &RenderOptions) -> String {
    let font_family_rule = match &opts.font_family {
        Some(family) => format!(" font-family: '{}', serif !important;", family.replace(['\'', '"'], "")),
        None => String::new(),
    };
    format!(
        "html, body {{ margin: 0 !important; }} \
         body {{ font-size: {}pt !important; line-height: {} !important;{font_family_rule} }}",
        opts.font_size_pt, opts.line_spacing
    )
}

/// Inserts `<style>{css}</style>` into `head`. EPUB content documents are
/// XHTML, which mandates lowercase element names, so a literal (not
/// case-insensitive) search for `</head>`/`<body` is sufficient and avoids
/// the byte-offset hazards of matching against a lowercased copy.
fn inject_style(html: &str, css: &str) -> String {
    let style_block = format!("<style>{css}</style>");
    if let Some(pos) = html.find("</head>") {
        format!("{}{}{}", &html[..pos], style_block, &html[pos..])
    } else if let Some(pos) = html.find("<body") {
        format!("{}<head>{}</head>{}", &html[..pos], style_block, &html[pos..])
    } else {
        format!("<head>{style_block}</head>{html}")
    }
}

/// True if `spine_path`'s markup references `cover_href`'s filename (e.g.
/// via `<img src="../images/cover.jpg">`) — i.e. this spine page's whole
/// job is displaying the cover image, so it shouldn't also be rendered as
/// a regular chapter once a synthetic cover page has already been added.
fn spine_item_is_cover_page(spine_path: &Path, cover_href: &str) -> bool {
    let Some(cover_filename) = Path::new(cover_href).file_name().and_then(|f| f.to_str()) else {
        return false;
    };
    let Ok(html) = std::fs::read_to_string(spine_path) else {
        return false;
    };
    html.contains(cover_filename)
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

    Err(ConvError::Other(
        "no bundled Chromium binary found — set the \"chromium_path\" option, \
         ANYFORM_CHROMIUM_PATH, or run scripts/fetch-chromium.sh"
            .into(),
    ))
}

fn launch_browser(path: &Path) -> Result<Browser, ConvError> {
    let options = LaunchOptions::default_builder()
        .path(Some(path.to_path_buf()))
        .headless(true)
        .sandbox(false)
        .build()
        .map_err(|e| ConvError::Other(format!("invalid chromium launch options: {e}")))?;
    Browser::new(options).map_err(|e| ConvError::Other(format!("failed to launch chromium: {e}")))
}

fn render_cover_page(
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

fn sniff_image_mime(bytes: &[u8]) -> &'static str {
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

/// Merges N single-chapter PDFs (each produced by one `print_to_pdf` call)
/// into one document and, if `generate_toc`, builds an outline from `toc`,
/// mapping each TOC entry's href to the first page of the chapter chunk it
/// points at. Object-consolidation logic follows lopdf's own
/// `examples/merge.rs` reference implementation.
fn merge_chapter_pdfs(
    chunks: Vec<(Option<String>, lopdf::Document)>,
    toc: &[TocNode],
    generate_toc: bool,
) -> Result<lopdf::Document, ConvError> {
    let mut max_id = 1u32;
    let mut documents_pages: BTreeMap<ObjectId, Object> = BTreeMap::new();
    let mut documents_objects: BTreeMap<ObjectId, Object> = BTreeMap::new();
    let mut document = lopdf::Document::with_version("1.5");
    let mut href_to_first_page: HashMap<String, ObjectId> = HashMap::new();

    for (href, mut doc) in chunks {
        doc.renumber_objects_with(max_id);
        max_id = doc.max_id + 1;

        let mut first_page: Option<ObjectId> = None;
        for object_id in doc.get_pages().into_values() {
            if first_page.is_none() {
                first_page = Some(object_id);
            }
            if let Ok(obj) = doc.get_object(object_id) {
                documents_pages.insert(object_id, obj.to_owned());
            }
        }
        if let (Some(href), Some(page_id)) = (href, first_page) {
            href_to_first_page.insert(href, page_id);
        }

        documents_objects.extend(doc.objects);
    }

    let mut catalog_object: Option<(ObjectId, Object)> = None;
    let mut pages_object: Option<(ObjectId, Object)> = None;

    for (object_id, object) in documents_objects.into_iter() {
        match object.type_name().unwrap_or(b"") {
            b"Catalog" => {
                catalog_object = Some((catalog_object.map(|(id, _)| id).unwrap_or(object_id), object));
            }
            b"Pages" => {
                if let Ok(dictionary) = object.as_dict() {
                    let mut dictionary = dictionary.clone();
                    if let Some((_, ref old_object)) = pages_object {
                        if let Ok(old_dictionary) = old_object.as_dict() {
                            dictionary.extend(old_dictionary);
                        }
                    }
                    pages_object = Some((
                        pages_object.map(|(id, _)| id).unwrap_or(object_id),
                        Object::Dictionary(dictionary),
                    ));
                }
            }
            b"Page" => {}
            b"Outlines" => {}
            b"Outline" => {}
            _ => {
                document.objects.insert(object_id, object);
            }
        }
    }

    let (pages_id, pages_object) =
        pages_object.ok_or_else(|| ConvError::Other("rendered chapters produced no Pages root".into()))?;
    let (catalog_id, catalog_object) =
        catalog_object.ok_or_else(|| ConvError::Other("rendered chapters produced no Catalog root".into()))?;

    for (object_id, object) in documents_pages.iter() {
        if let Ok(dictionary) = object.as_dict() {
            let mut dictionary = dictionary.clone();
            dictionary.set("Parent", pages_id);
            document.objects.insert(*object_id, Object::Dictionary(dictionary));
        }
    }

    if let Ok(dictionary) = pages_object.as_dict() {
        let mut dictionary = dictionary.clone();
        dictionary.set("Count", documents_pages.len() as u32);
        dictionary.set(
            "Kids",
            documents_pages.keys().map(|id| Object::Reference(*id)).collect::<Vec<_>>(),
        );
        document.objects.insert(pages_id, Object::Dictionary(dictionary));
    }

    if let Ok(dictionary) = catalog_object.as_dict() {
        let mut dictionary = dictionary.clone();
        dictionary.set("Pages", pages_id);
        if generate_toc {
            dictionary.set("PageMode", "UseOutlines");
        }
        dictionary.remove(b"Outlines");
        document.objects.insert(catalog_id, Object::Dictionary(dictionary));
    }

    document.trailer.set("Root", catalog_id);
    document.max_id = document.objects.len() as u32;

    if generate_toc {
        // Bookmarks must be added before the final renumber below — lopdf's
        // renumber_objects_with() walks self.bookmark_table and remaps each
        // bookmark's target page id alongside every other object reference
        // (see processor.rs::renumber_bookmarks), so ids captured here stay
        // valid after compaction.
        add_toc_bookmarks(&mut document, toc, None, &href_to_first_page);
    }

    document.renumber_objects();
    document.adjust_zero_pages();
    if generate_toc {
        if let Some(outline_id) = document.build_outline() {
            if let Ok(Object::Dictionary(dict)) = document.get_object_mut(catalog_id) {
                dict.set("Outlines", Object::Reference(outline_id));
            }
        }
    }

    Ok(document)
}

fn add_toc_bookmarks(
    doc: &mut lopdf::Document,
    nodes: &[TocNode],
    parent: Option<u32>,
    href_to_first_page: &HashMap<String, ObjectId>,
) {
    for node in nodes {
        let target = node
            .href
            .as_deref()
            .map(|h| h.split('#').next().unwrap_or(h))
            .and_then(|h| href_to_first_page.get(h))
            .copied()
            .unwrap_or((0, 0));
        let bookmark_id = doc.add_bookmark(Bookmark::new(node.title.clone(), [0.0, 0.0, 0.0], 0, target), parent);
        add_toc_bookmarks(doc, &node.children, Some(bookmark_id), href_to_first_page);
    }
}

/// Post-merge overlay pass for page numbers / running header / running
/// footer — done here (rather than via Chrome's own per-chapter
/// `headerTemplate`/`footerTemplate`) because each chapter is rendered as
/// a *separate* `print_to_pdf` call: Chrome's built-in page-number
/// substitution would restart at 1 for every chapter instead of counting
/// across the whole merged book. Mirrors Swift `PdfConverter.decoratePages`
/// (draw over each final page, book title top/bottom-center, page number
/// bottom-center) but as PDF content-stream operators instead of Core
/// Graphics, and with an approximate (not glyph-metric-exact) text width
/// for centering — close enough for short header/footer/page-number
/// strings at 8pt.
fn add_page_decorations(doc: &mut lopdf::Document, opts: &RenderOptions, book_title: &str) -> Result<(), ConvError> {
    if !(opts.show_page_numbers || opts.include_headers || opts.include_footers) {
        return Ok(());
    }

    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });

    let page_width_pt = opts.page_width_pt();
    let page_height_pt = opts.page_height_pt();
    let page_ids: Vec<ObjectId> = doc.get_pages().into_values().collect();

    for (i, page_id) in page_ids.iter().enumerate() {
        let mut operations = Vec::new();
        if opts.include_headers {
            operations.extend(centered_text_ops(book_title, 8.0, page_width_pt, page_height_pt - 20.0));
        }
        if opts.include_footers {
            operations.extend(centered_text_ops(book_title, 8.0, page_width_pt, 16.0));
        }
        if opts.show_page_numbers {
            let y = if opts.include_footers { 4.0 } else { 16.0 };
            operations.extend(centered_text_ops(&(i + 1).to_string(), 8.0, page_width_pt, y));
        }
        if operations.is_empty() {
            continue;
        }

        let encoded = Content { operations }
            .encode()
            .map_err(|e| ConvError::Other(format!("failed to encode page decoration: {e}")))?;
        let stream_id = doc.add_object(Stream::new(dictionary! {}, encoded));

        let (mut contents, resources_obj) = {
            let page_dict = doc.get_object(*page_id).ok().and_then(|o| o.as_dict().ok());
            let contents = match page_dict.and_then(|d| d.get(b"Contents").ok()) {
                Some(Object::Array(arr)) => arr.clone(),
                Some(Object::Reference(r)) => vec![Object::Reference(*r)],
                _ => Vec::new(),
            };
            let resources_obj = page_dict.and_then(|d| d.get(b"Resources").ok()).cloned();
            (contents, resources_obj)
        };
        contents.push(Object::Reference(stream_id));

        let mut resources_dict = match resources_obj {
            Some(Object::Reference(res_id)) => doc
                .get_object(res_id)
                .ok()
                .and_then(|o| o.as_dict().ok())
                .cloned()
                .unwrap_or_default(),
            Some(Object::Dictionary(d)) => d,
            _ => Dictionary::new(),
        };
        add_font_to_resources(&mut resources_dict, font_id);

        if let Some(page_dict) = doc.get_object_mut(*page_id).ok().and_then(|o| o.as_dict_mut().ok()) {
            page_dict.set("Contents", Object::Array(contents));
            page_dict.set("Resources", Object::Dictionary(resources_dict));
        }
    }

    Ok(())
}

fn add_font_to_resources(resources: &mut Dictionary, font_id: ObjectId) {
    let mut fonts = match resources.get(b"Font") {
        Ok(Object::Dictionary(d)) => d.clone(),
        _ => Dictionary::new(),
    };
    fonts.set("F_Anyform", Object::Reference(font_id));
    resources.set("Font", Object::Dictionary(fonts));
}

fn centered_text_ops(text: &str, font_size: f64, page_width_pt: f64, y: f64) -> Vec<Operation> {
    // Rough Helvetica average-advance-width estimate (not real glyph
    // metrics) — fine for centering a short title/page-number string.
    let approx_width = text.chars().count() as f64 * font_size * 0.5;
    let x = ((page_width_pt - approx_width) / 2.0).max(0.0);
    vec![
        Operation::new("q", vec![]),
        Operation::new("BT", vec![]),
        Operation::new("g", vec![Object::Real(0.35_f32)]),
        Operation::new("Tf", vec![Object::Name(b"F_Anyform".to_vec()), Object::Real(font_size as f32)]),
        Operation::new("Td", vec![Object::Real(x as f32), Object::Real(y as f32)]),
        Operation::new("Tj", vec![Object::string_literal(text)]),
        Operation::new("ET", vec![]),
        Operation::new("Q", vec![]),
    ]
}

fn set_metadata(doc: &mut lopdf::Document, ir: &DocumentIR) {
    let mut info = dictionary! {
        "Title" => Object::string_literal(ir.metadata.title.clone()),
        "Producer" => Object::string_literal("Bookdrop (anyform engine)"),
    };
    if let Some(author) = &ir.metadata.author {
        info.set("Author", Object::string_literal(author.clone()));
    }
    let info_id = doc.add_object(info);
    doc.trailer.set("Info", info_id);
}
