import Foundation
import UserNotifications

enum NotificationService {
    /// `UNUserNotificationCenter.current()` raises an uncaught NSException — not a
    /// throwing Swift error, so it can't be caught locally — when the process has no
    /// real app bundle (`bundleProxyForCurrentProcess` is nil). That's exactly a
    /// `swift run` debug build. Everything here is a no-op until packaged as a proper
    /// signed `.app`.
    private static var isNotificationCenterAvailable: Bool {
        Bundle.main.bundleIdentifier != nil
    }

    static func requestAuthorization() {
        guard isNotificationCenterAvailable else { return }
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound]) { _, _ in }
    }

    static func notifyConversionComplete(bookTitle: String, outputURL: URL) {
        guard isNotificationCenterAvailable else { return }
        let content = UNMutableNotificationContent()
        content.title = "Bookdrop"
        content.body = "\(bookTitle) has been converted to PDF."
        content.userInfo = ["outputPath": outputURL.path]
        let request = UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil)
        UNUserNotificationCenter.current().add(request) { _ in }
    }
}
