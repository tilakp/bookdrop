import Foundation

struct CompletionInfo {
    let outputURL: URL
    let pageCount: Int

    var fileSizeDisplay: String {
        let attributes = try? FileManager.default.attributesOfItem(atPath: outputURL.path)
        let size = (attributes?[.size] as? Int64) ?? 0
        return ByteCountFormatter.string(fromByteCount: size, countStyle: .file)
    }
}
