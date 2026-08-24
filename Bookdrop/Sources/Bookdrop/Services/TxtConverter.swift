import Foundation
import AppKit

enum TxtConverterError: LocalizedError {
    case emptyBook
    case renderFailed(String)

    var errorDescription: String? {
        switch self {
        case .emptyBook: return "This EPUB has no readable chapters."
        case .renderFailed: return "The EPUB appears to contain formatting that couldn't be converted to text."
        }
    }
}

enum TxtConverter {
    static func convert(book: Book, outputURL: URL) throws -> URL {
        guard !book.spine.isEmpty else { throw TxtConverterError.emptyBook }

        var chunks: [String] = []
        for item in book.spine {
            let fileURL = book.contentDirectory.appendingPathComponent(item.href)
            guard let data = try? Data(contentsOf: fileURL) else {
                throw TxtConverterError.renderFailed(item.href)
            }
            guard
                let attributed = try? NSAttributedString(
                    data: data, options: [.documentType: NSAttributedString.DocumentType.html],
                    documentAttributes: nil)
            else {
                throw TxtConverterError.renderFailed(item.href)
            }
            let text = attributed.string.trimmingCharacters(in: .whitespacesAndNewlines)
            if !text.isEmpty { chunks.append(text) }
        }

        var body = "\(book.title)\n"
        if let author = book.author { body += "\(author)\n" }
        body += "\n\n"
        body += chunks.joined(separator: "\n\n\n")
        body += "\n"

        try? FileManager.default.createDirectory(
            at: outputURL.deletingLastPathComponent(), withIntermediateDirectories: true)
        try body.write(to: outputURL, atomically: true, encoding: .utf8)
        return outputURL
    }
}
