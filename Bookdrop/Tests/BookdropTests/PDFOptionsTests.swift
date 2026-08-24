import XCTest
@testable import Bookdrop

final class PDFOptionsTests: XCTestCase {
    func testPresetPageSizeDimensions() {
        var options = PDFOptions()
        options.pageSize = .a4
        XCTAssertEqual(options.pageDimensions, CGSize(width: 595, height: 842))
    }

    func testCustomPageSizeUsesInchFields() {
        var options = PDFOptions()
        options.pageSize = .custom
        options.customPageWidthInches = 4
        options.customPageHeightInches = 6
        XCTAssertEqual(options.pageDimensions, CGSize(width: 288, height: 432))
    }

    func testCustomPageSizeRespectsLandscapeOrientation() {
        var options = PDFOptions()
        options.pageSize = .custom
        options.customPageWidthInches = 4
        options.customPageHeightInches = 6
        options.orientation = .landscape
        XCTAssertEqual(options.pageDimensions, CGSize(width: 432, height: 288))
    }
}
