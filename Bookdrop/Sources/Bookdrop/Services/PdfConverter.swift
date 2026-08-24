import Foundation
import WebKit
import PDFKit
import AppKit
import CoreText

enum PdfConverterError: LocalizedError {
    case cancelled
    case renderFailed(String)
    case emptyBook

    var errorDescription: String? {
        switch self {
        case .cancelled:
            return "Conversion was cancelled."
        case .renderFailed:
            return "The EPUB appears to contain formatting that couldn't be converted to PDF."
        case .emptyBook:
            return "This EPUB has no readable chapters."
        }
    }
}

@MainActor
enum PdfConverter {
    static func convert(book: Book, options: PDFOptions, outputURL: URL, progress: ConversionProgress) async throws -> URL {
        guard !book.spine.isEmpty else { throw PdfConverterError.emptyBook }

        progress.stageText = "Reading book…"
        progress.fraction = 0.02

        var chapterPDFs: [(href: String, document: PDFDocument)] = []
        let chapterCount = book.spine.count

        // Each chapter is rendered and paginated independently, so a new chapter always
        // lands on a fresh page — `startChaptersOnNewPage = false` (continuous flow across
        // chapter boundaries) isn't supported by this per-chapter architecture in v1.
        for (index, item) in book.spine.enumerated() {
            if progress.isCancelled { throw PdfConverterError.cancelled }
            progress.stageText = "Rendering chapter \(index + 1) of \(chapterCount)"
            progress.fraction = 0.05 + 0.7 * (Double(index) / Double(chapterCount))

            let fileURL = book.contentDirectory.appendingPathComponent(item.href)
            let pdfData = try await renderChapterToPDF(
                fileURL: fileURL, readAccessRoot: book.contentDirectory, options: options)
            guard let doc = PDFDocument(data: pdfData), doc.pageCount > 0 else {
                throw PdfConverterError.renderFailed(item.href)
            }
            chapterPDFs.append((item.href, doc))
        }

        if progress.isCancelled { throw PdfConverterError.cancelled }
        progress.stageText = "Creating PDF"
        progress.fraction = 0.8

        let merged = PDFDocument()
        var pageIndexForHref: [String: Int] = [:]

        if options.includeCover, let coverData = book.coverImage, let coverImage = NSImage(data: coverData) {
            if let coverPage = PDFPage(image: coverImage) {
                merged.insert(coverPage, at: merged.pageCount)
            }
        }

        for (href, doc) in chapterPDFs {
            pageIndexForHref[href] = merged.pageCount
            for pageIndex in 0..<doc.pageCount {
                if let page = doc.page(at: pageIndex) {
                    merged.insert(page, at: merged.pageCount)
                }
            }
        }

        progress.stageText = "Finalizing"
        progress.fraction = 0.92

        // Decorating (page numbers/headers/footers) recomposites each page but preserves
        // page order and count 1:1, so pageIndexForHref computed above still applies.
        let final = decoratePages(merged, options: options, bookTitle: book.title)

        if options.generateTableOfContents {
            buildOutline(for: final, toc: book.toc, pageIndexForHref: pageIndexForHref)
        }

        var attributes = final.documentAttributes ?? [:]
        attributes[PDFDocumentAttribute.titleAttribute] = book.title
        if let author = book.author {
            attributes[PDFDocumentAttribute.authorAttribute] = author
        }
        final.documentAttributes = attributes

        try? FileManager.default.createDirectory(
            at: outputURL.deletingLastPathComponent(), withIntermediateDirectories: true)
        guard final.write(to: outputURL) else {
            throw PdfConverterError.renderFailed("could not write output PDF")
        }

        progress.stageText = "Done"
        progress.fraction = 1.0
        return outputURL
    }

