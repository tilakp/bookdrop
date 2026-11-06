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

    private var runTask: Task<Void, Never>?

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

    /// Starts the batch. Returns the task so tests can await it.
    @discardableResult
    func start(options: PDFOptions, format: OutputFormat, historyStore: HistoryStore) -> Task<Void, Never> {
        let task = Task { await run(options: options, format: format, historyStore: historyStore) }
        runTask = task
        return task
    }

    /// Stops the file that is converting now (the engine sees the task
    /// cancellation) and every file still waiting.
    func cancelAll() {
        isCancelled = true
        runTask?.cancel()
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
            let progress = ConversionProgress()
            jobs[index].status = .converting(fraction: 0, stage: progress.stageText)
            // Mirrors `progress` into the job row. Must stop on every exit
            // path: if it outlived a failed conversion it would overwrite
            // `.failed` with `.converting` again, and the batch would never
            // finish.
            let watcher = Task { @MainActor [weak self] in
                while !Task.isCancelled {
                    self?.jobs[index].status = .converting(fraction: progress.fraction, stage: progress.stageText)
                    try? await Task.sleep(nanoseconds: 150_000_000)
                }
            }
            defer { watcher.cancel() }

            do {
                let book = try await BookLoader.load(sourceURL)
                let outputURL = nextAvailableURL(
                    directory: outputDirectory, baseName: sanitizedFilename(book.title),
                    extension: format.fileExtension)
                let resultURL = try await convert(book: book, options: options, outputURL: outputURL, progress: progress)

                jobs[index].status = .done(resultURL)
                historyStore.add(
                    HistoryEntry(
                        title: book.title,
                        conversionLabel: "\(book.sourceURL.pathExtension.uppercased()) → \(format.rawValue)",
                        outputPath: resultURL.path, sourcePath: sourceURL.path))
            } catch {
                let cancelled = isCancelled || error is CancellationError || (error as? EngineError)?.isCancellation == true
                jobs[index].status = .failed(cancelled ? "Cancelled" : error.localizedDescription)
            }
        }
    }

    private func convert(
        book: Book, options: PDFOptions, outputURL: URL, progress: ConversionProgress
    ) async throws -> URL {
        // Every output format now goes through the Rust engine (Phase 6 —
        // see ANYFORM-FULL-SPEC.md). Dispatch is by the output path's
        // extension, on the Rust side, so this Swift call is already
        // fully generic — see RustConversionEngine.swift's doc comment.
        try await RustConversionEngine.convert(book: book, options: options, outputURL: outputURL, progress: progress)
    }
}
