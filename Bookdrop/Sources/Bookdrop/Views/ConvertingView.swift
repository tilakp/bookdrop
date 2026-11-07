import SwiftUI
import AppKit

struct ConvertingView: View {
    let book: Book
    let formatName: String
    @ObservedObject var progress: ConversionProgress
    var onCancel: () -> Void

    var body: some View {
        VStack(spacing: 18) {
            Spacer()

            ZStack {
                Circle()
                    .stroke(Color.primary.opacity(0.08), lineWidth: 7)
                Circle()
                    .trim(from: 0, to: max(progress.fraction, 0.005))
                    .stroke(.tint, style: StrokeStyle(lineWidth: 7, lineCap: .round))
                    .rotationEffect(.degrees(-90))
                Text("\(Int(progress.fraction * 100))%")
                    .font(.system(size: 26, weight: .semibold, design: .rounded))
                    .monospacedDigit()
            }
            .frame(width: 120, height: 120)
            .animation(.easeInOut(duration: 0.3), value: progress.fraction)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel("Conversion progress")
            .accessibilityValue("\(Int(progress.fraction * 100)) percent")

            VStack(spacing: 6) {
                Text("Converting to \(formatName)")
                    .font(.title3.weight(.semibold))
                Text(book.title)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .lineLimit(2)
                    .frame(maxWidth: 420)
                Text(progress.stageText.isEmpty ? " " : progress.stageText)
                    .font(.callout)
                    .foregroundStyle(.tertiary)
                    .monospacedDigit()
            }

            Button("Cancel", action: onCancel)
                .controlSize(.large)
                .keyboardShortcut(.cancelAction)
                .padding(.top, 6)

            Spacer()
        }
        .padding(24)
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
