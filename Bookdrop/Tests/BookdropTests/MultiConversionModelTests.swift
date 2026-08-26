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
}
