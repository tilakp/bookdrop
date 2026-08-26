import Foundation

enum OutputFormat: String, CaseIterable, Identifiable {
    case pdf = "PDF"
    case epub = "EPUB"
    case txt = "TXT"
    case html = "HTML"
    case docx = "DOCX"

    var id: String { rawValue }

    var fileExtension: String {
        switch self {
        case .pdf: return "pdf"
        case .epub: return "epub"
        case .txt: return "txt"
        case .html: return "html"
        case .docx: return "docx"
        }
    }

    /// Whether the PDF-specific options section (page size/margins/orientation/
    /// typography) applies to this format.
    var hasPdfOptions: Bool { self == .pdf }

    /// Whether "Include cover" / "Generate table of contents" apply. Not
    /// plain text (never had either concept), and not EPUB — `EpubOutput`
    /// (Rust) ignores both options entirely: EPUB3 requires a nav
    /// document regardless of a "generate TOC" preference, and an EPUB
    /// missing its own cover resource is strictly worse, not a legitimate
    /// choice — see EpubOutput's own doc comment.
    var supportsCoverAndTOC: Bool { self != .txt && self != .epub }
}
