import XCTest
@testable import Bookdrop

final class TxtConverterTests: XCTestCase {
    private func fixtureURL() -> URL {
        Bundle.module.url(forResource: "minimal", withExtension: "epub", subdirectory: "Fixtures")!
    }

    private func scratchDir() -> URL {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("TxtConverterTests-\(UUID().uuidString)")
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        return dir
    }

    func testProducesReadablePlainText() throws {
        let book = try EpubParser.parse(fileAt: fixtureURL())
        let outputURL = scratchDir().appendingPathComponent("output.txt")

        let result = try TxtConverter.convert(book: book, outputURL: outputURL)

        let text = try String(contentsOf: result, encoding: .utf8)
        XCTAssertTrue(text.contains("Minimal Fixture Book"))
        XCTAssertTrue(text.contains("Test Author"))
        XCTAssertTrue(text.contains("Chapter One"))
        XCTAssertTrue(text.contains("Chapter Two"))
        XCTAssertTrue(text.contains("first chapter of the minimal fixture book"))
        // No HTML tags should leak through.
        XCTAssertFalse(text.contains("<"))
        XCTAssertFalse(text.contains(">"))
    }

    func testThrowsForBookWithNoChapters() {
        let empty = Book(
            title: "Empty", author: nil, coverImage: nil, fileSizeBytes: 0, spine: [], toc: [],
            manifest: [:], sourceURL: URL(fileURLWithPath: "/tmp/empty.epub"),
            contentDirectory: URL(fileURLWithPath: "/tmp"))
        XCTAssertThrowsError(try TxtConverter.convert(book: empty, outputURL: scratchDir().appendingPathComponent("x.txt")))
    }
}
