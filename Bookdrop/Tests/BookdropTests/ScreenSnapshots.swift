import AppKit
import SwiftUI
import XCTest
@testable import Bookdrop

/// Renders every screen to PNG files, in light and dark mode, for design
/// review. It runs only when BOOKDROP_SNAPSHOT_DIR names the folder to
/// write to:
///
///     BOOKDROP_SNAPSHOT_DIR=/tmp/shots swift test --filter ScreenSnapshots
///
/// The window chrome (title bar, toolbar) is not in the images: check that
/// in the running app.
@MainActor
final class ScreenSnapshots: XCTestCase {
    private var outputDir: URL!

    override func setUp() async throws {
        guard let path = ProcessInfo.processInfo.environment["BOOKDROP_SNAPSHOT_DIR"] else {
            throw XCTSkip("set BOOKDROP_SNAPSHOT_DIR to render screen snapshots")
        }
        outputDir = URL(fileURLWithPath: path)
        try FileManager.default.createDirectory(at: outputDir, withIntermediateDirectories: true)
    }

    private func fixture(_ name: String, _ ext: String) -> URL {
        Bundle.module.url(forResource: name, withExtension: ext, subdirectory: "Fixtures")!
    }

    private func render<V: View>(_ name: String, height: CGFloat = 560, _ view: V) async {
        for (suffix, appearance) in [("light", NSAppearance.Name.aqua), ("dark", .darkAqua)] {
            let size = CGSize(width: 720, height: height)
            let window = NSWindow(
                contentRect: CGRect(origin: .zero, size: size), styleMask: [.titled], backing: .buffered, defer: false)
            window.isReleasedWhenClosed = false
            window.appearance = NSAppearance(named: appearance)
            let host = NSHostingView(rootView: view.frame(width: size.width, height: size.height).tint(.brandPurple))
            host.frame = CGRect(origin: .zero, size: size)
            window.contentView = host
            host.layoutSubtreeIfNeeded()
            try? await Task.sleep(nanoseconds: 300_000_000)
            host.layoutSubtreeIfNeeded()
            guard let rep = host.bitmapImageRepForCachingDisplay(in: host.bounds) else { continue }
            host.cacheDisplay(in: host.bounds, to: rep)
            let url = outputDir.appendingPathComponent("\(name)-\(suffix).png")
            try? rep.representation(using: .png, properties: [:])?.write(to: url)
            window.close()
        }
    }

    private func history() -> HistoryStore {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("snap-history-\(UUID().uuidString)")
        let store = HistoryStore(directory: dir)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let day: TimeInterval = 86_400
        let titles: [(String, String, TimeInterval)] = [
            ("The Origin of Species", "EPUB → PDF", 600),
            ("Pride and Prejudice", "EPUB → Markdown", 2 * day),
            ("The Story of Doctor Dolittle", "AZW3 → EPUB", 9 * day),
            ("Moby-Dick; or, The Whale", "PDF → DOCX", 40 * day),
            ("Frankenstein", "EPUB → TXT", 70 * day),
            ("Middlemarch", "MOBI → PDF", 90 * day),
            ("Bleak House", "EPUB → HTML", 120 * day),
        ]
        for (index, (title, label, age)) in titles.reversed().enumerated() {
            // The last entry's file is missing, to show that state too.
            let ext = label.components(separatedBy: " ").last!.lowercased().replacingOccurrences(of: "markdown", with: "md")
            let output = dir.appendingPathComponent("\(title).\(ext)")
            if index > 0 { try? Data().write(to: output) }
            store.add(
                HistoryEntry(
                    title: title, conversionLabel: label, date: Date().addingTimeInterval(-age),
                    outputPath: output.path, sourcePath: "/tmp/\(title).epub"))
        }
        return store
    }

    func testRenderEveryScreen() async throws {
        let book = try await BookLoader.load(fixture("minimal", "epub"))

        await render(
            "01-empty", EmptyStateView(historyStore: history(), onFilesSelected: { _ in }, onChooseFiles: {}, onConvertAgain: { _ in }))
        let emptyHistory = HistoryStore(
            directory: FileManager.default.temporaryDirectory.appendingPathComponent("snap-\(UUID().uuidString)"))
        await render(
            "02-empty-first-run",
            EmptyStateView(historyStore: emptyHistory, onFilesSelected: { _ in }, onChooseFiles: {}, onConvertAgain: { _ in }))

        for format in [OutputFormat.pdf, .markdown] {
            await render(
                "03-loaded-\(format.fileExtension)", height: 700,
                FileLoadedView(
                    book: book, options: .constant(PDFOptions()), format: .constant(format),
                    outputDirectory: .constant(URL(fileURLWithPath: NSHomeDirectory()).appendingPathComponent("Downloads")),
                    needsOutputDirectoryPrompt: .constant(false), onConvert: {}))
        }


        let progress = ConversionProgress()
        progress.fraction = 0.42
        progress.stageText = "Rendering chapter 12/29"
        await render("05-converting", ConvertingView(book: book, formatName: "PDF", progress: progress, onCancel: {}))

        let output = FileManager.default.temporaryDirectory.appendingPathComponent("Minimal Fixture Book.pdf")
        try Data(count: 180_000).write(to: output)
        await render("06-complete", CompleteView(info: CompletionInfo(outputURL: output, pageCount: 214), onConvertAnother: {}))

        await render(
            "07-error",
            ErrorStateView(
                message: "This PDF is password-protected, so it can't be converted.",
                hint: "Open the PDF with its password, save an unprotected copy, and convert that.",
                technicalDetails: "EngineError(code: \"password_protected\")", onChooseAnother: {}))

        let urls = [fixture("minimal", "epub"), fixture("minimal", "azw3"), fixture("minimal", "epub")]
        let ready = MultiConversionModel(urls: urls, outputDirectory: URL(fileURLWithPath: NSHomeDirectory()))
        let conversions = ConversionService(
            historyStore: emptyHistory, settingsStore: SettingsStore(defaults: InMemoryKeyValueStore()),
            sideEffects: RecordingSideEffects())
        await render(
            "08-batch-ready",
            MultipleFilesView(model: ready, options: PDFOptions(), format: .constant(.pdf), conversions: conversions, onDone: {}))
        let running = MultiConversionModel(urls: urls, outputDirectory: URL(fileURLWithPath: NSHomeDirectory()))
        running.jobs[0].status = .done(output)
        running.jobs[1].status = .converting(fraction: 0.6, stage: "Rendering")
        running.isRunning = true
        await render(
            "09-batch-running",
            MultipleFilesView(model: running, options: PDFOptions(), format: .constant(.pdf), conversions: conversions, onDone: {}))
        let finished = MultiConversionModel(urls: urls, outputDirectory: URL(fileURLWithPath: NSHomeDirectory()))
        finished.jobs[0].status = .done(output)
        finished.jobs[1].status = .failed("This Kindle file couldn't be read: unexpected end of file")
        finished.jobs[2].status = .done(output)
        await render(
            "10-batch-finished",
            MultipleFilesView(model: finished, options: PDFOptions(), format: .constant(.pdf), conversions: conversions, onDone: {}))

        await render(
            "11-settings", height: 400, SettingsView(settings: SettingsStore(defaults: InMemoryKeyValueStore())))
    }
}
