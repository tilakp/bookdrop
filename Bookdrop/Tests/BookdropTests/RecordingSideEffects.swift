import Foundation
@testable import Bookdrop

/// Records the "After conversion" actions instead of doing them.
@MainActor
final class RecordingSideEffects: ConversionSideEffects {
    private(set) var notifications: [(bookTitles: [String], format: OutputFormat)] = []
    private(set) var revealed: [[URL]] = []
    private(set) var opened: [URL] = []

    func notify(bookTitles: [String], format: OutputFormat, outputURLs: [URL]) {
        notifications.append((bookTitles, format))
    }

    func reveal(_ urls: [URL]) { revealed.append(urls) }

    func open(_ url: URL) { opened.append(url) }
}
