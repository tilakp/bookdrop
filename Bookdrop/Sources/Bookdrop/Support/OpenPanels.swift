import AppKit
import UniformTypeIdentifiers

/// The app's Open panels.
@MainActor
enum OpenPanels {
    /// Asks for one or more books in a format the engine reads.
    static func chooseBooks() -> [URL] {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = BookLoader.supportedExtensions.sorted().compactMap { UTType(filenameExtension: $0) }
        panel.allowsMultipleSelection = true
        panel.canChooseDirectories = false
        panel.prompt = "Open"
        return panel.runModal() == .OK ? panel.urls : []
    }

    /// Asks for the folder to save converted books to.
    static func chooseFolder(startingAt directory: URL) -> URL? {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.canCreateDirectories = true
        panel.allowsMultipleSelection = false
        panel.directoryURL = directory
        panel.prompt = "Choose"
        return panel.runModal() == .OK ? panel.url : nil
    }
}
