import XCTest
@testable import Bookdrop

@MainActor
final class AppCoordinatorTests: XCTestCase {
    private func fixtureURL() -> URL {
        Bundle.module.url(forResource: "minimal", withExtension: "epub", subdirectory: "Fixtures")!
    }

    private func makeCoordinator() -> AppCoordinator {
        let history = HistoryStore(directory: scratchDir("history"))
        let settings = SettingsStore(defaults: scratchDefaults())
        let coordinator = AppCoordinator(historyStore: history, settingsStore: settings)
        coordinator.performSideEffects = false
        return coordinator
    }

    private func scratchDir(_ label: String) -> URL {
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("AppCoordinatorTests-\(label)-\(UUID().uuidString)")
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        return dir
    }

    private func scratchDefaults() -> KeyValueStore {
        InMemoryKeyValueStore()
    }

    /// Loads the fixture into `coordinator` and redirects its output directory to a
    /// scratch folder — `handleFilesSelected` resets `outputDirectory` from settings as
    /// part of loading, so the redirect must happen *after*, not before.
    private func loadFixture(into coordinator: AppCoordinator) async -> Book {
        await coordinator.handleFilesSelected([fixtureURL()])?.value
        coordinator.outputDirectory = scratchDir("output")
        guard case .loaded(let book) = coordinator.screen else {
            fatalError("expected .loaded after selecting the fixture")
        }
        return book
    }

    // MARK: - File selection

    func testSelectingValidFileTransitionsToLoaded() async {
        let coordinator = makeCoordinator()
        let book = await loadFixture(into: coordinator)
        XCTAssertEqual(book.title, "Minimal Fixture Book")
    }

    func testSelectingInvalidFileTransitionsToError() async throws {
        let coordinator = makeCoordinator()
        let bogus = FileManager.default.temporaryDirectory.appendingPathComponent("bogus-\(UUID().uuidString).epub")
        try "not a zip".write(to: bogus, atomically: true, encoding: .utf8)
        defer { try? FileManager.default.removeItem(at: bogus) }

        await coordinator.handleFilesSelected([bogus])?.value
        guard case .error(let message, _, let details) = coordinator.screen else {
            XCTFail("expected .error, got \(coordinator.screen)")
            return
        }
        XCTAssertFalse(message.isEmpty)
        XCTAssertNotNil(details)
    }

    func testSelectingMultipleFilesTransitionsToMultipleFiles() async {
        let coordinator = makeCoordinator()
        await coordinator.handleFilesSelected([fixtureURL(), fixtureURL()])?.value
        guard case .multipleFiles(let model) = coordinator.screen else {
            XCTFail("expected .multipleFiles, got \(coordinator.screen)")
            return
        }
        XCTAssertEqual(model.jobs.count, 2)
    }

    // MARK: - Duplicate detection

    func testConvertWithNoCollisionStartsConversionDirectly() async {
        let coordinator = makeCoordinator()
        let book = await loadFixture(into: coordinator)

        let task = coordinator.beginConvert(book: book)
        XCTAssertNotNil(task, "no existing file — should convert immediately, not prompt")
        guard case .converting = coordinator.screen else {
            XCTFail("expected .converting, got \(coordinator.screen)")
            return
        }
        await task?.value
        guard case .complete = coordinator.screen else {
            XCTFail("expected .complete after conversion finishes, got \(coordinator.screen)")
            return
        }
    }

    func testConvertWithCollisionShowsDuplicateDialog() async throws {
        let coordinator = makeCoordinator()
        let book = await loadFixture(into: coordinator)
        let candidate = coordinator.outputDirectory.appendingPathComponent("Minimal Fixture Book.pdf")
        try Data().write(to: candidate)

        let task = coordinator.beginConvert(book: book)
        XCTAssertNil(task, "existing file — should prompt instead of converting")
        guard case .duplicateConfirm(_, let filename) = coordinator.screen else {
            XCTFail("expected .duplicateConfirm, got \(coordinator.screen)")
            return
        }
        XCTAssertEqual(filename, "Minimal Fixture Book.pdf")
    }

