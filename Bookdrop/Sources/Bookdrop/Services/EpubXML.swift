import Foundation

/// Parsed contents of the EPUB's .opf package document.
struct OPFDocument {
    var title: String?
    var author: String?
    var manifest: [String: ManifestItem] = [:]
    var spineIDRefs: [String] = []
    var tocNCXID: String?
    var coverMetaContentID: String?

    var coverHref: String? {
        if let item = manifest.values.first(where: { $0.properties.contains("cover-image") }) {
            return item.href
        }
        if let id = coverMetaContentID, let item = manifest[id] {
            return item.href
        }
        return nil
    }
}

func xmlLocalName(_ qualified: String) -> String {
    if let range = qualified.range(of: ":") {
        return String(qualified[range.upperBound...])
    }
    return qualified
}

/// META-INF/container.xml — finds the path to the .opf package document.
final class ContainerParserDelegate: NSObject, XMLParserDelegate {
    var fullPath: String?

    func parser(
        _ parser: XMLParser, didStartElement elementName: String, namespaceURI: String?,
        qualifiedName qName: String?, attributes attributeDict: [String: String]
    ) {
        if xmlLocalName(elementName) == "rootfile", let path = attributeDict["full-path"] {
            fullPath = path
        }
    }
}

/// The .opf package document — metadata, manifest, spine, and cover reference.
final class OPFParserDelegate: NSObject, XMLParserDelegate {
    var document = OPFDocument()

    private var inMetadata = false
    private var currentText = ""
    private var currentElement = ""

    func parser(
        _ parser: XMLParser, didStartElement elementName: String, namespaceURI: String?,
        qualifiedName qName: String?, attributes attributeDict: [String: String]
    ) {
        let local = xmlLocalName(elementName)
        currentElement = local
        currentText = ""

        switch local {
        case "metadata":
            inMetadata = true
        case "item":
            guard let id = attributeDict["id"], let href = attributeDict["href"] else { return }
            let mediaType = attributeDict["media-type"] ?? ""
            let properties = Set((attributeDict["properties"] ?? "").split(separator: " ").map(String.init))
            document.manifest[id] = ManifestItem(id: id, href: href, mediaType: mediaType, properties: properties)
        case "itemref":
            if let idref = attributeDict["idref"], (attributeDict["linear"] ?? "yes") != "no" {
                document.spineIDRefs.append(idref)
            }
        case "spine":
            document.tocNCXID = attributeDict["toc"]
        case "meta":
            if attributeDict["name"] == "cover", let content = attributeDict["content"] {
                document.coverMetaContentID = content
            }
        default:
            break
        }
    }

    func parser(_ parser: XMLParser, foundCharacters string: String) {
        if currentElement == "title" || currentElement == "creator" {
            currentText += string
        }
    }

    func parser(
        _ parser: XMLParser, didEndElement elementName: String, namespaceURI: String?,
        qualifiedName qName: String?
    ) {
        let local = xmlLocalName(elementName)
        let text = currentText.trimmingCharacters(in: .whitespacesAndNewlines)

        if inMetadata {
            switch local {
            case "title":
                if document.title == nil, !text.isEmpty { document.title = text }
            case "creator":
                if document.author == nil, !text.isEmpty { document.author = text }
            case "metadata":
                inMetadata = false
            default:
                break
            }
        }
        currentText = ""
    }
}

/// EPUB3 nav.xhtml — the <nav epub:type="toc"> document.
final class NavParserDelegate: NSObject, XMLParserDelegate {
    var rootNodes: [TocNode] = []

    private var inTocNav = false
    private var tocNavDepth = 0
    private var listStack: [[TocNode]] = []
    private var currentHref: String?
    private var currentTitle = ""
    private var capturingLinkText = false

    func parser(
        _ parser: XMLParser, didStartElement elementName: String, namespaceURI: String?,
        qualifiedName qName: String?, attributes attributeDict: [String: String]
    ) {
        let local = xmlLocalName(elementName)

        if local == "nav" {
            if inTocNav {
                tocNavDepth += 1
            } else if (attributeDict["epub:type"] ?? attributeDict["type"]) == "toc" {
                inTocNav = true
                tocNavDepth = 1
            }
            return
        }
        guard inTocNav else { return }

        switch local {
        case "ol":
            listStack.append([])
        case "a":
            currentHref = attributeDict["href"]
            currentTitle = ""
            capturingLinkText = true
        default:
            break
        }
    }

    func parser(_ parser: XMLParser, foundCharacters string: String) {
        if capturingLinkText { currentTitle += string }
    }

    func parser(
        _ parser: XMLParser, didEndElement elementName: String, namespaceURI: String?,
        qualifiedName qName: String?
    ) {
        let local = xmlLocalName(elementName)

        if local == "nav" {
            if inTocNav {
                tocNavDepth -= 1
                if tocNavDepth == 0 { inTocNav = false }
            }
            return
        }
        guard inTocNav else { return }

        switch local {
        case "a":
            capturingLinkText = false
            if !listStack.isEmpty {
                let node = TocNode(
                    title: currentTitle.trimmingCharacters(in: .whitespacesAndNewlines),
                    href: currentHref, children: [])
                listStack[listStack.count - 1].append(node)
            }
        case "ol":
            let children = listStack.removeLast()
            if listStack.isEmpty {
                rootNodes = children
            } else if var last = listStack[listStack.count - 1].popLast() {
                last.children = children
                listStack[listStack.count - 1].append(last)
            }
        default:
            break
        }
    }
}

/// EPUB2 toc.ncx — the <navMap> document.
final class NCXParserDelegate: NSObject, XMLParserDelegate {
    var rootNodes: [TocNode] { childStack[0] }

    private var childStack: [[TocNode]] = [[]]
    private var pendingTitleStack: [String] = []
    private var pendingHrefStack: [String?] = []
    private var inNavLabelText = false
    private var currentText = ""

    func parser(
        _ parser: XMLParser, didStartElement elementName: String, namespaceURI: String?,
        qualifiedName qName: String?, attributes attributeDict: [String: String]
    ) {
        switch xmlLocalName(elementName) {
        case "navPoint":
            childStack.append([])
            pendingTitleStack.append("")
            pendingHrefStack.append(nil)
        case "text":
            inNavLabelText = true
            currentText = ""
        case "content":
            if !pendingHrefStack.isEmpty {
                pendingHrefStack[pendingHrefStack.count - 1] = attributeDict["src"]
            }
        default:
            break
        }
    }

    func parser(_ parser: XMLParser, foundCharacters string: String) {
        if inNavLabelText { currentText += string }
    }

    func parser(
        _ parser: XMLParser, didEndElement elementName: String, namespaceURI: String?,
        qualifiedName qName: String?
    ) {
        switch xmlLocalName(elementName) {
        case "text":
            if inNavLabelText, !pendingTitleStack.isEmpty {
                pendingTitleStack[pendingTitleStack.count - 1] =
                    currentText.trimmingCharacters(in: .whitespacesAndNewlines)
            }
            inNavLabelText = false
        case "navPoint":
            let children = childStack.removeLast()
            let title = pendingTitleStack.removeLast()
            let href = pendingHrefStack.removeLast()
            childStack[childStack.count - 1].append(TocNode(title: title, href: href, children: children))
        default:
            break
        }
    }
}
