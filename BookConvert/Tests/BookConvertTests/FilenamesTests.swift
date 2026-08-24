import XCTest
@testable import BookConvert

final class FilenamesTests: XCTestCase {
    func testSanitizedFilenameReplacesInvalidCharacters() {
        XCTAssertEqual(sanitizedFilename("Chapter 1: A/B Test"), "Chapter 1- A-B Test")
    }

    func testSanitizedFilenameFallsBackWhenEmpty() {
        XCTAssertEqual(sanitizedFilename(""), "Untitled")
    }

    func testSanitizedFilenameCollapsesToSeparatorsWithoutCrashing() {
        // Not empty after cleaning, so no fallback — documents the actual (slightly
        // ugly but harmless) behavior for all-separator input.
        XCTAssertEqual(sanitizedFilename("///"), "---")
    }

    func testNextAvailableURLReturnsBaseNameWhenFree() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("FilenamesTests-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let url = nextAvailableURL(directory: dir, baseName: "Book", extension: "pdf")
        XCTAssertEqual(url.lastPathComponent, "Book.pdf")
    }

    func testNextAvailableURLIncrementsOnCollision() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("FilenamesTests-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        try Data().write(to: dir.appendingPathComponent("Book.pdf"))
        try Data().write(to: dir.appendingPathComponent("Book (1).pdf"))

        let url = nextAvailableURL(directory: dir, baseName: "Book", extension: "pdf")
        XCTAssertEqual(url.lastPathComponent, "Book (2).pdf")
    }
}
