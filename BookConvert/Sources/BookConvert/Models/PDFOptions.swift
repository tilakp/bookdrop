import Foundation
import CoreGraphics

enum PageSize: String, CaseIterable, Identifiable {
    case usLetter = "US Letter"
    case a4 = "A4"
    case a5 = "A5"

    var id: String { rawValue }

    /// Points at 72dpi, portrait orientation.
    var dimensions: CGSize {
        switch self {
        case .usLetter: return CGSize(width: 612, height: 792)
        case .a4: return CGSize(width: 595, height: 842)
        case .a5: return CGSize(width: 420, height: 595)
        }
    }
}

enum PageMargins: String, CaseIterable, Identifiable {
    case narrow = "Narrow"
    case normal = "Normal"
    case wide = "Wide"

    var id: String { rawValue }

    var points: CGFloat {
        switch self {
        case .narrow: return 24
        case .normal: return 48
        case .wide: return 72
        }
    }
}

enum PageOrientation: String, CaseIterable, Identifiable {
    case portrait = "Portrait"
    case landscape = "Landscape"

    var id: String { rawValue }
}

struct PDFOptions {
    var pageSize: PageSize = .usLetter
    var margins: PageMargins = .normal
    var orientation: PageOrientation = .portrait
    var includeCover = true
    var generateTableOfContents = true

    // Advanced — wired in M4.
    var fontFamily: String = "Original"
    var fontSizePt: Double = 11
    var lineSpacing: Double = 1.2
    var startChaptersOnNewPage = true
    var preserveEpubStyling = true
    var removePublisherStyling = false
    var showPageNumbers = true
    var includeHeaders = false
    var includeFooters = false

    var pageDimensions: CGSize {
        let base = pageSize.dimensions
        return orientation == .landscape ? CGSize(width: base.height, height: base.width) : base
    }
}
