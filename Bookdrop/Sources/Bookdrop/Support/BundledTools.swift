import Foundation

/// The helper programs `Scripts/build-app.sh` copies into
/// `Contents/Resources/<Tool>/<arch>/`. The Rust engine reads their paths
/// from the options JSON. Outside a packaged app (`swift test`,
/// `swift run`) none is found, and the engine falls back to the copies
/// vendored under `rust/vendor/`.
enum BundledTools {
    /// Option key -> path inside the arch folder, one per tool.
    private static let tools: [(optionKey: String, relativePath: String)] = [
        ("chromium_path", "Chromium/\(arch)/chrome-headless-shell"),
        ("pdfium_path", "Pdfium/\(arch)/libpdfium.dylib"),
        ("boko_path", "Boko/\(arch)/boko"),
    ]

    #if arch(arm64)
        private static let arch = "mac-arm64"
    #else
        private static let arch = "mac-x64"
    #endif

    /// The `*_path` options for every tool present in the app bundle.
    static var engineOptions: [String: String] {
        guard let resources = Bundle.main.resourceURL else { return [:] }
        var options: [String: String] = [:]
        for tool in tools {
            let path = resources.appendingPathComponent(tool.relativePath).path
            if FileManager.default.fileExists(atPath: path) {
                options[tool.optionKey] = path
            }
        }
        return options
    }
}
