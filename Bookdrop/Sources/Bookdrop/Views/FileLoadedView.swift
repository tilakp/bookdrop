import SwiftUI
import AppKit

struct FileLoadedView: View {
    let book: Book
    @Binding var options: PDFOptions
    @Binding var format: OutputFormat
    @Binding var outputDirectory: URL
    @Binding var needsOutputDirectoryPrompt: Bool
    var onConvert: () -> Void

    @State private var showAdvanced = false

    var body: some View {
        VStack(spacing: 0) {
            Form {
                Section {
                    BookHeader(book: book)
                }

                Section("Output") {
                    Picker("Format", selection: $format) {
                        ForEach(OutputFormat.allCases) { format in
                            Text(format.rawValue).tag(format)
                        }
                    }
                    LabeledContent("Save to") {
                        FolderButton(directory: $outputDirectory)
                    }
                }

                if format.supportsCover || format.supportsTableOfContents {
                    Section("Contents") {
                        if format.supportsCover {
                            Toggle("Include cover", isOn: $options.includeCover)
                        }
                        if format.supportsTableOfContents {
                            Toggle("Table of contents", isOn: $options.generateTableOfContents)
                        }
                    }
                }

                if format.hasPdfOptions {
                    pageSection
                    Section {
                        Button {
                            withAnimation(.easeInOut(duration: 0.2)) { showAdvanced.toggle() }
                        } label: {
                            HStack {
                                Text("Typography and Layout")
                                Spacer()
                                Image(systemName: "chevron.right")
                                    .font(.footnote.weight(.semibold))
                                    .foregroundStyle(.secondary)
                                    .rotationEffect(.degrees(showAdvanced ? 90 : 0))
                            }
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .accessibilityLabel(showAdvanced ? "Hide typography and layout" : "Show typography and layout")
                    }
                    if showAdvanced {
                        AdvancedOptionsView(options: $options)
                    }
                }
            }
            .formStyle(.grouped)

            footer
        }
        .background(Color(nsColor: .windowBackgroundColor))
        .onAppear {
            // "Ask every time": ask once per loaded book, not each time
            // this screen appears again.
            if needsOutputDirectoryPrompt {
                needsOutputDirectoryPrompt = false
                if let url = OpenPanels.chooseFolder(startingAt: outputDirectory) { outputDirectory = url }
            }
        }
    }

    private var pageSection: some View {
        Section("Page") {
            Picker("Size", selection: $options.pageSize) {
                ForEach(PageSize.allCases) { size in
                    Text(size.rawValue).tag(size)
                }
            }
            if options.pageSize == .custom {
                inchesField("Width", value: $options.customPageWidthInches)
                inchesField("Height", value: $options.customPageHeightInches)
            }
            Picker("Margins", selection: $options.margins) {
                ForEach(PageMargins.allCases) { margin in
                    Text(margin.rawValue).tag(margin)
                }
            }
            LabeledContent("Orientation") {
                Picker("Orientation", selection: $options.orientation) {
                    ForEach(PageOrientation.allCases) { orientation in
                        Text(orientation.rawValue).tag(orientation)
                    }
                }
                .labelsHidden()
                .pickerStyle(.segmented)
                .fixedSize()
            }
        }
    }

    private func inchesField(_ title: String, value: Binding<Double>) -> some View {
        LabeledContent(title) {
            HStack(spacing: 6) {
                TextField(title, value: value, format: .number)
                    .labelsHidden()
                    .multilineTextAlignment(.trailing)
                    .frame(width: 64)
                Text("in")
                    .foregroundStyle(.secondary)
            }
        }
    }

    private var footer: some View {
        VStack(spacing: 0) {
            Divider()
            HStack(spacing: 12) {
                Text("\(book.sourceURL.pathExtension.uppercased())  \u{2192}  \(format.rawValue)")
                    .font(.callout.weight(.medium))
                    .foregroundStyle(.secondary)
                Spacer()
                Button(action: onConvert) {
                    Text("Convert")
                        .frame(minWidth: 80)
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
                .keyboardShortcut(.defaultAction)
            }
            .padding(.horizontal, 20)
            .padding(.vertical, 14)
        }
        .background(.bar)
    }
}

/// The book being converted: cover, title, author, and file details.
private struct BookHeader: View {
    let book: Book

    var body: some View {
        HStack(alignment: .center, spacing: 16) {
            cover
            VStack(alignment: .leading, spacing: 4) {
                Text(book.title)
                    .font(.title3.weight(.semibold))
                    .lineLimit(2)
                    .textSelection(.enabled)
                if let author = book.author {
                    Text(author)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
                Text(details)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .padding(.top, 2)
            }
            Spacer(minLength: 0)
        }
        .padding(.vertical, 4)
    }

    private var details: String {
        let chapters = "\(book.chapterCount) chapter\(book.chapterCount == 1 ? "" : "s")"
        return [book.sourceURL.pathExtension.uppercased(), book.fileSizeDisplay, chapters].joined(separator: "  \u{00B7}  ")
    }

    @ViewBuilder
    private var cover: some View {
        let shape = RoundedRectangle(cornerRadius: 5, style: .continuous)
        Group {
            if let data = book.coverImage, let image = NSImage(data: data) {
                Image(nsImage: image)
                    .resizable()
                    .aspectRatio(contentMode: .fill)
            } else {
                ZStack {
                    Color.accentColor.opacity(0.12)
                    Image(systemName: "book.closed.fill")
                        .font(.title2)
                        .foregroundStyle(.tint)
                }
            }
        }
        .frame(width: 60, height: 84)
        .clipShape(shape)
        .overlay(shape.strokeBorder(Color.primary.opacity(0.12), lineWidth: 0.5))
        .shadow(color: .black.opacity(0.18), radius: 3, y: 1)
        .accessibilityHidden(true)
    }
}

/// The output folder with its Finder icon, and a button to change it.
struct FolderButton: View {
    @Binding var directory: URL

    var body: some View {
        HStack(spacing: 8) {
            HStack(spacing: 5) {
                Image(nsImage: NSWorkspace.shared.icon(forFile: directory.path))
                    .resizable()
                    .frame(width: 16, height: 16)
                Text(FileManager.default.displayName(atPath: directory.path))
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
            .help(abbreviatedPath)
            Button("Choose\u{2026}") {
                if let url = OpenPanels.chooseFolder(startingAt: directory) { directory = url }
            }
        }
    }

    private var abbreviatedPath: String {
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        let path = directory.path
        return path.hasPrefix(home) ? "~" + path.dropFirst(home.count) : path
    }
}
