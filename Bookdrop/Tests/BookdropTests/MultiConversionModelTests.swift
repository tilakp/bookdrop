import XCTest
@testable import Bookdrop

@MainActor
final class MultiConversionModelTests: XCTestCase {
    private func fixtureURL() -> URL {
        Bundle.module.url(forResource: "minimal", withExtension: "epub", subdirectory: "Fixtures")!
    }

    func testConvertsMultipleFilesAndRecordsHistory() async throws {
        let outputDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("MultiConversionModelTests-\(UUID().uuidString)")
        let historyDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("MultiConversionModelTests-history-\(UUID().uuidString)")
        let history = HistoryStore(directory: historyDir)

        // Two distinct source URLs pointing at the same fixture — exercises the
        // "Keep Both"-style auto-numbering since both books share a title.
        let model = MultiConversionModel(urls: [fixtureURL(), fixtureURL()], outputDirectory: outputDir)

        await model.run(options: PDFOptions(), format: .pdf, historyStore: history)

        XCTAssertEqual(model.completedCount, 2)
        XCTAssertTrue(model.isFinished)
        for job in model.jobs {
            guard case .done(let url) = job.status else {
                XCTFail("expected job to be done, got \(job.status)")
                continue
            }
            XCTAssertTrue(FileManager.default.fileExists(atPath: url.path))
        }
        XCTAssertEqual(history.entries.count, 2)
    }

    func testCancelAllStopsRemainingJobs() async {
        let outputDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("MultiConversionModelTests-\(UUID().uuidString)")
        let historyDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("MultiConversionModelTests-history-\(UUID().uuidString)")
        let history = HistoryStore(directory: historyDir)

        let model = MultiConversionModel(urls: [fixtureURL()], outputDirectory: outputDir)
        model.cancelAll()
        await model.run(options: PDFOptions(), format: .pdf, historyStore: history)

        guard case .failed = model.jobs[0].status else {
            XCTFail("expected cancelled job to be marked failed, got \(model.jobs[0].status)")
            return
        }
    }

    func testConvertsMultipleFilesToNonPDFFormat() async throws {
        let outputDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("MultiConversionModelTests-\(UUID().uuidString)")
        let historyDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("MultiConversionModelTests-history-\(UUID().uuidString)")
        let history = HistoryStore(directory: historyDir)

        let model = MultiConversionModel(urls: [fixtureURL(), fixtureURL()], outputDirectory: outputDir)
        await model.run(options: PDFOptions(), format: .html, historyStore: history)

        XCTAssertEqual(model.completedCount, 2)
        for job in model.jobs {
            guard case .done(let url) = job.status else {
                XCTFail("expected job to be done, got \(job.status)")
                continue
            }
            XCTAssertEqual(url.pathExtension, "html")
            XCTAssertTrue(FileManager.default.fileExists(atPath: url.path))
        }
        XCTAssertTrue(history.entries.allSatisfy { $0.conversionLabel == "EPUB → HTML" })
    }

    func testConvertsToTxt() async throws {
        let outputDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("MultiConversionModelTests-\(UUID().uuidString)")
        let historyDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("MultiConversionModelTests-history-\(UUID().uuidString)")
        let history = HistoryStore(directory: historyDir)

        let model = MultiConversionModel(urls: [fixtureURL()], outputDirectory: outputDir)
        await model.run(options: PDFOptions(), format: .txt, historyStore: history)

        XCTAssertEqual(model.completedCount, 1)
        guard case .done(let url) = model.jobs[0].status else {
            XCTFail("expected job to be done, got \(model.jobs[0].status)")
            return
        }
        XCTAssertEqual(url.pathExtension, "txt")
        let text = try String(contentsOf: url, encoding: .utf8)
        XCTAssertTrue(text.contains("Minimal Fixture Book"))
        XCTAssertTrue(text.contains("Chapter One"))
    }

