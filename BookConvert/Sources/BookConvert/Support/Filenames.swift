import Foundation

func sanitizedFilename(_ name: String) -> String {
    let invalid = CharacterSet(charactersIn: "/:\\")
    let cleaned = name.components(separatedBy: invalid).joined(separator: "-")
    return cleaned.isEmpty ? "Untitled" : cleaned
}

/// Finds a non-colliding "Name (n).ext" URL in `directory`, per the "Keep Both" rule.
func nextAvailableURL(directory: URL, baseName: String, extension ext: String) -> URL {
    let candidate = directory.appendingPathComponent("\(baseName).\(ext)")
    guard FileManager.default.fileExists(atPath: candidate.path) else { return candidate }
    var n = 1
    while true {
        let url = directory.appendingPathComponent("\(baseName) (\(n)).\(ext)")
        if !FileManager.default.fileExists(atPath: url.path) { return url }
        n += 1
    }
}
