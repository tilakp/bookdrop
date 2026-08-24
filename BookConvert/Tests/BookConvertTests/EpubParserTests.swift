import XCTest
@testable import BookConvert

final class EpubParserTests: XCTestCase {
    private func fixtureURL() -> URL {
        Bundle.module.url(forResource: "minimal", withExtension: "epub", subdirectory: "Fixtures")!
    }

    func testParsesMetadata() throws {
        let book = try EpubParser.parse(fileAt: fixtureURL())
        XCTAssertEqual(book.title, "Minimal Fixture Book")
        XCTAssertEqual(book.author, "Test Author")
        XCTAssertGreaterThan(book.fileSizeBytes, 0)
    }

    func testParsesSpineInOrder() throws {
        let book = try EpubParser.parse(fileAt: fixtureURL())
        XCTAssertEqual(book.spine.map(\.href), ["ch1.xhtml", "ch2.xhtml"])
        XCTAssertEqual(book.chapterCount, 2)
    }

    func testParsesCoverImage() throws {
        let book = try EpubParser.parse(fileAt: fixtureURL())
        XCTAssertNotNil(book.coverImage)
        XCTAssertGreaterThan(book.coverImage?.count ?? 0, 0)
    }

    func testParsesNestedTableOfContents() throws {
        let book = try EpubParser.parse(fileAt: fixtureURL())
        XCTAssertEqual(book.toc.count, 2)
        XCTAssertEqual(book.toc[0].title, "Chapter One")
        XCTAssertEqual(book.toc[0].href, "ch1.xhtml")
        XCTAssertEqual(book.toc[1].title, "Chapter Two")
        XCTAssertEqual(book.toc[1].children.count, 1)
        XCTAssertEqual(book.toc[1].children[0].title, "Section One")
        XCTAssertEqual(book.toc[1].children[0].href, "ch2.xhtml#section1")
    }

    func testExtractsReadableChapterFiles() throws {
        let book = try EpubParser.parse(fileAt: fixtureURL())
        let ch1URL = book.contentDirectory.appendingPathComponent(book.spine[0].href)
        let contents = try String(contentsOf: ch1URL, encoding: .utf8)
        XCTAssertTrue(contents.contains("Chapter One"))
    }

    func testThrowsOnInvalidArchive() {
        let bogus = FileManager.default.temporaryDirectory.appendingPathComponent("not-a-real.epub")
        try? "not a zip file".write(to: bogus, atomically: true, encoding: .utf8)
        defer { try? FileManager.default.removeItem(at: bogus) }

        XCTAssertThrowsError(try EpubParser.parse(fileAt: bogus)) { error in
            XCTAssertTrue(error is EpubParserError)
        }
    }
}
