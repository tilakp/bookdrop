import CAnyform
import Foundation

/// A file the app does not try to open. Errors from reading a supported
/// file are `EngineError`s.
struct UnsupportedFormatError: LocalizedError, Equatable {
    let fileExtension: String

    var errorDescription: String? {
        fileExtension.isEmpty
            ? "Bookdrop can't open files without an extension."
            : "Bookdrop can't open .\(fileExtension) files."
    }

    var recoverySuggestion: String? {
        "Bookdrop opens \(BookLoader.supportedFormatsList(conjunction: "and")) files."
    }
}

/// Reads a book's details for the preview screen. Every format (EPUB,
/// Kindle, PDF) goes through the Rust engine, the same code that later
/// converts it, so the app has one parser and one set of security checks.
enum BookLoader {
    static let supportedExtensions = Set(EngineFormats.shared.inputExtensions)

    /// "EPUB, KEPUB, … or PDF", from the engine's input formats.
    static func supportedFormatsList(conjunction: String) -> String {
        let names = EngineFormats.shared.inputExtensions.map { $0.uppercased() }
        guard let last = names.last, names.count > 1 else { return names.joined() }
        return names.dropLast().joined(separator: ", ") + " \(conjunction) " + last
    }

    static func isSupported(_ url: URL) -> Bool {
        supportedExtensions.contains(url.pathExtension.lowercased())
    }

    /// Runs off the main actor: a large PDF or a Kindle file can take
    /// seconds to read, and the UI must stay responsive meanwhile.
    static func load(_ url: URL) async throws -> Book {
        guard isSupported(url) else {
            throw UnsupportedFormatError(fileExtension: url.pathExtension.lowercased())
        }
        return try await Task.detached(priority: .userInitiated) { try parse(url) }.value
    }

    private static func parse(_ url: URL) throws -> Book {
        let optionsJSON = (try? JSONSerialization.data(withJSONObject: BundledTools.engineOptions))
            .flatMap { String(data: $0, encoding: .utf8) } ?? "{}"
        var handle: OpaquePointer?
        let result = url.path.withCString { path in
            optionsJSON.withCString { options in anyform_open_book(path, options, &handle) }
        }
        // Owning the handle first means it is freed on every error path below.
        let engineBook = handle.map(EngineBook.init)
        guard let result else { throw EngineError.decode(nil) }
        defer { anyform_free_string(result) }

        let json = String(cString: result)
        guard let engineBook,
            let parsed = try? JSONDecoder().decode(ParseResult.self, from: Data(json.utf8)),
            parsed.status == "ok", let info = parsed.book
        else { throw EngineError.decode(json) }
        return Book(
            title: info.title,
            author: info.author,
            coverImage: info.coverBase64.flatMap { Data(base64Encoded: $0) },
            fileSizeBytes: info.fileSizeBytes,
            spine: info.spine.map { SpineItem(id: $0.id, href: $0.href) },
            toc: info.toc.map(TocNode.init),
            sourceURL: url,
            engineBook: engineBook)
    }
}

/// Owns an `AnyformBook*` from `anyform_open_book`: the engine's parsed
/// copy of one book, with its extracted files. Freed when the last `Book`
/// value that holds it goes away. A conversion that is still running keeps
/// its own reference on the Rust side, so freeing early is safe.
final class EngineBook: @unchecked Sendable {
    let pointer: OpaquePointer

    fileprivate init(_ pointer: OpaquePointer) {
        self.pointer = pointer
    }

    deinit {
        anyform_free_book(pointer)
    }
}

/// Mirrors `ParseResult`/`BookInfo` in `anyform-ffi/src/lib.rs`.
private struct ParseResult: Decodable {
    let status: String
    let book: BookInfo?
}

private struct BookInfo: Decodable {
    struct Spine: Decodable { let id: String; let href: String }
    struct Toc: Decodable {
        let title: String
        let href: String?
        let children: [Toc]
    }
    let title: String
    let author: String?
    let coverBase64: String?
    let fileSizeBytes: Int64
    let spine: [Spine]
    let toc: [Toc]

    enum CodingKeys: String, CodingKey {
        case title, author, spine, toc
        case coverBase64 = "cover_base64"
        case fileSizeBytes = "file_size_bytes"
    }
}

extension TocNode {
    fileprivate init(_ wire: BookInfo.Toc) {
        self.init(title: wire.title, href: wire.href, children: wire.children.map(TocNode.init))
    }
}
