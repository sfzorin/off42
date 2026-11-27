import AppKit
import ImageIO
import Kit42

/// Reflowable EPUB 3 interchange. Layout, CSS, scripts and interactive content are not imported.
enum EPUBFormat {
    enum Failure: LocalizedError {
        case invalid(String)
        var errorDescription: String? { if case .invalid(let reason) = self { return "EPUB: " + reason }; return nil }
    }
    private static func fail(_ message: String) -> Failure { .invalid(message) }

    /// Inspect central-directory sizes before ZipArchive allocates or inflates any entry.
    private static func boundedArchive(_ data: Data) throws -> ZipArchive {
        let b = [UInt8](data)
        guard b.count >= 22, b.count <= 128 * 1024 * 1024 else { throw fail("invalid or oversized archive") }
        func u16(_ p: Int) -> Int { Int(b[p]) | Int(b[p + 1]) << 8 }
        func u32(_ p: Int) -> Int { u16(p) | u16(p + 2) << 16 }
        var e = b.count - 22
        let minimum = max(0, b.count - 22 - 65535)
        while e >= minimum {
            if u32(e) == 0x06054b50 && e + 22 + u16(e + 20) == b.count { break }
            e -= 1
        }
        guard e >= minimum, u16(e + 4) == 0, u16(e + 6) == 0 else { throw fail("invalid ZIP directory") }
        let count = u16(e + 10), start = u32(e + 16), length = u32(e + 12)
        guard count <= 4096, count == u16(e + 8), start <= e, length <= e - start else { throw fail("unsupported ZIP directory") }
        var p = start, total = 0, names = Set<String>()
        for _ in 0..<count {
            guard p + 46 <= start + length, u32(p) == 0x02014b50 else { throw fail("invalid ZIP entry") }
            let flags = u16(p + 8), method = u16(p + 10), size = u32(p + 24)
            let n = u16(p + 28), x = u16(p + 30), c = u16(p + 32)
            guard flags & 0x41 == 0, method == 0 || method == 8, size <= 32 * 1024 * 1024,
                  p + 46 + n + x + c <= start + length else { throw fail("encrypted, unsupported or oversized ZIP entry") }
            total += size
            guard total <= 128 * 1024 * 1024, let name = String(bytes: b[(p + 46)..<(p + 46 + n)], encoding: .utf8),
                  !name.hasPrefix("/"), !name.contains("\\"), !name.split(separator: "/").contains(".."), names.insert(name).inserted else { throw fail("unsafe or duplicate ZIP entry") }
            p += 46 + n + x + c
        }
        guard p == start + length else { throw fail("invalid ZIP directory length") }
        do { return try ZipArchive(data: data) } catch { throw fail("invalid or unsupported ZIP archive") }
    }

    private static func xml(_ data: Data) throws -> XMLDocument {
        guard data.count <= 16 * 1024 * 1024, let s = String(data: data, encoding: .utf8),
              s.range(of: "<!DOCTYPE", options: .caseInsensitive) == nil,
              s.range(of: "<!ENTITY", options: .caseInsensitive) == nil else { throw fail("unsupported XML encoding or declaration") }
        let document: XMLDocument
        do { document = try XMLDocument(data: data, options: [.nodePreserveWhitespace]) }
        catch { throw fail("malformed XML") }
        guard let root = document.rootElement() else { throw fail("missing XML root") }
        var pending: [(XMLNode, Int)] = [(root, 0)], nodes = 0
        while let (node, depth) = pending.popLast() {
            nodes += 1
            guard depth <= 64, nodes <= 100_000 else { throw fail("XML nesting or node limit exceeded") }
            for child in node.children ?? [] { pending.append((child, depth + 1)) }
        }
        return document
    }
    private static func descendants(_ node: XMLElement, _ name: String) -> [XMLElement] {
        var result: [XMLElement] = []
        for case let child as XMLElement in node.children ?? [] {
            if child.localName == name { result.append(child) }
            result += descendants(child, name)
        }
        return result
    }
    private static func attr(_ node: XMLElement, _ name: String) -> String? {
        node.attributes?.first { $0.localName == name }?.stringValue
    }
    private static func resolve(_ href: String, relativeTo path: String) throws -> String {
        guard let url = URLComponents(string: href), url.scheme == nil, url.host == nil, url.query == nil,
              let decoded = url.percentEncodedPath.removingPercentEncoding, !decoded.hasPrefix("/"), !decoded.contains("\\") else { throw fail("external or invalid resource path") }
        var parts = path.split(separator: "/").dropLast().map(String.init)
        for part in decoded.split(separator: "/") {
            if part == "." { continue }
            if part == ".." { guard !parts.isEmpty else { throw fail("resource escapes archive") }; parts.removeLast() }
            else { parts.append(String(part)) }
        }
        guard !parts.isEmpty else { throw fail("empty resource path") }
        return parts.joined(separator: "/")
    }

