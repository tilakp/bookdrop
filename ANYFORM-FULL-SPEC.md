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

**Not started — Version 2.** Swapping the Swift-native conversion pipeline
for the Rust `anyform` engine (§5) via FFI, and/or expanding the
input/output format matrix beyond EPUB. See §6 "Version 2" for scope. This
is what's next when work resumes.

**Quick start:**
- Build: `cd Bookdrop && swift build`
- Test: `swift test` — 44 tests, ~2s, all use scratch directories / an
  in-memory `KeyValueStore` (see `Tests/BookdropTests/AppCoordinatorTests.swift`)
  so they never touch real user data — safe to run repeatedly.
- Run as a real `.app` bundle — needed for the Dock icon and system
  notifications, since `swift run` alone can't provide either (no real
  bundle identifier): `./Scripts/build-app.sh debug && open
  .build/debug/Bookdrop.app`
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

- Scaffold the actual Cargo workspace (`anyform-core`, `anyform-doc`,
  `anyform-cli`) with the EPUB input plugin fully implemented.
- Decide on the headless-Chromium integration approach (bundle a browser?
  require one on `PATH`? use CDP over an existing install?).
- Decide whether third-party dynamic plugin loading is in scope for v1.
- Flesh out `ImageIR`/`AudioIR` families once the document family is solid.
- Resolve §0: FFI bridge into the macOS app vs. a separate Swift-native path.

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

### Version 2 — full conversion engine

Expand into the full matrix (§1), backed by the multi-IR engine (§2, §5):
additional input formats (MOBI, AZW/AZW3, FB2, HTML, TXT, DOCX) and output
formats (EPUB, MOBI/AZW3, TXT, HTML, DOCX), plus `ImageIR`/`AudioIR`
families beyond documents.

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
