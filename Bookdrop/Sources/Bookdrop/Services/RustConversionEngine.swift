import CAnyform
import Foundation

enum RustConversionEngineError: LocalizedError {
    case cancelled
    case engine(String)

    var errorDescription: String? {
        switch self {
        case .cancelled:
            return "Conversion was cancelled."
        case .engine(let message):
            return message
        }
    }
}

/// Bridges the C callbacks (which can't capture Swift context — they're
/// plain `@convention(c)` function pointers) to one in-flight conversion
/// via an `Unmanaged` reference passed through as `ctx`. Not `@MainActor`
/// itself since Rust invokes these callbacks from its own background
/// thread; state that touches `ConversionProgress` hops to the main actor
/// explicitly (see `reportProgress`).
private final class ConversionContext: @unchecked Sendable {
    private let progress: ConversionProgress
    private let lock = NSLock()
    private var continuation: CheckedContinuation<Void, Error>?
    private var token: OpaquePointer?
    private var wasCancelled = false

    init(progress: ConversionProgress) {
        self.progress = progress
    }

    func setContinuation(_ continuation: CheckedContinuation<Void, Error>) {
        lock.lock()
        self.continuation = continuation
        lock.unlock()
    }

    func setToken(_ token: OpaquePointer?) {
        lock.lock()
        self.token = token
        let cancelPending = wasCancelled
        lock.unlock()
        // Cancellation may have already been requested before the token
        // existed (the task can be cancelled the instant it's created,
        // before `anyform_convert_start` returns) — apply it now instead
        // of silently dropping it.
        if cancelPending, let token {
            anyform_cancel(token)
        }
    }

    /// Called from `withTaskCancellationHandler`'s `onCancel` — mirrors how
    /// `AppCoordinator.cancelConversion` already cancels the wrapping
    /// `Task` for the Swift-native converters.
    func requestCancellation() {
        lock.lock()
        let token = self.token
        wasCancelled = true
        lock.unlock()
        if let token {
            anyform_cancel(token)
        }
    }

    func reportProgress(fraction: Double, stage: String) {
        let progress = self.progress
        Task { @MainActor in
            progress.fraction = fraction
            progress.stageText = stage
        }
    }

    func complete(success: Bool, message: String?) {
        lock.lock()
        let continuation = self.continuation
        self.continuation = nil
        let token = self.token
        self.token = nil
        let cancelled = wasCancelled
        lock.unlock()
        if let token {
            anyform_free_cancel_token(token)
        }
        if success {
            continuation?.resume()
        } else if cancelled {
            continuation?.resume(throwing: RustConversionEngineError.cancelled)
        } else {
            continuation?.resume(throwing: RustConversionEngineError.engine(message ?? "Conversion failed"))
        }
    }
}

private func rustProgressCallback(
    _ fraction: Double, _ stage: UnsafePointer<CChar>?, _ ctx: UnsafeMutableRawPointer?
) {
    guard let ctx else { return }
    let context = Unmanaged<ConversionContext>.fromOpaque(ctx).takeUnretainedValue()
    context.reportProgress(fraction: fraction, stage: stage.map { String(cString: $0) } ?? "")
}

/// The Rust side's error payload is always `{"status":"error","message":"..."}`
/// (`ErrorResult` in `anyform-ffi/src/lib.rs`) — decoded here so the user
/// sees the actual message rather than the raw JSON blob. Falls back to
/// the raw string if decoding ever fails, so a malformed payload still
/// surfaces *something* rather than silently losing the error entirely.
private struct RustErrorPayload: Decodable {
    let message: String
}

private func rustCompleteCallback(
    _ success: Int32, _ errorJSON: UnsafePointer<CChar>?, _ ctx: UnsafeMutableRawPointer?
) {
    guard let ctx else { return }
    // Balances the `passRetained` in `RustConversionEngine.convert` — this
    // callback fires exactly once per conversion, so this is where the
    // context's retain count comes back down.
    let context = Unmanaged<ConversionContext>.fromOpaque(ctx).takeRetainedValue()
    let rawJSON = errorJSON.map { String(cString: $0) }
    let message = rawJSON.flatMap { json -> String? in
        guard let data = json.data(using: .utf8) else { return nil }
        return try? JSONDecoder().decode(RustErrorPayload.self, from: data).message
    } ?? rawJSON
    context.complete(success: success == 1, message: message)
}

