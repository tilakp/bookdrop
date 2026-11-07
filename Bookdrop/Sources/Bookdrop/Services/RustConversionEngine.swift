import CAnyform
import Foundation

/// Bridges the C callbacks (plain `@convention(c)` function pointers, which
/// cannot capture Swift context) to one in-flight conversion
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
        // before `anyform_convert_book_start` returns): apply it now instead
        // of silently dropping it.
        if cancelPending, let token {
            anyform_cancel(token)
        }
    }

    /// Called from `withTaskCancellationHandler`'s `onCancel`, when the
    /// task that runs the conversion is cancelled.
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

    func complete(success: Bool, errorJSON: String?) {
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
            continuation?.resume(throwing: EngineError.cancelled)
        } else {
            continuation?.resume(throwing: EngineError.decode(errorJSON))
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

private func rustCompleteCallback(
    _ success: Int32, _ errorJSON: UnsafePointer<CChar>?, _ ctx: UnsafeMutableRawPointer?
) {
    guard let ctx else { return }
    // Balances the `passRetained` in `RustConversionEngine.convert`: this
    // callback fires exactly once per conversion, so this is where the
    // context's retain count comes back down.
    let context = Unmanaged<ConversionContext>.fromOpaque(ctx).takeRetainedValue()
    context.complete(success: success == 1, errorJSON: errorJSON.map { String(cString: $0) })
}

/// The options JSON for `anyform_convert_book_start`: every field of
/// `PDFOptions` plus the bundled tool paths. All formats get the same
/// options. Each output plugin reads only the names it lists in
/// `anyform_formats()`, and the engine refuses names that no plugin reads.
/// `pageDimensions` and `margins.points` are in points (orientation
/// already applied), and the engine wants inches.
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
    payload.merge(BundledTools.engineOptions) { _, tool in tool }
    guard let data = try? JSONSerialization.data(withJSONObject: payload),
        let json = String(data: data, encoding: .utf8)
    else { return "{}" }
    return json
}

/// Runs one conversion in the Rust `anyform` engine through the
/// `anyform-ffi` C ABI. It converts the engine's own parsed copy of the
/// book (`Book.engineBook`), so the source is not read again. The engine
/// picks the output plugin from the output file's extension, so no format
/// needs Swift code.
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
                let token = outputURL.path.withCString { outputPath in
                    conversionOptionsJSON(pdfOptions: options).withCString { optionsJSON in
                        anyform_convert_book_start(
                            book.engineBook.pointer, outputPath, optionsJSON,
                            rustProgressCallback, ctxPtr,
                            rustCompleteCallback, ctxPtr)
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
