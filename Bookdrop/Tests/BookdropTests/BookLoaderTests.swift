import XCTest
@testable import Bookdrop

/// `BookLoader` reads every format through the Rust engine. The Kindle
/// tests need the vendored `boko` binary (`rust/scripts/fetch-boko.sh`).
final class BookLoaderTests: XCTestCase {
    private func fixture(_ name: String, _ ext: String) -> URL {
        Bundle.module.url(forResource: name, withExtension: ext, subdirectory: "Fixtures")!
    }

    private func scratchFile(_ name: String, contents: String) throws -> URL {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("\(UUID().uuidString)-\(name)")
        try contents.write(to: url, atomically: true, encoding: .utf8)
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        return url
    }

    func testLoadsEpubMetadataSpineCoverAndToc() async throws {
        let book = try await BookLoader.load(fixture("minimal", "epub"))
        XCTAssertEqual(book.title, "Minimal Fixture Book")
        XCTAssertEqual(book.author, "Test Author")
        XCTAssertGreaterThan(book.fileSizeBytes, 0)
        XCTAssertEqual(book.spine.map(\.href), ["ch1.xhtml", "ch2.xhtml"])
        XCTAssertGreaterThan(book.coverImage?.count ?? 0, 0)
        XCTAssertEqual(book.toc.map(\.title), ["Chapter One", "Chapter Two"])
        XCTAssertEqual(book.toc[1].children.first?.href, "ch2.xhtml#section1")
    }

    func testAzw3LoadsLikeItsSourceEpubAndKeepsItsOwnURL() async throws {
        let azw3 = fixture("minimal", "azw3")
        let fromAzw3 = try await BookLoader.load(azw3)
        let fromEpub = try await BookLoader.load(fixture("minimal", "epub"))
        XCTAssertEqual(fromAzw3.title, fromEpub.title)
        XCTAssertEqual(fromAzw3.spine.count, fromEpub.spine.count)
        XCTAssertEqual(fromAzw3.toc.map(\.title), fromEpub.toc.map(\.title))
        // "Convert Again" and the conversion itself reopen `sourceURL`.
        XCTAssertEqual(fromAzw3.sourceURL, azw3)
        let size = try FileManager.default.attributesOfItem(atPath: azw3.path)[.size] as? Int64
        XCTAssertEqual(fromAzw3.fileSizeBytes, size)
    }

    func testUnsupportedExtensionIsRejectedWithAClearMessage() async {
        do {
            _ = try await BookLoader.load(URL(fileURLWithPath: "/tmp/notes.rtf"))
            XCTFail("expected an error")
        } catch {
            XCTAssertEqual(error as? BookLoadError, .unsupportedFormat("rtf"))
            XCTAssertEqual(error.localizedDescription, "Bookdrop can't open .rtf files.")
        }
    }

    func testBrokenFilesReportTheEngineMessage() async throws {
        let cases = [("not-a-book.epub", "isn't a valid archive"), ("not-a-book.pdf", "PDF")]
        for (name, expected) in cases {
            let url = try scratchFile(name, contents: "not a real book")
            do {
                _ = try await BookLoader.load(url)
                XCTFail("\(name): expected an error")
            } catch {
                let message = error.localizedDescription
                XCTAssertTrue(message.contains(expected), "\(name): got \(message)")
                XCTAssertFalse(message.contains("Kindle"), "a non-Kindle file must not be called a Kindle file")
            }
        }
    }

    /// The old Swift extractor wrote archive entries named `../../…`
    /// outside its temp folder (zip slip). Loading must never do that.
    func testArchiveEntriesCannotEscapeTheExtractionFolder() async throws {
        let probe = URL(fileURLWithPath: "/tmp/bookdrop-zip-slip-probe.txt")
        try? FileManager.default.removeItem(at: probe)
        let book = try await BookLoader.load(fixture("zip-slip", "epub"))
        XCTAssertEqual(book.title, "Zip Slip")
        XCTAssertFalse(FileManager.default.fileExists(atPath: probe.path), "an archive entry escaped to \(probe.path)")
    }
}
