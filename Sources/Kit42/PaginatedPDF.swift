import AppKit
import PDFKit

/// Renders a view to a paginated PDF through the print system (honours knowsPageRange/rectForPage
/// and automatic pagination), without showing any panel.
public enum PaginatedPDF {
    public static func render(_ view: NSView, info: NSPrintInfo) -> Data {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString + ".pdf")
        defer { try? FileManager.default.removeItem(at: url) }
        let job = info.copy() as! NSPrintInfo
        // Printer corrections are never part of a document export, even if
        // AppKit retained custom print settings on the source document.
        ProductionPrint.setCorrection(nil, in: job)
        job.jobDisposition = .save
        job.dictionary()[NSPrintInfo.AttributeKey.jobSavingURL] = url
        let op = NSPrintOperation(view: view, printInfo: job)
        op.showsPrintPanel = false
        op.showsProgressPanel = false
        op.run()
        return consistentXref((try? Data(contentsOf: url)) ?? Data())
    }
}

// MARK: - Bookmarks, links, notes and metadata

public extension PaginatedPDF {
    /// A heading for the PDF outline: page index from 0, `y` from the top of the page.
    struct OutlineItem {
        public var title: String
        public var level: Int
        public var page: Int
        public var y: CGFloat
        public init(title: String, level: Int, page: Int, y: CGFloat) { self.title = title; self.level = level; self.page = page; self.y = y }
    }

    /// A clickable area (rect from the page's top-left) to a web address or to a place in the document.
    struct Link {
        public var page: Int
        public var rect: CGRect
        public var url: URL?
        public var target: (page: Int, y: CGFloat)?
        public init(page: Int, rect: CGRect, url: URL? = nil, target: (page: Int, y: CGFloat)? = nil) {
            self.page = page; self.rect = rect; self.url = url; self.target = target
        }
    }

    /// A note annotation (comments of the document).
    struct Note {
        public var page: Int
        public var rect: CGRect
        public var text: String
        public var author: String
        public init(page: Int, rect: CGRect, text: String, author: String) { self.page = page; self.rect = rect; self.text = text; self.author = author }
    }

    /// Adds outline, links, notes and title/author to a PDF; `pages`: the pages to keep (indices from 0), nil all.
    /// Pages are redrawn into a PDF context (named destinations, links, outline, metadata); notes are added with PDFKit.
    static func finish(_ data: Data, title: String = "", author: String = "", creator: String = "", outline: [OutlineItem] = [],
                       links: [Link] = [], notes: [Note] = [], pages keep: Set<Int>? = nil) -> Data {
        guard let provider = CGDataProvider(data: data as CFData), let src = CGPDFDocument(provider), src.numberOfPages > 0 else { return data }
        let count = src.numberOfPages
        func kept(_ i: Int) -> Bool { i >= 0 && i < count && (keep?.contains(i) ?? true) }
        let order = (0..<count).filter(kept)
        let out = NSMutableData()
        guard let consumer = CGDataConsumer(data: out as CFMutableData) else { return data }
        var info: [CFString: Any] = [:]
        if !title.isEmpty { info[kCGPDFContextTitle] = title }
        if !author.isEmpty { info[kCGPDFContextAuthor] = author }
        if !creator.isEmpty { info[kCGPDFContextCreator] = creator }
        var first = src.page(at: 1)?.getBoxRect(.mediaBox) ?? CGRect(x: 0, y: 0, width: 595, height: 842)
        guard let ctx = CGContext(consumer: consumer, mediaBox: &first, info as CFDictionary) else { return data }
        // Named destinations: "p<page>_<y>" for every outline entry and link target.
        func name(_ page: Int, _ y: CGFloat) -> String { "p\(page)_\(Int(y.rounded()))" }
        var targets: [Int: Set<String>] = [:]
        var ys: [String: CGFloat] = [:]
        for l in links { if let t = l.target, kept(t.page) { let n = name(t.page, t.y); targets[t.page, default: []].insert(n); ys[n] = t.y } }
        for i in order {
            guard let page = src.page(at: i + 1) else { continue }
            var box = page.getBoxRect(.mediaBox)
            ctx.beginPage(mediaBox: &box)
            ctx.drawPDFPage(page)
            for n in targets[i] ?? [] { ctx.addDestination(n as CFString, at: CGPoint(x: box.minX, y: box.maxY - max(0, (ys[n] ?? 0) - 12))) }
            for l in links where l.page == i {
                let r = CGRect(x: box.minX + l.rect.minX, y: box.maxY - l.rect.maxY, width: l.rect.width, height: l.rect.height)
                if let url = l.url { ctx.setURL(url as CFURL, for: r) } else if let t = l.target, kept(t.page) { ctx.setDestination(name(t.page, t.y) as CFString, for: r) }
            }
            ctx.endPage()
        }
        if !outline.isEmpty {
            // Nested dictionaries: children under the last entry of a lower level.
            final class Node { var title = ""; var page = 1; var rect = CGRect.zero; var level = -1; var children: [Node] = [] }
            let root = Node()
            var stack = [root]
            for o in outline where kept(o.page) {
                let n = Node()
                n.title = o.title; n.level = o.level
                n.page = (order.firstIndex(of: o.page) ?? 0) + 1
                let box = src.page(at: o.page + 1)?.getBoxRect(.mediaBox) ?? .zero
                n.rect = CGRect(x: box.minX, y: box.maxY - max(0, o.y - 12) - 20, width: box.width, height: 20)
                while let last = stack.last, last.level >= o.level { stack.removeLast() }
                (stack.last ?? root).children.append(n)
                stack.append(n)
            }
            func dict(_ n: Node) -> [String: Any] {
                var d: [String: Any] = [:]
                if n.level >= 0 {
                    d[kCGPDFOutlineTitle as String] = n.title
                    d[kCGPDFOutlineDestination as String] = n.page
                    d[kCGPDFOutlineDestinationRect as String] = n.rect.dictionaryRepresentation
                }
                if !n.children.isEmpty { d[kCGPDFOutlineChildren as String] = n.children.map(dict) }
                return d
            }
            if !root.children.isEmpty { CGPDFContextSetOutline(ctx, dict(root) as CFDictionary) }
        }
        ctx.closePDF()
        var result = out as Data
        // Notes: PDFKit keeps the outline and links it reads, and adds the annotations.
        if !notes.isEmpty, let doc = PDFDocument(data: result) {
            for n in notes where kept(n.page) {
                guard let k = order.firstIndex(of: n.page), let page = doc.page(at: k) else { continue }
                let box = page.bounds(for: .mediaBox)
                let r = CGRect(x: box.minX + n.rect.minX, y: box.maxY - n.rect.maxY, width: n.rect.width, height: n.rect.height)
                let mark = PDFAnnotation(bounds: r, forType: .highlight, withProperties: nil)
                mark.color = NSColor(calibratedRed: 1, green: 0.86, blue: 0.4, alpha: 0.5)
                mark.contents = n.text
                mark.userName = n.author
                page.addAnnotation(mark)
                let a = PDFAnnotation(bounds: CGRect(x: min(box.maxX - 20, r.maxX + 2), y: r.maxY - 16, width: 18, height: 18), forType: .text, withProperties: nil)
                a.contents = n.text
                a.userName = n.author
                a.color = NSColor(calibratedRed: 1, green: 0.8, blue: 0.2, alpha: 1)
                page.addAnnotation(a)
            }
            result = doc.dataRepresentation() ?? result
        }
        return consistentXref(result)
    }

