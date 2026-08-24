import SwiftUI
import AppKit

struct MultipleFilesView: View {
    @ObservedObject var model: MultiConversionModel
    let options: PDFOptions
    @Binding var format: OutputFormat
    let historyStore: HistoryStore
    var onDone: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            header
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    statusHeadline

                    VStack(spacing: 0) {
                        ForEach(model.jobs) { job in
                            row(for: job)
                            if job.id != model.jobs.last?.id {
                                Divider()
                            }
                        }
                    }
                    .background(RoundedRectangle(cornerRadius: 8).fill(Color(nsColor: .controlBackgroundColor)))

                    if !model.isRunning && !model.isFinished {
                        HStack {
                            Text("Convert all to")
                            Picker("", selection: $format) {
                                ForEach(OutputFormat.allCases) { format in
                                    Text(format.rawValue).tag(format)
                                }
                            }
                            .labelsHidden()
                            .frame(width: 120)
                        }
                        .font(.callout)

                        HStack {
                            Text("Save to")
                            Spacer()
                            Text(model.outputDirectory.path)
                                .foregroundStyle(.secondary)
                                .lineLimit(1)
                                .truncationMode(.head)
                        }
                        .font(.callout)
                    }
                }
                .padding(20)
            }
            footer
        }
        .background(Color(nsColor: .windowBackgroundColor))
    }

    private var header: some View {
        HStack {
            if !model.isRunning {
                Button(action: onDone) { Image(systemName: "chevron.left") }
                    .buttonStyle(.plain)
            }
            Spacer()
            Text("Bookdrop")
                .font(.headline)
            Spacer()
            if model.isRunning { Color.clear.frame(width: 16) }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
        .background(.bar)
    }

    @ViewBuilder
    private var statusHeadline: some View {
        if model.isFinished && !model.isCancelled {
            Label("\(model.completedCount) books converted", systemImage: "checkmark.circle.fill")
                .font(.title3.weight(.semibold))
                .foregroundStyle(.green)
        } else if model.isRunning {
            let index = min(model.completedCount + 1, model.jobs.count)
            Text("Converting \(index) of \(model.jobs.count)")
                .font(.title3.weight(.semibold))
        } else {
            Text("\(model.jobs.count) books ready to convert")
                .font(.title3.weight(.semibold))
        }
    }

    private func row(for job: MultiConversionModel.Job) -> some View {
        HStack {
            Image(systemName: "book.closed.fill")
                .foregroundStyle(Color.brandPurple)
            Text(job.sourceURL.lastPathComponent)
            Spacer()
            statusView(for: job.status, sourceURL: job.sourceURL)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
    }

    @ViewBuilder
    private func statusView(for status: FileJobStatus, sourceURL: URL) -> some View {
        switch status {
        case .waiting:
            Text(fileSizeDisplay(for: sourceURL))
                .font(.caption)
                .foregroundStyle(.secondary)
        case .converting(let fraction, _):
            ProgressView(value: fraction)
                .frame(width: 100)
        case .done:
            Text("Done")
                .font(.caption)
                .foregroundStyle(.green)
        case .failed(let message):
            Text(message)
                .font(.caption)
                .foregroundStyle(.red)
                .lineLimit(1)
        }
    }

    private func fileSizeDisplay(for url: URL) -> String {
        let attributes = try? FileManager.default.attributesOfItem(atPath: url.path)
        let size = (attributes?[.size] as? Int64) ?? 0
        return ByteCountFormatter.string(fromByteCount: size, countStyle: .file)
    }

    @ViewBuilder
    private var footer: some View {
        HStack {
            Spacer()
            if model.isFinished {
                if model.isCancelled {
                    Button("Close", action: onDone)
                        .buttonStyle(.bordered)
                } else {
                    Button("Open Folder") {
                        NSWorkspace.shared.open(model.outputDirectory)
                    }
                    .buttonStyle(.borderedProminent)
                    .tint(Color.brandPurple)
                }
            } else if model.isRunning {
                Button("Cancel All") { model.cancelAll() }
                    .buttonStyle(.bordered)
            } else {
                Button("Convert All") {
                    Task { await model.run(options: options, format: format, historyStore: historyStore) }
                }
                .buttonStyle(.borderedProminent)
                .tint(Color.brandPurple)
            }
        }
        .padding(16)
        .background(.bar)
    }
}
