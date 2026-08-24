# Anyform — general file conversion utility

Status: design sketch, not yet implemented.

## Motivation

Prompted by looking at how calibre implements EPUB → PDF conversion: a generic
`Plumber` pipeline (`input plugin → intermediate representation (IR) → output
plugin`) built in Python/C++, where the actual HTML/CSS layout work for PDF is
delegated to Chromium (Qt WebEngine) and PDF post-processing (font/image
dedup, TOC/outline, metadata) is done via a podofo C++ binding.

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

## Core design

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
lower-priority write.

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

## Where Rust actually earns its keep

- Zip/XML parsing, font deobfuscation, manifest/spine building: memory-safe,
  fast, no GIL — a genuine win over calibre's Python input plugin.
- PDF post-processing (merge, outline, font/image dedup) via `lopdf`: single
  static binary instead of a podofo C++ binding + Python glue.
- The rendering step still needs Chromium (or another real engine)
  underneath — that part doesn't get better by being called from Rust.

## Naming

App name: **Anyform**. Tagline: "Convert anything into any form."

- Crate/binary name: `anyform` (check crates.io availability before publishing).
- CLI usage: `anyform convert book.epub book.pdf`.
- Proposed workspace layout:
  - `anyform-core` — traits, registry, error types, options.
  - `anyform-doc` — `DocumentIR` + epub/html/pdf/markdown plugins.
  - `anyform-cli` — the `anyform` binary.
  - (later) `anyform-image`, `anyform-audio` for other families.

## Open questions / next steps

- Scaffold the actual Cargo workspace (`anyform-core`, `anyform-doc`,
  `anyform-cli`) with the EPUB input plugin fully implemented.
- Decide on the headless-Chromium integration approach (bundle a browser?
  require one on `PATH`? use CDP over an existing install?).
- Decide whether third-party dynamic plugin loading is in scope for v1.
- Flesh out `ImageIR`/`AudioIR` families once the document family is solid.
