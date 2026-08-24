import Foundation
import ZIPFoundation

enum EpubParserError: LocalizedError {
    case invalidArchive
    case missingContainer
    case missingOPF
    case malformedXML(String)

    var errorDescription: String? {
        switch self {
        case .invalidArchive:
            return "This file isn't a valid EPUB archive."
        case .missingContainer:
            return "This EPUB is missing META-INF/container.xml."
        case .missingOPF:
            return "This EPUB's content file (.opf) couldn't be found."
        case .malformedXML(let detail):
            return "This EPUB's formatting couldn't be read (\(detail))."
        }
    }
}

enum EpubParser {
    /// Extracts the EPUB into a fresh temp directory and parses metadata/manifest/spine/TOC.
    /// The returned Book's `contentDirectory` stays on disk — callers are responsible for
    /// staging the temp directory for as long as they need it (e.g. through PDF rendering).
    static func parse(fileAt sourceURL: URL) throws -> Book {
        let sourceAttributes = try? FileManager.default.attributesOfItem(atPath: sourceURL.path)
        let fileSize = (sourceAttributes?[.size] as? Int64) ?? 0

        let archive: Archive
        do {
            archive = try Archive(url: sourceURL, accessMode: .read)
        } catch {
            throw EpubParserError.invalidArchive
        }

        let workDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("Bookdrop", isDirectory: true)
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: workDir, withIntermediateDirectories: true)

        for entry in archive {
            let destination = workDir.appendingPathComponent(entry.path)
            if entry.type == .directory {
                try? FileManager.default.createDirectory(at: destination, withIntermediateDirectories: true)
                continue
            }
            try FileManager.default.createDirectory(
                at: destination.deletingLastPathComponent(), withIntermediateDirectories: true)
            _ = try archive.extract(entry, to: destination)
        }

        let containerURL = workDir.appendingPathComponent("META-INF/container.xml")
        guard FileManager.default.fileExists(atPath: containerURL.path) else {
            throw EpubParserError.missingContainer
        }
        let opfRelativePath = try parseContainer(at: containerURL)
        let opfURL = workDir.appendingPathComponent(opfRelativePath)
        guard FileManager.default.fileExists(atPath: opfURL.path) else {
            throw EpubParserError.missingOPF
        }
        let contentDir = opfURL.deletingLastPathComponent()

        let opf = try parseOPF(at: opfURL)
        let toc = parseTOC(opf: opf, contentDir: contentDir)

        var coverData: Data?
        if let coverHref = opf.coverHref {
            coverData = try? Data(contentsOf: contentDir.appendingPathComponent(coverHref))
        }

        let spineItems: [SpineItem] = opf.spineIDRefs.compactMap { idref in
            guard let item = opf.manifest[idref] else { return nil }
            return SpineItem(id: item.id, href: item.href, mediaType: item.mediaType)
        }

        return Book(
            title: opf.title ?? sourceURL.deletingPathExtension().lastPathComponent,
            author: opf.author,
            coverImage: coverData,
            fileSizeBytes: fileSize,
            spine: spineItems,
            toc: toc,
            manifest: opf.manifest,
            sourceURL: sourceURL,
            contentDirectory: contentDir
        )
    }

    private static func parseContainer(at url: URL) throws -> String {
        guard let data = try? Data(contentsOf: url) else {
            throw EpubParserError.malformedXML("container.xml unreadable")
        }
        let delegate = ContainerParserDelegate()
        let parser = XMLParser(data: data)
        parser.delegate = delegate
        guard parser.parse(), let path = delegate.fullPath else {
            throw EpubParserError.malformedXML("container.xml has no rootfile")
        }
        return path
    }

    private static func parseOPF(at url: URL) throws -> OPFDocument {
        guard let data = try? Data(contentsOf: url) else {
            throw EpubParserError.malformedXML("OPF unreadable")
        }
        let delegate = OPFParserDelegate()
        let parser = XMLParser(data: data)
        parser.delegate = delegate
        guard parser.parse() else {
            throw EpubParserError.malformedXML(parser.parserError?.localizedDescription ?? "unknown")
        }
        return delegate.document
    }

    private static func parseTOC(opf: OPFDocument, contentDir: URL) -> [TocNode] {
        if let navItem = opf.manifest.values.first(where: { $0.properties.contains("nav") }) {
            let navURL = contentDir.appendingPathComponent(navItem.href)
            if let data = try? Data(contentsOf: navURL) {
                let delegate = NavParserDelegate()
                let parser = XMLParser(data: data)
                parser.delegate = delegate
                if parser.parse(), !delegate.rootNodes.isEmpty {
                    return delegate.rootNodes
                }
            }
        }
        if let ncxID = opf.tocNCXID, let ncxItem = opf.manifest[ncxID] {
            let ncxURL = contentDir.appendingPathComponent(ncxItem.href)
            if let data = try? Data(contentsOf: ncxURL) {
                let delegate = NCXParserDelegate()
                let parser = XMLParser(data: data)
                parser.delegate = delegate
                if parser.parse() {
                    return delegate.rootNodes
                }
            }
        }
        return []
    }
}
