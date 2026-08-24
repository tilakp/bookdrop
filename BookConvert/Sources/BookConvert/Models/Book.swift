import Foundation

struct Book {
    var title: String
    var author: String?
    var coverImage: Data?
    var fileSizeBytes: Int64
    var spine: [SpineItem]
    var toc: [TocNode]
    var manifest: [String: ManifestItem]
    var sourceURL: URL
    /// Directory (inside the extracted working copy) containing the OPF — spine/manifest hrefs are relative to this.
    var contentDirectory: URL

    var chapterCount: Int { spine.count }

    var fileSizeDisplay: String {
        ByteCountFormatter.string(fromByteCount: fileSizeBytes, countStyle: .file)
    }
}

struct SpineItem: Identifiable, Equatable {
    var id: String
    var href: String
    var mediaType: String
}

struct ManifestItem: Equatable {
    var id: String
    var href: String
    var mediaType: String
    var properties: Set<String>
}

struct TocNode: Identifiable, Equatable {
    let id = UUID()
    var title: String
    /// Relative to the OPF's content directory, may include a "#fragment".
    var href: String?
    var children: [TocNode]

    static func == (lhs: TocNode, rhs: TocNode) -> Bool {
        lhs.title == rhs.title && lhs.href == rhs.href && lhs.children == rhs.children
    }
}
