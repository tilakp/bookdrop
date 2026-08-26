import XCTest
@testable import Bookdrop

/// Requires the vendored `boko` binary — run `rust/scripts/fetch-boko.sh`
/// once before `swift test` (mirrors the Rust-side `kindle_tests.rs`).
/// `KindleNormalizer.parse` launches the real subprocess via its dev-tree
/// path fallback here, not a mock — per the earlier lesson that a bundled
/// subprocess binary needs verifying from something that actually
/// launches it, not assumed working because the equivalent Rust-side
/// plugin (`kindle_tests.rs`) already passes.
final class KindleNormalizerTests: XCTestCase {
    private func epubFixtureURL() -> URL {
        Bundle.module.url(forResource: "minimal", withExtension: "epub", subdirectory: "Fixtures")!
    }

    private func azw3FixtureURL() -> URL {
        Bundle.module.url(forResource: "minimal", withExtension: "azw3", subdirectory: "Fixtures")!
    }

    func testAzw3ParsesToTheSameContentAsItsSourceEpub() throws {
        let fromAzw3 = try KindleNormalizer.parse(fileAt: azw3FixtureURL())
        let fromEpub = try EpubParser.parse(fileAt: epubFixtureURL())

        XCTAssertEqual(fromAzw3.title, fromEpub.title)
        XCTAssertEqual(fromAzw3.author, fromEpub.author)
        XCTAssertEqual(fromAzw3.spine.count, fromEpub.spine.count)
        XCTAssertEqual(fromAzw3.toc.map(\.title), fromEpub.toc.map(\.title))
    }

    func testAzw3ReportsItsOwnSourceURLAndFileSizeNotTheIntermediateEpub() throws {
        let book = try KindleNormalizer.parse(fileAt: azw3FixtureURL())
        let actualSize = try FileManager.default.attributesOfItem(atPath: azw3FixtureURL().path)[.size] as? Int64

        // sourceURL matters beyond display: RustConversionEngine passes it
        // straight through to the Rust engine's own registry dispatch for
        // the PDF path, and HistoryEntry.sourcePath uses it to reopen the
        // file for "Convert Again" — both would break silently if this
        // pointed at the temp normalized EPUB instead.
        XCTAssertEqual(book.sourceURL, azw3FixtureURL())
        XCTAssertEqual(book.fileSizeBytes, actualSize)
    }

    func testChapterContentIsReadableAfterNormalization() throws {
        let book = try KindleNormalizer.parse(fileAt: azw3FixtureURL())
        let chapterURL = book.contentDirectory.appendingPathComponent(book.spine[0].href)
        let contents = try String(contentsOf: chapterURL, encoding: .utf8)
        XCTAssertTrue(contents.contains("Chapter One") || contents.contains("Chapter Two"))
    }

    func testPlainEpubIsPassedThroughWithoutInvokingTheConverter() throws {
        // Extensions outside kindleExtensions must never touch the boko
        // subprocess at all — this is what makes it safe for every
        // existing EPUB call site to switch to KindleNormalizer.parse
        // unconditionally.
        let book = try KindleNormalizer.parse(fileAt: epubFixtureURL())
        XCTAssertEqual(book.sourceURL, epubFixtureURL())
        XCTAssertEqual(book.title, "Minimal Fixture Book")
    }

    func testUnreadableKindleFileReportsAUsefulError() {
        let bogus = FileManager.default.temporaryDirectory.appendingPathComponent("not-a-real.azw3")
        try? "definitely not a kindle book".write(to: bogus, atomically: true, encoding: .utf8)
        defer { try? FileManager.default.removeItem(at: bogus) }

        XCTAssertThrowsError(try KindleNormalizer.parse(fileAt: bogus)) { error in
            guard let normalizerError = error as? KindleNormalizerError else {
                return XCTFail("expected KindleNormalizerError, got \(error)")
            }
            let message = normalizerError.errorDescription ?? ""
            XCTAssertTrue(
                message.contains("couldn't be read") || message.contains("DRM-protected"),
                "error should be user-facing, got: \(message)")
        }
    }
}
