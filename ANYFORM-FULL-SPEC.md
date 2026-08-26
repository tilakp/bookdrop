# Anyform — Full Specification (Engine + macOS App)

Combines the conversion-engine architecture (previously `SPEC.md`) with the
macOS app UX design (previously `Mac Book Converter — UX Specification.md`)
into one document. Source files kept as-is in `archive/`; this is the
merged reference going forward.

## Status (as of 2026-08-24)

**Shipped — Bookdrop v1.0 + v1.1.** A native SwiftUI macOS app converting
EPUB → PDF / TXT / HTML / DOCX. Source:
[github.com/tilakp/bookdrop](https://github.com/tilakp/bookdrop) (public),
3 commits. 44 passing tests, live-verified (each output format opened in a
real consuming app — Preview/PDFKit, a real browser tab, and real
Microsoft Word — not just "the code ran without throwing").

**Shipped — Version 2 EPUB→PDF vertical slice, on Rust via FFI.** PDF
conversion now runs through a real Rust `anyform` engine end to end,
including the full `PDFOptions` surface (page size/margins/orientation,
cover, TOC, typography, headers/footers/page numbers — nothing in Advanced
Options is silently ignored). What exists, in `Bookdrop/rust/`:

- `anyform-core` — the §2 trait/registry design (`InputPlugin`/
  `OutputPlugin`/`Transform`/`Registry`/`Options`), plus a `Log` trait
  doubling as the plugin↔host channel for progress/cancellation.
- `anyform-doc` — `EpubInput` (parity-tested against the same fixture as
  `EpubParserTests.swift`) and `PdfOutput`, which renders each chapter
  through a **bundled headless-Chromium binary** (`chrome-headless-shell`,
  fetched by `rust/scripts/fetch-chromium.sh` from Chrome for Testing, not
  committed to git — both mac-arm64 and mac-x64 fetched) via CDP, applying
  custom page size/margins/typography (via a regex-based CSS
  strip-and-inject pass on each chapter's HTML before rendering) and a
  post-merge content-stream overlay for headers/footers/page numbers
  (needed because per-chapter Chrome renders can't produce numbering that's
  consistent across the whole merged book), then merges everything with
  `lopdf`, including a real outline built from the EPUB's TOC (skippable).
- `anyform-ffi` — a small C ABI (`anyform_parse_epub`,
  `anyform_convert_start`/`anyform_cancel`, JSON for structured data),
  built as a universal xcframework by `rust/scripts/build-ffi.sh`.
- Wired into Bookdrop's Swift side (`Sources/Bookdrop/Services/
  RustConversionEngine.swift` + a `CAnyform` C target in `Package.swift`)
  for the **`.pdf` output format only** — `.txt`/`.html`/`.docx` still go
  through the original Swift-native converters, a deliberate accepted
  interim state (see decision below). `Scripts/build-app.sh` builds the
  Rust engine, bundles **both** Chromium architectures into
  `Contents/Resources/Chromium/<mac-arm64|mac-x64>/` (Swift picks the
  right one at runtime via `#if arch(...)`), and ad-hoc codesigns. The
  `Bookdrop` executable itself is still built single-arch (host only) —
  true cross-architecture distribution would also need a universal
  `swift build --arch arm64 --arch x86_64` + `lipo`, not done.

All 45 Swift tests + 15 Rust tests pass, including tests that construct
non-default `PDFOptions` (custom page size, disabled cover/TOC,
headers/footers/page numbers, typography) and assert the *rendered PDF*
reflects them — both at the Rust-engine level and end-to-end through
`AppCoordinator` (Swift's own JSON-building code, not just Rust's JSON
parsing). Live-verified: real `.app` bundle launches cleanly (screenshot
checked); a full drag-and-drop click-through was deliberately skipped, see
[[feedback-macos-testing-and-automation]] (this exact app's file picker has
previously caused a keystroke-leak incident during automated verification)
— instead, a real conversion was driven through `AppCoordinator` (the
same class the UI calls) and the output PDF opened in Preview and
screenshotted for a genuine visual check.

**Decision — Phase 6 (TXT/HTML/DOCX → Rust) is deferred, not required for
"done."** PDF-on-Rust + TXT/HTML/DOCX-on-Swift-native is an acceptable
shipped state: the Swift converters are fully functional, well-tested, and
untouched. Revisit Phase 6 only if/when there's a concrete reason to want
those formats behind the Rust engine too (e.g. reuse on another platform).

**Quick start:**
- Build Swift only: `cd Bookdrop && swift build` (needs
  `rust/target/universal/libanyform_ffi.a` to already exist — run
  `rust/scripts/build-ffi.sh` once first, or use `Scripts/build-app.sh`
  which does it automatically).
- Build the Rust engine: `cd Bookdrop/rust && cargo test` (needs
  `scripts/fetch-chromium.sh` run once first for the PDF-output tests).
- Test: `swift test` — 45 tests (~28s, mostly real Rust/Chromium PDF
  renders now, not mocked), all use scratch directories / an in-memory
  `KeyValueStore` so they never touch real user data.
- Run as a real `.app` bundle — needed for the Dock icon, system
  notifications, and the bundled Chromium resource path: `./Scripts/build-app.sh
  debug && open .build/debug/Bookdrop.app`
- Icon source is `Scripts/make_icon.swift` (regenerate + re-run
  `sips`/`iconutil` only if the design changes — see §7).

**Architecture at a glance:**
- `Sources/Bookdrop/Services/AppCoordinator.swift` — the screen state
  machine + conversion flow, unit-testable independent of SwiftUI.
- `Sources/Bookdrop/Services/{Pdf,Txt,Html,Docx}Converter.swift` — one
  converter per `OutputFormat` case, each a self-contained `Book → URL`
  function; `AppCoordinator`/`MultiConversionModel` dispatch on the format.
- `Sources/Bookdrop/Services/EpubParser.swift` + `EpubXML.swift` — EPUB →
  `Book` (OPF/NCX/nav parsing, cover extraction).
- `PDFOptions` (`Models/PDFOptions.swift`) carries options reused by every
  format, not just PDF (cover/TOC/style-preservation) — the name is known
  naming debt, not urgent enough to have risked the rename yet.

---

## 0. Naming — resolved

The two source docs used different names for what is effectively the same
product at different layers:

- **Anyform** — the Rust conversion *engine*: a format-agnostic
  input→IR→output pipeline, not ebook-specific, CLI-first (`anyform convert
  book.epub book.pdf`).
- **Bookdrop** — a macOS *app* scoped to ebooks specifically (EPUB → PDF
  etc.), with its own native SwiftUI UX, recommended to be "built as a native
  SwiftUI macOS app, with the conversion engine isolated behind a clean
  `BookConverter` protocol."

That raised a real architecture question: does the macOS app call into the
Rust `anyform` engine via FFI, reusing the EPUB parsing / PDF
post-processing work in §2, or does it wrap a separate Swift-native
conversion path?

**Decision:** Swift-native for v1. A concrete branded mock
(`ChatGPT Image Aug 24, 2026 at 12_01_06 AM.png`) confirmed the app's own
identity is **Bookdrop** (purple mark, its own visual language — see
§4.21), distinct from the **Anyform** engine name. The app ships v1
(EPUB → PDF) with its own Swift implementation of the input-parsing →
rendering → merge pipeline (WKWebView + PDFKit in place of headless
Chromium + `lopdf`, same shape as §2's `DocumentIR` pipeline) — see the
implementation plan for `Bookdrop/`. The Rust `anyform` engine remains
the design for a general-purpose, multi-format engine and is a candidate to
become Bookdrop's backend via FFI in v2, once the format matrix grows
past EPUB → PDF and reuse across platforms starts to matter. Until then the
two evolve independently: **Anyform** names the engine/CLI concept,
**Bookdrop** names the shipping macOS app.

---

## 1. Motivation

Prompted by looking at how calibre implements EPUB → PDF conversion: a
generic `Plumber` pipeline (`input plugin → intermediate representation (IR)
→ output plugin`) built in Python/C++, where the actual HTML/CSS layout work
for PDF is delegated to Chromium (Qt WebEngine) and PDF post-processing
(font/image dedup, TOC/outline, metadata) is done via a podofo C++ binding.

Question explored: could this be done in Rust, and better? Conclusion:

- The **input parsing/normalization** side (zip + OPF/NCX/nav parsing, DRM
  font deobfuscation, cover/titlepage detection) is a well-understood,
  self-contained problem. Rust is a clear win here: speed, memory safety,
  single static binary, no Python/GIL overhead.
- The **PDF post-processing** side (merge pages, build outline, dedup
  fonts/images, write metadata) is also tractable in Rust via a PDF library
  (e.g. `lopdf`) instead of a C++ binding + glue.
- The **HTML/CSS layout + pagination** step (the actual "print to PDF" work)
  is *not* worth reimplementing. Matching Chromium's CSS fidelity is a
  browser-engine-scale problem; no Rust library today (Servo's layout,
  `taffy`, etc.) is close. calibre itself moved *away* from a custom
  WebKit-based renderer toward Chromium for this exact reason — reinventing
  it would likely regress quality, not improve it. The right move is to keep
  shelling out to a real rendering engine (headless Chromium via CDP, or
  similar) for any output format that needs full HTML/CSS layout.

So the value of a Rust rewrite isn't "replace Chromium," it's "generalize and
harden everything around it" — and generalize past ebooks specifically, into
a format-agnostic conversion framework, since the input-normalization →
IR → output-rendering shape isn't specific to ebooks.

**Product framing on top of that:** the first consumer of this engine is a
lightweight macOS utility for converting ebooks and documents between common
book formats — a person should be able to drag an EPUB onto a window and get
a PDF back without knowing what an OPF file is. The engine is general; the
first product surface is narrow and ebook-specific on purpose (§4).

### Primary use case

> EPUB → PDF

### Format matrix (target, not v1)

**Input:** EPUB, MOBI, AZW / AZW3, FB2, HTML, TXT, DOCX
**Output:** PDF, EPUB, MOBI/AZW3, TXT, HTML, DOCX

For the MVP (both engine and app), support only **EPUB → PDF**, then expand
the matrix once the core workflow is solid (§6).

---

## 2. Core engine design

### The multiple-IR problem

calibre pretends there is one universal intermediate representation (OEB) and
then special-cases everything that doesn't actually fit it (e.g.
`is_image_collection` checks scattered through the PDF output plugin for
comic/image-based inputs). Conversions only make sense within a "family" that
actually shares a representation — you can't meaningfully convert epub→pdf
and png→jpg through the same IR. Anyform makes this split explicit instead of
implicit: the plugin registry is generic over an IR type, and a thin
dispatcher picks the right registry by family.

### Core traits

```rust
// anyform-core

pub trait InputPlugin<IR>: Send + Sync {
    fn name(&self) -> &'static str;
    fn extensions(&self) -> &'static [&'static str];
    fn convert(&self, input: &Path, opts: &Options, log: &dyn Log) -> Result<IR, ConvError>;
}

pub trait OutputPlugin<IR>: Send + Sync {
    fn name(&self) -> &'static str;
    fn extension(&self) -> &'static str;
    fn options(&self) -> &'static [OptionSpec] { &[] }
    fn convert(&self, ir: &IR, output: &Path, opts: &Options, log: &dyn Log) -> Result<(), ConvError>;
}

// Optional IR-level passes shared across output formats (hyphenation,
// font subsetting, link rewriting) — analogous to calibre's oeb/polish/*.
pub trait Transform<IR>: Send + Sync {
    fn apply(&self, ir: &mut IR, opts: &Options) -> Result<(), ConvError>;
}

pub struct Registry<IR> {
    inputs: HashMap<&'static str, Arc<dyn InputPlugin<IR>>>,
    outputs: HashMap<&'static str, Arc<dyn OutputPlugin<IR>>>,
    transforms: Vec<Arc<dyn Transform<IR>>>,
}

impl<IR> Registry<IR> {
    pub fn convert(&self, input: &Path, output: &Path, opts: &Options, log: &dyn Log)
        -> Result<(), ConvError>
    {
        let inp = self.inputs.get(ext_of(input)?).ok_or(ConvError::NoInputPlugin)?;
        let out = self.outputs.get(ext_of(output)?).ok_or(ConvError::NoOutputPlugin)?;
        let mut ir = inp.convert(input, opts, log)?;
        for t in &self.transforms { t.apply(&mut ir, opts)?; }
        out.convert(&ir, output, opts, log)
    }
}
```

### Top-level dispatch across families

```rust
pub enum Family { Document, Image, Audio }

pub struct Converter {
    document: Registry<DocumentIR>,
    image: Registry<ImageIR>,
    // audio: Registry<AudioIR>, ...
}

impl Converter {
    pub fn convert(&self, input: &Path, output: &Path, opts: &Options) -> Result<(), ConvError> {
        let fam_in = Family::for_ext(ext_of(input)?)?;
        let fam_out = Family::for_ext(ext_of(output)?)?;
        if fam_in != fam_out { return Err(ConvError::IncompatibleFamilies(fam_in, fam_out)); }
        match fam_in {
            Family::Document => self.document.convert(input, output, opts, &StdLog),
            Family::Image    => self.image.convert(input, output, opts, &StdLog),
            Family::Audio    => todo!(),
        }
    }
}
```

### The Document IR (epub/html/docx/markdown live here)

Kept shallow deliberately: a manifest of resources plus an ordered spine of
*raw normalized HTML*, not a fully typed DOM tree. Modeling every format's
semantics into one strict tree is where calibre's OEB layer gets hairy.
Deferring HTML parsing to whichever output plugin needs it (or not, if it
just re-emits HTML) keeps the IR honest about what it actually knows.

```rust
pub struct DocumentIR {
    pub metadata: Metadata,                  // title, authors, identifiers, cover id
    pub manifest: HashMap<String, Resource>,  // id -> (mime, bytes | href)
    pub spine: Vec<SpineItem>,                // ordered content, id + raw (X)HTML string
    pub toc: TocNode,
}

pub struct SpineItem { pub id: String, pub href: String, pub html: String }
pub struct Resource  { pub mime: String, pub data: Bytes }
```

This `DocumentIR` is also the natural boundary the macOS app's "Book
Information" panel (§4.6) reads from — `metadata` maps directly to
title/author/cover, and `spine.len()` gives chapter count.

### Example input plugin — EPUB

```rust
pub struct EpubInput;
impl InputPlugin<DocumentIR> for EpubInput {
    fn extensions(&self) -> &'static [&'static str] { &["epub", "kepub"] }

    fn convert(&self, input: &Path, opts: &Options, log: &dyn Log) -> Result<DocumentIR, ConvError> {
        let mut zip = zip::ZipArchive::new(File::open(input)?)?;
        let opf_path = find_opf(&mut zip)?;                 // META-INF/container.xml
        let opf = parse_opf(&mut zip, &opf_path)?;           // quick-xml
        if let Some(enc) = read_optional(&mut zip, "META-INF/encryption.xml")? {
            decrypt_fonts(&mut zip, &enc, &opf)?;             // Adobe/IDPF obfuscation
        }
        let manifest = load_manifest(&mut zip, &opf)?;
        let spine = build_spine(&manifest, &opf)?;
        let toc = load_nav_or_ncx(&mut zip, &opf)?;
        Ok(DocumentIR { metadata: opf.metadata, manifest, spine, toc })
    }
}
```

### Example output plugin — PDF (delegates rendering, per the conclusion above)

```rust
pub struct PdfOutput;
impl OutputPlugin<DocumentIR> for PdfOutput {
    fn extension(&self) -> &'static str { "pdf" }

    fn convert(&self, ir: &DocumentIR, output: &Path, opts: &Options, log: &dyn Log) -> Result<(), ConvError> {
        let workdir = stage_ir_as_browsable_html(ir)?;        // write spine + manifest to disk
        let browser = HeadlessChrome::launch()?;
        let page_docs: Vec<PdfBytes> = ir.spine.iter()
            .map(|item| browser.print_to_pdf(&workdir.join(&item.href), &page_layout(opts)))
            .collect::<Result<_, _>>()?;
        let mut doc = merge_pdfs(&page_docs)?;                 // lopdf
        add_outline_from_toc(&mut doc, &ir.toc)?;
        write_metadata(&mut doc, &ir.metadata)?;
        doc.save(output)?;
        Ok(())
    }
}
```

`opts` here is exactly where the macOS app's PDF Options panel (§4.8) plugs
in: page size, margins, orientation, include-cover, TOC, and the advanced
typography/layout knobs all map to `OptionSpec` entries on this plugin.

### Example output plugin — Markdown (pure Rust, no external engine)

Included to prove the framework isn't PDF-only / Chromium-only.

```rust
pub struct MarkdownOutput;
impl OutputPlugin<DocumentIR> for MarkdownOutput {
    fn extension(&self) -> &'static str { "md" }
    fn convert(&self, ir: &DocumentIR, output: &Path, _: &Options, _: &dyn Log) -> Result<(), ConvError> {
        let mut buf = String::new();
        for item in &ir.spine {
            buf.push_str(&html2md::parse_html(&item.html));
            buf.push_str("\n\n");
        }
        Ok(std::fs::write(output, buf)?)
    }
}
```

### Options with priority

Mirrors calibre's `OptionRecommendation` levels: a plugin's own default can be
overridden by a device/output profile, which can in turn be overridden by an
explicit user setting — but a user-set value is never silently clobbered by a
lower-priority write. This priority chain is also what lets the app's
Settings (§4.17) — e.g. "Preserve original styling by default" — set a
`Profile`-level default that a one-off Advanced Options change (`UserSet`)
still overrides for a single conversion.

```rust
pub enum Priority { PluginDefault, Profile, UserSet }
pub struct OptionSpec { pub name: &'static str, pub default: Value, pub help: &'static str }
pub struct Options { values: HashMap<&'static str, (Value, Priority)> }
```

### Plugin registration

Explicit registration for now (no macro magic), same spirit as calibre's
`customize/builtins.py` list:

```rust
pub fn document_registry() -> Registry<DocumentIR> {
    let mut r = Registry::new();
    r.add_input(Arc::new(EpubInput));
    r.add_input(Arc::new(HtmlInput));
    r.add_output(Arc::new(PdfOutput));
    r.add_output(Arc::new(MarkdownOutput));
    r.add_transform(Arc::new(HyphenationTransform));
    r
}
```

Third-party/dynamically-loaded plugins (calibre supports user-installed ZIP
plugins) would need `libloading` + a stable ABI — deliberately out of scope
for the initial design, worth a separate pass if/when needed.

### Where Rust actually earns its keep

- Zip/XML parsing, font deobfuscation, manifest/spine building: memory-safe,
  fast, no GIL — a genuine win over calibre's Python input plugin.
- PDF post-processing (merge, outline, font/image dedup) via `lopdf`: single
  static binary instead of a podofo C++ binding + Python glue.
- The rendering step still needs Chromium (or another real engine)
  underneath — that part doesn't get better by being called from Rust.

---

## 3. Design principles (macOS app)

### 1. One-screen workflow

The user should not need to understand ebook internals.

### 2. Drag and drop first

The primary interaction should be:

**Drag book here**

rather than navigating through a file picker.

### 3. Progressive disclosure

Don't expose 25 conversion options immediately.

Show:

- Output format
- Output location
- A few common formatting options

Put advanced options behind **Advanced Settings**.

### 4. Native macOS feel

Use:

- SF Symbols
- standard macOS controls
- sidebar/popover patterns
- familiar Save/Open panels
- keyboard shortcuts
- Finder integration
- light/dark mode

Avoid making it look like a web app inside a Mac window.

---

## 4. macOS App — UX Specification

### 4.1 Main Window

Recommended size:

**720 × 520 px**

Resizable, with a minimum around:

**600 × 450 px**

#### Layout

```text
┌──────────────────────────────────────────────────────────┐
│  Bookdrop                                  ⚙              │
├──────────────────────────────────────────────────────────┤
│                                                          │
│                   Convert your book                      │
│                                                          │
│       ┌──────────────────────────────────────┐           │
│       │                                      │           │
│       │             ↓                       │           │
│       │                                      │           │
│       │       Drop an ebook here             │           │
│       │                                      │           │
│       │       or  Choose File…              │           │
│       │                                      │           │
│       └──────────────────────────────────────┘           │
│                                                          │
│                                                          │
│  Recent conversions                                      │
│                                                          │
│  📕  The Great Gatsby        EPUB → PDF       Today      │
│  📕  Design Patterns         EPUB → PDF       Yesterday  │
│                                                          │
└──────────────────────────────────────────────────────────┘
```

The empty state should dominate the interface.

### 4.2 Drag & Drop State

When the user drags a supported file over the application:

```text
┌──────────────────────────────────────────────┐
│                                              │
│                    ↓                         │
│                                              │
│             Drop to convert                  │
│                                              │
│             EPUB → PDF                       │
│                                              │
└──────────────────────────────────────────────┘
```

The drop zone should visually expand/highlight.

Unsupported files should show:

> This file format isn't supported.

Do not simply fail silently.

### 4.3 File Loaded State

After selecting an EPUB:

```text
┌──────────────────────────────────────────────────────────┐
│  Bookdrop                                                   │
├──────────────────────────────────────────────────────────┤
│                                                          │
│  BOOK                                                     │
│                                                          │
│  ┌───────┐                                                │
│  │       │   The Great Gatsby                             │
│  │  📕   │   F. Scott Fitzgerald                          │
│  │       │   1.2 MB · 9 chapters                          │
│  └───────┘                                                │
│                                                          │
│  Output                                                   │
│                                                          │
│  Format                                                   │
│  ┌──────────────────────────────────────┐                 │
│  │ PDF                                  │⌄                │
│  └──────────────────────────────────────┘                 │
│                                                          │
│  Save to                                                  │
│  ┌──────────────────────────────────────┐                 │
│  │ ~/Downloads                         │  Choose…         │
│  └──────────────────────────────────────┘                 │
│                                                          │
│  PDF Options                              Advanced…       │
│                                                          │
│  Page size       US Letter              ⌄                │
│  Margins         Normal                 ⌄                │
│  Include cover   ●                                      │
│                                                          │
│                       [ Convert ]                         │
│                                                          │
└──────────────────────────────────────────────────────────┘
```

### 4.4 Book Information

Extract metadata from the EPUB automatically (via `DocumentIR.metadata`, §2).

Display:

- Cover
- Title
- Author
- File size
- Chapter count
- Estimated page count, if available

Example:

> **The Great Gatsby**
> F. Scott Fitzgerald
> 1.2 MB · 9 chapters

If metadata is missing:

> Untitled Book
> Unknown Author

Allow editing metadata later, but don't make it part of the MVP.

### 4.5 Output Format

Use a native macOS popup.

```text
Output Format

✓ PDF
  EPUB
  MOBI
  AZW3
  TXT
  HTML
```

Only display formats that are actually supported by the current input —
this list should be driven directly by which `OutputPlugin<DocumentIR>`
entries are registered for the input's family (§2), so the UI can never
offer an impossible conversion by construction.

```text
EPUB

Convert to:

✓ PDF
  TXT
  HTML
```

### 4.6 PDF Options

For EPUB → PDF, keep the first version simple.

#### Basic options

**Page Size**

- US Letter
- A4
- A5
- Custom

**Margins**

- Narrow
- Normal
- Wide

**Orientation**

- Portrait
- Landscape

**Include Cover**

On / Off

**Table of Contents**

On / Off

#### Advanced options

Hidden initially:

- Font family
- Font size
- Line spacing
- Paragraph spacing
- Page numbers
- Header/footer
- Chapter start on new page
- Preserve original styling

Example:

```text
PDF Options

Page size        A4                         ⌄
Margins          Normal                     ⌄
Orientation      Portrait                   ⌄

☑ Include cover
☑ Generate table of contents
☑ Page numbers

                 Advanced Options
```

### 4.7 Advanced Options

Use a disclosure section rather than another screen.

```text
Advanced PDF Options

Typography

Font              Original                 ⌄
Font size         11 pt                    −  +
Line spacing      1.2                       ⌄

Layout

☑ Start chapters on new page
☑ Preserve EPUB styling
☐ Remove publisher styling

Pages

☑ Show page numbers
☐ Include headers
☐ Include footers
```

Avoid exposing technical concepts like CSS, EPUB manifests, OPF files, etc.

Those belong in a developer/debugging mode, not the normal UX.

### 4.8 Output Filename

Automatically derive the filename.

Input:

```text
The Great Gatsby.epub
```

Output:

```text
The Great Gatsby.pdf
```

If the file already exists:

```text
A file named "The Great Gatsby.pdf"
already exists.

○ Replace
○ Keep Both
○ Cancel
```

Default:

**Keep Both**

with:

```text
The Great Gatsby (1).pdf
```

### 4.9 Conversion Progress

Once the user presses Convert, transition the main area into a progress state.

```text
┌──────────────────────────────────────────────────────────┐
│                                                          │
│                    Converting…                            │
│                                                          │
│                  The Great Gatsby                         │
│                                                          │
│             ███████████████░░░░░░░                       │
│                       67%                                │
│                                                          │
│             Rendering chapter 6 of 9                      │
│                                                          │
│                    Cancel                                │
│                                                          │
└──────────────────────────────────────────────────────────┘
```

Progress stages could be:

1. Reading book
2. Processing chapters
3. Rendering pages
4. Creating PDF
5. Finalizing

Show human-readable status rather than technical logs. These stages map
naturally onto the engine pipeline in §2: (1)/(2) are the `InputPlugin`
parsing the EPUB and `Transform` passes running, (3) is
`HeadlessChrome::print_to_pdf` per spine item, (4)/(5) are `merge_pdfs` +
`add_outline_from_toc` + `write_metadata`.

### 4.10 Conversion Complete

This is an important UX moment.

```text
┌──────────────────────────────────────────────────────────┐
│                                                          │
│                       ✓                                   │
│                                                          │
│                  Conversion complete                     │
│                                                          │
│                  The Great Gatsby.pdf                    │
│                                                          │
│             2.8 MB · 184 pages                           │
│                                                          │
│          [ Show in Finder ]   [ Open PDF ]               │
│                                                          │
│                    Convert Another                       │
│                                                          │
└──────────────────────────────────────────────────────────┘
```

Primary action: **Open PDF**
Secondary: **Show in Finder**
Tertiary: **Convert Another**

### 4.11 Error State

Errors should be human-readable.

Bad:

> Error: subprocess exited with code 1

Good:

> **Couldn't convert this book**
>
> The EPUB appears to contain formatting that couldn't be converted to PDF.
>
> Try enabling **Preserve EPUB Styling** or choose another output format.
>
> **[ Try Again ]**

Advanced users can optionally access:

> Show Technical Details

which reveals the actual error/log — this should surface the underlying
`ConvError` variant and engine log output (§2) verbatim, not a re-summarized
version.

### 4.12 Multiple Files

This is a natural second-step feature and worth designing for now.

Allow dropping multiple books:

```text
3 books ready to convert

┌─────────────────────────────────────────────┐
│ 📕 The Great Gatsby.epub             1.2 MB │
│ 📕 Dracula.epub                       2.4 MB │
│ 📕 Emma.epub                          1.8 MB │
└─────────────────────────────────────────────┘

Convert all to

[ PDF ▼ ]

Save to
[ ~/Downloads ]

                    [ Convert All ]
```

During conversion:

```text
Converting 2 of 3

Dracula.epub

████████████████░░░░  82%

[ Cancel All ]
```

At completion:

```text
✓ 3 books converted

[ Open Folder ]
```

### 4.13 Recent Conversions

The home screen can show a small history.

```text
Recent

The Great Gatsby
EPUB → PDF · Today

Dracula
EPUB → PDF · Yesterday

Emma
EPUB → PDF · Aug 20
```

Each row could have a trailing `…` menu:

```text
Open
Show in Finder
Convert Again
Remove from History
```

Do not store the actual books unless the user explicitly chooses to.

Store only conversion history/metadata.

### 4.14 Menu Bar

Native macOS menu:

```text
Bookdrop

About Bookdrop
Settings…
Check for Updates…
Quit Bookdrop

File

Open…
Open Recent
Convert…
Close Window

Edit

Undo
Redo
Cut
Copy
Paste
Select All

View

Show Sidebar
Enter Full Screen

Help

Bookdrop Help
```

Keyboard shortcuts:

**⌘O** — Open file
**⌘⇧O** — Open output folder
**⌘Enter** — Convert
**⌘,** — Settings
**Esc** — Cancel conversion

### 4.15 Settings

Keep Settings extremely small for MVP.

#### General

**Default output location**

- Same folder as source
- Downloads
- Ask every time
- Custom folder

**After conversion**

☑ Open converted file
☑ Show notification
☑ Reveal in Finder

#### Conversion

☑ Remember last output format
☑ Preserve original styling by default

#### Advanced

**Temporary files**

[ Clear Temporary Files ]

**Logs**

[ Open Logs Folder ]

### 4.16 macOS Notifications

After a background conversion:

> **Bookdrop**
>
> The Great Gatsby has been converted to PDF.
>
> [Open PDF]

This makes the app useful even when minimized.

### 4.17 Dock Behavior

While conversion is running, the Dock icon can show progress.

Conceptually:

```text
Bookdrop
   ↓
[████████░░] 67%
```

On completion, optionally show a brief Dock bounce.

Do not bounce indefinitely.

### 4.18 Accessibility

Support:

- Full keyboard navigation
- VoiceOver labels
- Dynamic text sizing
- High contrast
- Reduced motion
- Clear focus states

Drag-and-drop must never be the only way to perform an action.

Every drag/drop operation should have an equivalent: **Choose File…**

### 4.19 First Launch

Don't show a giant onboarding carousel.

Just show:

```text
                   Bookdrop

             Convert books easily.

         Drop an ebook here to begin

               [ Choose File… ]

      EPUB · PDF · MOBI · TXT · HTML
```

That's it.

### 4.20 The Ideal Core Screen

If we boil the entire product down to one screen, this is it:

```text
┌─────────────────────────────────────────────────────────────┐
│  Bookdrop                                            ⚙         │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│                  Convert your book                         │
│                                                             │
│     ┌───────────────────────────────────────────────┐       │
│     │                                               │       │
│     │                 📕                            │       │
│     │                                               │       │
│     │             The Great Gatsby                  │       │
│     │             F. Scott Fitzgerald               │       │
│     │                                               │       │
│     │              EPUB · 1.2 MB                    │       │
│     │                                               │       │
│     └───────────────────────────────────────────────┘       │
│                                                             │
│                  EPUB       →       PDF                     │
│                                                             │
│     Save to                                                 │
│     ~/Downloads                              Choose…        │
│                                                             │
│     PDF Options                                             │
│     Page Size     US Letter       Margins    Normal         │
│                                                             │
│     ☑ Include cover       ☑ Table of contents               │
│                                                             │
│                                        [ Convert ]           │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

**That's the product.** Everything else should support this workflow rather
than compete with it.

### 4.21 Visual direction

Minimal Apple utility aesthetic.

**Colors** — mostly system colors: system background, secondary background,
label colors, system accent color.

**Typography** — SF Pro / system font.

**Icons** — SF Symbols: `arrow.down.doc`, `book.closed`, `doc.richtext`,
`arrow.right`, `checkmark.circle.fill`, `folder`, `gearshape`, `xmark`.

**Visual hierarchy** — the most important element should always be obvious:

What file am I converting? → What format am I converting it to? → Where will
it go? → Convert.

Everything else should stay out of the way.

---

## 5. Engine/app integration architecture (Version 2 target)

Both source documents converge on the same shape independently: don't
implement every conversion pair directly (EPUB→PDF, EPUB→DOCX, MOBI→PDF,
MOBI→DOCX, ...) — go through a shared intermediate representation.

```text
                    ┌───────────────┐
                    │     EPUB      │
                    └───────┬───────┘
                            │
                            ▼
                    ┌───────────────┐
                    │  DocumentIR   │
                    │ (§2, engine)  │
                    └───────┬───────┘
                            │
             ┌──────────────┼──────────────┐
             ▼              ▼              ▼
          ┌──────┐       ┌──────┐       ┌──────┐
          │ PDF  │       │ EPUB │       │ DOCX │
          └──────┘       └──────┘       └──────┘
```

On the app side, this is exactly `Registry<DocumentIR>` (§2) — the UX doc's
"Book Model / Intermediate Representation" is the `DocumentIR` struct, and
its "conversion engine isolated behind a clean protocol" is the
`InputPlugin`/`OutputPlugin` trait boundary. Resolving §0 (FFI vs
Swift-native) determines whether that protocol is a thin Swift wrapper
calling into the Rust `anyform` crate, or a parallel Swift implementation of
the same shape.

---

## 6. Roadmap

### Engine — open questions / next steps

- ✅ Cargo workspace (`anyform-core`, `anyform-doc`, `anyform-ffi`,
  `anyform-cli`) scaffolded with a working EPUB input plugin.
- ✅ Headless-Chromium integration resolved: bundle `chrome-headless-shell`
  (Chrome for Testing) for both macOS architectures, driven over CDP.
- ✅ FFI bridge into the macOS app resolved and shipped (§0, §5), not a
  separate Swift-native path, for the PDF output plugin.
- Third-party dynamic plugin loading: still undecided, still out of scope.
  No concrete need for it has come up.
- `ImageIR`/`AudioIR` families: not started. Only relevant once/if the
  format matrix expands beyond documents (see the "Format matrix" table
  in §1).

### App — Version 1.0 (MVP)

**Input:** EPUB. **Output:** PDF.

UX scope:

- Drag & drop
- File picker
- EPUB metadata extraction
- Cover preview
- Output filename
- Output location
- Page size
- Margins
- Include cover
- Include table of contents
- Progress indicator
- Cancel
- Success state
- Open PDF
- Show in Finder
- Basic conversion history
- macOS notifications
- Settings

This is enough to make the app feel complete.

### App — Version 1.1 (shipped)

- ✅ EPUB → TXT — `TxtConverter`: each chapter imported via
  `NSAttributedString(html)`, plain-text chapters joined with a blank-line
  separator.
- ✅ EPUB → HTML — `HtmlConverter`: single self-contained file, CSS inlined
  into `<style>`, images embedded as base64 `data:` URIs (zero external
  references, works offline).
- ✅ EPUB → DOCX — `DocxConverter`: uses `NSAttributedString`'s native
  `.officeOpenXML` write path (the same mechanism TextEdit uses for "Save
  as Word Document") rather than hand-rolled OOXML — verified by opening
  real output in actual Microsoft Word.
- ✅ Multiple file conversion — shipped ahead of schedule in v1.0.
- ✅ Custom page sizes — `PageSize.custom` + width/height (inches) fields
  on `PDFOptions`.
- ✅ Typography controls, header/footer, page numbering — shipped ahead of
  schedule in v1.0.
- **"Better EPUB CSS handling" — scoped down, not a full pagination
  rewrite.** The PDF renderer already uses a real engine (WKWebView), so
  CSS support is structurally good; the one known real gap (pagination is a
  raw Y-slice, not CSS-`break`-aware, so content can slice mid-paragraph at
  a page boundary) remains open — a deeper pagination rewrite, deferred.
  What v1.1 actually delivered here: the preserve/remove-styling toggle now
  applies consistently across PDF/HTML/DOCX, not just PDF.

**Architecture added:** an `OutputFormat` enum (`pdf`/`txt`/`html`/`docx`)
that `AppCoordinator` and `MultiConversionModel` dispatch on, so each
format's converter is a self-contained unit (`Services/TxtConverter.swift`,
`HtmlConverter.swift`, `DocxConverter.swift`, alongside the existing
`PdfConverter.swift`) — mirrors the input→IR→output shape from §2's Rust
design, just without the Rust. `PDFOptions` carries fields
(`includeCover`, `generateTableOfContents`, `preserveEpubStyling`) reused
by every format, not just PDF — the struct name is legacy naming debt worth
a rename in a future pass, not worth the churn/risk in this one.

### Version 2: shipped (EPUB → PDF on the Rust engine)

The vertical slice described in §5: EPUB parsing and Chromium-backed PDF
rendering both on `anyform`, called from Swift through `anyform-ffi`.
TXT/HTML/DOCX still go through the original Swift-native converters. See
the Status section at the top of this file for the full detail and for
what changed along the way (two real correctness bugs found against real
books and fixed, default typography matched to calibre's own defaults).

### What's next: full roadmap

Grouped by how close each item is to being worth picking up, not by
importance. An item lower down isn't lower value, it's usually just
bigger or needs more scoping before starting.

**Near-term, scoped, worth doing next:**

- ~~**A permanent real-book regression suite.**~~ Done. Four real,
  public-domain Gutenberg EPUBs committed to `anyform-doc/tests/fixtures/`
  (`doctor-dolittle.epub` - image-heavy, `pride-and-prejudice.epub` - long
  many-chapter novel, `origin-of-species.epub` - footnotes + glossary +
  large index, `scientific-american-supplement.epub` - real `<table>`
  markup), with `real_book_tests.rs` asserting page-count sanity and known
  text content against each. All bounds/strings were calibrated against
  actual conversion output, not guessed.
- ~~**Parallel chapter rendering.**~~ Done. Chapters now render across a
  pool of Chrome tabs (`render_chapters_parallel` in `pdf.rs`) sized to
  `available_parallelism` (capped at 8), matching calibre's own
  CPU-count-sized worker pool. Measured 4.2x speedup on the 29-chapter
  Origin of Species fixture (41.0s to 9.7s on an 8-core Mac). Verified
  spine order survives even though workers finish wildly out of order,
  and that cancellation (polled independently by every worker against the
  same shared flag) actually stops in-flight tabs rather than running
  each chunk to completion - this had zero prior Rust-level test coverage.
- ~~**A broader defensive-CSS audit.**~~ Done, partially. Wide unwrapped
  `<pre>`/`<code>` blocks hit the exact same "no shrink to fit" clipping
  bug as images - confirmed by rendering a synthetic fixture
  (`css-edge-cases.epub`) before and after, screenshot showed a line
  cut off mid-word. Fixed with `white-space: pre-wrap` +
  `overflow-wrap: break-word`. CSS multi-column layouts were audited the
  same way and did *not* reproduce a clipping bug - Chrome sizes columns
  to fit their container rather than overflowing it - so no fix was
  applied there; a hypothesis that didn't hold up under an actual test.
  Absolutely-positioned decorative elements are still unaudited: no
  concrete failure case or real fixture surfaced one, and a blanket CSS
  override for arbitrary `position: absolute` content risks breaking
  legitimate use (footnote markers, etc.) without evidence it's needed.
  Left for whenever a real book actually exercises it.
~~**DRM font deobfuscation.**~~ Done. No real DRM'd EPUB ever turned up to
  develop against, so a synthetic fixture (`drm-fonts.epub`) was built
  instead: two font files obfuscated independently (a Python script using
  `hashlib`/`uuid` directly, not this codebase) with the exact algorithm
  calibre's own `epub_input.py` implements - fetched and read directly
  from calibre's source rather than trusted from a written description,
  since a byte-count or key-derivation mistake would silently corrupt
  fonts without any test catching it. Both EPUB font-obfuscation schemes
  are handled: IDPF (`http://www.idpf.org/2008/embedding`, 1040 bytes, key
  = SHA-1 of the whitespace-stripped unique identifier) and Adobe
  (`http://ns.adobe.com/pdf/enc#RC`, 1024 bytes, key = the raw 16 bytes of
  whichever `<dc:identifier>` is a UUID). Applied automatically whenever
  `META-INF/encryption.xml` declares one of these two algorithms for a
  resource; anything else (real encryption, an unrecognized algorithm) is
  left untouched rather than guessed at.
~~**Verify internal EPUB links survive conversion.**~~ Done - and it
  turned out they didn't survive at all, a real bug, not just an
  unverified assumption. EPUB footnote/cross-reference links are written
  as `<a href="thisfile.html#note3">` even when linking within the same
  file, and Chrome only recognizes a fragment link as same-document
  navigation when the href resolves to the exact URL of the page loaded -
  every chapter rendered from a renamed temp file (`__anyform_render_N`),
  so *every* internal link, same-chapter or cross-chapter, silently
  became a dead `file://` URI pointing at a temp file deleted right after
  rendering. Fixed two ways: chapters now render in place under their
  original filename (temporarily overwritten, then restored - safe since
  worker threads never share a chapter's file), so same-chapter links get
  Chrome's native working destination again; and a post-merge pass
  (`fix_cross_chapter_links`) repairs remaining cross-chapter dead links,
  either to the exact anchor (if the target chapter's fragment survived
  into the merged Dests dictionary) or to the top of the target chapter as
  a fallback. Verified on Origin of Species: 1296 dead links before the
  fix, 0 after (8 legitimate external Gutenberg links correctly left
  alone, 6 exact same-chapter footnote jumps, 1282 chapter-top jumps for
  cross-chapter references).

**Medium-term, needs scoping before starting:**

- **Phase 6: port TXT/HTML/DOCX output to the Rust engine.** Explicitly
  deferred earlier this session as an accepted interim state, not a
  problem. Revisit only if there's a concrete reason to want format
  parity on the Rust engine (e.g. reusing the engine outside Bookdrop).
  Mechanical extension of the same `OutputPlugin<DocumentIR>` trait;
  `MarkdownOutput` in §2 is the template. Once done, the now-unused Swift
  `EpubParser`/`EpubXML`/`TxtConverter`/`HtmlConverter`/`DocxConverter`/
  `PdfConverter` get deleted outright, no compatibility shims.
- **New output formats: true EPUB, MOBI/AZW3, Markdown.** Markdown is
  cheap (§2 already has a worked example, pure Rust, no rendering engine
  needed). EPUB-out and MOBI/AZW3-out are bigger: EPUB-out means
  repackaging/re-flowing the `DocumentIR` back into a valid EPUB
  container (manifest, spine, nav document), and MOBI/AZW3 needs either a
  Rust KF8 writer (check crates.io first) or shelling out to Amazon's
  `kindlegen`-equivalent tooling.
- **New input formats: MOBI/AZW3, FB2, DOCX-in.** Check for an existing
  well-maintained Rust `mobi`/`azw3` crate before writing a parser from
  scratch. FB2 (common for Russian-language ebooks) is XML-based and
  structurally closer to EPUB's own input plugin than MOBI is. DOCX-in
  would need real OOXML parsing (a `docx-rs`-style crate), distinct from
  the `NSAttributedString`-based DOCX *output* the Swift path already
  does.
- **Universal (arm64 + x86_64) Bookdrop binary.** Both Chromium
  architectures are already bundled (this session), but the Swift
  executable itself is still host-arch-only (`swift build` with no
  `--arch` flags). Needs `swift build --arch arm64 --arch x86_64` + `lipo`
  in `build-app.sh`/`release.sh`, and testing on actual Intel hardware
  (or at minimum Rosetta) before trusting it, given this session's own
  lesson about not trusting a build without running it for real.

**Long-term / speculative, needs its own dedicated scoping pass:**

- **PDF as an input format.** Came up directly this session (user asked
  about PDF→EPUB) and is worth flagging clearly: this is not a routine
  format-plugin addition. Every input plugin so far (EPUB, and the
  planned MOBI/FB2/DOCX) is fundamentally *flowable text with structure
  markup* being read into `DocumentIR`. PDF is fixed-layout with no
  guaranteed semantic structure, positioned glyphs, not flowing text.
  Reconstructing readable, reflowable EPUB output from that is closer to
  the document-layout-analysis/OCR problem space than to anything the
  engine does today (calibre's own PDF input support has historically
  been one of its weaker, most complained-about conversion paths, for
  exactly this reason). Worth a dedicated research spike before
  committing to it, not a "just add the plugin" task.
- **Third-party plugin loading.** Still explicitly out of scope (§2, §6
  engine notes above). Would need `libloading` + a stable ABI. No
  concrete driving need yet.
- **`ImageIR`/`AudioIR` families.** The original engine ambition (§1, §2)
  was explicitly broader than ebooks. Nothing currently pulls for this;
  revisit only if there's an actual product reason to convert images or
  audio, not just because the architecture allows for it.

**Distribution and polish (not engine work, but real gaps):**

- **Notarization.** Currently ad-hoc signed only, which is why every
  install needs the right-click-Open Gatekeeper workaround documented in
  the README. Real notarization needs a paid Apple Developer Program
  membership ($99/year), a cost/logistics decision for the user to make,
  not an engineering one.
- **Auto-update.** No update mechanism today; users re-download the DMG
  manually. Sparkle is the standard choice for this on macOS.
- **Saved PDFOptions presets.** Advanced Options resets to the same
  defaults every conversion. A "save as preset" / "my usual settings"
  flow would help anyone who consistently wants non-default margins,
  fonts, or headers/footers.

---

## 7. Naming

Engine name: **Anyform**. Tagline: "Convert anything into any form."

- Crate/binary name: `anyform` (check crates.io availability before
  publishing).
- CLI usage: `anyform convert book.epub book.pdf`.
- Proposed workspace layout:
  - `anyform-core` — traits, registry, error types, options.
  - `anyform-doc` — `DocumentIR` + epub/html/pdf/markdown plugins.
  - `anyform-cli` — the `anyform` binary.
  - (later) `anyform-image`, `anyform-audio` for other families.
- macOS app: **Bookdrop** — its own brand (see §0), Swift-native for v1,
  a candidate to sit on top of the `anyform` engine via FFI in v2.
