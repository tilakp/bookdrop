# Test fixtures

`minimal.epub` is a small synthetic fixture used for fast, targeted unit
tests (see `pdf_tests.rs`, `epub_tests.rs`).

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
