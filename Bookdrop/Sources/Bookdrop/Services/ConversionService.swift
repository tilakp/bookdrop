import AppKit
import Foundation

/// The actions the "After conversion" settings ask for. A protocol, so
/// tests can record the calls instead of opening Finder or posting
/// notifications.
@MainActor
protocol ConversionSideEffects {
    func notify(bookTitles: [String], format: OutputFormat, outputURLs: [URL])
    func reveal(_ urls: [URL])
    func open(_ url: URL)
}

struct SystemSideEffects: ConversionSideEffects {
    /// Not main-actor isolated, so it can be a default argument.
    nonisolated init() {}

    func notify(bookTitles: [String], format: OutputFormat, outputURLs: [URL]) {
        NotificationService.notifyConversionComplete(bookTitles: bookTitles, format: format, outputURLs: outputURLs)
    }

    func reveal(_ urls: [URL]) {
        NSWorkspace.shared.activateFileViewerSelecting(urls)
    }

    func open(_ url: URL) {
        NSWorkspace.shared.open(url)
    }
}

/// Converts books and does the steps after conversion. The single-book
/// screen and the batch both use it, so both record history and apply the
/// "After conversion" settings in the same way.
@MainActor
final class ConversionService {
    let historyStore: HistoryStore
    let settingsStore: SettingsStore
    private let sideEffects: ConversionSideEffects

    init(historyStore: HistoryStore, settingsStore: SettingsStore, sideEffects: ConversionSideEffects) {
        self.historyStore = historyStore
        self.settingsStore = settingsStore
        self.sideEffects = sideEffects
    }

    /// Converts `book` and records the result in the history.
    func convert(
        book: Book, format: OutputFormat, options: PDFOptions, outputURL: URL, progress: ConversionProgress
    ) async throws -> URL {
        let resultURL = try await RustConversionEngine.convert(
            book: book, options: options, outputURL: outputURL, progress: progress)
        historyStore.add(
            HistoryEntry(
                title: book.title,
                conversionLabel: "\(book.sourceURL.pathExtension.uppercased()) → \(format.rawValue)",
                outputPath: resultURL.path, sourcePath: book.sourceURL.path))
        return resultURL
    }

    /// Applies the "After conversion" settings once for all the books that
    /// were converted together. A batch gets one notification and one
    /// Finder window. Only a single book is opened: a batch could open
    /// many windows at once.
    func finish(converted: [(title: String, url: URL)], format: OutputFormat) {
        guard !converted.isEmpty else { return }
        let urls = converted.map(\.url)
        if settingsStore.showNotification {
            sideEffects.notify(bookTitles: converted.map(\.title), format: format, outputURLs: urls)
        }
        if settingsStore.revealInFinder {
            sideEffects.reveal(urls)
        }
        if settingsStore.openConvertedFile, urls.count == 1 {
            sideEffects.open(urls[0])
        }
    }
}
