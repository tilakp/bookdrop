import SwiftUI
import AppKit

struct MultipleFilesView: View {
    @ObservedObject var model: MultiConversionModel
    let options: PDFOptions
    @Binding var format: OutputFormat
    let conversions: ConversionService
    var onDone: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            Form {
                Section("\(model.jobs.count) Books") {
                    ForEach(model.jobs) { job in
                        JobRow(job: job)
                    }
                }
                if !model.isRunning && !model.isFinished {
                    Section("Output") {
                        Picker("Format", selection: $format) {
                            ForEach(OutputFormat.allCases) { format in
                                Text(format.rawValue).tag(format)
                            }
                        }
                        LabeledContent("Save to") {
                            FolderButton(directory: $model.outputDirectory)
                        }
                    }
                }
            }
            .formStyle(.grouped)

            footer
        }
        .background(Color(nsColor: .windowBackgroundColor))
    }

    private var failedCount: Int {
        model.jobs.filter { if case .failed = $0.status { return true }; return false }.count
    }

    private var statusText: String {
        if model.isFinished {
            if model.isCancelled { return "Cancelled. \(model.completedCount) of \(model.jobs.count) converted." }
            return failedCount == 0
                ? "All \(model.completedCount) books converted."
                : "\(model.completedCount) converted, \(failedCount) failed."
        }
        if model.isRunning {
            let index = min(model.completedCount + failedCount + 1, model.jobs.count)
            return "Converting \(index) of \(model.jobs.count)\u{2026}"
        }
        return "Each book is saved with its title as the file name."
    }

    private var footer: some View {
        VStack(spacing: 0) {
            Divider()
            HStack(spacing: 12) {
                Text(statusText)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                Spacer()
                if model.isFinished {
                    Button("Done", action: onDone)
                    Button("Show in Finder") { NSWorkspace.shared.open(model.outputDirectory) }
                        .buttonStyle(.borderedProminent)
                        .keyboardShortcut(.defaultAction)
                } else if model.isRunning {
                    Button("Cancel All") { model.cancelAll() }
                        .keyboardShortcut(.cancelAction)
                } else {
                    Button {
                        model.start(options: options, format: format, conversions: conversions)
                    } label: {
                        Text("Convert All").frame(minWidth: 80)
                    }
                    .buttonStyle(.borderedProminent)
                    .keyboardShortcut(.defaultAction)
                }
            }
            .controlSize(.large)
            .padding(.horizontal, 20)
            .padding(.vertical, 14)
        }
        .background(.bar)
    }
}

/// One book in the batch: its file, and how far it is.
private struct JobRow: View {
    let job: MultiConversionModel.Job

    var body: some View {
        HStack(spacing: 10) {
            Image(nsImage: NSWorkspace.shared.icon(forFile: job.sourceURL.path))
                .resizable()
                .frame(width: 24, height: 24)
            VStack(alignment: .leading, spacing: 2) {
                Text(job.sourceURL.lastPathComponent)
                    .lineLimit(1)
                    .truncationMode(.middle)
                if case .failed(let message) = job.status {
                    Text(message)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                        .help(message)
                }
            }
            Spacer(minLength: 12)
            status
        }
    }

    @ViewBuilder
    private var status: some View {
        switch job.status {
        case .waiting:
            Text(fileSize)
                .font(.callout)
                .foregroundStyle(.secondary)
                .monospacedDigit()
        case .converting(let fraction, _):
            HStack(spacing: 8) {
                ProgressView(value: fraction)
                    .frame(width: 90)
                Text("\(Int(fraction * 100))%")
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
                    .frame(width: 36, alignment: .trailing)
            }
        case .done(let url):
            HStack(spacing: 6) {
                Button {
                    NSWorkspace.shared.activateFileViewerSelecting([url])
                } label: {
                    Image(systemName: "magnifyingglass")
                }
                .buttonStyle(.borderless)
                .help("Show in Finder")
                Image(systemName: "checkmark.circle.fill")
                    .foregroundStyle(.green)
                    .accessibilityLabel("Converted")
            }
        case .failed:
            Image(systemName: "xmark.octagon.fill")
                .foregroundStyle(.red)
                .accessibilityLabel("Failed")
        }
    }

    private var fileSize: String {
        let attributes = try? FileManager.default.attributesOfItem(atPath: job.sourceURL.path)
        let size = (attributes?[.size] as? Int64) ?? 0
        return ByteCountFormatter.string(fromByteCount: size, countStyle: .file)
    }
}
