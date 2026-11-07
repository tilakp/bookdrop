import SwiftUI
import AppKit

/// The Settings window (Bookdrop › Settings…, ⌘,).
struct SettingsView: View {
    @ObservedObject var settings: SettingsStore

    var body: some View {
        TabView {
            GeneralSettings(settings: settings)
                .tabItem { Label("General", systemImage: "gearshape") }
            ConversionSettings(settings: settings)
                .tabItem { Label("Conversion", systemImage: "arrow.triangle.2.circlepath") }
            AdvancedSettings()
                .tabItem { Label("Advanced", systemImage: "wrench.and.screwdriver") }
        }
        .frame(width: 480)
    }
}

private struct GeneralSettings: View {
    @ObservedObject var settings: SettingsStore

    var body: some View {
        Form {
            Section {
                Picker("Save converted books to", selection: $settings.defaultOutputLocation) {
                    ForEach(DefaultOutputLocation.allCases) { location in
                        Text(location.rawValue).tag(location)
                    }
                }
            }
            Section("After Converting") {
                Toggle("Open the converted file", isOn: $settings.openConvertedFile)
                Toggle("Show a notification", isOn: $settings.showNotification)
                Toggle("Show the file in Finder", isOn: $settings.revealInFinder)
            }
        }
        .formStyle(.grouped)
        .fixedSize(horizontal: false, vertical: true)
    }
}

private struct ConversionSettings: View {
    @ObservedObject var settings: SettingsStore

    var body: some View {
        Form {
            Section {
                Toggle(isOn: $settings.rememberLastOutputFormat) {
                    Text("Remember the last output format")
                    Text("When Bookdrop opens, it selects the format you used last.")
                }
                Toggle(isOn: $settings.preserveOriginalStylingByDefault) {
                    Text("Keep the book's styling by default")
                    Text("Sets the PDF option \u{201C}Keep the book\u{2019}s styling\u{201D} for each new book.")
                }
            }
        }
        .formStyle(.grouped)
        .fixedSize(horizontal: false, vertical: true)
    }
}

private struct AdvancedSettings: View {
    @State private var cleared = false

    var body: some View {
        Form {
            Section {
                LabeledContent {
                    HStack(spacing: 8) {
                        if cleared {
                            Label("Cleared", systemImage: "checkmark")
                                .labelStyle(.titleAndIcon)
                                .foregroundStyle(.secondary)
                                .transition(.opacity)
                        }
                        Button("Clear") {
                            // Keeps the files of the book that is open now.
                            TemporaryFiles.clearLeftovers()
                            withAnimation { cleared = true }
                        }
                    }
                } label: {
                    Text("Temporary files")
                    Text("Working copies that a crash or a forced quit left behind.")
                }
            }
        }
        .formStyle(.grouped)
        .fixedSize(horizontal: false, vertical: true)
    }
}