/// Locates the Chromium binary bundled into the real `.app`
/// (`Contents/Resources/Chromium/<arch>/…`, copied there by
/// `Scripts/build-app.sh` for both architectures — see plan Phase 7/item 4)
/// for whichever architecture is actually running. Returns `nil` when not
/// present — `swift test`/`swift run` without going through build-app.sh —
/// so the Rust side's own dev-time fallback (relative to `anyform-doc`'s
/// `CARGO_MANIFEST_DIR`) finds the vendored copy instead.
private func bundledChromiumPath() -> String? {
    guard let resourceURL = Bundle.main.resourceURL else { return nil }
    #if arch(arm64)
        let arch = "mac-arm64"
    #elseif arch(x86_64)
        let arch = "mac-x64"
    #else
        return nil
    #endif
    let path = resourceURL.appendingPathComponent("Chromium/\(arch)/chrome-headless-shell").path
    return FileManager.default.fileExists(atPath: path) ? path : nil
}

/// Same pattern as `bundledChromiumPath()`, for the `libpdfium.dylib`
/// `Scripts/build-app.sh` copies into `Contents/Resources/Pdfium/<arch>/`
/// (see `rust/scripts/fetch-pdfium.sh`). `PdfInput`'s own dev-tree fallback
/// (relative to `anyform-doc`'s `CARGO_MANIFEST_DIR`) covers `swift test`/
/// `swift run` when this returns `nil`.
private func bundledPdfiumPath() -> String? {
    guard let resourceURL = Bundle.main.resourceURL else { return nil }
    #if arch(arm64)
        let arch = "mac-arm64"
    #elseif arch(x86_64)
        let arch = "mac-x64"
    #else
        return nil
    #endif
    let path = resourceURL.appendingPathComponent("Pdfium/\(arch)/libpdfium.dylib").path
    return FileManager.default.fileExists(atPath: path) ? path : nil
}

/// Same pattern again, for the bundled `boko` binary. This was a real,
/// pre-existing gap: `KindleInput` on the Rust side (`kindle.rs`) has
/// looked for a `boko_path` option since it was written, but nothing here
/// ever set one — every shipped-app AZW3/KFX/MOBI conversion silently fell
/// through to Rust's dev-tree fallback (a path baked in from
/// `CARGO_MANIFEST_DIR` at compile time), which only happens to exist on a
/// developer's own machine. `KindleNormalizer` has its own private copy of
/// this same lookup for the Swift-side preview parse — kept separate
/// rather than shared, matching this codebase's existing pattern of
/// independent per-call-site path resolution (Rust has three near-identical
/// `resolve_*_path` functions for exactly this reason).
private func bundledBokoPath() -> String? {
    guard let resourceURL = Bundle.main.resourceURL else { return nil }
    #if arch(arm64)
        let arch = "mac-arm64"
    #elseif arch(x86_64)
        let arch = "mac-x64"
    #else
        return nil
    #endif
    let path = resourceURL.appendingPathComponent("Boko/\(arch)/boko").path
    return FileManager.default.fileExists(atPath: path) ? path : nil
}

