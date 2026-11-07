import SwiftUI
import AppKit

final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        _ = TemporaryFiles.launchDate
        NSApp.setActivationPolicy(.regular)
        NSApp.activate(ignoringOtherApps: true)
        NotificationService.requestAuthorization()
    }
}

@main
struct BookdropApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate
    @StateObject private var historyStore: HistoryStore
    @StateObject private var settingsStore: SettingsStore
    @StateObject private var coordinator: AppCoordinator

    init() {
        let history = HistoryStore()
        let settings = SettingsStore()
        _historyStore = StateObject(wrappedValue: history)
        _settingsStore = StateObject(wrappedValue: settings)
        _coordinator = StateObject(wrappedValue: AppCoordinator(historyStore: history, settingsStore: settings))
    }

    var body: some Scene {
        WindowGroup {
            ContentView(coordinator: coordinator, historyStore: historyStore)
                .frame(minWidth: 640, minHeight: 540)
                .tint(.brandPurple)
        }
        .windowResizability(.contentMinSize)
        .defaultSize(width: 720, height: 620)
        .commands {
            // One window: the app has one conversion flow, so "New Window"
            // is replaced by "Open…".
            CommandGroup(replacing: .newItem) {
                Button("Open\u{2026}") { coordinator.chooseFiles() }
                    .keyboardShortcut("o")
            }
        }

        Settings {
            SettingsView(settings: settingsStore)
                .tint(.brandPurple)
        }
    }
}
