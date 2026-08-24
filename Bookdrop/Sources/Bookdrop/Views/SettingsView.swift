import SwiftUI
import AppKit

private enum SettingsTab: String, CaseIterable, Identifiable {
    case general = "General"
    case conversion = "Conversion"
    case advanced = "Advanced"
    case about = "About"
    var id: String { rawValue }
}

struct SettingsView: View {
    @ObservedObject var settings: SettingsStore
    var onClose: () -> Void

    @State private var tab: SettingsTab = .general

    var body: some View {
        HStack(spacing: 0) {
            List(SettingsTab.allCases, selection: $tab) { t in
                Text(t.rawValue).tag(t)
            }
            .listStyle(.sidebar)
            .frame(width: 140)

            Divider()

            VStack(alignment: .leading, spacing: 0) {
                ScrollView {
                    content
                        .padding(20)
                }
                Divider()
                HStack {
                    Spacer()
                    Button("Done", action: onClose)
                        .buttonStyle(.borderedProminent)
                        .tint(Color.brandPurple)
                }
                .padding(12)
            }
        }
        .frame(width: 520, height: 380)
    }

    @ViewBuilder
    private var content: some View {
        switch tab {
        case .general: generalSection
        case .conversion: conversionSection
        case .advanced: advancedSection
        case .about: aboutSection
        }
    }

    private var generalSection: some View {
        VStack(alignment: .leading, spacing: 16) {
            sectionTitle("General")

            LabeledContent("Default output location") {
                Picker("", selection: $settings.defaultOutputLocation) {
                    ForEach(DefaultOutputLocation.allCases) { location in
                        Text(location.rawValue).tag(location)
                    }
                }
                .labelsHidden()
            }

            VStack(alignment: .leading, spacing: 8) {
                Text("After conversion").font(.subheadline.weight(.semibold)).foregroundStyle(.secondary)
                Toggle("Open converted file", isOn: $settings.openConvertedFile)
                Toggle("Show notification", isOn: $settings.showNotification)
                Toggle("Reveal in Finder", isOn: $settings.revealInFinder)
            }
        }
    }

    private var conversionSection: some View {
        VStack(alignment: .leading, spacing: 16) {
            sectionTitle("Conversion")
            VStack(alignment: .leading, spacing: 2) {
                Toggle("Remember last output format", isOn: $settings.rememberLastOutputFormat)
                Text("No effect yet — PDF is the only output format in v1.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Toggle("Preserve original styling by default", isOn: $settings.preserveOriginalStylingByDefault)
        }
    }

    private var advancedSection: some View {
        VStack(alignment: .leading, spacing: 16) {
            sectionTitle("Advanced")

            VStack(alignment: .leading, spacing: 8) {
                Text("Temporary files").font(.subheadline.weight(.semibold)).foregroundStyle(.secondary)
                Button("Clear Temporary Files") { settings.clearTemporaryFiles() }
            }

            VStack(alignment: .leading, spacing: 8) {
                Text("Logs").font(.subheadline.weight(.semibold)).foregroundStyle(.secondary)
                Button("Open Logs Folder") {
                    try? FileManager.default.createDirectory(
                        at: settings.logsDirectory, withIntermediateDirectories: true)
                    NSWorkspace.shared.open(settings.logsDirectory)
                }
            }
        }
    }

    private var aboutSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            sectionTitle("About")
            Text("Bookdrop")
                .font(.title3.weight(.semibold))
            Text("Convert books easily.")
                .foregroundStyle(.secondary)
        }
    }

    private func sectionTitle(_ text: String) -> some View {
        Text(text)
            .font(.headline)
    }
}
