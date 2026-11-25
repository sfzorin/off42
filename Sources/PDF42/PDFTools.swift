import AppKit
import PDFKit
import UniformTypeIdentifiers

/// Document-level operations shared by the UI and the command line: page ranges, split, merge,
/// form fields and text comparison.
enum PDFTools {
    // MARK: - Page ranges

    /// Parses "1-3, 5, 8-" into groups of 0-based page indices (one group per comma-separated part).
    /// Returns nil when a part is malformed or out of 1…`pageCount`.
    static func parseRanges(_ text: String, pageCount: Int) -> [[Int]]? {
        guard pageCount > 0 else { return nil }
        var groups: [[Int]] = []
        for part in text.split(omittingEmptySubsequences: false, whereSeparator: { $0 == "," || $0 == ";" }) {
            let p = part.trimmingCharacters(in: .whitespaces).replacingOccurrences(of: "–", with: "-")
            guard !p.isEmpty else { return nil }
            let ends = p.split(separator: "-", omittingEmptySubsequences: false).map { $0.trimmingCharacters(in: .whitespaces) }
            guard ends.count <= 2, let a = Int(ends[0].isEmpty ? "1" : ends[0]) else { return nil }
            let b = ends.count == 2 ? (ends[1].isEmpty ? pageCount : Int(ends[1]) ?? -1) : a
            guard a >= 1, b >= a, b <= pageCount else { return nil }
            groups.append(Array((a - 1)..<b))
        }
        return groups.isEmpty ? nil : groups
    }

    /// Page indices of a range field; empty text means all pages.
    static func pageIndices(_ text: String, pageCount: Int) -> [Int]? {
        guard pageCount >= 0 else { return nil }
        if text.trimmingCharacters(in: .whitespaces).isEmpty { return Array(0..<pageCount) }
        return parseRanges(text, pageCount: pageCount).map { Array(Set($0.joined())).sorted() }
    }

    static func everyN(_ n: Int, pageCount: Int) -> [[Int]] {
        guard n > 0, pageCount > 0 else { return [] }
        return stride(from: 0, to: pageCount, by: n).map { start in
            Array(start..<(start + min(n, pageCount - start)))
        }
    }

    // MARK: - Split and merge

    /// One new document per group of page indices (pages copied, annotations included).
    static func split(_ pdf: PDFDocument, groups: [[Int]]) -> [PDFDocument] {
        groups.map { group in
            let part = PDFDocument()
            for i in group { if let p = pdf.page(at: i)?.copy() as? PDFPage { part.insert(p, at: part.pageCount) } }
            return part
        }
    }

    /// File name for a split part: "Report — 1-3.pdf" / "Report — 4.pdf".
    static func partName(_ base: String, _ group: [Int]) -> String {
        guard let first = group.first, let last = group.last else { return base + ".pdf" }
        return first == last ? "\(base) — \(first + 1).pdf" : "\(base) — \(first + 1)-\(last + 1).pdf"
    }

    /// Pages of a PDF or an image file (an image becomes one A4 page).
    static func pages(of url: URL) -> [PDFPage] {
        if UTType(filenameExtension: url.pathExtension)?.conforms(to: .image) == true {
            return NSImage(contentsOf: url).flatMap(PDFDoc.page(from:)).map { [$0] } ?? []
        }
        guard let doc = PDFDocument(url: url), !doc.isLocked else { return [] }
        return (0..<doc.pageCount).compactMap { doc.page(at: $0)?.copy() as? PDFPage }
    }

    /// Preflight every input: a failed merge must not quietly omit a chosen file.
    static func mergeChecked(_ urls: [URL]) throws -> PDFDocument {
        guard !urls.isEmpty else { throw NSError(domain: NSCocoaErrorDomain, code: NSFileReadCorruptFileError) }
        let inputs = try urls.map { url -> [PDFPage] in
            let pages = pages(of: url)
            guard !pages.isEmpty else {
                throw NSError(domain: NSCocoaErrorDomain, code: NSFileReadCorruptFileError,
                              userInfo: [NSURLErrorKey: url])
            }
            return pages
        }
        let out = PDFDocument()
        for pages in inputs { for page in pages { out.insert(page, at: out.pageCount) } }
        return out
    }

    static func merge(_ urls: [URL]) -> PDFDocument {
        let out = PDFDocument()
        for url in urls { for p in pages(of: url) { out.insert(p, at: out.pageCount) } }
        return out
    }

    // MARK: - Form fields

    enum FieldKind { case text, checkBox, choice }

