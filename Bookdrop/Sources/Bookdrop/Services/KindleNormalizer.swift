import CAnyform
import Foundation

enum KindleNormalizerError: LocalizedError {
    case converterMissing
    case drmProtected
    case conversionFailed(String)

    var errorDescription: String? {
        switch self {
        case .converterMissing:
            return "This app's Kindle-format converter is missing from the build."
        case .drmProtected:
            return "This book is DRM-protected, so it can't be converted. Kindle books bought from Amazon are encrypted."
        case .conversionFailed(let detail):
            return detail.isEmpty ? "This Kindle file couldn't be read." : "This Kindle file couldn't be read: \(detail)"
        }
    }
}

/// Mirrors `BookInfo`/`SpineItemWire`/`TocNodeWire` in `anyform-ffi/src/lib.rs`
/// exactly — the JSON `anyform_parse_book` returns.
private struct RustBookInfo: Decodable {
    struct SpineItemWire: Decodable { let id: String; let href: String }
    struct TocNodeWire: Decodable {
        let title: String
        let href: String?
        let children: [TocNodeWire]
    }
    let title: String
    let author: String?
    let coverBase64: String?
    let fileSizeBytes: Int64
    let spine: [SpineItemWire]
    let toc: [TocNodeWire]

    enum CodingKeys: String, CodingKey {
        case title, author, spine, toc
        case coverBase64 = "cover_base64"
        case fileSizeBytes = "file_size_bytes"
    }
}

private struct RustParseResult: Decodable {
    let status: String
    let book: RustBookInfo?
    let message: String?
}

private func toTocNode(_ wire: RustBookInfo.TocNodeWire) -> TocNode {
    TocNode(title: wire.title, href: wire.href, children: wire.children.map(toTocNode))
}

/// Normalizes Kindle-family ebook files (AZW3, AZW, KFX, MOBI) to EPUB
/// using the bundled `boko` binary, so the rest of the app — the
/// Swift-native EpubParser and every output-format converter that reads
/// from `Book.contentDirectory` — never needs to know these formats exist.
/// This mirrors `KindleInput` on the Rust side (`anyform-doc/src/kindle.rs`)
/// but is a separate, independent call: the Rust engine's own `.pdf`
/// output path re-reads `Book.sourceURL` from scratch and normalizes again
/// there via its own registry dispatch, so a book's *source* URL is always
/// kept pointing at the original Kindle file rather than this temp EPUB —
/// see `parse(fileAt:)` below.
///
/// `boko` runs as a subprocess, not linked code, because it's
/// GPL-3.0-or-later and Bookdrop is MIT (see `rust/scripts/fetch-boko.sh`
/// for the full rationale — same one that applies here).
enum KindleNormalizer {
    static let kindleExtensions: Set<String> = ["azw3", "azw", "kfx", "mobi"]

    /// Parses any book file Bookdrop accepts — EPUB directly, or a
    /// Kindle-family file normalized to EPUB first. The returned `Book`'s
    /// `sourceURL` and `fileSizeBytes` always describe the file the user
    /// actually picked, never the intermediate normalized EPUB, so
    /// "Convert Again" (which reopens `HistoryEntry.sourcePath`) and the
    /// PDF output path (which re-reads `Book.sourceURL` through the Rust
    /// engine's own registry) both keep working unchanged.
    static func parse(fileAt url: URL) throws -> Book {
        if url.pathExtension.lowercased() == "pdf" {
            return try parseViaRustEngine(fileAt: url)
        }
        guard kindleExtensions.contains(url.pathExtension.lowercased()) else {
            return try EpubParser.parse(fileAt: url)
        }

        let normalizedURL = try normalizeToEpub(url)
        defer { try? FileManager.default.removeItem(at: normalizedURL.deletingLastPathComponent()) }

        var book = try EpubParser.parse(fileAt: normalizedURL)
        book.sourceURL = url
        if let size = try? FileManager.default.attributesOfItem(atPath: url.path)[.size] as? Int64 {
            book.fileSizeBytes = size
        }
        return book
    }

