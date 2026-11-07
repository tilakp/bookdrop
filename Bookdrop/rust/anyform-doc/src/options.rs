//! The option names the document plugins read, and the options that more
//! than one plugin reads, each with one default. A plugin lists the names
//! it reads in `options()`, and the registry refuses all other names, so a
//! misspelled name fails instead of silently using a default.

use anyform_core::Options;

pub(crate) const INCLUDE_COVER: &str = "include_cover";
pub(crate) const GENERATE_TOC: &str = "generate_table_of_contents";

pub(crate) const PAGE_WIDTH_IN: &str = "page_width_in";
pub(crate) const PAGE_HEIGHT_IN: &str = "page_height_in";
pub(crate) const MARGIN_IN: &str = "margin_in";
pub(crate) const FONT_FAMILY: &str = "font_family";
pub(crate) const FONT_SIZE_PT: &str = "font_size_pt";
pub(crate) const LINE_SPACING: &str = "line_spacing";
pub(crate) const PRESERVE_EPUB_STYLING: &str = "preserve_epub_styling";
pub(crate) const REMOVE_PUBLISHER_STYLING: &str = "remove_publisher_styling";
pub(crate) const SHOW_PAGE_NUMBERS: &str = "show_page_numbers";
pub(crate) const INCLUDE_HEADERS: &str = "include_headers";
pub(crate) const INCLUDE_FOOTERS: &str = "include_footers";

pub(crate) const PDF_FORCE_SINGLE_COLUMN: &str = "pdf_force_single_column";

pub(crate) const CHROMIUM_PATH: &str = "chromium_path";
pub(crate) const PDFIUM_PATH: &str = "pdfium_path";
pub(crate) const BOKO_PATH: &str = "boko_path";

/// The cover and table-of-contents choices, which several output formats
/// share.
pub(crate) struct BookOptions {
    pub include_cover: bool,
    pub generate_toc: bool,
}

impl BookOptions {
    pub fn from_options(opts: &Options) -> Self {
        BookOptions {
            include_cover: opts.get_bool(INCLUDE_COVER, true),
            generate_toc: opts.get_bool(GENERATE_TOC, true),
        }
    }
}