    /// The same file with a cross-reference table that lists only objects really in it: Quartz and PDFKit
    /// leave entries "in use at offset 0" for objects they never wrote (readers report the file as damaged).
    /// Entries that don't point at "N G obj" become free; the table is written anew (one section).
    static func consistentXref(_ data: Data) -> Data {
        let b = [UInt8](data)
        guard let sx = lastRange(of: Array("startxref".utf8), in: b) else { return data }
        var i = sx + 9
        while i < b.count, isSpace(b[i]) { i += 1 }
        var xrefAt = 0
        while i < b.count, b[i] >= 48, b[i] <= 57 { xrefAt = xrefAt * 10 + Int(b[i] - 48); i += 1 }
        guard xrefAt > 0, xrefAt + 4 < b.count, Array(b[xrefAt..<xrefAt + 4]) == Array("xref".utf8) else { return data }
        // Classic table: subsections "first count", then 20-byte entries, then the trailer.
        var p = xrefAt + 4
        func skip() { while p < b.count, isSpace(b[p]) { p += 1 } }
        func number() -> Int? {
            skip()
            var v = 0, any = false
            while p < b.count, b[p] >= 48, b[p] <= 57 { v = v * 10 + Int(b[p] - 48); p += 1; any = true }
            return any ? v : nil
        }
        var entries: [Int: (offset: Int, gen: Int)] = [:]
        while true {
            skip()
            if p + 7 <= b.count, Array(b[p..<p + 7]) == Array("trailer".utf8) { break }
            guard let first = number(), let count = number() else { return data }
            for k in 0..<count {
                guard let off = number(), let gen = number() else { return data }
                skip()
                guard p < b.count else { return data }
                let kind = b[p]; p += 1
                if kind == UInt8(ascii: "n") { entries[first + k] = (off, gen) }
            }
        }
        let trailerStart = p
        let trailerText = String(decoding: b[trailerStart..<sx], as: UTF8.self)
        guard !trailerText.contains("/Prev") else { return data }   // incremental files: left as they are
        func points(_ n: Int, _ off: Int) -> Bool {
            guard off > 0, off < b.count else { return false }
            let head = String(decoding: b[off..<min(b.count, off + 24)], as: UTF8.self)
            return head.hasPrefix("\(n) ") && head.contains("obj")
        }
        let size = max((entries.keys.max() ?? 0) + 1, 1)
        let used = entries.filter { points($0.key, $0.value.offset) }
        guard used.count != entries.count else { return data }
        let free = (1..<size).filter { used[$0] == nil }
        var next: [Int: Int] = [:]
        for (k, n) in free.enumerated() { next[n] = k + 1 < free.count ? free[k + 1] : 0 }
        var x = "xref\n0 \(size)\n" + String(format: "%010d 65535 f\r\n", free.first ?? 0)
        for n in 1..<size {
            if let e = used[n] { x += String(format: "%010d %05d n\r\n", e.offset, e.gen) } else { x += String(format: "%010d 00001 f\r\n", next[n] ?? 0) }
        }
        var trailer = trailerText.replacingOccurrences(of: "/Size\\s+\\d+", with: "/Size \(size)", options: .regularExpression)
        if !trailer.hasSuffix("\n") { trailer += "\n" }
        let out = Array(b[0..<xrefAt]) + Array((x + trailer + "startxref\n\(xrefAt)\n%%EOF\n").utf8)
        return Data(out)
    }

    private static func isSpace(_ c: UInt8) -> Bool { c == 32 || c == 10 || c == 13 || c == 9 || c == 12 || c == 0 }

    private static func lastRange(of needle: [UInt8], in b: [UInt8]) -> Int? {
        guard b.count >= needle.count else { return nil }
        var i = b.count - needle.count
        while i >= 0 {
            if b[i] == needle[0], Array(b[i..<i + needle.count]) == needle { return i }
            i -= 1
        }
        return nil
    }
}