    /// PDF has no Swift-native parser (unlike EPUB/`EpubParser` and
    /// Kindle-family/`normalizeToEpub` above) — `PdfInput` on the Rust side
    /// is the only place that knows how to turn a PDF into chapters, so the
    /// preview panel goes through `anyform_parse_book` (previously
    /// `anyform_parse_epub` — renamed once it was clear it was never
    /// EPUB-specific: `registry.parse` dispatches by extension the same way
    /// `registry.convert` does) rather than parsing anything itself. This
    /// means a PDF gets parsed twice per conversion — once here for the
    /// preview, once again inside `RustConversionEngine.convert` — which is
    /// the same double-parse `KindleNormalizer.parse` above already accepts
    /// for Kindle files (normalize-then-EpubParser here, `KindleInput`
    /// again on the Rust side). `contentDirectory`/`manifest` on the
    /// returned `Book` are unused placeholders — nothing in the UI reads
    /// them (see `RustConversionEngine`'s own doc comment) — the real
    /// content only exists inside the Rust engine's own temp work
    /// directory for the lifetime of a single `PdfInput::convert` call.
    private static func parseViaRustEngine(fileAt url: URL) throws -> Book {
        guard let cString = url.path.withCString({ anyform_parse_book($0) }) else {
            throw KindleNormalizerError.conversionFailed("the PDF reader returned no result")
        }
        defer { anyform_free_string(cString) }
        let json = String(cString: cString)

        guard let data = json.data(using: .utf8),
            let result = try? JSONDecoder().decode(RustParseResult.self, from: data)
        else {
            throw KindleNormalizerError.conversionFailed("the PDF reader's response couldn't be understood")
        }
        guard result.status == "ok", let info = result.book else {
            throw KindleNormalizerError.conversionFailed(result.message ?? "")
        }

        let coverImage = info.coverBase64.flatMap { Data(base64Encoded: $0) }
        let spine = info.spine.map { SpineItem(id: $0.id, href: $0.href, mediaType: "application/xhtml+xml") }
        let toc = info.toc.map(toTocNode)

        return Book(
            title: info.title,
            author: info.author,
            coverImage: coverImage,
            fileSizeBytes: info.fileSizeBytes,
            spine: spine,
            toc: toc,
            manifest: [:],
            sourceURL: url,
            contentDirectory: FileManager.default.temporaryDirectory
        )
    }

    private static func normalizeToEpub(_ url: URL) throws -> URL {
        guard let boko = bundledBokoPath() else {
            throw KindleNormalizerError.converterMissing
        }

        let workDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("Bookdrop-kindle", isDirectory: true)
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: workDir, withIntermediateDirectories: true)
        let epubURL = workDir.appendingPathComponent("normalized.epub")

        let process = Process()
        process.executableURL = URL(fileURLWithPath: boko)
        process.arguments = ["convert", url.path, epubURL.path]
        process.standardOutput = FileHandle.nullDevice
        let stderrPipe = Pipe()
        process.standardError = stderrPipe

        do {
            try process.run()
        } catch {
            throw KindleNormalizerError.conversionFailed(error.localizedDescription)
        }
        // Drain stderr before waiting on exit — reading only after
        // waitUntilExit() can deadlock if boko writes enough to fill the
        // pipe buffer before we start reading it.
        let stderrData = stderrPipe.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()

        guard process.terminationStatus == 0, FileManager.default.fileExists(atPath: epubURL.path) else {
            let stderr = String(data: stderrData, encoding: .utf8) ?? ""
            if stderr.lowercased().contains("drm") || stderr.lowercased().contains("encrypt") {
                throw KindleNormalizerError.drmProtected
            }
            let detail = stderr.split(separator: "\n").last.map(String.init) ?? ""
            throw KindleNormalizerError.conversionFailed(detail)
        }
        return epubURL
    }

    /// Locates the bundled `boko` binary (`Contents/Resources/Boko/<arch>/boko`,
    /// copied there by `Scripts/build-app.sh`) for whichever architecture is
    /// actually running — same pattern as `RustConversionEngine.bundledChromiumPath()`.
    /// Falls back to the vendored dev-tree copy (relative to this source
    /// file, mirroring `Package.swift`'s own `#filePath`-based `packageDir`)
    /// so `swift test`/`swift run` work without going through
    /// `build-app.sh`, as long as `rust/scripts/fetch-boko.sh` has run.
    private static func bundledBokoPath() -> String? {
        #if arch(arm64)
            let arch = "mac-arm64"
        #elseif arch(x86_64)
            let arch = "mac-x64"
        #else
            return nil
        #endif

        if let resourceURL = Bundle.main.resourceURL {
            let path = resourceURL.appendingPathComponent("Boko/\(arch)/boko").path
            if FileManager.default.fileExists(atPath: path) {
                return path
            }
        }

        let packageDir = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()  // Services/
            .deletingLastPathComponent()  // Bookdrop/
            .deletingLastPathComponent()  // Sources/
            .deletingLastPathComponent()  // package root
        let devPath = packageDir.appendingPathComponent("rust/vendor/boko/\(arch)/boko").path
        return FileManager.default.fileExists(atPath: devPath) ? devPath : nil
    }
}
