import SwiftUI
import AppKit

struct ContentView: View {
    @ObservedObject var coordinator: AppCoordinator
    @ObservedObject var historyStore: HistoryStore

    var body: some View {
        screen
            .navigationTitle("Bookdrop")
            .toolbar {
                ToolbarItem(placement: .navigation) { backButton }
                ToolbarItem(placement: .primaryAction) { SettingsButton() }
            }
            .alert(
                duplicateTitle, isPresented: duplicatePromptShown,
                actions: {
                    Button("Keep Both") { resolveDuplicate(.keepBoth) }
                        .keyboardShortcut(.defaultAction)
                    Button("Replace", role: .destructive) { resolveDuplicate(.replace) }
                    Button("Cancel", role: .cancel) { resolveDuplicate(.cancel) }
                },
                message: { Text("Do you want to replace it, or keep both files?") })
    }

    @ViewBuilder
    private var screen: some View {
        switch coordinator.screen {
        case .empty:
            EmptyStateView(
                historyStore: historyStore,
                onFilesSelected: { coordinator.handleFilesSelected($0) },
                onChooseFiles: { coordinator.chooseFiles() },
                onConvertAgain: { entry in
                    coordinator.handleFilesSelected([URL(fileURLWithPath: entry.sourcePath)])
                })
        case .loading(let filename):
            ProgressView("Opening \u{201C}\(filename)\u{201D}\u{2026}")
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        case .loaded(let book), .duplicateConfirm(let book, _):
            FileLoadedView(
                book: book,
                options: $coordinator.pdfOptions,
                format: $coordinator.outputFormat,
                outputDirectory: $coordinator.outputDirectory,
                needsOutputDirectoryPrompt: $coordinator.needsOutputDirectoryPrompt,
                onConvert: { coordinator.beginConvert(book: book) })
        case .converting(let book, let progress):
            ConvertingView(book: book, formatName: coordinator.outputFormat.rawValue, progress: progress) {
                coordinator.cancelConversion(progress: progress)
            }
        case .complete(let info):
            CompleteView(info: info) { coordinator.screen = .empty }
        case .error(let message, let hint, let details):
            ErrorStateView(
                message: message, hint: hint, technicalDetails: details,
                onChooseAnother: { coordinator.screen = .empty })
        case .multipleFiles(let model):
            MultipleFilesView(
                model: model, options: coordinator.pdfOptions, format: $coordinator.outputFormat,
                conversions: coordinator.conversions,
                onDone: { coordinator.screen = .empty })
        }
    }

    // MARK: - Toolbar

    @ViewBuilder
    private var backButton: some View {
        switch coordinator.screen {
        case .loaded, .duplicateConfirm, .complete, .error:
            BackButton { coordinator.screen = .empty }
        case .multipleFiles(let model):
            BatchBackButton(model: model) { coordinator.screen = .empty }
        case .empty, .loading, .converting:
            EmptyView()
        }
    }

    // MARK: - Duplicate file alert

    private var duplicateTitle: String {
        guard case .duplicateConfirm(_, let filename) = coordinator.screen else { return "" }
        return "\u{201C}\(filename)\u{201D} already exists in this folder."
    }

    private var duplicatePromptShown: Binding<Bool> {
        Binding(
            get: { if case .duplicateConfirm = coordinator.screen { return true } else { return false } },
            set: { shown in
                // Escape or a click outside closes the alert: that is Cancel.
                if !shown, case .duplicateConfirm = coordinator.screen { resolveDuplicate(.cancel) }
            })
    }

    private func resolveDuplicate(_ resolution: DuplicateResolution) {
        guard case .duplicateConfirm(let book, _) = coordinator.screen else { return }
        coordinator.handleDuplicateResolution(resolution, book: book)
    }
}

private struct BackButton: View {
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Label("Back", systemImage: "chevron.left")
        }
        .help("Back")
    }
}

/// Back is hidden while a batch runs: leaving would hide its progress.
private struct BatchBackButton: View {
    @ObservedObject var model: MultiConversionModel
    let action: () -> Void

    var body: some View {
        if !model.isRunning {
            BackButton(action: action)
        }
    }
}

/// Opens the Settings window, like the app menu's Settings… item.
private struct SettingsButton: View {
    var body: some View {
        if #available(macOS 14, *) {
            SettingsLink {
                Label("Settings", systemImage: "gearshape")
            }
            .help("Settings")
        } else {
            Button {
                NSApp.sendAction(Selector(("showSettingsWindow:")), to: nil, from: nil)
            } label: {
                Label("Settings", systemImage: "gearshape")
            }
            .help("Settings")
        }
    }
}
