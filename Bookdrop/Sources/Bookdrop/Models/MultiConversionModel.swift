import Foundation

enum FileJobStatus: Equatable {
    case waiting
    case converting(fraction: Double, stage: String)
    case done(URL)
    case failed(String)
}

@MainActor
final class MultiConversionModel: ObservableObject {
    struct Job: Identifiable {
        let id = UUID()
        let sourceURL: URL
        var status: FileJobStatus = .waiting
    }

    @Published var jobs: [Job]
    @Published var outputDirectory: URL
    @Published var isRunning = false
    @Published var isCancelled = false

    init(urls: [URL], outputDirectory: URL) {
        self.jobs = urls.map { Job(sourceURL: $0) }
        self.outputDirectory = outputDirectory
    }

    var completedCount: Int {
        jobs.filter { if case .done = $0.status { return true }; return false }.count
    }

    var isFinished: Bool {
        jobs.allSatisfy {
            switch $0.status {
            case .waiting, .converting: return false
            case .done, .failed: return true
            }
        }
    }

    func cancelAll() {
        isCancelled = true
    }

    func run(options: PDFOptions, format: OutputFormat, historyStore: HistoryStore) async {
        isRunning = true
        defer { isRunning = false }

        for index in jobs.indices {
            if isCancelled {
                if case .waiting = jobs[index].status {
                    jobs[index].status = .failed("Cancelled")
                }
                continue
            }

            let sourceURL = jobs[index].sourceURL
            do {
                let book = try EpubParser.parse(fileAt: sourceURL)
                let outputURL = nextAvailableURL(
                    directory: outputDirectory, baseName: sanitizedFilename(book.title),
                    extension: format.fileExtension)
                let progress = ConversionProgress()
                jobs[index].status = .converting(fraction: 0, stage: progress.stageText)

                let watcher = Task { @MainActor [weak self] in
                    while !Task.isCancelled {
                        self?.jobs[index].status = .converting(fraction: progress.fraction, stage: progress.stageText)
                        try? await Task.sleep(nanoseconds: 150_000_000)
                    }
                }

                let resultURL = try await convert(
                    book: book, options: options, format: format, outputURL: outputURL, progress: progress)
                watcher.cancel()

                jobs[index].status = .done(resultURL)
                historyStore.add(
                    HistoryEntry(
                        title: book.title, conversionLabel: "EPUB → \(format.rawValue)",
                        outputPath: resultURL.path, sourcePath: sourceURL.path))
            } catch {
                jobs[index].status = .failed(error.localizedDescription)
            }
        }
    }

    private func convert(
        book: Book, options: PDFOptions, format: OutputFormat, outputURL: URL, progress: ConversionProgress
    ) async throws -> URL {
        switch format {
        case .pdf:
            // Routed through the Rust engine (plan Phase 4) — .txt/.html/.docx
            // stay on the Swift-native converters until Phase 6 ports them too.
            return try await RustConversionEngine.convert(
                book: book, options: options, outputURL: outputURL, progress: progress)
        case .txt:
            return try TxtConverter.convert(book: book, outputURL: outputURL)
        case .html:
            return try HtmlConverter.convert(
                book: book, includeCover: options.includeCover,
                generateTOC: options.generateTableOfContents, outputURL: outputURL)
        case .docx:
            return try DocxConverter.convert(book: book, includeCover: options.includeCover, outputURL: outputURL)
        }
    }
}