    /// Renders one chapter by measuring its laid-out content height, then capturing it
    /// as a sequence of page-sized PDF snapshots via `createPDF` and compositing each
    /// onto a full-size page inset by the configured margin. Deliberately not
    /// `NSPrintOperation`/`printOperation(with:)`: that goes through AppKit's real print
    /// pipeline (printer/color-sync subsystem) which was observed to hang indefinitely
    /// in this non-interactive session — `createPDF` is a pure rendering snapshot with
    /// no such dependency, and composed pagination gives every page (not just the
    /// first/last) a correct margin on all four sides.
    private static func renderChapterToPDF(fileURL: URL, readAccessRoot: URL, options: PDFOptions) async throws -> Data {
        let pageSize = options.pageDimensions
        let margin = options.margins.points
        let contentWidth = max(pageSize.width - margin * 2, 100)
        let contentHeight = max(pageSize.height - margin * 2, 100)

        let webView = WKWebView(frame: CGRect(x: 0, y: 0, width: contentWidth, height: contentHeight))
        let navDelegate = NavigationWaiter()
        webView.navigationDelegate = navDelegate

        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            navDelegate.completion = { result in continuation.resume(with: result) }
            webView.loadFileURL(fileURL, allowingReadAccessTo: readAccessRoot)
        }

        _ = try? await webView.evaluateJavaScript(typographyInjectionScript(for: options))

        let scrollHeightResult = try? await webView.evaluateJavaScript("document.documentElement.scrollHeight")
        let scrollHeight = (scrollHeightResult as? NSNumber)?.doubleValue ?? Double(contentHeight)
        let totalContentHeight = max(CGFloat(scrollHeight), contentHeight)
        let pageCount = max(1, Int(ceil(totalContentHeight / contentHeight)))

