import Foundation

/// Narrow key-value storage abstraction so `SettingsStore` doesn't have to talk
/// to real `UserDefaults` in tests. `UserDefaults(suiteName:)` writes a real
/// plist under ~/Library/Preferences via the system `cfprefsd` daemon
/// asynchronously — outside the test process's control — so no amount of
/// in-process teardown (removePersistentDomain, synchronize, even deleting the
/// file directly) reliably prevents it from reappearing on disk after the test
/// finishes. Testing against an in-memory store sidesteps the problem entirely
/// rather than fighting it.
protocol KeyValueStore {
    func data(forKey key: String) -> Data?
    func set(_ value: Data?, forKey key: String)
}

extension UserDefaults: KeyValueStore {
    func set(_ value: Data?, forKey key: String) {
        self.set(value as Any?, forKey: key)
    }
}

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

    private static let key = "Bookdrop.Settings"
    private let defaults: KeyValueStore

    /// `defaults` is injectable so tests can use an in-memory store instead of
    /// the user's real UserDefaults (see `KeyValueStore` above for why).
    init(defaults: KeyValueStore = UserDefaults.standard) {
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
        let tempDir = FileManager.default.temporaryDirectory.appendingPathComponent("Bookdrop", isDirectory: true)
        try? FileManager.default.removeItem(at: tempDir)
    }

    var logsDirectory: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("Bookdrop/Logs", isDirectory: true)
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
