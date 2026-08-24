import SwiftUI
import AppKit

struct CompleteView: View {
    let info: CompletionInfo
    var onConvertAnother: () -> Void

    var body: some View {
        VStack(spacing: 16) {
            Spacer()

            Image(systemName: "checkmark.circle.fill")
                .font(.system(size: 48))
                .foregroundStyle(.green)

            Text("Conversion complete")
                .font(.title3.weight(.semibold))

            Text(info.outputURL.lastPathComponent)
                .foregroundStyle(.secondary)

            Text("\(info.fileSizeDisplay) · \(info.pageCount) page\(info.pageCount == 1 ? "" : "s")")
                .font(.callout)
                .foregroundStyle(.secondary)

            HStack(spacing: 12) {
                Button("Show in Finder") {
                    NSWorkspace.shared.activateFileViewerSelecting([info.outputURL])
                }
                .buttonStyle(.bordered)

                Button("Open PDF") {
                    NSWorkspace.shared.open(info.outputURL)
                }
                .buttonStyle(.borderedProminent)
                .tint(Color.brandPurple)
            }
            .padding(.top, 4)

            Button("Convert Another", action: onConvertAnother)
                .buttonStyle(.plain)
                .foregroundStyle(Color.brandPurple)
                .padding(.top, 4)

            Spacer()
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color(nsColor: .windowBackgroundColor))
    }
}
