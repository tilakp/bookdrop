# Test fixtures

`minimal.epub`, `css-edge-cases.epub`, and `drm-fonts.epub` are small
synthetic fixtures used for fast, targeted unit tests (see `pdf_tests.rs`,
`epub_tests.rs`). `css-edge-cases.epub` has a wide unwrapped `<pre><code>`
block and a CSS `column-count` layout, purpose-built to exercise Chrome
print's lack of "shrink to fit" (see `wide_pre_blocks_wrap_instead_of_clipping`).
`drm-fonts.epub` has two fonts obfuscated with the standard IDPF and Adobe
EPUB font-obfuscation schemes, generated independently of this codebase's
own deobfuscation code (see `deobfuscates_both_font_obfuscation_schemes`).

`doctype-nav.epub` and `doctype-ncx.epub` carry a DOCTYPE in their EPUB3
nav and EPUB2 NCX TOC documents respectively. Real EPUBs very commonly do
(the NISO ncx DOCTYPE, `<!DOCTYPE html>`), but every Gutenberg fixture here
happens not to, which is why a DTD-rejection bug silently wiped the table
of contents for a long time without any test noticing.

`text-entities.epub` has a deliberately non-well-formed chapter (an
unclosed `<br>`, undeclared HTML named entities like `&nbsp;`/`&mdash;`) to
force `htmltext.rs`'s regex fallback path rather than its normal roxmltree
fast path - the only fixture that exercises it (see
`txt_tests.rs::decodes_entities_and_drops_script_and_style`).

`nested-nav.epub` is a minimal fixture whose EPUB3 nav document lives at
`OEBPS/text/nav.xhtml` (not the content-directory root, unlike every other
fixture here) with chapter TOC hrefs written relative to `text/` - the only
fixture that pins down `TocNode.href` being relative to the nav document's
own directory, not `content_dir` (see `epub_output_tests.rs`'s
`nested_toc_directory_keeps_hrefs_valid`).

`minimal.azw3` is `minimal.epub` converted by the bundled `boko` binary,
used by `kindle_tests.rs` to assert Kindle-format input parses to the same
`DocumentIR` as its EPUB source. Regenerate with
`boko convert minimal.epub minimal.azw3` after running
`rust/scripts/fetch-boko.sh`.

The rest are real books used by `real_book_tests.rs`, a regression suite
against actual, structurally diverse content instead of only the synthetic
fixture. All are public domain, downloaded from
[Project Gutenberg](https://www.gutenberg.org/), and safe to redistribute
in git history:

- `doctor-dolittle.epub` - *The Story of Doctor Dolittle* by Hugh Lofting
  ([gutenberg.org/ebooks/501](https://www.gutenberg.org/ebooks/501)).
  Image-heavy: illustrated by the author throughout.
- `pride-and-prejudice.epub` - *Pride and Prejudice* by Jane Austen
  ([gutenberg.org/ebooks/1342](https://www.gutenberg.org/ebooks/1342)).
  A long, many-chapter plain-text novel.
- `origin-of-species.epub` - *On the Origin of Species* by Charles Darwin
  ([gutenberg.org/ebooks/2009](https://www.gutenberg.org/ebooks/2009)).
  Has real footnotes, a glossary, and a large back-of-book index.
- `scientific-american-supplement.epub` - *Scientific American Supplement*,
  Feb 24 1877 ([gutenberg.org/ebooks/19406](https://www.gutenberg.org/ebooks/19406)).
  Has real `<table>` markup: price lists and a paginated table of contents.
