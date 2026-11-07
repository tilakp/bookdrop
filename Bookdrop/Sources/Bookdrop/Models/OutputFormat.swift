import Foundation

enum OutputFormat: String, CaseIterable, Identifiable {
    case pdf = "PDF"
    case epub = "EPUB"
    case txt = "TXT"
    case html = "HTML"
    case docx = "DOCX"
    case markdown = "Markdown"

    var id: String { rawValue }

    var fileExtension: String {
        switch self {
        case .pdf: return "pdf"
        case .epub: return "epub"
        case .txt: return "txt"
        case .html: return "html"
        case .docx: return "docx"
        case .markdown: return "md"
        }
    }

    /// The engine options this format's output plugin reads. Each control
    /// below shows only for a format that reads its option.
    private var engineOptions: [String] { EngineFormats.shared.options(forOutputExtension: fileExtension) }

    var supportsCover: Bool { engineOptions.contains("include_cover") }
    var supportsTableOfContents: Bool { engineOptions.contains("generate_table_of_contents") }
    /// Page size, margins, typography, headers and footers.
    var hasPdfOptions: Bool { engineOptions.contains("page_width_in") }

    /// The suffix of the folder this format writes next to its file
    /// ("_images" for Markdown), or nil if it writes only the file.
    var companionSuffix: String? {
        EngineFormats.shared.outputs.first { $0.extension == fileExtension }?.companionSuffix
    }
}
