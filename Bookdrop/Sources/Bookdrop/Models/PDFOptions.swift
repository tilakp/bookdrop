import Foundation
import CoreGraphics

enum PageSize: String, CaseIterable, Identifiable {
    case usLetter = "US Letter"
    case a4 = "A4"
    case a5 = "A5"
    case custom = "Custom"

    var id: String { rawValue }

    /// Points at 72dpi, portrait orientation. `.custom` has no fixed size —
    /// callers read `PDFOptions.pageDimensions` instead, which falls back to
    /// the user-entered custom width/height for this case.
    var dimensions: CGSize? {
        switch self {
        case .usLetter: return CGSize(width: 612, height: 792)
        case .a4: return CGSize(width: 595, height: 842)
        case .a5: return CGSize(width: 420, height: 595)
        case .custom: return nil
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

    // Only read when pageSize == .custom.
    var customPageWidthInches: Double = 8.5
    var customPageHeightInches: Double = 11

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
        let pointsPerInch: CGFloat = 72
        let base = pageSize.dimensions
            ?? CGSize(width: customPageWidthInches * pointsPerInch, height: customPageHeightInches * pointsPerInch)
        return orientation == .landscape ? CGSize(width: base.height, height: base.width) : base
    }
}
