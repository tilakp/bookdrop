import SwiftUI
import AppKit

struct ConvertingView: View {
    let bookTitle: String
    @ObservedObject var progress: ConversionProgress
    var onCancel: () -> Void

    var body: some View {
        VStack(spacing: 16) {
            Spacer()

            ZStack {
                Circle()
                    .stroke(Color.brandPurple.opacity(0.15), lineWidth: 8)
                Circle()
                    .trim(from: 0, to: progress.fraction)
                    .stroke(Color.brandPurple, style: StrokeStyle(lineWidth: 8, lineCap: .round))
                    .rotationEffect(.degrees(-90))
                Image(systemName: "book.closed.fill")
                    .font(.system(size: 28))
                    .foregroundStyle(Color.brandPurple)
            }
            .frame(width: 96, height: 96)
            .animation(.easeInOut, value: progress.fraction)

            Text("Converting…")
                .font(.title3.weight(.semibold))
            Text(bookTitle)
                .foregroundStyle(.secondary)

            VStack(spacing: 6) {
                ProgressView(value: progress.fraction)
                    .frame(width: 280)
                    .tint(Color.brandPurple)
                Text("\(Int(progress.fraction * 100))%")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Text(progress.stageText)
                .font(.callout)
                .foregroundStyle(.secondary)

            Button("Cancel", action: onCancel)
                .buttonStyle(.bordered)
                .keyboardShortcut(.escape, modifiers: [])
                .padding(.top, 8)

            Spacer()
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color(nsColor: .windowBackgroundColor))
        .onChange(of: progress.fraction) { fraction in
            NSApp.dockTile.badgeLabel = fraction >= 1.0 ? nil : "\(Int(fraction * 100))%"
            NSApp.dockTile.display()
        }
        .onDisappear {
            NSApp.dockTile.badgeLabel = nil
            NSApp.dockTile.display()
        }
    }
}
