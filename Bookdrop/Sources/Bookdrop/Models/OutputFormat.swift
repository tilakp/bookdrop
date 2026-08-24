import Foundation

enum OutputFormat: String, CaseIterable, Identifiable {
    case pdf = "PDF"
    case txt = "TXT"
    case html = "HTML"
    case docx = "DOCX"

    var id: String { rawValue }

    var fileExtension: String {
        switch self {
        case .pdf: return "pdf"
        case .txt: return "txt"
        case .html: return "html"
        case .docx: return "docx"
        }
    }

    /// Whether the PDF-specific options section (page size/margins/orientation/
    /// typography) applies to this format.
    var hasPdfOptions: Bool { self == .pdf }

    /// Whether "Include cover" / "Generate table of contents" apply — meaningful
    /// for every format except plain text.
    var supportsCoverAndTOC: Bool { self != .txt }
}
