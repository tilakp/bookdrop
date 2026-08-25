import PDFKit
import XCTest
@testable import Bookdrop

/// Drives a real conversion through `AppCoordinator` (the same entry point
/// the UI uses) with non-default `PDFOptions`, to prove those options
/// actually reach the Rust engine — not just that the Rust engine honors
/// options when handed a hand-built JSON blob directly (covered in
/// `anyform-doc/tests/pdf_tests.rs`), but that Swift's own JSON-building
/// code (`RustConversionEngine.conversionOptionsJSON`) wires every field
/// correctly. Real (bundled-Chromium-backed) PDF renders, not mocked.
@MainActor
final class RustConversionEngineOptionsTests: XCTestCase {
    private func fixtureURL() -> URL {
        Bundle.module.url(forResource: "minimal", withExtension: "epub", subdirectory: "Fixtures")!
    }

    private func scratchDir(_ label: String) -> URL {
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("RustConversionEngineOptionsTests-\(label)-\(UUID().uuidString)")
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        return dir
    }

    private func makeCoordinator() -> AppCoordinator {
        let history = HistoryStore(directory: scratchDir("history"))
        let settings = SettingsStore(defaults: InMemoryKeyValueStore())
        let coordinator = AppCoordinator(historyStore: history, settingsStore: settings)
        coordinator.performSideEffects = false
        return coordinator
    }

    func testCustomPDFOptionsReachTheRenderedOutput() async throws {
        let coordinator = makeCoordinator()
        coordinator.handleFilesSelected([fixtureURL()])
        coordinator.outputDirectory = scratchDir("output")
        guard case .loaded(let book) = coordinator.screen else {
            return XCTFail("expected .loaded after selecting the fixture")
        }

        coordinator.pdfOptions.pageSize = .custom
        coordinator.pdfOptions.customPageWidthInches = 6
        coordinator.pdfOptions.customPageHeightInches = 9
        coordinator.pdfOptions.includeCover = false
        coordinator.pdfOptions.generateTableOfContents = false
        coordinator.pdfOptions.showPageNumbers = true
        coordinator.pdfOptions.includeHeaders = true

        coordinator.beginConvert(book: book)
        guard case .converting(_, let progress) = coordinator.screen else {
            return XCTFail("expected .converting after beginConvert")
        }
        await coordinator.performConversion(
            book: book,
            outputURL: coordinator.outputDirectory.appendingPathComponent("options-test.pdf"),
            progress: progress)

        guard case .complete(let info) = coordinator.screen else {
            return XCTFail("expected .complete, got \(coordinator.screen)")
        }

        let document = try XCTUnwrap(PDFDocument(url: info.outputURL))
        XCTAssertEqual(document.pageCount, 2, "cover disabled, so only the two chapters should render")

        let page = try XCTUnwrap(document.page(at: 0))
        let widthIn = page.bounds(for: .mediaBox).width / 72
        let heightIn = page.bounds(for: .mediaBox).height / 72
        XCTAssertEqual(widthIn, 6, accuracy: 0.05, "custom page width should reach the Rust renderer")
        XCTAssertEqual(heightIn, 9, accuracy: 0.05, "custom page height should reach the Rust renderer")

        XCTAssertEqual(
            document.outlineRoot?.numberOfChildren ?? 0, 0,
            "TOC generation was disabled, so no outline entries should be present")

        let allText = (0..<document.pageCount)
            .compactMap { document.page(at: $0)?.string }
            .joined()
        XCTAssertTrue(allText.contains("Minimal Fixture Book"), "expected book title from the enabled header")
    }
}
