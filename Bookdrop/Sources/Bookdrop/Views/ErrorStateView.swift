import SwiftUI

struct ErrorStateView: View {
    let message: String
    let hint: String?
    let technicalDetails: String?
    var onTryAgain: () -> Void

    @State private var showDetails = false

    var body: some View {
        VStack(spacing: 16) {
            Spacer()

            Image(systemName: "xmark.circle.fill")
                .font(.system(size: 48))
                .foregroundStyle(.red)

            Text("Couldn't convert this book")
                .font(.title3.weight(.semibold))

            VStack(spacing: 4) {
                Text(message)
                    .multilineTextAlignment(.center)
                if let hint {
                    Text(hint)
                        .multilineTextAlignment(.center)
                }
            }
            .foregroundStyle(.secondary)
            .frame(maxWidth: 380)

            Button("Try Again", action: onTryAgain)
                .buttonStyle(.borderedProminent)
                .tint(Color.brandPurple)

            if let technicalDetails {
                DisclosureGroup("Show Technical Details", isExpanded: $showDetails) {
                    ScrollView {
                        Text(technicalDetails)
                            .font(.system(.caption, design: .monospaced))
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(8)
                    }
                    .frame(maxHeight: 120)
                    .background(RoundedRectangle(cornerRadius: 6).fill(Color(nsColor: .controlBackgroundColor)))
                }
                .frame(maxWidth: 380)
            }

            Spacer()
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color(nsColor: .windowBackgroundColor))
    }
}
