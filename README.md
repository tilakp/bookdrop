# Bookdrop

A native macOS app for converting ebooks — drag an EPUB in, get back a PDF,
TXT, HTML, or DOCX.

## Status

**v1.1 shipped.** EPUB → PDF / TXT / HTML / DOCX, drag-and-drop, multi-file
conversion, history, settings, notifications. 44 passing tests, each output
format verified by opening real output in a real consuming app (not just
"the code ran without throwing").

**Next up: Version 2** — swapping the current Swift-native conversion
pipeline for the Rust `anyform` engine design via FFI, and/or expanding
past EPUB input. See [`ANYFORM-FULL-SPEC.md`](ANYFORM-FULL-SPEC.md) — the
`## Status` section at the top is the fastest way back up to speed, and §6
has the full roadmap.

## Building

```sh
cd Bookdrop
swift build
swift test
```

44 tests, ~2s. All use scratch directories or an in-memory settings store
(see `Tests/BookdropTests/AppCoordinatorTests.swift`) — safe to run
repeatedly, never touches real user data.

## Running

`swift run` launches the app, but as a bare executable with no real bundle
identifier — no Dock icon, no working system notifications. For the real
thing:

```sh
./Scripts/build-app.sh debug
open .build/debug/Bookdrop.app
```

## Project layout

- `Bookdrop/Sources/Bookdrop/` — the app: `Models/`, `Services/` (parser,
  per-format converters, app coordinator), `Views/`, `App/`
- `Bookdrop/Tests/BookdropTests/` — the test suite
- `Bookdrop/Resources/` — `Info.plist`, app icon
- `Bookdrop/Scripts/` — icon generation (`make_icon.swift`), `.app` bundle
  assembly (`build-app.sh`)
- `ANYFORM-FULL-SPEC.md` — the full spec: engine architecture, macOS UX
  design, decisions made along the way, and the roadmap
- `archive/` — the original source documents the spec was merged from
