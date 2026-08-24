# Mac Book Converter — UX Specification

## 1. Product Goal

A lightweight macOS utility for converting ebooks and documents between common book formats.

### Primary use case

> EPUB → PDF

### Secondary formats

**Input**
- EPUB
- MOBI
- AZW / AZW3
- FB2
- HTML
- TXT
- DOCX

**Output**
- PDF
- EPUB
- MOBI/AZW3
- TXT
- HTML
- DOCX

For the MVP, support:

**EPUB → PDF**

Then expand the format matrix after the core workflow is solid.

---

# 2. Design Principles

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

# 3. Main Window

Recommended size:

**720 × 520 px**

Resizable, with a minimum around:

**600 × 450 px**

### Layout

```text
┌──────────────────────────────────────────────────────────┐
│  BookConvert                              ⚙              │
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

---

# 4. Drag & Drop State

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

---

# 5. File Loaded State

After selecting an EPUB:

```text
┌──────────────────────────────────────────────────────────┐
│  BookConvert                                              │
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

---

# 6. Book Information

Extract metadata from the EPUB automatically.

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

---

# 7. Output Format

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

Only display formats that are actually supported by the current input.

For example:

```text
EPUB

Convert to:

✓ PDF
  TXT
  HTML
```

This prevents the user from selecting an impossible conversion.

---

# 8. PDF Options

For EPUB → PDF, keep the first version simple.

### Basic options

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

### Advanced options

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

---

# 9. Advanced Options

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

---

# 10. Output Filename

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

---

# 11. Conversion Progress

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

Show human-readable status rather than technical logs.

---

# 12. Conversion Complete

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

Primary action:

**Open PDF**

Secondary:

**Show in Finder**

Tertiary:

**Convert Another**

---

# 13. Error State

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

which reveals the actual error/log.

---

# 14. Multiple Files

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

---

# 15. Recent Conversions

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

---

# 16. Menu Bar

Native macOS menu:

```text
BookConvert

About BookConvert
Settings…
Check for Updates…
Quit BookConvert

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

BookConvert Help
```

Keyboard shortcuts:

**⌘O** — Open file

**⌘⇧O** — Open output folder

**⌘Enter** — Convert

**⌘,** — Settings

**Esc** — Cancel conversion

---

# 17. Settings

Keep Settings extremely small for MVP.

### General

**Default output location**

- Same folder as source
- Downloads
- Ask every time
- Custom folder

**After conversion**

☑ Open converted file

☑ Show notification

☑ Reveal in Finder

### Conversion

☑ Remember last output format

☑ Preserve original styling by default

### Advanced

**Temporary files**

[ Clear Temporary Files ]

**Logs**

[ Open Logs Folder ]

---

# 18. macOS Notifications

After a background conversion:

> **BookConvert**
>
> The Great Gatsby has been converted to PDF.
>
> [Open PDF]

This makes the app useful even when minimized.

---

# 19. Dock Behavior

While conversion is running, the Dock icon can show progress.

Conceptually:

```text
BookConvert
   ↓
[████████░░] 67%
```

On completion, optionally show a brief Dock bounce.

Do not bounce indefinitely.

---

# 20. Accessibility

Support:

- Full keyboard navigation
- VoiceOver labels
- Dynamic text sizing
- High contrast
- Reduced motion
- Clear focus states

Drag-and-drop must never be the only way to perform an action.

Every drag/drop operation should have an equivalent:

**Choose File…**

---

# 21. First Launch

Don't show a giant onboarding carousel.

Just show:

```text
                 BookConvert

             Convert books easily.

         Drop an ebook here to begin

               [ Choose File… ]

      EPUB · PDF · MOBI · TXT · HTML
```

That's it.

---

# 22. Recommended MVP Scope

### Version 1.0

Implement only:

**Input**
- EPUB

**Output**
- PDF

### UX

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

---

# 23. Version 1.1

Add:

- EPUB → TXT
- EPUB → HTML
- EPUB → DOCX
- Multiple file conversion
- Custom page sizes
- Typography controls
- Header/footer
- Page numbering
- Better EPUB CSS handling

---

# 24. Version 2

Expand into a proper conversion engine:

```text
                    ┌───────────────┐
                    │     EPUB      │
                    └───────┬───────┘
                            │
                            ▼
                    ┌───────────────┐
                    │ Book Model /  │
                    │ Intermediate  │
                    │ Representation│
                    └───────┬───────┘
                            │
             ┌──────────────┼──────────────┐
             ▼              ▼              ▼
          ┌──────┐       ┌──────┐       ┌──────┐
          │ PDF  │       │ EPUB │       │ DOCX │
          └──────┘       └──────┘       └──────┘
```

This architecture is important.

Rather than implementing every conversion as:

```text
EPUB → PDF
EPUB → DOCX
EPUB → TXT
MOBI → PDF
MOBI → DOCX
...
```

use an intermediate book representation where possible.

That makes the conversion matrix much easier to grow.

---

# 25. Visual Direction

I'd go with a **minimal Apple utility aesthetic**.

### Colors

Use mostly system colors:

- System background
- Secondary background
- Label colors
- System accent color

### Typography

SF Pro / system font.

### Icons

SF Symbols:

- `arrow.down.doc`
- `book.closed`
- `doc.richtext`
- `arrow.right`
- `checkmark.circle.fill`
- `folder`
- `gearshape`
- `xmark`

### Visual hierarchy

The most important element should always be obvious:

**What file am I converting?**

↓

**What format am I converting it to?**

↓

**Where will it go?**

↓

**Convert**

Everything else should stay out of the way.

---

# 26. The Ideal Core Screen

If we boil the entire product down to one screen, I'd aim for this:

```text
┌─────────────────────────────────────────────────────────────┐
│  BookConvert                                      ⚙         │
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

**That's the product.** Everything else should support this workflow rather than compete with it.

For implementation, I'd strongly recommend building the first version as a **native SwiftUI macOS app**, with the conversion engine isolated behind a clean `BookConverter` protocol. That will let you start with EPUB → PDF and later swap in additional conversion engines without redesigning the UI.