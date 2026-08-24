import XCTest
@testable import Bookdrop

final class HtmlConverterTests: XCTestCase {
    private func fixtureURL() -> URL {
        Bundle.module.url(forResource: "minimal", withExtension: "epub", subdirectory: "Fixtures")!
    }

    private func scratchDir() -> URL {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("HtmlConverterTests-\(UUID().uuidString)")
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        return dir
    }

    func testProducesSelfContainedHTML() throws {
        let book = try EpubParser.parse(fileAt: fixtureURL())
        let outputURL = scratchDir().appendingPathComponent("output.html")

        let result = try HtmlConverter.convert(
            book: book, includeCover: true, generateTOC: true, outputURL: outputURL)

        let html = try String(contentsOf: result, encoding: .utf8)
        XCTAssertTrue(html.contains("<!DOCTYPE html>"))
        XCTAssertTrue(html.contains("Minimal Fixture Book"))
        XCTAssertTrue(html.contains("Chapter One"))
        XCTAssertTrue(html.contains("Chapter Two"))
        XCTAssertTrue(html.contains("id=\"chapter-0\""))
        XCTAssertTrue(html.contains("id=\"chapter-1\""))
        // Cover embedded as a data URI, not an external reference.
        XCTAssertTrue(html.contains("data:image/"))
        // The CSS from the EPUB's style.css should be inlined.
        XCTAssertTrue(html.contains("font-family: serif"))
        // No external references that would break with the network off.
        XCTAssertFalse(html.contains("src=\"cover.jpg\""))
    }

    func testOmitsCoverWhenDisabled() throws {
        let book = try EpubParser.parse(fileAt: fixtureURL())
        let outputURL = scratchDir().appendingPathComponent("output.html")

        let result = try HtmlConverter.convert(
            book: book, includeCover: false, generateTOC: false, outputURL: outputURL)

        let html = try String(contentsOf: result, encoding: .utf8)
        // The CSS rules for these classes are always present in <style> — check
        // for the actual elements, not just the class name appearing anywhere.
        XCTAssertFalse(html.contains("<img class=\"bookdrop-cover\""))
        XCTAssertFalse(html.contains("<nav class=\"bookdrop-toc\""))
    }

    func testThrowsForBookWithNoChapters() {
        let empty = Book(
            title: "Empty", author: nil, coverImage: nil, fileSizeBytes: 0, spine: [], toc: [],
            manifest: [:], sourceURL: URL(fileURLWithPath: "/tmp/empty.epub"),
            contentDirectory: URL(fileURLWithPath: "/tmp"))
        XCTAssertThrowsError(
            try HtmlConverter.convert(
                book: empty, includeCover: false, generateTOC: false,
                outputURL: scratchDir().appendingPathComponent("x.html")))
    }
}
