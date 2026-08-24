import Foundation

enum DefaultOutputLocation: String, CaseIterable, Identifiable, Codable {
    case sameAsSource = "Same folder as source"
    case downloads = "Downloads"
    case askEveryTime = "Ask every time"

    var id: String { rawValue }
}

@MainActor
final class SettingsStore: ObservableObject {
    @Published var defaultOutputLocation: DefaultOutputLocation { didSet { save() } }
    @Published var openConvertedFile: Bool { didSet { save() } }
    @Published var showNotification: Bool { didSet { save() } }
    @Published var revealInFinder: Bool { didSet { save() } }
    /// No effect yet: PDF is the only output format in v1, so there's nothing to
    /// "remember" — revisit once v1.1 adds more output formats.
    @Published var rememberLastOutputFormat: Bool { didSet { save() } }
    @Published var preserveOriginalStylingByDefault: Bool { didSet { save() } }

    private static let key = "BookConvert.Settings"
    private let defaults: UserDefaults

    /// `defaults` is injectable so tests can use a private suite instead of the
    /// user's real UserDefaults.
    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        if let data = defaults.data(forKey: Self.key),
            let stored = try? JSONDecoder().decode(StoredSettings.self, from: data)
        {
            defaultOutputLocation = stored.defaultOutputLocation
            openConvertedFile = stored.openConvertedFile
            showNotification = stored.showNotification
            revealInFinder = stored.revealInFinder
            rememberLastOutputFormat = stored.rememberLastOutputFormat
            preserveOriginalStylingByDefault = stored.preserveOriginalStylingByDefault
        } else {
            defaultOutputLocation = .downloads
            openConvertedFile = true
            showNotification = true
            revealInFinder = false
            rememberLastOutputFormat = true
            preserveOriginalStylingByDefault = true
        }
    }

    func clearTemporaryFiles() {
        let tempDir = FileManager.default.temporaryDirectory.appendingPathComponent("BookConvert", isDirectory: true)
        try? FileManager.default.removeItem(at: tempDir)
    }

    var logsDirectory: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("BookConvert/Logs", isDirectory: true)
    }

    private func save() {
        let stored = StoredSettings(
            defaultOutputLocation: defaultOutputLocation, openConvertedFile: openConvertedFile,
            showNotification: showNotification, revealInFinder: revealInFinder,
            rememberLastOutputFormat: rememberLastOutputFormat,
            preserveOriginalStylingByDefault: preserveOriginalStylingByDefault)
        if let data = try? JSONEncoder().encode(stored) {
            defaults.set(data, forKey: Self.key)
        }
    }

    private struct StoredSettings: Codable {
        var defaultOutputLocation: DefaultOutputLocation
        var openConvertedFile: Bool
        var showNotification: Bool
        var revealInFinder: Bool
        var rememberLastOutputFormat: Bool
        var preserveOriginalStylingByDefault: Bool
    }
}
