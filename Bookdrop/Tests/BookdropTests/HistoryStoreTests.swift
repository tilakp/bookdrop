import XCTest
@testable import Bookdrop

@MainActor
final class HistoryStoreTests: XCTestCase {
    private func scratchDir() -> URL {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("HistoryStoreTests-\(UUID().uuidString)")
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        return dir
    }

    func testAddInsertsAtFront() {
        let store = HistoryStore(directory: scratchDir())
        let a = HistoryEntry(title: "A", conversionLabel: "EPUB → PDF", outputPath: "/a.pdf", sourcePath: "/a.epub")
        let b = HistoryEntry(title: "B", conversionLabel: "EPUB → PDF", outputPath: "/b.pdf", sourcePath: "/b.epub")
        store.add(a)
        store.add(b)
        XCTAssertEqual(store.entries.map(\.title), ["B", "A"])
    }

    func testRemoveDeletesOnlyMatchingEntry() {
        let store = HistoryStore(directory: scratchDir())
        let a = HistoryEntry(title: "A", conversionLabel: "EPUB → PDF", outputPath: "/a.pdf", sourcePath: "/a.epub")
        let b = HistoryEntry(title: "B", conversionLabel: "EPUB → PDF", outputPath: "/b.pdf", sourcePath: "/b.epub")
        store.add(a)
        store.add(b)
        store.remove(a)
        XCTAssertEqual(store.entries.map(\.title), ["B"])
    }

    func testClearEmptiesHistory() {
        let store = HistoryStore(directory: scratchDir())
        store.add(HistoryEntry(title: "A", conversionLabel: "EPUB → PDF", outputPath: "/a.pdf", sourcePath: "/a.epub"))
        store.clear()
        XCTAssertTrue(store.entries.isEmpty)
    }

    func testPersistsAcrossInstances() {
        let dir = scratchDir()
        let first = HistoryStore(directory: dir)
        first.add(HistoryEntry(title: "A", conversionLabel: "EPUB → PDF", outputPath: "/a.pdf", sourcePath: "/a.epub"))

        let second = HistoryStore(directory: dir)
        XCTAssertEqual(second.entries.map(\.title), ["A"])
    }
}
