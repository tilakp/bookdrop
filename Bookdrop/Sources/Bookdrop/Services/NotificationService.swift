import Foundation
import UserNotifications

enum NotificationService {
    /// `UNUserNotificationCenter.current()` raises an NSException, which Swift cannot
    /// catch, when the process has no app bundle, as in `swift run` or `swift test`.
    /// So everything here does nothing outside a packaged `.app`.
    private static var isNotificationCenterAvailable: Bool {
        Bundle.main.bundleIdentifier != nil
    }

    static func requestAuthorization() {
        guard isNotificationCenterAvailable else { return }
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound]) { _, _ in }
    }

    /// One notification for books converted together: by title for one
    /// book, by count for a batch.
    static func notifyConversionComplete(bookTitles: [String], format: OutputFormat, outputURLs: [URL]) {
        guard isNotificationCenterAvailable, let first = bookTitles.first else { return }
        let content = UNMutableNotificationContent()
        content.title = "Bookdrop"
        content.body =
            bookTitles.count == 1
            ? "\(first) has been converted to \(format.rawValue)."
            : "\(bookTitles.count) books have been converted to \(format.rawValue)."
        content.userInfo = ["outputPaths": outputURLs.map(\.path)]
        let request = UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil)
        UNUserNotificationCenter.current().add(request) { _ in }
    }
}