/// Builds the JSON `anyform_convert_start` reads its render options from —
/// mirrors every field of `PDFOptions` (`Sources/Bookdrop/Models/PDFOptions.swift`)
/// so nothing in the Advanced Options UI is silently ignored by the Rust
/// engine for the `.pdf` path. `PDFOptions.pageDimensions`/`.margins.points`
/// are already in points (72/inch, orientation pre-swapped for landscape)
/// — converted to inches here since that's the unit Chrome's
/// `Page.printToPDF` wants.
///
/// Reused unchanged for every other output format too (deliberate — see
/// `RustConversionEngine`'s own doc comment for why a new/narrower options
/// type wasn't introduced): each Rust output plugin reads only the
/// `Options` keys it actually cares about via its own `opts.get_*` calls,
/// each with its own Rust-side default, so page-size/margin/font/
/// typography/header-footer keys are simply ignored by `TxtOutput`/
/// `HtmlOutput`/`DocxOutput`/`EpubOutput`. Only `include_cover` matters to
/// all of them; `generate_table_of_contents` additionally matters to
/// `HtmlOutput` (`DocxOutput`/`EpubOutput` ignore it too — see their own
/// doc comments for why neither has a TOC-toggle concept).
private func conversionOptionsJSON(pdfOptions: PDFOptions) -> String {
    let pointsPerInch = 72.0
    let dimensions = pdfOptions.pageDimensions
    var payload: [String: Any] = [
        "page_width_in": Double(dimensions.width) / pointsPerInch,
        "page_height_in": Double(dimensions.height) / pointsPerInch,
        "margin_in": Double(pdfOptions.margins.points) / pointsPerInch,
        "include_cover": pdfOptions.includeCover,
        "generate_table_of_contents": pdfOptions.generateTableOfContents,
        "font_family": pdfOptions.fontFamily,
        "font_size_pt": pdfOptions.fontSizePt,
        "line_spacing": pdfOptions.lineSpacing,
        "preserve_epub_styling": pdfOptions.preserveEpubStyling,
        "remove_publisher_styling": pdfOptions.removePublisherStyling,
        "show_page_numbers": pdfOptions.showPageNumbers,
        "include_headers": pdfOptions.includeHeaders,
        "include_footers": pdfOptions.includeFooters,
    ]
    if let chromiumPath = bundledChromiumPath() {
        payload["chromium_path"] = chromiumPath
    }
    if let pdfiumPath = bundledPdfiumPath() {
        payload["pdfium_path"] = pdfiumPath
    }
    if let bokoPath = bundledBokoPath() {
        payload["boko_path"] = bokoPath
    }
    guard let data = try? JSONSerialization.data(withJSONObject: payload),
        let json = String(data: data, encoding: .utf8)
    else { return "{}" }
    return json
}

/// Calls into the Rust `anyform` engine (`Bookdrop/rust/`) via the
/// `anyform-ffi` C ABI — every output format (PDF/EPUB/TXT/HTML/DOCX) as
/// of Phase 6 (see `ANYFORM-FULL-SPEC.md`). Only `book.sourceURL.path` and
/// `outputURL.path` cross the FFI boundary; the Rust side re-parses the
/// original file from scratch via its own registry (`document_registry()`
/// in `anyform-doc`), which dispatches both input parsing (by
/// `sourceURL`'s extension) and output format (by `outputURL`'s
/// extension) — so this Swift function needed no format-specific
/// branching to begin with, and none was added for the three new output
/// formats. `book.contentDirectory`/`.spine`/`.manifest`/`.toc` (the rest
/// of what `EpubParser`/`KindleNormalizer` populate) are unused here;
/// they still matter for `FileLoadedView`'s UI display and
/// `HistoryEntry` — see `KindleNormalizer`'s own doc comment.
@MainActor
enum RustConversionEngine {
    static func convert(
        book: Book, options: PDFOptions, outputURL: URL, progress: ConversionProgress
    ) async throws -> URL {
        let context = ConversionContext(progress: progress)
        let ctxPtr = Unmanaged.passRetained(context).toOpaque()

        try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
                context.setContinuation(continuation)
                let token = book.sourceURL.path.withCString { inputPath in
                    outputURL.path.withCString { outputPath in
                        conversionOptionsJSON(pdfOptions: options).withCString { optionsJSON in
                            anyform_convert_start(
                                inputPath, outputPath, optionsJSON,
                                rustProgressCallback, ctxPtr,
                                rustCompleteCallback, ctxPtr)
                        }
                    }
                }
                context.setToken(token)
            }
        } onCancel: {
            context.requestCancellation()
        }

        return outputURL
    }
}
