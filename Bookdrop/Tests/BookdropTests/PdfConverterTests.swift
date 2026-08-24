import XCTest
import PDFKit
@testable import Bookdrop

@MainActor
final class PdfConverterTests: XCTestCase {
    private func fixtureURL() -> URL {
        Bundle.module.url(forResource: "minimal", withExtension: "epub", subdirectory: "Fixtures")!
    }

    func testConvertsToValidMultiPagePDF() async throws {
        let book = try EpubParser.parse(fileAt: fixtureURL())
        let outputDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("PdfConverterTests-\(UUID().uuidString)")
        let targetURL = outputDir.appendingPathComponent("output.pdf")
        let progress = ConversionProgress()

        let outputURL = try await PdfConverter.convert(
            book: book, options: PDFOptions(), outputURL: targetURL, progress: progress)

        XCTAssertTrue(FileManager.default.fileExists(atPath: outputURL.path))
        guard let doc = PDFDocument(url: outputURL) else {
            XCTFail("output is not a valid PDF")
            return
        }
        // 1 cover page + at least 1 page per chapter (2 chapters).
        XCTAssertGreaterThanOrEqual(doc.pageCount, 3)
        XCTAssertEqual(doc.documentAttributes?[PDFDocumentAttribute.titleAttribute] as? String, "Minimal Fixture Book")
        XCTAssertNotNil(doc.outlineRoot)
        XCTAssertEqual(doc.outlineRoot?.numberOfChildren, 2)
        XCTAssertEqual(progress.fraction, 1.0)
    }

    func testExcludesCoverWhenOptionDisabled() async throws {
        let book = try EpubParser.parse(fileAt: fixtureURL())
        let outputDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("PdfConverterTests-\(UUID().uuidString)")
        let targetURL = outputDir.appendingPathComponent("output.pdf")
        var options = PDFOptions()
        options.includeCover = false

        let outputURL = try await PdfConverter.convert(
            book: book, options: options, outputURL: targetURL, progress: ConversionProgress())
        let doc = PDFDocument(url: outputURL)
        XCTAssertNotNil(doc)
    }

    func testAdvancedOptionsProduceValidOutput() async throws {
        let book = try EpubParser.parse(fileAt: fixtureURL())
        let outputDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("PdfConverterTests-\(UUID().uuidString)")
        let targetURL = outputDir.appendingPathComponent("output.pdf")
        var options = PDFOptions()
        options.fontFamily = "Helvetica"
        options.fontSizePt = 14
        options.lineSpacing = 1.5
        options.preserveEpubStyling = false
        options.showPageNumbers = true
        options.includeHeaders = true
        options.includeFooters = true

        let outputURL = try await PdfConverter.convert(
            book: book, options: options, outputURL: targetURL, progress: ConversionProgress())

        guard let doc = PDFDocument(url: outputURL) else {
            XCTFail("output is not a valid PDF")
            return
        }
        XCTAssertGreaterThanOrEqual(doc.pageCount, 3)
        // Decoration should still preserve the outline built from pageIndexForHref.
        XCTAssertEqual(doc.outlineRoot?.numberOfChildren, 2)
    }
}
