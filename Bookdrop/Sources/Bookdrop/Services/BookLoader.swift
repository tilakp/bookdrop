import CAnyform
import Foundation

enum BookLoadError: LocalizedError, Equatable {
    case unsupportedFormat(String)
    case unreadable(String)

    var errorDescription: String? {
        switch self {
        case .unsupportedFormat(let ext):
            return ext.isEmpty
                ? "Bookdrop can't open files without an extension."
                : "Bookdrop can't open .\(ext) files."
        case .unreadable(let detail):
            return detail.isEmpty ? "This book couldn't be read." : detail
        }
    }
}

/// Reads a book's details for the preview screen. Every format (EPUB,
/// Kindle, PDF) goes through the Rust engine, the same code that later
/// converts it, so the app has one parser and one set of security checks.
enum BookLoader {
    static let supportedExtensions: Set<String> = ["epub", "kepub", "pdf", "azw3", "azw", "kfx", "mobi"]

    static func isSupported(_ url: URL) -> Bool {
        supportedExtensions.contains(url.pathExtension.lowercased())
    }

    /// Runs off the main actor: a large PDF or a Kindle file can take
    /// seconds to read, and the UI must stay responsive meanwhile.
    static func load(_ url: URL) async throws -> Book {
        guard isSupported(url) else {
            throw BookLoadError.unsupportedFormat(url.pathExtension.lowercased())
        }
        return try await Task.detached(priority: .userInitiated) { try parse(url) }.value
    }

    private static func parse(_ url: URL) throws -> Book {
        let optionsJSON = (try? JSONSerialization.data(withJSONObject: BundledTools.engineOptions))
            .flatMap { String(data: $0, encoding: .utf8) } ?? "{}"
        let result = url.path.withCString { path in
            optionsJSON.withCString { options in anyform_parse_book(path, options) }
        }
        guard let result else { throw BookLoadError.unreadable("") }
        defer { anyform_free_string(result) }

        let parsed = try? JSONDecoder().decode(ParseResult.self, from: Data(String(cString: result).utf8))
        guard let parsed, parsed.status == "ok", let info = parsed.book else {
            throw BookLoadError.unreadable(parsed?.message ?? "")
        }
        return Book(
            title: info.title,
            author: info.author,
            coverImage: info.coverBase64.flatMap { Data(base64Encoded: $0) },
            fileSizeBytes: info.fileSizeBytes,
            spine: info.spine.map { SpineItem(id: $0.id, href: $0.href) },
            toc: info.toc.map(TocNode.init),
            sourceURL: url)
    }
}

/// Mirrors `ParseResult`/`BookInfo` in `anyform-ffi/src/lib.rs`.
private struct ParseResult: Decodable {
    let status: String
    let book: BookInfo?
    let message: String?
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
