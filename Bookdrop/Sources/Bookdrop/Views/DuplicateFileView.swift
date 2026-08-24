import SwiftUI

enum DuplicateResolution {
    case replace
    case keepBoth
    case cancel
}

struct DuplicateFileView: View {
    let filename: String
    var onResolve: (DuplicateResolution) -> Void

    @State private var selection: DuplicateResolution = .keepBoth

    var body: some View {
        VStack(spacing: 16) {
            Spacer()

            Image(systemName: "exclamationmark.triangle.fill")
                .font(.system(size: 40))
                .foregroundStyle(.yellow)

            Text("A file named \u{201C}\(filename)\u{201D}\nalready exists.")
                .multilineTextAlignment(.center)
                .font(.body.weight(.medium))

            VStack(alignment: .leading, spacing: 8) {
                radioRow("Replace", .replace)
                radioRow("Keep Both", .keepBoth)
                radioRow("Cancel", .cancel)
            }

            HStack {
                Button("Cancel") { onResolve(.cancel) }
                    .buttonStyle(.bordered)
                Button("Continue") { onResolve(selection) }
                    .buttonStyle(.borderedProminent)
                    .tint(Color.brandPurple)
            }
            .padding(.top, 8)

            Spacer()
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color(nsColor: .windowBackgroundColor))
    }

    private func radioRow(_ title: String, _ value: DuplicateResolution) -> some View {
        HStack {
            Image(systemName: selection == value ? "largecircle.fill.circle" : "circle")
                .foregroundStyle(selection == value ? Color.brandPurple : .secondary)
            Text(title)
        }
        .contentShape(Rectangle())
        .onTapGesture { selection = value }
    }
}
