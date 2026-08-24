import Foundation
import AppKit

enum DocxConverterError: LocalizedError {
    case emptyBook
    case renderFailed(String)
    case exportFailed

    var errorDescription: String? {
        switch self {
        case .emptyBook: return "This EPUB has no readable chapters."
        case .renderFailed: return "The EPUB appears to contain formatting that couldn't be converted to a Word document."
        case .exportFailed: return "Couldn't write the Word document."
        }
    }
}

/// Uses `NSAttributedString`'s native `.officeOpenXML` document type — the same
/// mechanism TextEdit uses for "Save as Word Document" — rather than hand-rolling
/// OOXML. It preserves basic structure (paragraphs, bold/italic, headings, images)
/// but not full CSS fidelity, which is an acceptable v1.1 tradeoff (see plan).
enum DocxConverter {
    static func convert(book: Book, includeCover: Bool, outputURL: URL) throws -> URL {
        guard !book.spine.isEmpty else { throw DocxConverterError.emptyBook }

        let document = NSMutableAttributedString()

        let titleAttrs: [NSAttributedString.Key: Any] = [.font: NSFont.boldSystemFont(ofSize: 24)]
        document.append(NSAttributedString(string: book.title + "\n", attributes: titleAttrs))
        if let author = book.author {
            let authorAttrs: [NSAttributedString.Key: Any] = [.font: NSFont.systemFont(ofSize: 14)]
            document.append(NSAttributedString(string: author + "\n", attributes: authorAttrs))
        }
        document.append(NSAttributedString(string: "\n"))

        if includeCover, let coverData = book.coverImage, let coverImage = NSImage(data: coverData) {
            let attachment = NSTextAttachment()
            attachment.image = coverImage
            document.append(NSAttributedString(attachment: attachment))
            document.append(NSAttributedString(string: "\n\n"))
        }

        // No synthetic chapter-title paragraph here: EPUB chapter XHTML almost
        // always already has its own heading (as this fixture does), and
        // NSAttributedString's HTML import renders that heading too — adding
        // another from the TOC title produced a visibly duplicated heading in
        // real Word (caught by opening the actual output, not just checking it
        // parses). Chapters are separated with blank lines only, same as the
        // TXT/HTML converters.
        for item in book.spine {
            let fileURL = book.contentDirectory.appendingPathComponent(item.href)
            guard let data = try? Data(contentsOf: fileURL) else {
                throw DocxConverterError.renderFailed(item.href)
            }
            guard
                let chapter = try? NSAttributedString(
                    data: data, options: [.documentType: NSAttributedString.DocumentType.html],
                    documentAttributes: nil)
            else {
                throw DocxConverterError.renderFailed(item.href)
            }

            document.append(chapter)
            document.append(NSAttributedString(string: "\n\n"))
        }

        let fullRange = NSRange(location: 0, length: document.length)
        guard
            let data = try? document.data(
                from: fullRange, documentAttributes: [.documentType: NSAttributedString.DocumentType.officeOpenXML])
        else {
            throw DocxConverterError.exportFailed
        }

        try? FileManager.default.createDirectory(
            at: outputURL.deletingLastPathComponent(), withIntermediateDirectories: true)
        try data.write(to: outputURL)
        return outputURL
    }
}