    func testConvertsToDocx() async throws {
        let outputDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("MultiConversionModelTests-\(UUID().uuidString)")
        let historyDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("MultiConversionModelTests-history-\(UUID().uuidString)")
        let history = HistoryStore(directory: historyDir)

        let model = MultiConversionModel(urls: [fixtureURL()], outputDirectory: outputDir)
        await model.run(options: PDFOptions(), format: .docx, historyStore: history)

        XCTAssertEqual(model.completedCount, 1)
        guard case .done(let url) = model.jobs[0].status else {
            XCTFail("expected job to be done, got \(model.jobs[0].status)")
            return
        }
        XCTAssertEqual(url.pathExtension, "docx")
        let data = try Data(contentsOf: url)
        XCTAssertGreaterThan(data.count, 0)
        // "PK" zip-magic-bytes — a .docx is a zip container; a genuinely
        // empty or corrupted output wouldn't even have this much.
        XCTAssertEqual(data.prefix(2), Data([0x50, 0x4B]))
    }

    func testConvertsToEpub() async throws {
        let outputDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("MultiConversionModelTests-\(UUID().uuidString)")
        let historyDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("MultiConversionModelTests-history-\(UUID().uuidString)")
        let history = HistoryStore(directory: historyDir)

        let model = MultiConversionModel(urls: [fixtureURL()], outputDirectory: outputDir)
        await model.run(options: PDFOptions(), format: .epub, historyStore: history)

        XCTAssertEqual(model.completedCount, 1)
        guard case .done(let url) = model.jobs[0].status else {
            XCTFail("expected job to be done, got \(model.jobs[0].status)")
            return
        }
        XCTAssertEqual(url.pathExtension, "epub")
        // The regenerated EPUB should itself be re-parseable — the same
        // round-trip property anyform-doc's own epub_output_tests.rs
        // verifies at the Rust level, checked here end to end through the
        // real Swift/FFI boundary too.
        let regenerated = try await BookLoader.load(url)
        XCTAssertEqual(regenerated.title, "Minimal Fixture Book")
        XCTAssertEqual(regenerated.chapterCount, 2)
    }

    /// The progress watcher used to keep running after a failed file and
    /// set its row back to "converting" every 150 ms, so the batch never
    /// finished.
    func testFailedJobStaysFailedAndTheBatchFinishes() async throws {
        let outputDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("MultiConversionModelTests-\(UUID().uuidString)")
        let history = HistoryStore(directory: outputDir.appendingPathComponent("history"))
        let broken = FileManager.default.temporaryDirectory.appendingPathComponent("broken-\(UUID().uuidString).epub")
        try "not a zip".write(to: broken, atomically: true, encoding: .utf8)
        defer { try? FileManager.default.removeItem(at: broken) }

        let model = MultiConversionModel(urls: [broken, fixtureURL()], outputDirectory: outputDir)
        await model.run(options: PDFOptions(), format: .txt, historyStore: history)
        try await Task.sleep(nanoseconds: 400_000_000)

        guard case .failed = model.jobs[0].status else {
            return XCTFail("expected the broken file to stay failed, got \(model.jobs[0].status)")
        }
        XCTAssertEqual(model.completedCount, 1)
        XCTAssertTrue(model.isFinished)
    }

    func testCancelAllStopsTheFileThatIsConverting() async throws {
        let outputDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("MultiConversionModelTests-\(UUID().uuidString)")
        let history = HistoryStore(directory: outputDir.appendingPathComponent("history"))
        let model = MultiConversionModel(urls: [fixtureURL(), fixtureURL()], outputDirectory: outputDir)

        let task = model.start(options: PDFOptions(), format: .pdf, historyStore: history)
        while case .waiting = model.jobs[0].status { await Task.yield() }
        model.cancelAll()
        await task.value

        for job in model.jobs {
            XCTAssertEqual(job.status, .failed("Cancelled"))
        }
        XCTAssertTrue(model.isFinished)
    }
}
