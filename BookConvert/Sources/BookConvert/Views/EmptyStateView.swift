import SwiftUI
import AppKit
import UniformTypeIdentifiers

struct EmptyStateView: View {
    @ObservedObject var historyStore: HistoryStore
    let onFilesSelected: ([URL]) -> Void
    let onSettings: () -> Void
    let onConvertAgain: (HistoryEntry) -> Void

    @State private var isTargeted = false

    private static let epubType = UTType(filenameExtension: "epub") ?? .data

    var body: some View {
        VStack(spacing: 0) {
            header
            VStack(spacing: 24) {
                Text("Convert your book")
                    .font(.title2.weight(.semibold))
                    .padding(.top, 24)

                dropZone
                    .padding(.horizontal, 40)
                    .frame(maxHeight: .infinity)

                if !historyStore.entries.isEmpty {
                    recentConversions
                        .padding(.horizontal, 40)
                        .padding(.bottom, 24)
                }
            }
        }
        .background(Color(nsColor: .windowBackgroundColor))
    }

    private var header: some View {
        HStack {
            Text("BookConvert")
                .font(.headline)
            Spacer()
            Button(action: onSettings) {
                Image(systemName: "gearshape")
            }
            .buttonStyle(.plain)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
        .background(.bar)
    }

    private var dropZone: some View {
        VStack(spacing: 12) {
            Image(systemName: "arrow.down.doc")
                .font(.system(size: 32))
                .foregroundStyle(Color.brandPurple)

            Text(isTargeted ? "Drop to convert" : "Drop an ebook here")
                .font(.body.weight(.medium))

            HStack(spacing: 4) {
                Text("or")
                    .foregroundStyle(.secondary)
                Button("Choose File…", action: presentOpenPanel)
                    .buttonStyle(.plain)
                    .foregroundStyle(Color.brandPurple)
                    .keyboardShortcut("o", modifiers: .command)
                    .onHover { inside in
                        if inside { NSCursor.pointingHand.push() } else { NSCursor.pop() }
                    }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(.vertical, 48)
        .background(
            RoundedRectangle(cornerRadius: 12)
                .fill(isTargeted ? Color.brandPurple.opacity(0.08) : Color(nsColor: .controlBackgroundColor))
        )
        .overlay(
            RoundedRectangle(cornerRadius: 12)
                .strokeBorder(style: StrokeStyle(lineWidth: 1.5, dash: [6, 4]))
                .foregroundStyle(isTargeted ? Color.brandPurple : Color.secondary.opacity(0.4))
        )
        .onDrop(of: [.fileURL], isTargeted: $isTargeted, perform: handleDrop)
        .accessibilityLabel("Ebook drop zone")
        .accessibilityHint("Drag an EPUB file here, or use the Choose File button below")
    }

    private var recentConversions: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text("Recent conversions")
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(.secondary)
                Spacer()
                if !historyStore.entries.isEmpty {
                    Button("Clear") { historyStore.clear() }
                        .buttonStyle(.plain)
                        .font(.caption)
                        .foregroundStyle(Color.brandPurple)
                }
            }

            ForEach(historyStore.entries) { entry in
                HStack {
                    Image(systemName: "book.closed.fill")
                        .foregroundStyle(Color.brandPurple)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(entry.title)
                        Text(entry.conversionLabel)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    Spacer()
                    Text(entry.date, style: .relative)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .padding(.vertical, 4)
                .contentShape(Rectangle())
                .contextMenu {
                    Button("Open") { NSWorkspace.shared.open(URL(fileURLWithPath: entry.outputPath)) }
                    Button("Show in Finder") {
                        NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: entry.outputPath)])
                    }
                    Button("Convert Again") { onConvertAgain(entry) }
                    Divider()
                    Button("Remove from History") { historyStore.remove(entry) }
                }
            }
        }
    }

    private func handleDrop(providers: [NSItemProvider]) -> Bool {
        let epubProviders = providers.filter { $0.hasItemConformingToTypeIdentifier(UTType.fileURL.identifier) }
        guard !epubProviders.isEmpty else { return false }

        let collected = NSMutableArray()
        let group = DispatchGroup()
        for provider in epubProviders {
            group.enter()
            provider.loadItem(forTypeIdentifier: UTType.fileURL.identifier) { item, _ in
                if let data = item as? Data,
                    let url = URL(dataRepresentation: data, relativeTo: nil),
                    url.pathExtension.lowercased() == "epub"
                {
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

    private func presentOpenPanel() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [Self.epubType]
        panel.allowsMultipleSelection = true
        panel.canChooseDirectories = false
        if panel.runModal() == .OK, !panel.urls.isEmpty {
            onFilesSelected(panel.urls)
        }
    }
}
