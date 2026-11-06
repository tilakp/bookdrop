import Foundation

/// The engine's work folders are under `$TMPDIR/Bookdrop/` (see
/// `fresh_work_dir` in anyform-core). Each one is deleted when the book or
/// conversion that made it ends, so only a crash leaves one behind.
enum TemporaryFiles {
    /// When this app run started. A folder made since then belongs to a
    /// book that is open now, so clearing must keep it. Touched once at
    /// launch (`AppDelegate`), because a static `let` is set lazily.
    static let launchDate = Date()

    static var defaultRoot: URL {
        FileManager.default.temporaryDirectory.appendingPathComponent("Bookdrop", isDirectory: true)
    }

    /// Folders that older versions used. Nothing creates them now.
    static var legacyRoots: [URL] {
        ["Bookdrop-pdf", "Bookdrop-kindle"].map {
            FileManager.default.temporaryDirectory.appendingPathComponent($0, isDirectory: true)
        }
    }

    /// Deletes the work folders left by earlier runs (and the folders of
    /// older versions) and returns how many it deleted.
    @discardableResult
    static func clearLeftovers(
        in root: URL = defaultRoot, createdBefore cutoff: Date = launchDate, legacyRoots: [URL] = legacyRoots
    ) -> Int {
        let entries = (try? FileManager.default.contentsOfDirectory(
            at: root, includingPropertiesForKeys: [.creationDateKey])) ?? []
        var removed = legacyRoots.filter { (try? FileManager.default.removeItem(at: $0)) != nil }.count
        for entry in entries {
            let created = (try? entry.resourceValues(forKeys: [.creationDateKey]))?.creationDate ?? .distantFuture
            if created < cutoff, (try? FileManager.default.removeItem(at: entry)) != nil {
                removed += 1
            }
        }
        return removed
    }
}
