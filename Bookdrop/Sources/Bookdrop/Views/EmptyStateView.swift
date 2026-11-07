import SwiftUI
import AppKit
import UniformTypeIdentifiers

struct EmptyStateView: View {
    @ObservedObject var historyStore: HistoryStore
    let onFilesSelected: ([URL]) -> Void
    let onChooseFiles: () -> Void
    let onConvertAgain: (HistoryEntry) -> Void

    @State private var isTargeted = false
    @State private var confirmClear = false

    var body: some View {
        VStack(spacing: 0) {
            if historyStore.entries.isEmpty {
                dropZone
                    .padding(24)
            } else {
                dropZone
                    .frame(height: 240)
                    .padding([.horizontal, .top], 24)
                    .padding(.bottom, 20)
                recentConversions
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color(nsColor: .windowBackgroundColor))
        .confirmationDialog(
            "Clear the list of recent conversions?", isPresented: $confirmClear,
            actions: { Button("Clear List", role: .destructive) { historyStore.clear() } },
            message: { Text("The converted files stay where they are.") })
    }

    private var dropZone: some View {
        VStack(spacing: 14) {
            Image(systemName: isTargeted ? "arrow.down.doc.fill" : "arrow.down.doc")
                .font(.system(size: 42, weight: .light))
                .foregroundStyle(.tint)
                .frame(height: 50)

            VStack(spacing: 4) {
                Text(isTargeted ? "Release to open" : "Drop a book here")
                    .font(.title3.weight(.semibold))
                Text(BookLoader.supportedFormatsList(conjunction: "or"))
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }

            Button("Choose Files\u{2026}", action: onChooseFiles)
                .controlSize(.large)
                .buttonStyle(.borderedProminent)
                .padding(.top, 4)
        }
        .padding(24)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .fill(isTargeted ? Color.accentColor.opacity(0.08) : Color.primary.opacity(0.025))
        )
        .overlay(
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .strokeBorder(
                    isTargeted ? AnyShapeStyle(.tint) : AnyShapeStyle(Color.secondary.opacity(0.35)),
                    style: StrokeStyle(lineWidth: isTargeted ? 2 : 1.5, dash: isTargeted ? [] : [7, 5]))
        )
        .animation(.easeOut(duration: 0.15), value: isTargeted)
        .onDrop(of: [.fileURL], isTargeted: $isTargeted, perform: handleDrop)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Drop zone")
        .accessibilityHint("Drag a book here, or choose files")
    }

    private var recentConversions: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(alignment: .firstTextBaseline) {
                Text("Recent Conversions")
                    .font(.headline)
                Spacer()
                Button("Clear\u{2026}") { confirmClear = true }
                    .buttonStyle(.plain)
                    .foregroundStyle(.tint)
                    .font(.callout)
            }
            .padding(.horizontal, 24)

            ScrollView {
                LazyVStack(spacing: 2) {
                    ForEach(historyStore.entries) { entry in
                        HistoryRow(
                            entry: entry,
                            onConvertAgain: { onConvertAgain(entry) },
                            onRemove: { historyStore.remove(entry) })
                    }
                }
                .padding(.horizontal, 16)
                .padding(.bottom, 16)
            }
        }
    }

    private func handleDrop(providers: [NSItemProvider]) -> Bool {
        let fileProviders = providers.filter { $0.hasItemConformingToTypeIdentifier(UTType.fileURL.identifier) }
        guard !fileProviders.isEmpty else { return false }

        let collected = NSMutableArray()
        let group = DispatchGroup()
        for provider in fileProviders {
            group.enter()
            provider.loadItem(forTypeIdentifier: UTType.fileURL.identifier) { item, _ in
                // Unsupported files are passed on too, so the app can say
                // why it will not open them instead of doing nothing.
                if let data = item as? Data, let url = URL(dataRepresentation: data, relativeTo: nil) {
                    collected.add(url)
                }
                group.leave()
            }
        }
        group.notify(queue: .main) {
            let urls = collected.compactMap { $0 as? URL }
            if !urls.isEmpty { onFilesSelected(urls) }
        }
        return true
    }
}

/// One past conversion. Double-click opens the file; Open and Show in
/// Finder appear on hover; the context menu has every action.
private struct HistoryRow: View {
    let entry: HistoryEntry
    let onConvertAgain: () -> Void
    let onRemove: () -> Void

    @State private var hovering = false

    private var outputURL: URL { URL(fileURLWithPath: entry.outputPath) }
    private var outputExists: Bool { FileManager.default.fileExists(atPath: entry.outputPath) }

    var body: some View {
        HStack(spacing: 12) {
            Image(nsImage: icon)
                .resizable()
                .frame(width: 32, height: 32)
                .opacity(outputExists ? 1 : 0.4)

            VStack(alignment: .leading, spacing: 2) {
                Text(entry.title)
                    .lineLimit(1)
                    .truncationMode(.tail)
                    .foregroundStyle(outputExists ? .primary : .secondary)
                Text(subtitle)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }

            Spacer(minLength: 8)

            if hovering && outputExists {
                HStack(spacing: 2) {
                    rowButton("Open", systemImage: "arrow.up.forward.app") { NSWorkspace.shared.open(outputURL) }
                    rowButton("Show in Finder", systemImage: "magnifyingglass") {
                        NSWorkspace.shared.activateFileViewerSelecting([outputURL])
                    }
                }
            }
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 6)
        .background(
            RoundedRectangle(cornerRadius: 8, style: .continuous)
                .fill(hovering ? Color.primary.opacity(0.06) : .clear)
        )
        .contentShape(Rectangle())
        .onHover { hovering = $0 }
        .onTapGesture(count: 2) { if outputExists { NSWorkspace.shared.open(outputURL) } }
        .contextMenu {
            Button("Open") { NSWorkspace.shared.open(outputURL) }
                .disabled(!outputExists)
            Button("Show in Finder") { NSWorkspace.shared.activateFileViewerSelecting([outputURL]) }
                .disabled(!outputExists)
            Button("Convert Again") { onConvertAgain() }
                .disabled(!FileManager.default.fileExists(atPath: entry.sourcePath))
            Divider()
            Button("Remove from List", action: onRemove)
        }
        .help(entry.outputPath)
    }

    private var subtitle: String {
        let when = entry.date.formatted(.relative(presentation: .named))
        let status = outputExists ? when : "Moved or deleted"
        return "\(entry.conversionLabel)  \u{00B7}  \(status)"
    }

    /// The file's own Finder icon, or the icon for its type if it is gone.
    private var icon: NSImage {
        if outputExists { return NSWorkspace.shared.icon(forFile: entry.outputPath) }
        let type = UTType(filenameExtension: outputURL.pathExtension) ?? .data
        return NSWorkspace.shared.icon(for: type)
    }

    private func rowButton(_ title: String, systemImage: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: systemImage)
                .frame(width: 24, height: 24)
        }
        .buttonStyle(.borderless)
        .help(title)
        .accessibilityLabel(title)
    }
}
