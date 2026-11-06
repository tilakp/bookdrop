import Foundation

struct Book {
    var title: String
    var author: String?
    var coverImage: Data?
    var fileSizeBytes: Int64
    var spine: [SpineItem]
    var toc: [TocNode]
    var sourceURL: URL
    /// The engine's parsed copy, which conversions reuse.
    let engineBook: EngineBook

    var chapterCount: Int { spine.count }

    var fileSizeDisplay: String {
        ByteCountFormatter.string(fromByteCount: fileSizeBytes, countStyle: .file)
    }
}

struct SpineItem: Identifiable, Equatable {
    var id: String
    var href: String
}

struct TocNode: Identifiable, Equatable {
    let id = UUID()
    var title: String
    /// Relative to the book's content root, may include a "#fragment".
    var href: String?
    var children: [TocNode]

    static func == (lhs: TocNode, rhs: TocNode) -> Bool {
        lhs.title == rhs.title && lhs.href == rhs.href && lhs.children == rhs.children
    }
}
