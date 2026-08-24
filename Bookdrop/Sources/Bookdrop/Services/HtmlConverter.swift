import Foundation

enum HtmlConverterError: LocalizedError {
    case emptyBook
    var errorDescription: String? { "This EPUB has no readable chapters." }
}

/// Produces a single, self-contained HTML file: CSS inlined into a `<style>` block,
/// images embedded as base64 `data:` URIs, so the output has zero external
/// dependencies and works with the network off.
enum HtmlConverter {
    static func convert(book: Book, includeCover: Bool, generateTOC: Bool, outputURL: URL) throws -> URL {
        guard !book.spine.isEmpty else { throw HtmlConverterError.emptyBook }

        var dataURIs: [String: String] = [:]
        for item in book.manifest.values where item.mediaType.hasPrefix("image/") {
            let fileURL = book.contentDirectory.appendingPathComponent(item.href)
            guard let data = try? Data(contentsOf: fileURL) else { continue }
            dataURIs[item.href] = "data:\(item.mediaType);base64,\(data.base64EncodedString())"
        }

        var css = ""
        for item in book.manifest.values where item.mediaType == "text/css" {
            let fileURL = book.contentDirectory.appendingPathComponent(item.href)
            if let data = try? Data(contentsOf: fileURL), let text = String(data: data, encoding: .utf8) {
                css += rewriteReferences(in: text, dataURIs: dataURIs) + "\n"
            }
        }

        var bodyContent = ""

        if includeCover, let coverData = book.coverImage {
            let uri = "data:\(mimeType(forImageData: coverData));base64,\(coverData.base64EncodedString())"
            bodyContent += "<img class=\"bookdrop-cover\" src=\"\(uri)\" alt=\"Cover\"/>\n"
        }

        if generateTOC {
            bodyContent += "<nav class=\"bookdrop-toc\"><h2>Contents</h2><ul>\n"
            for (index, item) in book.spine.enumerated() {
                let title = tocTitle(for: item, in: book.toc) ?? "Chapter \(index + 1)"
                bodyContent += "<li><a href=\"#chapter-\(index)\">\(escapeHTML(title))</a></li>\n"
            }
            bodyContent += "</ul></nav>\n"
        }

        for (index, item) in book.spine.enumerated() {
            let fileURL = book.contentDirectory.appendingPathComponent(item.href)
            guard let data = try? Data(contentsOf: fileURL), let raw = String(data: data, encoding: .utf8) else {
                continue
            }
            let inner = extractBody(from: raw)
            bodyContent += "<section id=\"chapter-\(index)\">\n\(rewriteReferences(in: inner, dataURIs: dataURIs))\n</section>\n"
        }

        let html = """
            <!DOCTYPE html>
            <html>
            <head>
            <meta charset="utf-8">
            <title>\(escapeHTML(book.title))</title>
            <style>
            \(css)
            .bookdrop-cover { max-width: 100%; display: block; margin: 0 auto 2em; }
            .bookdrop-toc { margin-bottom: 2em; }
            </style>
            </head>
            <body>
            \(bodyContent)
            </body>
            </html>
            """

        try? FileManager.default.createDirectory(
            at: outputURL.deletingLastPathComponent(), withIntermediateDirectories: true)
        try html.write(to: outputURL, atomically: true, encoding: .utf8)
        return outputURL
    }

    /// Best-effort: matches manifest hrefs (and their bare filename) verbatim
    /// against quoted attribute values and CSS `url(...)` references. Doesn't
    /// resolve arbitrary relative-path forms (`../images/x.jpg` from a nested
    /// chapter file won't match a manifest href of `images/x.jpg`) — good enough
    /// for the common case of a flat or single-level EPUB layout.
    private static func rewriteReferences(in text: String, dataURIs: [String: String]) -> String {
        var result = text
        for (href, dataURI) in dataURIs {
            result = result.replacingOccurrences(of: "\"\(href)\"", with: "\"\(dataURI)\"")
            result = result.replacingOccurrences(of: "'\(href)'", with: "'\(dataURI)'")
            result = result.replacingOccurrences(of: "(\(href))", with: "(\(dataURI))")
            let filename = (href as NSString).lastPathComponent
            if filename != href {
                result = result.replacingOccurrences(of: "\"\(filename)\"", with: "\"\(dataURI)\"")
                result = result.replacingOccurrences(of: "'\(filename)'", with: "'\(dataURI)'")
            }
        }
        return result
    }

    private static func extractBody(from html: String) -> String {
        guard let bodyStart = html.range(of: "<body", options: .caseInsensitive),
            let bodyOpenEnd = html.range(of: ">", range: bodyStart.upperBound..<html.endIndex),
            let bodyEnd = html.range(of: "</body>", options: [.caseInsensitive, .backwards])
        else {
            return html
        }
        return String(html[bodyOpenEnd.upperBound..<bodyEnd.lowerBound])
    }

    private static func tocTitle(for item: SpineItem, in nodes: [TocNode]) -> String? {
        for node in nodes {
            if let href = node.href, href.split(separator: "#", maxSplits: 1).first.map(String.init) == item.href {
                return node.title
            }
            if let found = tocTitle(for: item, in: node.children) { return found }
        }
        return nil
    }

    private static func mimeType(forImageData data: Data) -> String {
        if data.starts(with: [0x89, 0x50, 0x4E, 0x47]) { return "image/png" }
        if data.starts(with: [0xFF, 0xD8, 0xFF]) { return "image/jpeg" }
        if data.starts(with: Array("GIF8".utf8)) { return "image/gif" }
        return "image/jpeg"
    }

    private static func escapeHTML(_ text: String) -> String {
        text.replacingOccurrences(of: "&", with: "&amp;")
            .replacingOccurrences(of: "<", with: "&lt;")
            .replacingOccurrences(of: ">", with: "&gt;")
    }
}