    /// New AcroForm widget. PDFKit writes it to the file's /AcroForm, so Acrobat and Preview can fill it.
    static func makeField(_ kind: FieldKind, name: String, bounds: CGRect, options: [String] = []) -> PDFAnnotation {
        let w = PDFAnnotation(bounds: bounds, forType: .widget, withProperties: nil)
        let border = PDFBorder()
        border.lineWidth = 1
        w.border = border
        w.color = NSColor(white: 0.45, alpha: 1)
        w.backgroundColor = NSColor(srgbRed: 0.9, green: 0.94, blue: 1, alpha: 1)
        switch kind {
        case .text:
            w.widgetFieldType = .text
            w.font = NSFont(name: "Helvetica", size: min(12, max(7, bounds.height * 0.6)))
            w.fontColor = .black
            w.widgetStringValue = ""
        case .checkBox:
            w.widgetFieldType = .button
            w.widgetControlType = .checkBoxControl
            w.buttonWidgetState = .offState
            w.buttonWidgetStateString = "Yes"
        case .choice:
            w.widgetFieldType = .choice
            w.isListChoice = false
            w.choices = options
            w.font = NSFont(name: "Helvetica", size: min(12, max(7, bounds.height * 0.6)))
            w.fontColor = .black
            w.widgetStringValue = options.first ?? ""
        }
        w.fieldName = name   // after the type: setting the type resets the name
        w.userName = NSFullUserName()
        w.modificationDate = Date()
        return w
    }

    /// All widgets of the document: (page index, widget).
    static func fields(_ pdf: PDFDocument) -> [(Int, PDFAnnotation)] {
        (0..<pdf.pageCount).flatMap { i in (pdf.page(at: i)?.annotations ?? []).filter { $0.isKind(.widget) }.map { (i, $0) } }
    }

    /// Name not yet used by a field: "Text1", "Text2"…
    static func uniqueFieldName(_ base: String, in pdf: PDFDocument) -> String {
        let used = Set(fields(pdf).compactMap(\.1.fieldName))
        return (1...).lazy.map { "\(base)\($0)" }.first { !used.contains($0) }!
    }

    static func value(of field: PDFAnnotation) -> String {
        if field.widgetFieldType == .button { return field.buttonWidgetState == .onState ? "on" : "off" }
        return field.widgetStringValue ?? ""
    }

    static func setValue(_ value: String, of field: PDFAnnotation) {
        if field.widgetFieldType == .button {
            field.buttonWidgetState = ["1", "on", "yes", "true"].contains(value.lowercased()) ? .onState : .offState
        } else {
            field.widgetStringValue = value
        }
    }

    // MARK: - Compare

    /// A word of page text with its character range in `page.string`.
    struct Word { let text: String; let range: NSRange }

    static func words(_ page: PDFPage?) -> [Word] {
        guard let s = page?.string else { return [] }
        let ns = s as NSString
        var out: [Word] = []
        ns.enumerateSubstrings(in: NSRange(location: 0, length: ns.length), options: [.byWords, .substringNotRequired]) { _, range, _, _ in
            out.append(Word(text: ns.substring(with: range), range: range))
        }
        return out
    }

    /// A run of changed words on one page: `removed` are in this document, `added` in the other.
    struct Change {
        let page: Int
        let removed: [Word]
        let added: [String]
    }

    /// Word-level differences, page by page (page N of this document against page N of the other).
    static func compare(_ a: PDFDocument, _ b: PDFDocument) -> [Change] {
        var changes: [Change] = []
        for i in 0..<max(a.pageCount, b.pageCount) {
            let old = words(i < a.pageCount ? a.page(at: i) : nil)
            let new = words(i < b.pageCount ? b.page(at: i) : nil).map(\.text)
            let diff = new.difference(from: old.map(\.text))
            var removed = Set<Int>(), inserted = Set<Int>()
            for c in diff {
                switch c {
                case .remove(let o, _, _): removed.insert(o)
                case .insert(let o, _, _): inserted.insert(o)
                }
            }
            var x = 0, y = 0
            while x < old.count || y < new.count {
                var r: [Word] = [], ad: [String] = []
                while x < old.count, removed.contains(x) { r.append(old[x]); x += 1 }
                while y < new.count, inserted.contains(y) { ad.append(new[y]); y += 1 }
                if r.isEmpty, ad.isEmpty { x += 1; y += 1 } else { changes.append(Change(page: i, removed: r, added: ad)) }
            }
        }
        return changes
    }
}
