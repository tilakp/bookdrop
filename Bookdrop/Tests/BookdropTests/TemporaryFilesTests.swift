import XCTest
@testable import Bookdrop

final class TemporaryFilesTests: XCTestCase {
    /// "Clear Temporary Files" used to delete all of `$TMPDIR/Bookdrop`,
    /// including the extracted files of the book that was open.
    func testClearsOnlyFoldersFromEarlierRuns() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("TemporaryFilesTests-\(UUID())")
        addTeardownBlock { try? FileManager.default.removeItem(at: root) }
        let leftover = root.appendingPathComponent("epub-old")
        let inUse = root.appendingPathComponent("epub-open-book")
        for dir in [leftover, inUse] {
            try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        }
        let launch = Date()
        try FileManager.default.setAttributes([.creationDate: launch.addingTimeInterval(-3600)], ofItemAtPath: leftover.path)
        try FileManager.default.setAttributes([.creationDate: launch.addingTimeInterval(60)], ofItemAtPath: inUse.path)

        let legacy = FileManager.default.temporaryDirectory.appendingPathComponent("TemporaryFilesTests-legacy-\(UUID())")
        addTeardownBlock { try? FileManager.default.removeItem(at: legacy) }
        try FileManager.default.createDirectory(at: legacy.appendingPathComponent("x"), withIntermediateDirectories: true)

        let removed = TemporaryFiles.clearLeftovers(in: root, createdBefore: launch, legacyRoots: [legacy])
        XCTAssertEqual(removed, 2)
        XCTAssertFalse(FileManager.default.fileExists(atPath: legacy.path))
        XCTAssertFalse(FileManager.default.fileExists(atPath: leftover.path))
        XCTAssertTrue(FileManager.default.fileExists(atPath: inUse.path), "an open book's files must survive")
    }
}
