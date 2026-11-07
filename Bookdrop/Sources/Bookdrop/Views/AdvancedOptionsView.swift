import SwiftUI

/// The typography and layout options of PDF output, as form sections.
struct AdvancedOptionsView: View {
    @Binding var options: PDFOptions

    private let fontChoices = ["Original", "Georgia", "Helvetica", "Times New Roman", "Avenir"]
    private let lineSpacings: [Double] = [1.0, 1.2, 1.5, 2.0]

    var body: some View {
        Section("Typography") {
            Picker("Font", selection: $options.fontFamily) {
                ForEach(fontChoices, id: \.self) { name in
                    Text(name == "Original" ? "Book's own font" : name).tag(name)
                }
            }
            LabeledContent("Font size") {
                HStack(spacing: 6) {
                    Text("\(Int(options.fontSizePt)) pt")
                        .monospacedDigit()
                    Stepper("Font size", value: $options.fontSizePt, in: 8...24, step: 1)
                        .labelsHidden()
                }
            }
            Picker("Line spacing", selection: $options.lineSpacing) {
                ForEach(lineSpacings, id: \.self) { spacing in
                    Text(String(format: "%.1f", spacing)).tag(spacing)
                }
            }
        }

        Section("Styling") {
            Toggle(isOn: $options.preserveEpubStyling) {
                Text("Keep the book's styling")
                Text("Use the colors, spacing and layout the book defines.")
            }
            Toggle(isOn: $options.removePublisherStyling) {
                Text("Remove publisher styling")
                Text("Remove all of the book's styles, even when its styling is kept.")
            }
        }

        Section("Page Decorations") {
            Toggle("Page numbers", isOn: $options.showPageNumbers)
            Toggle("Book title at the top of each page", isOn: $options.includeHeaders)
            Toggle("Book title at the bottom of each page", isOn: $options.includeFooters)
        }
    }
}