    func testDuplicateCancelReturnsToLoaded() async throws {
        let coordinator = makeCoordinator()
        let book = await loadFixture(into: coordinator)
        try Data().write(to: coordinator.outputDirectory.appendingPathComponent("Minimal Fixture Book.pdf"))
        coordinator.beginConvert(book: book)

        coordinator.handleDuplicateResolution(.cancel, book: book)
        guard case .loaded = coordinator.screen else {
            XCTFail("expected .loaded after cancelling duplicate prompt, got \(coordinator.screen)")
            return
        }
    }

    func testDuplicateReplaceOverwritesExistingFile() async throws {
        let coordinator = makeCoordinator()
        let book = await loadFixture(into: coordinator)
        let candidate = coordinator.outputDirectory.appendingPathComponent("Minimal Fixture Book.pdf")
        try Data().write(to: candidate)
        coordinator.beginConvert(book: book)

        let task = coordinator.handleDuplicateResolution(.replace, book: book)
        await task?.value

        guard case .complete(let info) = coordinator.screen else {
            XCTFail("expected .complete, got \(coordinator.screen)")
            return
        }
        XCTAssertEqual(info.outputURL.path, candidate.path)
        XCTAssertGreaterThan(info.pageCount ?? 0, 0)
    }

    func testDuplicateKeepBothWritesNumberedFile() async throws {
        let coordinator = makeCoordinator()
        let book = await loadFixture(into: coordinator)
        let candidate = coordinator.outputDirectory.appendingPathComponent("Minimal Fixture Book.pdf")
        try Data().write(to: candidate)
        coordinator.beginConvert(book: book)

        let task = coordinator.handleDuplicateResolution(.keepBoth, book: book)
        await task?.value

        guard case .complete(let info) = coordinator.screen else {
            XCTFail("expected .complete, got \(coordinator.screen)")
            return
        }
        XCTAssertEqual(info.outputURL.lastPathComponent, "Minimal Fixture Book (1).pdf")
    }

    // MARK: - Settings wiring

    func testDefaultOutputDirectoryForDownloads() {
        let downloads = FileManager.default.urls(for: .downloadsDirectory, in: .userDomainMask).first!
        XCTAssertEqual(AppCoordinator.defaultOutputDirectory(for: .downloads), downloads)
    }

    func testDefaultOutputDirectoryForSameAsSource() {
        let sourceDir = URL(fileURLWithPath: "/tmp/some-books-folder")
        let sourceFile = sourceDir.appendingPathComponent("book.epub")
        let resolved = AppCoordinator.defaultOutputDirectory(for: .sameAsSource, sourceURL: sourceFile)
        XCTAssertEqual(resolved.path, sourceDir.path)
    }

    func testPreserveStylingSettingAppliedWhenBookLoads() async {
        let history = HistoryStore(directory: scratchDir("history"))
        let settings = SettingsStore(defaults: scratchDefaults())
        settings.preserveOriginalStylingByDefault = false
        let coordinator = AppCoordinator(historyStore: history, settingsStore: settings)
        coordinator.performSideEffects = false

        await coordinator.handleFilesSelected([fixtureURL()])?.value
        XCTAssertFalse(coordinator.pdfOptions.preserveEpubStyling)
    }

    func testOutputDirectoryFollowsSameAsSourceSettingOnLoad() async {
        let history = HistoryStore(directory: scratchDir("history"))
        let settings = SettingsStore(defaults: scratchDefaults())
        settings.defaultOutputLocation = .sameAsSource
        let coordinator = AppCoordinator(historyStore: history, settingsStore: settings)
        coordinator.performSideEffects = false

        await coordinator.handleFilesSelected([fixtureURL()])?.value
        XCTAssertEqual(coordinator.outputDirectory.path, fixtureURL().deletingLastPathComponent().path)
    }

    // MARK: - Cancellation

    func testCancelConversionReturnsToLoaded() async {
        let coordinator = makeCoordinator()
        let book = await loadFixture(into: coordinator)

        let task = coordinator.beginConvert(book: book)
        guard case .converting(_, let progress) = coordinator.screen else {
            XCTFail("expected .converting"); return
        }
        coordinator.cancelConversion(progress: progress)
        await task?.value

        guard case .loaded = coordinator.screen else {
            XCTFail("expected .loaded after cancel, got \(coordinator.screen)")
            return
        }
    }

    // MARK: - Same-path overwrite guard

