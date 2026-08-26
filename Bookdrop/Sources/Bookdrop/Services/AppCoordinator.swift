import Foundation
import AppKit
import PDFKit

/// Owns the app's screen state machine and conversion flow. Pulled out of `ContentView`
/// so the flow (file selection → duplicate check → convert → complete/error) is unit
/// testable without driving real UI.
@MainActor
final class AppCoordinator: ObservableObject {
    @Published var screen: AppScreen = .empty
    @Published var pdfOptions = PDFOptions()
    @Published var outputFormat: OutputFormat = .pdf
    @Published var outputDirectory: URL

    let historyStore: HistoryStore
    let settingsStore: SettingsStore

    /// Disabled in tests so conversions don't open Finder/PDF viewers or fire real
    /// system notifications as a side effect of running the test suite.
    var performSideEffects = true

    private var conversionTask: Task<Void, Never>?

    init(historyStore: HistoryStore, settingsStore: SettingsStore) {
        self.historyStore = historyStore
        self.settingsStore = settingsStore
        self.outputDirectory = Self.defaultOutputDirectory(for: settingsStore.defaultOutputLocation)
    }

    static func defaultOutputDirectory(for location: DefaultOutputLocation, sourceURL: URL? = nil) -> URL {
        let downloads =
            FileManager.default.urls(for: .downloadsDirectory, in: .userDomainMask).first
            ?? FileManager.default.homeDirectoryForCurrentUser
        switch location {
        case .downloads, .askEveryTime:
            return downloads
        case .sameAsSource:
            return sourceURL?.deletingLastPathComponent() ?? downloads
        }
    }

    func handleFilesSelected(_ urls: [URL]) {
        guard urls.count > 1 else {
            if let url = urls.first { handleSingleFile(url) }
            return
        }
        let model = MultiConversionModel(urls: urls, outputDirectory: outputDirectory)
        screen = .multipleFiles(model)
    }

    func handleSingleFile(_ url: URL) {
        do {
            let book = try KindleNormalizer.parse(fileAt: url)
            pdfOptions.preserveEpubStyling = settingsStore.preserveOriginalStylingByDefault
            outputDirectory = Self.defaultOutputDirectory(
                for: settingsStore.defaultOutputLocation, sourceURL: url)
            screen = .loaded(book)
        } catch let error as EpubParserError {
            screen = .error(
                message: error.errorDescription ?? "This EPUB couldn't be read.",
                hint: "Try another file, or choose a different output format.",
                technicalDetails: String(describing: error))
        } catch let error as KindleNormalizerError {
            screen = .error(
                message: error.errorDescription ?? "This Kindle file couldn't be read.",
                hint: "Try another file, or choose a different output format.",
                technicalDetails: String(describing: error))
        } catch {
            screen = .error(
                message: "This file couldn't be read.", hint: nil,
                technicalDetails: String(describing: error))
        }
    }

    @discardableResult
    func beginConvert(book: Book) -> Task<Void, Never>? {
        let candidate = outputDirectory.appendingPathComponent(
            sanitizedFilename(book.title) + "." + outputFormat.fileExtension)
        if FileManager.default.fileExists(atPath: candidate.path) {
            screen = .duplicateConfirm(book: book, filename: candidate.lastPathComponent)
            return nil
        } else {
            return startConversion(book: book, outputURL: candidate)
        }
    }

    @discardableResult
    func handleDuplicateResolution(_ resolution: DuplicateResolution, book: Book) -> Task<Void, Never>? {
        switch resolution {
        case .cancel:
            screen = .loaded(book)
            return nil
        case .replace:
            let url = outputDirectory.appendingPathComponent(
                sanitizedFilename(book.title) + "." + outputFormat.fileExtension)
            return startConversion(book: book, outputURL: url)
        case .keepBoth:
            let url = nextAvailableURL(
                directory: outputDirectory, baseName: sanitizedFilename(book.title),
                extension: outputFormat.fileExtension)
            return startConversion(book: book, outputURL: url)
        }
    }

    func cancelConversion(progress: ConversionProgress) {
        progress.cancel()
        conversionTask?.cancel()
    }

    @discardableResult
    private func startConversion(book: Book, outputURL: URL) -> Task<Void, Never> {
        let progress = ConversionProgress()
        screen = .converting(book: book, progress: progress)
        let task = Task { await self.performConversion(book: book, outputURL: outputURL, progress: progress) }
        conversionTask = task
        return task
    }

    /// The actual conversion + completion handling, awaitable directly by tests
    /// (bypassing the `Task` wrapper `startConversion` uses for fire-and-forget UI calls).
    func performConversion(book: Book, outputURL: URL, progress: ConversionProgress) async {
        do {
            let resultURL = try await convert(book: book, outputURL: outputURL, progress: progress)
            let pageCount = outputFormat == .pdf ? PDFDocument(url: resultURL)?.pageCount : nil
            let info = CompletionInfo(outputURL: resultURL, pageCount: pageCount)
            historyStore.add(
                HistoryEntry(
                    title: book.title,
                    conversionLabel: "\(book.sourceURL.pathExtension.uppercased()) → \(outputFormat.rawValue)",
                    outputPath: resultURL.path, sourcePath: book.sourceURL.path))

            if performSideEffects {
                if settingsStore.showNotification {
                    NotificationService.notifyConversionComplete(bookTitle: book.title, outputURL: resultURL)
                }
                if settingsStore.revealInFinder {
                    NSWorkspace.shared.activateFileViewerSelecting([resultURL])
                }
                if settingsStore.openConvertedFile {
                    NSWorkspace.shared.open(resultURL)
                }
            }
            screen = .complete(info)
        } catch is CancellationError {
            screen = .loaded(book)
        } catch RustConversionEngineError.cancelled {
            screen = .loaded(book)
        } catch let error as LocalizedError {
            screen = .error(
                message: error.errorDescription ?? "Couldn't convert this book.",
                hint: outputFormat == .pdf
                    ? "Try enabling \u{201C}Preserve EPUB Styling\u{201D} or choose another output format."
                    : "Try another output format.",
                technicalDetails: String(describing: error))
        } catch {
            screen = .error(
                message: "Couldn't convert this book.", hint: nil,
                technicalDetails: String(describing: error))
        }
    }

    private func convert(book: Book, outputURL: URL, progress: ConversionProgress) async throws -> URL {
        switch outputFormat {
        case .pdf:
            // Routed through the Rust engine (plan Phase 4) — .txt/.html/.docx
            // stay on the Swift-native converters until Phase 6 ports them too.
            return try await RustConversionEngine.convert(
                book: book, options: pdfOptions, outputURL: outputURL, progress: progress)
        case .txt:
            progress.stageText = "Converting…"
            progress.fraction = 0.5
            let url = try TxtConverter.convert(book: book, outputURL: outputURL)
            progress.fraction = 1.0
            return url
        case .html:
            progress.stageText = "Converting…"
            progress.fraction = 0.5
            let url = try HtmlConverter.convert(
                book: book, includeCover: pdfOptions.includeCover,
                generateTOC: pdfOptions.generateTableOfContents, outputURL: outputURL)
            progress.fraction = 1.0
            return url
        case .docx:
            progress.stageText = "Converting…"
            progress.fraction = 0.5
            let url = try DocxConverter.convert(
                book: book, includeCover: pdfOptions.includeCover, outputURL: outputURL)
            progress.fraction = 1.0
            return url
        }
    }
}
