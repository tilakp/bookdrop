import SwiftUI
import AppKit

struct CompleteView: View {
    let info: CompletionInfo
    var onConvertAnother: () -> Void

    var body: some View {
        VStack(spacing: 18) {
            Spacer()

            Image(nsImage: NSWorkspace.shared.icon(forFile: info.outputURL.path))
                .resizable()
                .frame(width: 96, height: 96)
                .overlay(alignment: .bottomTrailing) {
                    Image(systemName: "checkmark.circle.fill")
                        .symbolRenderingMode(.palette)
                        .foregroundStyle(.white, .green)
                        .font(.system(size: 30))
                        .background(Circle().fill(Color(nsColor: .windowBackgroundColor)).padding(2))
                        .offset(x: 6, y: 4)
                }
                .accessibilityHidden(true)

            VStack(spacing: 6) {
                Text("Conversion Complete")
                    .font(.title3.weight(.semibold))
                Text(info.outputURL.lastPathComponent)
                    .multilineTextAlignment(.center)
                    .lineLimit(2)
                    .truncationMode(.middle)
                    .textSelection(.enabled)
                    .frame(maxWidth: 440)
                Text(summaryText)
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }

            HStack(spacing: 12) {
                Button("Show in Finder") {
                    NSWorkspace.shared.activateFileViewerSelecting([info.outputURL])
                }
                Button {
                    NSWorkspace.shared.open(info.outputURL)
                } label: {
                    Text("Open").frame(minWidth: 70)
                }
                .buttonStyle(.borderedProminent)
                .keyboardShortcut(.defaultAction)
            }
            .controlSize(.large)
            .padding(.top, 4)

            Button("Convert Another Book", action: onConvertAnother)
                .buttonStyle(.plain)
                .foregroundStyle(.tint)

            Spacer()
        }
        .padding(24)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color(nsColor: .windowBackgroundColor))
    }

    private var summaryText: String {
        if let pageCount = info.pageCount {
            return "\(pageCount) page\(pageCount == 1 ? "" : "s")  \u{00B7}  \(info.fileSizeDisplay)"
        }
        return info.fileSizeDisplay
    }
}
