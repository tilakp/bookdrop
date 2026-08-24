import XCTest
@testable import Bookdrop

final class DocxConverterTests: XCTestCase {
    private func fixtureURL() -> URL {
        Bundle.module.url(forResource: "minimal", withExtension: "epub", subdirectory: "Fixtures")!
    }

    private func scratchDir() -> URL {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("DocxConverterTests-\(UUID().uuidString)")
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        return dir
    }

    func testProducesValidDocxThatRoundTrips() throws {
        let book = try EpubParser.parse(fileAt: fixtureURL())
        let outputURL = scratchDir().appendingPathComponent("output.docx")

        let result = try DocxConverter.convert(book: book, includeCover: true, outputURL: outputURL)
        XCTAssertTrue(FileManager.default.fileExists(atPath: result.path))

        // Round-trip: a real Word app opening this file goes through the same
        // NSAttributedString import path, so this is a genuine validity check,
        // not just "a file exists at this path."
        let readBack = try NSAttributedString(
            url: result, options: [.documentType: NSAttributedString.DocumentType.officeOpenXML],
            documentAttributes: nil)
        let text = readBack.string
        XCTAssertTrue(text.contains("Minimal Fixture Book"))
        XCTAssertTrue(text.contains("Test Author"))
        XCTAssertTrue(text.contains("Chapter One"))
        XCTAssertTrue(text.contains("Chapter Two"))
        XCTAssertTrue(text.contains("first chapter of the minimal fixture book"))
    }

    func testThrowsForBookWithNoChapters() {
        let empty = Book(
            title: "Empty", author: nil, coverImage: nil, fileSizeBytes: 0, spine: [], toc: [],
            manifest: [:], sourceURL: URL(fileURLWithPath: "/tmp/empty.epub"),
            contentDirectory: URL(fileURLWithPath: "/tmp"))
        XCTAssertThrowsError(
            try DocxConverter.convert(book: empty, includeCover: false, outputURL: scratchDir().appendingPathComponent("x.docx")))
    }
}
