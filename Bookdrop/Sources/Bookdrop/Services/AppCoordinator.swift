import Foundation
import PDFKit

/// Owns the app's screen state machine and conversion flow. Pulled out of `ContentView`
/// so the flow (file selection → duplicate check → convert → complete/error) is unit
/// testable without driving real UI.
@MainActor
final class AppCoordinator: ObservableObject {
    @Published var screen: AppScreen = .empty
    @Published var pdfOptions = PDFOptions()
    @Published var outputFormat: OutputFormat = .pdf {
        didSet {
            if settingsStore.rememberLastOutputFormat { settingsStore.lastOutputFormat = outputFormat }
        }
    }
    @Published var outputDirectory: URL
    /// Set when a new book loads with "Ask every time" on. The loaded screen
    /// asks for a folder once, then clears it, so returning to that screen
    /// (after Cancel, for example) does not ask again.
    @Published var needsOutputDirectoryPrompt = false

    let historyStore: HistoryStore
    let settingsStore: SettingsStore
    let conversions: ConversionService

    private var conversionTask: Task<Void, Never>?

    init(
        historyStore: HistoryStore, settingsStore: SettingsStore,
        sideEffects: ConversionSideEffects = SystemSideEffects()
    ) {
        self.historyStore = historyStore
        self.settingsStore = settingsStore
        self.conversions = ConversionService(
            historyStore: historyStore, settingsStore: settingsStore, sideEffects: sideEffects)
        self.outputDirectory = Self.defaultOutputDirectory(for: settingsStore.defaultOutputLocation)
        if settingsStore.rememberLastOutputFormat, let last = settingsStore.lastOutputFormat {
            self.outputFormat = last
        }
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

    /// Returns the loading task for a single file (tests await it), or nil
    /// when nothing needs loading.
    @discardableResult
    func handleFilesSelected(_ urls: [URL]) -> Task<Void, Never>? {
        let supported = urls.filter(BookLoader.isSupported)
        guard !supported.isEmpty else {
            if let first = urls.first {
                let error = UnsupportedFormatError(fileExtension: first.pathExtension.lowercased())
                screen = .error(message: error.localizedDescription, hint: error.recoverySuggestion, technicalDetails: nil)
            }
            return nil
        }
        guard supported.count > 1 else { return loadSingleFile(supported[0]) }
        screen = .multipleFiles(MultiConversionModel(urls: supported, outputDirectory: outputDirectory))
        return nil
    }

    private func loadSingleFile(_ url: URL) -> Task<Void, Never> {
        screen = .loading(filename: url.lastPathComponent)
        return Task {
            do {
                let book = try await BookLoader.load(url)
                pdfOptions.preserveEpubStyling = settingsStore.preserveOriginalStylingByDefault
                outputDirectory = Self.defaultOutputDirectory(
                    for: settingsStore.defaultOutputLocation, sourceURL: url)
                needsOutputDirectoryPrompt = settingsStore.defaultOutputLocation == .askEveryTime
                screen = .loaded(book)
            } catch {
                screen = .error(
                    message: error.localizedDescription,
                    hint: (error as? LocalizedError)?.recoverySuggestion ?? "Try another file.",
                    technicalDetails: String(describing: error))
            }
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
            // PDF to PDF is a valid conversion. If the output folder is the
            // source's folder and the title gives the source's file name,
            // `url` is the source file, and replacing it would destroy the
            // original. Keep both instead: the conversion is still the one
            // that was asked for, only not an overwrite.
            if isSameFile(url, book.sourceURL) {
                let keepBothURL = nextAvailableURL(
                    directory: outputDirectory, baseName: sanitizedFilename(book.title),
                    extension: outputFormat.fileExtension)
                return startConversion(book: book, outputURL: keepBothURL)
            }
            return startConversion(book: book, outputURL: url)
        case .keepBoth:
            let url = nextAvailableURL(
                directory: outputDirectory, baseName: sanitizedFilename(book.title),
                extension: outputFormat.fileExtension)
            return startConversion(book: book, outputURL: url)
        }
    }

    /// Compares resolved, standardized paths, not plain strings, so that
    /// symlinks and the kind of path noise
    /// `.standardizedFileURL` normalizes away.
    private func isSameFile(_ a: URL, _ b: URL) -> Bool {
        a.resolvingSymlinksInPath().standardizedFileURL.path == b.resolvingSymlinksInPath().standardizedFileURL.path
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
            let resultURL = try await conversions.convert(
                book: book, format: outputFormat, options: pdfOptions, outputURL: outputURL, progress: progress)
            let pageCount = outputFormat == .pdf ? PDFDocument(url: resultURL)?.pageCount : nil
            conversions.finish(converted: [(book.title, resultURL)], format: outputFormat)
            screen = .complete(CompletionInfo(outputURL: resultURL, pageCount: pageCount))
        } catch is CancellationError {
            screen = .loaded(book)
        } catch let error as EngineError where error.isCancellation {
            screen = .loaded(book)
        } catch let error as LocalizedError {
            // "Preserve EPUB Styling" only helps a source that has EPUB
            // styling, so a PDF source gets the general hint.
            let sourceIsStyledEpubLike = book.sourceURL.pathExtension.lowercased() != "pdf"
            let generalHint =
                outputFormat == .pdf && sourceIsStyledEpubLike
                ? "Try enabling \u{201C}Preserve EPUB Styling\u{201D} or choose another output format."
                : "Try another output format."
            screen = .error(
                message: error.errorDescription ?? "Couldn't convert this book.",
                hint: error.recoverySuggestion ?? generalHint,
                technicalDetails: String(describing: error))
        } catch {
            screen = .error(
                message: "Couldn't convert this book.", hint: nil,
                technicalDetails: String(describing: error))
        }
    }
}
