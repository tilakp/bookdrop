import SwiftUI

struct AdvancedOptionsView: View {
    @Binding var options: PDFOptions

    private let fontChoices = ["Original", "Georgia", "Helvetica", "Times New Roman", "Avenir"]

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Advanced PDF Options")
                .font(.subheadline.weight(.semibold))
                .foregroundStyle(.secondary)

            section("Typography") {
                LabeledContent("Font") {
                    Picker("", selection: $options.fontFamily) {
                        ForEach(fontChoices, id: \.self) { Text($0).tag($0) }
                    }
                    .labelsHidden()
                    .frame(width: 160)
                }
                LabeledContent("Font size") {
                    Stepper(
                        "\(Int(options.fontSizePt)) pt",
                        value: $options.fontSizePt, in: 8...24, step: 1
                    )
                    .frame(width: 160)
                }
                LabeledContent("Line spacing") {
                    Picker("", selection: $options.lineSpacing) {
                        ForEach([1.0, 1.2, 1.5, 2.0], id: \.self) { Text(String(format: "%.1f", $0)).tag($0) }
                    }
                    .labelsHidden()
                    .frame(width: 160)
                }
            }

            section("Layout") {
                Toggle("Start chapters on new page", isOn: $options.startChaptersOnNewPage)
                Toggle("Preserve EPUB styling", isOn: $options.preserveEpubStyling)
                Toggle("Remove publisher styling", isOn: $options.removePublisherStyling)
            }

            section("Pages") {
                Toggle("Show page numbers", isOn: $options.showPageNumbers)
                Toggle("Include headers", isOn: $options.includeHeaders)
                Toggle("Include footers", isOn: $options.includeFooters)
            }
        }
        .padding(16)
        .background(RoundedRectangle(cornerRadius: 10).fill(Color(nsColor: .controlBackgroundColor)))
    }

    @ViewBuilder
    private func section<Content: View>(_ title: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(title)
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)
            content()
        }
    }
}
