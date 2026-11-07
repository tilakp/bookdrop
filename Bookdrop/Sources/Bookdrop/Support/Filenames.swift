import Foundation

func sanitizedFilename(_ name: String) -> String {
    let invalid = CharacterSet(charactersIn: "/:\\")
    let cleaned = name.components(separatedBy: invalid).joined(separator: "-")
    return cleaned.isEmpty ? "Untitled" : cleaned
}

/// The folder an output with a companion suffix writes next to itself:
/// "Book.md" with "_images" is "Book_images". Matches `companion_dir` in
/// the engine.
func companionURL(for output: URL, suffix: String) -> URL {
    output.deletingLastPathComponent()
        .appendingPathComponent(output.deletingPathExtension().lastPathComponent + suffix)
}

/// Whether `url`, or its companion folder, already exists.
func outputNameIsTaken(_ url: URL, companionSuffix: String?) -> Bool {
    let fm = FileManager.default
    if fm.fileExists(atPath: url.path) { return true }
    guard let companionSuffix else { return false }
    return fm.fileExists(atPath: companionURL(for: url, suffix: companionSuffix).path)
}

/// Finds a non-colliding "Name (n).ext" URL in `directory`, per the "Keep
/// Both" rule. A name is taken if the file or its companion folder exists.
func nextAvailableURL(directory: URL, baseName: String, extension ext: String, companionSuffix: String? = nil) -> URL {
    let candidate = directory.appendingPathComponent("\(baseName).\(ext)")
    guard outputNameIsTaken(candidate, companionSuffix: companionSuffix) else { return candidate }
    var n = 1
    while true {
        let url = directory.appendingPathComponent("\(baseName) (\(n)).\(ext)")
        if !outputNameIsTaken(url, companionSuffix: companionSuffix) { return url }
        n += 1
    }
}
