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

    /// `UserDefaults(suiteName:)` writes a real plist under ~/Library/Preferences.
    /// `removePersistentDomain` only clears the in-memory cache — cfprefsd doesn't
    /// reliably unlink the file itself — so the teardown deletes it directly too.
    private func scratchDefaults() -> UserDefaults {
        let suiteName = "AppCoordinatorTests-\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suiteName)!
        addTeardownBlock {
            defaults.removePersistentDomain(forName: suiteName)
            defaults.synchronize()  // force any pending write to land before we delete the file
            let plistURL = FileManager.default.urls(for: .libraryDirectory, in: .userDomainMask)[0]
                .appendingPathComponent("Preferences/\(suiteName).plist")
            try? FileManager.default.removeItem(at: plistURL)
        }
        return defaults
    }

    /// Loads the fixture into `coordinator` and redirects its output directory to a
    /// scratch folder — `handleFilesSelected` resets `outputDirectory` from settings as
    /// part of loading, so the redirect must happen *after*, not before.
    private func loadFixture(into coordinator: AppCoordinator) -> Book {
        coordinator.handleFilesSelected([fixtureURL()])
        coordinator.outputDirectory = scratchDir("output")
        guard case .loaded(let book) = coordinator.screen else {
            fatalError("expected .loaded after selecting the fixture")
        }
        return book
    }

    // MARK: - File selection

    func testSelectingValidFileTransitionsToLoaded() {
        let coordinator = makeCoordinator()
        let book = loadFixture(into: coordinator)
        XCTAssertEqual(book.title, "Minimal Fixture Book")
    }

    func testSelectingInvalidFileTransitionsToError() throws {
        let coordinator = makeCoordinator()
        let bogus = FileManager.default.temporaryDirectory.appendingPathComponent("bogus-\(UUID().uuidString).epub")
        try "not a zip".write(to: bogus, atomically: true, encoding: .utf8)
        defer { try? FileManager.default.removeItem(at: bogus) }

        coordinator.handleFilesSelected([bogus])
        guard case .error(let message, _, let details) = coordinator.screen else {
            XCTFail("expected .error, got \(coordinator.screen)")
            return
        }
        XCTAssertFalse(message.isEmpty)
        XCTAssertNotNil(details)
    }

    func testSelectingMultipleFilesTransitionsToMultipleFiles() {
        let coordinator = makeCoordinator()
        coordinator.handleFilesSelected([fixtureURL(), fixtureURL()])
        guard case .multipleFiles(let model) = coordinator.screen else {
            XCTFail("expected .multipleFiles, got \(coordinator.screen)")
            return
        }
        XCTAssertEqual(model.jobs.count, 2)
    }

    // MARK: - Duplicate detection

    func testConvertWithNoCollisionStartsConversionDirectly() async {
        let coordinator = makeCoordinator()
        let book = loadFixture(into: coordinator)

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

    func testConvertWithCollisionShowsDuplicateDialog() throws {
        let coordinator = makeCoordinator()
        let book = loadFixture(into: coordinator)
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

    func testDuplicateCancelReturnsToLoaded() throws {
        let coordinator = makeCoordinator()
        let book = loadFixture(into: coordinator)
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
        let book = loadFixture(into: coordinator)
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
        XCTAssertGreaterThan(info.pageCount, 0)
    }

    func testDuplicateKeepBothWritesNumberedFile() async throws {
        let coordinator = makeCoordinator()
        let book = loadFixture(into: coordinator)
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

    func testPreserveStylingSettingAppliedWhenBookLoads() {
        let history = HistoryStore(directory: scratchDir("history"))
        let settings = SettingsStore(defaults: scratchDefaults())
        settings.preserveOriginalStylingByDefault = false
        let coordinator = AppCoordinator(historyStore: history, settingsStore: settings)
        coordinator.performSideEffects = false

        coordinator.handleFilesSelected([fixtureURL()])
        XCTAssertFalse(coordinator.pdfOptions.preserveEpubStyling)
    }

    func testOutputDirectoryFollowsSameAsSourceSettingOnLoad() {
        let history = HistoryStore(directory: scratchDir("history"))
        let settings = SettingsStore(defaults: scratchDefaults())
        settings.defaultOutputLocation = .sameAsSource
        let coordinator = AppCoordinator(historyStore: history, settingsStore: settings)
        coordinator.performSideEffects = false

        coordinator.handleFilesSelected([fixtureURL()])
        XCTAssertEqual(coordinator.outputDirectory.path, fixtureURL().deletingLastPathComponent().path)
    }

    // MARK: - Cancellation

    func testCancelConversionReturnsToLoaded() async {
        let coordinator = makeCoordinator()
        let book = loadFixture(into: coordinator)

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
}
