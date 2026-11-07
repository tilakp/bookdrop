import XCTest
@testable import Bookdrop

/// The app's format lists and option controls come from the engine.
@MainActor
final class EngineFormatsTests: XCTestCase {
    func testEveryOutputFormatHasAnEnginePluginAndTheOtherWayAround() {
        let engine = Set(EngineFormats.shared.outputs.map(\.extension))
        let app = Set(OutputFormat.allCases.map(\.fileExtension))
        XCTAssertEqual(app, engine)
    }

    func testOpenableFilesAreTheEngineInputs() {
        XCTAssertEqual(BookLoader.supportedExtensions, Set(EngineFormats.shared.inputExtensions))
        XCTAssertTrue(BookLoader.isSupported(URL(fileURLWithPath: "/tmp/book.kepub")))
        let hint = UnsupportedFormatError(fileExtension: "rtf").recoverySuggestion ?? ""
        for ext in ["EPUB", "KEPUB", "AZW3", "MOBI", "PDF"] {
            XCTAssertTrue(hint.contains(ext), "the hint should name \(ext): \(hint)")
        }
    }

    func testControlsFollowTheOptionsEachFormatReads() {
        XCTAssertTrue(OutputFormat.pdf.hasPdfOptions)
        XCTAssertTrue(OutputFormat.pdf.supportsTableOfContents)
        XCTAssertTrue(OutputFormat.html.supportsCover && OutputFormat.html.supportsTableOfContents)
        XCTAssertTrue(OutputFormat.docx.supportsCover)
        XCTAssertFalse(OutputFormat.docx.supportsTableOfContents, "DOCX output has no TOC")
        XCTAssertFalse(OutputFormat.epub.supportsCover || OutputFormat.txt.supportsCover)
        XCTAssertTrue(OutputFormat.markdown.supportsCover)
        XCTAssertFalse(OutputFormat.markdown.hasPdfOptions || OutputFormat.markdown.supportsTableOfContents)
    }

    func testOnlyMarkdownWritesACompanionFolder() {
        for format in OutputFormat.allCases {
            XCTAssertEqual(format.companionSuffix, format == .markdown ? "_images" : nil, "\(format)")
        }
    }
}
