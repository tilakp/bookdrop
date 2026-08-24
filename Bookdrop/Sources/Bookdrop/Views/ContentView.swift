import SwiftUI

struct ContentView: View {
    @StateObject private var historyStore: HistoryStore
    @StateObject private var settingsStore: SettingsStore
    @StateObject private var coordinator: AppCoordinator
    @State private var showSettings = false

    init() {
        let history = HistoryStore()
        let settings = SettingsStore()
        _historyStore = StateObject(wrappedValue: history)
        _settingsStore = StateObject(wrappedValue: settings)
        _coordinator = StateObject(wrappedValue: AppCoordinator(historyStore: history, settingsStore: settings))
    }

    var body: some View {
        Group {
            switch coordinator.screen {
            case .empty:
                EmptyStateView(
                    historyStore: historyStore,
                    onFilesSelected: coordinator.handleFilesSelected,
                    onSettings: { showSettings = true },
                    onConvertAgain: { entry in
                        coordinator.handleFilesSelected([URL(fileURLWithPath: entry.sourcePath)])
                    }
                )
            case .loaded(let book):
                FileLoadedView(
                    book: book,
                    options: $coordinator.pdfOptions,
                    format: $coordinator.outputFormat,
                    outputDirectory: $coordinator.outputDirectory,
                    settingsStore: settingsStore,
                    onBack: { coordinator.screen = .empty },
                    onConvert: { coordinator.beginConvert(book: book) },
                    onSettings: { showSettings = true }
                )
            case .duplicateConfirm(let book, let filename):
                DuplicateFileView(filename: filename) { resolution in
                    coordinator.handleDuplicateResolution(resolution, book: book)
                }
            case .converting(let book, let progress):
                ConvertingView(bookTitle: book.title, progress: progress) {
                    coordinator.cancelConversion(progress: progress)
                }
            case .complete(let info):
                CompleteView(info: info) { coordinator.screen = .empty }
            case .error(let message, let hint, let details):
                ErrorStateView(
                    message: message, hint: hint, technicalDetails: details,
                    onTryAgain: { coordinator.screen = .empty })
            case .multipleFiles(let model):
                MultipleFilesView(
                    model: model, options: coordinator.pdfOptions, format: $coordinator.outputFormat,
                    historyStore: historyStore,
                    onDone: { coordinator.screen = .empty })
            }
        }
        .sheet(isPresented: $showSettings) {
            SettingsView(settings: settingsStore, onClose: { showSettings = false })
        }
    }
}