        let merged = PDFDocument()
        for i in 0..<pageCount {
            let config = WKPDFConfiguration()
            config.rect = CGRect(x: 0, y: CGFloat(i) * contentHeight, width: contentWidth, height: contentHeight)

            let sliceData = try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Data, Error>) in
                webView.createPDF(configuration: config) { result in
                    continuation.resume(with: result)
                }
            }
            guard let page = compositedPage(fromSlice: sliceData, pageSize: pageSize, margin: margin) else {
                throw PdfConverterError.renderFailed(fileURL.lastPathComponent)
            }
            merged.insert(page, at: merged.pageCount)
        }

        guard let outputData = merged.dataRepresentation() else {
            throw PdfConverterError.renderFailed(fileURL.lastPathComponent)
        }
        return outputData
    }

    /// Builds a script that (optionally) strips the EPUB's own CSS and applies the
    /// typography overrides from Advanced Options, run once against the loaded chapter
    /// before it's measured/captured.
    private static func typographyInjectionScript(for options: PDFOptions) -> String {
        var script = ""
        if !options.preserveEpubStyling || options.removePublisherStyling {
            script += """
            document.querySelectorAll('link[rel="stylesheet"], style').forEach(function(el) { el.remove(); });
            document.querySelectorAll('[style]').forEach(function(el) { el.removeAttribute('style'); });
            """
        }
        let fontFamilyRule = options.fontFamily == "Original"
            ? "" : "font-family: '\(options.fontFamily)', serif !important;"
        script += """
        (function() {
            var style = document.createElement('style');
            style.textContent = 'html, body { margin: 0 !important; }' +
                'body { font-size: \(options.fontSizePt)pt !important; ' +
                'line-height: \(options.lineSpacing) !important; \(fontFamilyRule) }';
            document.head.appendChild(style);
        })();
        """
        return script
    }

    private static func compositedPage(fromSlice data: Data, pageSize: CGSize, margin: CGFloat) -> PDFPage? {
        guard let provider = CGDataProvider(data: data as CFData),
              let contentDoc = CGPDFDocument(provider),
              let cgPage = contentDoc.page(at: 1)
        else { return nil }

        let outputData = NSMutableData()
        guard let consumer = CGDataConsumer(data: outputData) else { return nil }
        var mediaBox = CGRect(origin: .zero, size: pageSize)
        guard let context = CGContext(consumer: consumer, mediaBox: &mediaBox, nil) else { return nil }

        context.beginPDFPage(nil)
        context.saveGState()
        context.translateBy(x: margin, y: margin)
        context.drawPDFPage(cgPage)
        context.restoreGState()
        context.endPDFPage()
        context.closePDF()

        return PDFDocument(data: outputData as Data)?.page(at: 0)
    }

    /// Recomposites every page, drawing over the existing content, to add a running
    /// page number and/or a title header/footer — a no-op copy when none are enabled.
    private static func decoratePages(_ document: PDFDocument, options: PDFOptions, bookTitle: String) -> PDFDocument {
        guard options.showPageNumbers || options.includeHeaders || options.includeFooters else { return document }
        guard let data = document.dataRepresentation(),
              let provider = CGDataProvider(data: data as CFData),
              let cgDoc = CGPDFDocument(provider)
        else { return document }

        let decorated = PDFDocument()
        let pageCount = cgDoc.numberOfPages
        guard pageCount > 0 else { return document }

        for i in 1...pageCount {
            guard let cgPage = cgDoc.page(at: i) else { continue }
            let mediaBox = cgPage.getBoxRect(.mediaBox)

            let outData = NSMutableData()
            guard let consumer = CGDataConsumer(data: outData) else { continue }
            var box = mediaBox
            guard let context = CGContext(consumer: consumer, mediaBox: &box, nil) else { continue }

            context.beginPDFPage(nil)
            context.drawPDFPage(cgPage)

            if options.includeHeaders {
                drawCenteredText(
                    bookTitle, fontSize: 8, in: context,
                    rect: CGRect(x: 0, y: mediaBox.height - 20, width: mediaBox.width, height: 14))
            }
            if options.includeFooters {
                drawCenteredText(
                    bookTitle, fontSize: 8, in: context,
                    rect: CGRect(x: 0, y: 16, width: mediaBox.width, height: 14))
            }
            if options.showPageNumbers {
                let y: CGFloat = options.includeFooters ? 4 : 16
                drawCenteredText(
                    "\(i)", fontSize: 8, in: context,
                    rect: CGRect(x: 0, y: y, width: mediaBox.width, height: 12))
            }

            context.endPDFPage()
            context.closePDF()

            if let page = PDFDocument(data: outData as Data)?.page(at: 0) {
                decorated.insert(page, at: decorated.pageCount)
            }
        }
        return decorated
    }

    private static func drawCenteredText(_ text: String, fontSize: CGFloat, in context: CGContext, rect: CGRect) {
        let font = CTFontCreateWithName("Helvetica" as CFString, fontSize, nil)
        let attributes: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: NSColor.darkGray.cgColor]
        let line = CTLineCreateWithAttributedString(NSAttributedString(string: text, attributes: attributes))
        let bounds = CTLineGetBoundsWithOptions(line, [])
        context.textPosition = CGPoint(x: rect.midX - bounds.width / 2, y: rect.midY - bounds.height / 2)
        CTLineDraw(line, context)
    }

    private static func buildOutline(for document: PDFDocument, toc: [TocNode], pageIndexForHref: [String: Int]) {
        let root = PDFOutline()
        document.outlineRoot = root
        for node in toc {
            if let item = makeOutlineItem(node: node, document: document, pageIndexForHref: pageIndexForHref) {
                root.insertChild(item, at: root.numberOfChildren)
            }
        }
    }

    /// TOC entries map to the top of their chapter's starting page — fragment
    /// targets within a chapter aren't deep-linked to a specific page in v1.
    private static func makeOutlineItem(node: TocNode, document: PDFDocument, pageIndexForHref: [String: Int]) -> PDFOutline? {
        let hrefWithoutFragment = node.href.map { String($0.split(separator: "#", maxSplits: 1).first ?? "") }
        guard let pageIndex = hrefWithoutFragment.flatMap({ pageIndexForHref[$0] }),
              let page = document.page(at: pageIndex)
        else { return nil }

        let item = PDFOutline()
        item.label = node.title
        item.destination = PDFDestination(page: page, at: .zero)

        for child in node.children {
            if let childItem = makeOutlineItem(node: child, document: document, pageIndexForHref: pageIndexForHref) {
                item.insertChild(childItem, at: item.numberOfChildren)
            }
        }
        return item
    }
}

private final class NavigationWaiter: NSObject, WKNavigationDelegate {
    var completion: ((Result<Void, Error>) -> Void)?

    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        completion?(.success(()))
        completion = nil
    }

    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
        completion?(.failure(error))
        completion = nil
    }

    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
        completion?(.failure(error))
        completion = nil
    }
}