    static func read(_ data: Data) throws -> NSAttributedString {
        let zip = try boundedArchive(data)
        guard zip["mimetype"] == Data("application/epub+zip".utf8), zip["META-INF/encryption.xml"] == nil,
              let container = zip["META-INF/container.xml"], let root = try xml(container).rootElement(), root.localName == "container", root.uri == "urn:oasis:names:tc:opendocument:xmlns:container",
              let rootfile = descendants(root, "rootfile").first, let packagePath = attr(rootfile, "full-path"),
              let packageName = try? resolve(packagePath, relativeTo: "root"), let packageData = zip[packageName],
              let package = try xml(packageData).rootElement(), package.localName == "package", package.uri == "http://www.idpf.org/2007/opf",
              let manifest = descendants(package, "manifest").first, let spine = descendants(package, "spine").first else { throw fail("missing or invalid package, or encrypted publication") }
        if descendants(package, "meta").contains(where: { attr($0, "property") == "rendition:layout" && $0.stringValue?.trimmingCharacters(in: .whitespacesAndNewlines) == "pre-paginated" }) { throw fail("fixed-layout publications are unsupported") }
        var items: [String: (String, String)] = [:]
        for item in manifest.children ?? [] {
            guard let e = item as? XMLElement, e.localName == "item", let id = attr(e, "id"), let href = attr(e, "href"), let media = attr(e, "media-type"), items[id] == nil else { throw fail("invalid manifest") }
            items[id] = (try resolve(href, relativeTo: packageName), media)
        }
        let output = NSMutableAttributedString()
        var count = 0, chapterBytes = 0, assetBytes = 0, assetPixels = 0
        for case let ref as XMLElement in spine.children ?? [] where ref.localName == "itemref" {
            count += 1
            guard count <= 1024, let id = attr(ref, "idref"), let item = items[id], item.1 == "application/xhtml+xml", let chapter = zip[item.0], chapter.count <= 32 * 1024 * 1024 - chapterBytes else { throw fail("spine or chapter limit exceeded") }
            chapterBytes += chapter.count
            guard let html = try xml(chapter).rootElement(), html.localName == "html", html.uri == "http://www.w3.org/1999/xhtml", let body = descendants(html, "body").first else { throw fail("invalid or unsupported spine item") }
            if !descendants(body, "script").isEmpty || !descendants(body, "iframe").isEmpty || !descendants(body, "object").isEmpty || !descendants(body, "svg").isEmpty || !descendants(body, "math").isEmpty { throw fail("interactive or embedded content is unsupported") }
            func append(_ s: String, _ attributes: [NSAttributedString.Key: Any]) throws {
                guard s.utf16.count <= 8_000_000 - output.length else { throw fail("text output limit exceeded") }
                if !s.isEmpty { output.append(NSAttributedString(string: s, attributes: attributes)) }
            }
            func walk(_ node: XMLNode, _ attributes: [NSAttributedString.Key: Any], pre: Bool = false) throws {
                if node.kind == .text {
                    var value = pre ? (node.stringValue ?? "") : (node.stringValue ?? "").replacingOccurrences(of: "[\\t\\r\\n ]+", with: " ", options: .regularExpression)
                    if !pre && (output.length == 0 || output.string.hasSuffix("\n") || output.string.hasSuffix(" ")) { while value.hasPrefix(" ") { value.removeFirst() } }
                    try append(value, attributes); return
                }
                guard let e = node as? XMLElement else { return }
                let tag = e.localName?.lowercased() ?? ""
                var a = attributes
                let heading = ["h1": 1, "h2": 2, "h3": 3, "h4": 3, "h5": 3, "h6": 3][tag]
                if let heading { a = Styles.all[heading].attributes; a[Styles.styleKey] = Styles.wordIds[heading] }
                if tag == "a", let href = attr(e, "href"), let url = URL(string: href), ["http", "https", "mailto"].contains(url.scheme?.lowercased() ?? "") { a[.link] = url }
                if tag == "img" {
                    guard let src = attr(e, "src"), let bytes = zip[try resolve(src, relativeTo: item.0)], bytes.count <= 64 * 1024 * 1024 - assetBytes, let source = CGImageSourceCreateWithData(bytes as CFData, nil), let props = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any], let w = (props[kCGImagePropertyPixelWidth] as? NSNumber)?.intValue, let h = (props[kCGImagePropertyPixelHeight] as? NSNumber)?.intValue, w > 0, h > 0, w <= 20_000, h <= 20_000, w <= (100_000_000 - assetPixels) / h, let image = NSImage(data: bytes) else { throw fail("missing, oversized or unsupported image") }
                    assetBytes += bytes.count
                    assetPixels += w * h
                    guard output.length < 8_000_000 else { throw fail("text output limit exceeded") }
                    let attachment = NSTextAttachment(); attachment.fileWrapper = FileWrapper(regularFileWithContents: bytes); attachment.image = image; attachment.bounds = NSRect(origin: .zero, size: image.size)
                    output.append(NSAttributedString(attachment: attachment)); return
                }
                if tag == "br" { try append("\n", a); return }
                let block = heading != nil || ["p", "div", "li", "blockquote", "pre", "tr"].contains(tag)
                if block && output.length > 0 && !output.string.hasSuffix("\n") { try append("\n", a) }
                if tag == "li" { try append("• ", a) }
                if ["td", "th"].contains(tag), let siblings = e.parent?.children,
                   siblings.prefix(e.index).contains(where: { ($0 as? XMLElement).map { ["td", "th"].contains($0.localName ?? "") } ?? false }) { try append("\t", a) }
                for child in e.children ?? [] { try walk(child, a, pre: pre || tag == "pre") }
                if block && !output.string.hasSuffix("\n") { try append("\n", a) }
            }
            try walk(body, Styles.normal.attributes)
        }
        guard count > 0 else { throw fail("empty spine") }
        return output
    }

    static func write(_ source: NSAttributedString, title: String = "Document", author: String = "") throws -> Data {
        var zip = ZipArchive()
        zip["mimetype"] = Data("application/epub+zip".utf8); zip.storedUncompressed.insert("mimetype")
        zip["META-INF/container.xml"] = Data("<?xml version=\"1.0\" encoding=\"UTF-8\"?><container xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\" version=\"1.0\"><rootfiles><rootfile full-path=\"OEBPS/package.opf\" media-type=\"application/oebps-package+xml\"/></rootfiles></container>".utf8)
        var body = "", images = "", imageNumber = 0
        let ns = source.string as NSString
        var location = 0
        while location < ns.length {
            let paragraph = ns.paragraphRange(for: NSRange(location: location, length: 0))
            let textRange = NSRange(location: paragraph.location, length: paragraph.length - (ns.substring(with: paragraph).hasSuffix("\n") ? 1 : 0))
            let style = source.attribute(Styles.styleKey, at: location, effectiveRange: nil) as? String ?? ""
            let tag = ["Heading1": "h1", "Heading2": "h2", "Heading3": "h3"][style] ?? "p"
            body += "<\(tag)>"
            source.enumerateAttributes(in: textRange) { attrs, range, _ in
                if let attachment = attrs[.attachment] as? NSTextAttachment, let image = attachment.picture, let tiff = image.tiffRepresentation, let png = NSBitmapImageRep(data: tiff)?.representation(using: .png, properties: [:]) {
                    imageNumber += 1; let name = "image\(imageNumber).png"; zip["OEBPS/" + name] = png
                    images += "<item id=\"image\(imageNumber)\" href=\"\(name)\" media-type=\"image/png\"/>"
                    body += "<img src=\"\(name)\" alt=\"\"/>"
                } else {
                    let value = xmlEscape(ns.substring(with: range))
                    if let link = attrs[.link] { body += "<a href=\"\(xmlEscape(String(describing: link)))\">\(value)</a>" } else { body += value }
                }
            }
            body += "</\(tag)>\n"; location = NSMaxRange(paragraph)
        }
        let safeTitle = xmlEscape(title.isEmpty ? "Document" : title)
        zip["OEBPS/content.xhtml"] = Data("<?xml version=\"1.0\" encoding=\"UTF-8\"?><html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>\(safeTitle)</title></head><body>\(body)</body></html>".utf8)
        zip["OEBPS/nav.xhtml"] = Data("<?xml version=\"1.0\" encoding=\"UTF-8\"?><html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\"><head><title>Contents</title></head><body><nav epub:type=\"toc\" id=\"toc\"><h1>Contents</h1><ol><li><a href=\"content.xhtml\">\(safeTitle)</a></li></ol></nav></body></html>".utf8)
        let date = ISO8601DateFormatter().string(from: Date())
        let id = UUID().uuidString
        let creator = author.isEmpty ? "" : "<dc:creator>\(xmlEscape(author))</dc:creator>"
        zip["OEBPS/package.opf"] = Data("<?xml version=\"1.0\" encoding=\"UTF-8\"?><package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"pub-id\"><metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:identifier id=\"pub-id\">urn:uuid:\(id)</dc:identifier><dc:title>\(safeTitle)</dc:title><dc:language>und</dc:language>\(creator)<meta property=\"dcterms:modified\">\(date)</meta></metadata><manifest><item id=\"content\" href=\"content.xhtml\" media-type=\"application/xhtml+xml\"/><item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>\(images)</manifest><spine><itemref idref=\"content\"/></spine></package>".utf8)
        return zip.data()
    }
}
