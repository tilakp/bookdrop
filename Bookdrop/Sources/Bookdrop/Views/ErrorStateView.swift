import SwiftUI

struct ErrorStateView: View {
    let message: String
    let hint: String?
    let technicalDetails: String?
    var onChooseAnother: () -> Void

    @State private var showDetails = false

    var body: some View {
        VStack(spacing: 18) {
            Spacer()

            Image(systemName: "exclamationmark.triangle.fill")
                .symbolRenderingMode(.multicolor)
                .font(.system(size: 52))
                .accessibilityHidden(true)

            VStack(spacing: 8) {
                Text(message)
                    .font(.title3.weight(.semibold))
                    .multilineTextAlignment(.center)
                if let hint {
                    Text(hint)
                        .foregroundStyle(.secondary)
                        .multilineTextAlignment(.center)
                }
            }
            .frame(maxWidth: 420)
            .textSelection(.enabled)

            Button {
                onChooseAnother()
            } label: {
                Text("Choose Another Book").frame(minWidth: 140)
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
            .keyboardShortcut(.defaultAction)
            .padding(.top, 4)

            if let technicalDetails {
                VStack(spacing: 8) {
                    Button(showDetails ? "Hide Details" : "Show Details") {
                        withAnimation(.easeInOut(duration: 0.2)) { showDetails.toggle() }
                    }
                    .buttonStyle(.plain)
                    .foregroundStyle(.tint)
                    .font(.callout)
                    if showDetails {
                        ScrollView {
                            Text(technicalDetails)
                                .font(.system(.caption, design: .monospaced))
                                .textSelection(.enabled)
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .padding(10)
                        }
                        .frame(maxWidth: 420, maxHeight: 110)
                        .background(
                            RoundedRectangle(cornerRadius: 8, style: .continuous)
                                .fill(Color.primary.opacity(0.05)))
                    }
                }
            }

            Spacer()
        }
        .padding(24)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color(nsColor: .windowBackgroundColor))
    }
}
