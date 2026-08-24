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

        await model.run(options: PDFOptions(), historyStore: history)

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
        await model.run(options: PDFOptions(), historyStore: history)

        guard case .failed = model.jobs[0].status else {
            XCTFail("expected cancelled job to be marked failed, got \(model.jobs[0].status)")
            return
        }
    }
}