    /// No input format shared an extension with any output format until
    /// `EpubOutput`/`PdfInput` landed - `.epub -> .epub` and `.pdf -> .pdf`
    /// ("clean up this book") are now both real, and if the output
    /// directory is the source's own folder with a title-derived filename
    /// matching the original, the candidate output path can resolve to the
    /// exact same file as `book.sourceURL`. Choosing "Replace" must never
    /// destroy that file - this pins `AppCoordinator.isSameFile`'s guard in
    /// `handleDuplicateResolution`, not just documents the intent.
    func testDuplicateReplaceNeverOverwritesTheSourceFile() async throws {
        let coordinator = makeCoordinator()
        coordinator.outputFormat = .epub

        let workDir = scratchDir("same-path")
        let sourceCopy = workDir.appendingPathComponent("Minimal Fixture Book.epub")
        try FileManager.default.copyItem(at: fixtureURL(), to: sourceCopy)
        let originalBytes = try Data(contentsOf: sourceCopy)

        let book = try await BookLoader.load(sourceCopy)
        coordinator.outputDirectory = workDir
        XCTAssertEqual(book.sourceURL.path, sourceCopy.path, "test setup: source and candidate output must collide")

        let beginTask = coordinator.beginConvert(book: book)
        XCTAssertNil(beginTask, "the source file already exists at the candidate path — should prompt, not convert directly")
        guard case .duplicateConfirm = coordinator.screen else {
            XCTFail("expected .duplicateConfirm, got \(coordinator.screen)")
            return
        }

        let task = coordinator.handleDuplicateResolution(.replace, book: book)
        await task?.value

        let survivingBytes = try Data(contentsOf: sourceCopy)
        XCTAssertEqual(survivingBytes, originalBytes, "choosing Replace must never truncate/overwrite the source file")

        guard case .complete(let info) = coordinator.screen else {
            XCTFail("expected .complete (falling back to Keep Both), got \(coordinator.screen)")
            return
        }
        XCTAssertNotEqual(info.outputURL.path, sourceCopy.path, "output must land at a different path than the source")
        XCTAssertEqual(info.outputURL.lastPathComponent, "Minimal Fixture Book (1).epub")
    }

    // MARK: - Review fixes

    func testUnsupportedFileShowsAnErrorInsteadOfDoingNothing() async {
        let coordinator = makeCoordinator()
        await coordinator.handleFilesSelected([URL(fileURLWithPath: "/tmp/notes.rtf")])?.value
        guard case .error(let message, _, _) = coordinator.screen else {
            return XCTFail("expected .error, got \(coordinator.screen)")
        }
        XCTAssertEqual(message, "Bookdrop can't open .rtf files.")
    }

    func testUnsupportedFilesInABatchAreSkipped() async {
        let coordinator = makeCoordinator()
        await coordinator.handleFilesSelected([fixtureURL(), URL(fileURLWithPath: "/tmp/notes.rtf")])?.value
        guard case .loaded = coordinator.screen else {
            return XCTFail("one supported file should load directly, got \(coordinator.screen)")
        }
    }

    func testLastOutputFormatIsRemembered() {
        let settings = SettingsStore(defaults: InMemoryKeyValueStore())
        let first = AppCoordinator(historyStore: HistoryStore(directory: scratchDir("h1")), settingsStore: settings)
        first.outputFormat = .docx
        let second = AppCoordinator(historyStore: HistoryStore(directory: scratchDir("h2")), settingsStore: settings)
        XCTAssertEqual(second.outputFormat, .docx)

        settings.rememberLastOutputFormat = false
        let third = AppCoordinator(historyStore: HistoryStore(directory: scratchDir("h3")), settingsStore: settings)
        XCTAssertEqual(third.outputFormat, .pdf)
    }

    func testAskEveryTimePromptsOncePerLoadedBook() async {
        let settings = SettingsStore(defaults: InMemoryKeyValueStore())
        settings.defaultOutputLocation = .askEveryTime
        let coordinator = AppCoordinator(historyStore: HistoryStore(directory: scratchDir("h")), settingsStore: settings)
        coordinator.performSideEffects = false
        await coordinator.handleFilesSelected([fixtureURL()])?.value
        XCTAssertTrue(coordinator.needsOutputDirectoryPrompt)
    }
}
