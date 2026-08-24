import SwiftUI
import AppKit
import UniformTypeIdentifiers

struct FileLoadedView: View {
    let book: Book
    @Binding var options: PDFOptions
    @Binding var outputDirectory: URL
    @ObservedObject var settingsStore: SettingsStore
    var onBack: () -> Void
    var onConvert: () -> Void
    var onSettings: () -> Void

    @State private var showAdvanced = false

    var body: some View {
        VStack(spacing: 0) {
            header
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    bookRow
                    outputSection
                    pdfOptionsSection
                    if showAdvanced {
                        AdvancedOptionsView(options: $options)
                    }
                }
                .padding(20)
            }
            footer
        }
        .background(Color(nsColor: .windowBackgroundColor))
        .onAppear {
            // "Ask every time" means don't silently default the save location —
            // prompt for it as soon as the book loads.
            if settingsStore.defaultOutputLocation == .askEveryTime {
                chooseOutputDirectory()
            }
        }
    }

    private var header: some View {
        HStack {
            Button(action: onBack) {
                Image(systemName: "chevron.left")
            }
            .buttonStyle(.plain)
            Spacer()
            Text("Bookdrop")
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

    private var bookRow: some View {
        HStack(alignment: .top, spacing: 16) {
            coverThumbnail
            VStack(alignment: .leading, spacing: 4) {
                Text(book.title)
                    .font(.title3.weight(.semibold))
                if let author = book.author {
                    Text(author)
                        .foregroundStyle(.secondary)
                }
                Text("\(book.fileSizeDisplay) · \(book.chapterCount) chapter\(book.chapterCount == 1 ? "" : "s")")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
            Spacer()
        }
    }

    @ViewBuilder
    private var coverThumbnail: some View {
        let shape = RoundedRectangle(cornerRadius: 6)
        if let data = book.coverImage, let image = NSImage(data: data) {
            Image(nsImage: image)
                .resizable()
                .aspectRatio(contentMode: .fill)
                .frame(width: 64, height: 88)
                .clipShape(shape)
                .overlay(shape.strokeBorder(Color.black.opacity(0.1)))
        } else {
            shape
                .fill(Color.brandPurple.opacity(0.12))
                .frame(width: 64, height: 88)
                .overlay(
                    Image(systemName: "book.closed.fill")
                        .foregroundStyle(Color.brandPurple)
                )
        }
    }

    private var outputSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Output")
                .font(.subheadline.weight(.semibold))
                .foregroundStyle(.secondary)

            LabeledContent("Format") {
                Picker("", selection: .constant(0)) {
                    Text("PDF").tag(0)
                }
                .labelsHidden()
                .frame(width: 160)
            }

            LabeledContent("Save to") {
                HStack {
                    Text(outputDirectory.path)
                        .lineLimit(1)
                        .truncationMode(.head)
                        .foregroundStyle(.secondary)
                    Button("Choose…", action: chooseOutputDirectory)
                }
            }
        }
    }

    private var pdfOptionsSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("PDF Options")
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(.secondary)
                Spacer()
                Button(showAdvanced ? "Hide Advanced" : "Advanced…") {
                    withAnimation { showAdvanced.toggle() }
                }
                .buttonStyle(.plain)
                .foregroundStyle(Color.brandPurple)
            }

            LabeledContent("Page size") {
                Picker("", selection: $options.pageSize) {
                    ForEach(PageSize.allCases) { size in
                        Text(size.rawValue).tag(size)
                    }
                }
                .labelsHidden()
                .frame(width: 160)
            }

            LabeledContent("Margins") {
                Picker("", selection: $options.margins) {
                    ForEach(PageMargins.allCases) { margin in
                        Text(margin.rawValue).tag(margin)
                    }
                }
                .labelsHidden()
                .frame(width: 160)
            }

            LabeledContent("Orientation") {
                Picker("", selection: $options.orientation) {
                    ForEach(PageOrientation.allCases) { orientation in
                        Text(orientation.rawValue).tag(orientation)
                    }
                }
                .labelsHidden()
                .frame(width: 160)
            }

            Toggle("Include cover", isOn: $options.includeCover)
            Toggle("Generate table of contents", isOn: $options.generateTableOfContents)
        }
    }

    private var footer: some View {
        HStack {
            Spacer()
            Button("Convert", action: onConvert)
                .buttonStyle(.borderedProminent)
                .tint(Color.brandPurple)
                .controlSize(.large)
                .keyboardShortcut(.return, modifiers: .command)
        }
        .padding(16)
        .background(.bar)
    }

    private func chooseOutputDirectory() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        panel.directoryURL = outputDirectory
        if panel.runModal() == .OK, let url = panel.url {
            outputDirectory = url
        }
    }
}
