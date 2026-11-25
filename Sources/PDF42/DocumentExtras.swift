import AppKit
import CoreGraphics
import Kit42
import PDFKit
import UniformTypeIdentifiers

// Document-level parts of a PDF that PDFKit does not write (or writes wrongly) on this macOS:
// the outline after edits, the Info dictionary, XMP metadata, page labels, initial view settings,
// embedded files and GoTo destinations of links. They are read once when the file is opened
// (`DocumentExtras.read`), edited in the UI and written after PDFKit has saved the file
// (`DocumentWriter.apply`), as an incremental update that is then compacted into a clean file.

// MARK: - Page labels

/// One /PageLabels range: from page index `start` on, numbers `first`, `first + 1`… in `style`, after `prefix`.
struct PageLabelRange: Equatable {
    enum Style: String, CaseIterable {
        case decimal = "D", upperRoman = "R", lowerRoman = "r", upperLetters = "A", lowerLetters = "a", none = ""

        var title: String {
            switch self {
            case .decimal: return "1, 2, 3"
            case .upperRoman: return "I, II, III"
            case .lowerRoman: return "i, ii, iii"
            case .upperLetters: return "A, B, C"
            case .lowerLetters: return "a, b, c"
            case .none: return L("Prefix only")
            }
        }
    }
    var start: Int
    var style: Style = .decimal
    var prefix = ""
    var first = 1
}

enum PageLabels {
    /// Logical number of page `index`, nil when the document has no labels.
    static func label(_ index: Int, _ ranges: [PageLabelRange]) -> String? {
        guard let r = ranges.filter({ $0.start <= index }).max(by: { $0.start < $1.start }) else { return nil }
        let (delta, subtractOverflow) = index.subtractingReportingOverflow(r.start)
        let (number, addOverflow) = r.first.addingReportingOverflow(delta)
        guard !subtractOverflow, !addOverflow, number > 0 else { return nil }
        return r.prefix + format(number, r.style)
    }

    static func format(_ n: Int, _ style: PageLabelRange.Style) -> String {
        switch style {
        case .decimal: return "\(n)"
        case .upperRoman: return roman(n)
        case .lowerRoman: return roman(n).lowercased()
        case .upperLetters: return letters(n)
        case .lowerLetters: return letters(n).lowercased()
        case .none: return ""
        }
    }

    static func roman(_ n: Int) -> String {
        guard n > 0, n < 5000 else { return "\(n)" }
        let table: [(Int, String)] = [(1000, "M"), (900, "CM"), (500, "D"), (400, "CD"), (100, "C"), (90, "XC"),
                                      (50, "L"), (40, "XL"), (10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I")]
        var v = n, s = ""
        for (k, r) in table { while v >= k { s += r; v -= k } }
        return s
    }

    /// PDF letter numbering: A…Z, then AA…ZZ, AAA… (the letter repeated).
    static func letters(_ n: Int) -> String {
        guard n > 0 else { return "" }
        let letter = Character(UnicodeScalar(UInt8(65 + (n - 1) % 26)))
        let count = (n - 1) / 26 + 1
        // Malformed or extreme /St values must not allocate gigabytes for one thumbnail caption.
        guard count <= 4096 else { return "\(n)" }
        return String(repeating: letter, count: count)
    }

    /// Page index of the page whose label (or number) is `text`; labels first, then plain numbers.
    static func index(of text: String, _ ranges: [PageLabelRange], pageCount: Int) -> Int? {
        let t = text.trimmingCharacters(in: .whitespaces)
        guard !t.isEmpty else { return nil }
        if !ranges.isEmpty, let i = (0..<pageCount).first(where: { label($0, ranges)?.caseInsensitiveCompare(t) == .orderedSame }) { return i }
        if let n = Int(t), n >= 1, n <= pageCount { return n - 1 }
        return nil
    }

    /// "1-4:r;5-:D;9-:D:A-:1" → ranges. Each part: first page (from 1)[-last]:style[:prefix[:start number]].
    /// Styles D R r A a, or "-" for a prefix only.
    static func parse(_ spec: String, pageCount: Int) -> [PageLabelRange]? {
        var out: [PageLabelRange] = []
        for part in spec.split(separator: ";").map({ $0.trimmingCharacters(in: .whitespaces) }) where !part.isEmpty {
            let fields = part.split(separator: ":", omittingEmptySubsequences: false).map(String.init)
            guard let pages = fields.first, let first = Int(pages.split(separator: "-").first ?? ""), first >= 1, first <= pageCount else { return nil }
            var r = PageLabelRange(start: first - 1)
            if fields.count > 1 {
                guard let style = fields[1] == "-" ? PageLabelRange.Style.none : PageLabelRange.Style(rawValue: fields[1]) else { return nil }
                r.style = style
            }
            if fields.count > 2 { r.prefix = fields[2] }
            if fields.count > 3 { guard let n = Int(fields[3]), n >= 1 else { return nil }; r.first = n }
            out.append(r)
        }
        out.sort { $0.start < $1.start }
        return out
    }

    /// Back to the text form of `parse`.
    static func describe(_ ranges: [PageLabelRange]) -> String {
        ranges.map { r in
            var s = "\(r.start + 1)-:\(r.style == .none ? "-" : r.style.rawValue)"
            if !r.prefix.isEmpty || r.first != 1 { s += ":\(r.prefix)" }
            if r.first != 1 { s += ":\(r.first)" }
            return s
        }.joined(separator: ";")
    }
}

// MARK: - Model

struct DocumentInfo: Equatable {
    var title = "", author = "", subject = "", keywords = "", creator = "", producer = ""
    var created: Date?
    var modified: Date?
}

/// Acrobat's "Initial View": how viewers open the document.
struct InitialView: Equatable {
    /// /PageLayout: "" (not set), SinglePage, OneColumn, TwoColumnLeft, TwoColumnRight, TwoPageLeft, TwoPageRight.
    var pageLayout = ""
    /// /PageMode: "" (not set), UseNone, UseOutlines, UseThumbs, UseAttachments, FullScreen.
    var pageMode = ""
    var fitWindow = false, centerWindow = false, displayDocTitle = false
    var hideToolbar = false, hideMenubar = false
    /// /OpenAction destination: page index and magnification ("" = default, Fit, FitH, FitV or a percentage).
    var openPage = 0
    var openZoom = ""

    static let layouts = ["", "SinglePage", "OneColumn", "TwoColumnLeft", "TwoColumnRight", "TwoPageLeft", "TwoPageRight"]
    static let modes = ["", "UseNone", "UseOutlines", "UseThumbs", "UseAttachments", "FullScreen"]
}

/// File in the document's /EmbeddedFiles name tree.
struct EmbeddedFile: Equatable {
    var name: String
    var data: Data
    var desc = ""
    var modified: Date?
    var mime: String?
}

/// File carried by a FileAttachment annotation on a page (the annotation itself is a PDFKit object).
struct AnnotationFile {
    var page: Int
    var rect: CGRect
    var name: String
    var data: Data
    var annotation: PDFAnnotation? = nil
}

struct OutlineStyle: Equatable {
    var color: NSColor?
    var bold = false
    var italic = false
    var isPlain: Bool { color == nil && !bold && !italic }
}

struct DocumentExtras {
    enum Part: Hashable { case info, view, labels, files, outline }

    var info = DocumentInfo()
    var view = InitialView()
    var labels: [PageLabelRange] = []
    var files: [EmbeddedFile] = []
    var annotationFiles: [AnnotationFile] = []
    /// Colour and bold/italic of outline items (/C, /F), by PDFOutline identity.
    var outlineStyles: [ObjectIdentifier: OutlineStyle] = [:]
    /// Links whose destination should be written as /Fit or /FitH (default: /XYZ at the point).
    var linkFit: [ObjectIdentifier: String] = [:]
    /// What changed since the file was read: only those parts are rewritten.
    var dirty: Set<Part> = []
    /// Header version, encryption, file ID as read (for the Properties window).
    var version = ""
    var hadXMP = false

    func annotationFile(for annotation: PDFAnnotation, on page: Int) -> AnnotationFile? {
        annotationFiles.first { $0.annotation === annotation }
            ?? annotationFiles.first { $0.page == page && $0.annotation == nil && abs($0.rect.minX - annotation.bounds.minX) < 1 && abs($0.rect.minY - annotation.bounds.minY) < 1 }
    }

    func label(_ index: Int) -> String? { PageLabels.label(index, labels) }

    /// Page number as shown to the user: the label, with the physical number when they differ.
    func displayLabel(_ index: Int) -> String {
        guard let l = label(index), l != "\(index + 1)" else { return "\(index + 1)" }
        return l
    }
}

// MARK: - Reading

extension DocumentExtras {
    static func read(_ data: Data, pdf: PDFDocument) -> DocumentExtras {
        var x = DocumentExtras()
        guard let provider = CGDataProvider(data: data as CFData), let cg = CGPDFDocument(provider) else { return x }
        if cg.isEncrypted, !cg.isUnlocked { _ = cg.unlockWithPassword("") }
        var major: Int32 = 0, minor: Int32 = 0
        cg.getVersion(majorVersion: &major, minorVersion: &minor)
        x.version = "\(major).\(minor)"
        if let info = cg.info {
            func s(_ k: String) -> String { PDFRead.text(info, k) ?? "" }
            x.info = DocumentInfo(title: s("Title"), author: s("Author"), subject: s("Subject"), keywords: s("Keywords"),
                                  creator: s("Creator"), producer: s("Producer"),
                                  created: PDFRead.text(info, "CreationDate").flatMap(PDFDate.parse),
                                  modified: PDFRead.text(info, "ModDate").flatMap(PDFDate.parse))
        }
        guard let catalog = cg.catalog else { return x }
        x.hadXMP = CGPDF.stream(catalog, "Metadata") != nil
        // Page labels (number tree).
        if let tree = CGPDF.dict(catalog, "PageLabels") {
            var pairs: [(Int, CGPDFDictionaryRef)] = []
            PDFRead.numberTree(tree, depth: 0) { key, obj in
                var d: CGPDFDictionaryRef?
                if CGPDFObjectGetValue(obj, .dictionary, &d), let d { pairs.append((key, d)) }
            }
            x.labels = pairs.sorted { $0.0 < $1.0 }.compactMap { key, d in
                let value = CGPDF.number(d, "St") ?? 1
                guard key >= 0, key < pdf.pageCount, value.isFinite, value >= 1, value < Double(Int.max) else { return nil }
                return PageLabelRange(start: key, style: CGPDF.name(d, "S").flatMap(PageLabelRange.Style.init) ?? .none,
                                      prefix: PDFRead.text(d, "P") ?? "", first: Int(value))
            }
        }
        // Initial view.
        x.view.pageLayout = CGPDF.name(catalog, "PageLayout") ?? ""
        x.view.pageMode = CGPDF.name(catalog, "PageMode") ?? ""
        if let vp = CGPDF.dict(catalog, "ViewerPreferences") {
            x.view.fitWindow = CGPDF.bool(vp, "FitWindow")
            x.view.centerWindow = CGPDF.bool(vp, "CenterWindow")
            x.view.displayDocTitle = CGPDF.bool(vp, "DisplayDocTitle")
            x.view.hideToolbar = CGPDF.bool(vp, "HideToolbar")
            x.view.hideMenubar = CGPDF.bool(vp, "HideMenubar")
        }
        if let open = CGPDF.array(catalog, "OpenAction"), CGPDFArrayGetCount(open) >= 2 {
            var pageDict: CGPDFDictionaryRef?
            if CGPDFArrayGetDictionary(open, 0, &pageDict), let pageDict {
                for i in 0..<cg.numberOfPages where cg.page(at: i + 1)?.dictionary == pageDict { x.view.openPage = i }
            }
            let kind = CGPDF.name(open, 1) ?? ""
            if kind == "XYZ", CGPDFArrayGetCount(open) >= 5 {
                var z: CGPDFReal = 0
                if CGPDFArrayGetNumber(open, 4, &z), z.isFinite, z > 0, z <= 100 { x.view.openZoom = "\(Int((z * 100).rounded()))" }
            } else if let fit = ["Fit": "Fit", "FitB": "Fit", "FitH": "FitH", "FitBH": "FitH", "FitV": "FitV", "FitBV": "FitV"][kind] {
                x.view.openZoom = fit
            }
        }
        // Embedded files.
        if let names = CGPDF.dict(catalog, "Names"), let tree = CGPDF.dict(names, "EmbeddedFiles") {
            PDFRead.nameTree(tree, depth: 0) { key, obj in
                var spec: CGPDFDictionaryRef?
                guard CGPDFObjectGetValue(obj, .dictionary, &spec), let spec, let file = PDFRead.embeddedFile(spec, fallbackName: key) else { return }
                x.files.append(file)
            }
        }
        // FileAttachment annotations.
        for i in 0..<cg.numberOfPages {
            guard let page = cg.page(at: i + 1)?.dictionary, let annots = CGPDF.array(page, "Annots") else { continue }
            for k in 0..<CGPDFArrayGetCount(annots) {
                var a: CGPDFDictionaryRef?
                guard CGPDFArrayGetDictionary(annots, k, &a), let a, CGPDF.name(a, "Subtype") == "FileAttachment",
                      let fs = CGPDF.dict(a, "FS"), let file = PDFRead.embeddedFile(fs, fallbackName: L("Attachment")) else { continue }
                let r = CGPDF.numbers(CGPDF.array(a, "Rect"))
                let rect = r.count == 4 ? CGRect(x: min(r[0], r[2]), y: min(r[1], r[3]), width: abs(r[2] - r[0]), height: abs(r[3] - r[1])) : .zero
                let used = Set(x.annotationFiles.compactMap { $0.annotation.map(ObjectIdentifier.init) })
                let candidates: [PDFAnnotation] = pdf.page(at: i)?.annotations ?? []
                let match: PDFAnnotation? = candidates.first(where: { annotation in
                    guard annotation.kind == "FileAttachment", !used.contains(ObjectIdentifier(annotation)) else { return false }
                    let dx = abs(annotation.bounds.minX - rect.minX), dy = abs(annotation.bounds.minY - rect.minY)
                    return dx < 1 && dy < 1
                })
                x.annotationFiles.append(AnnotationFile(page: i, rect: rect, name: file.name, data: file.data, annotation: match))
            }
        }
        // Outline styles: the file's items in document order match PDFKit's tree in the same order.
        if let outlines = CGPDF.dict(catalog, "Outlines"), let root = pdf.outlineRoot {
            var styles: [OutlineStyle] = []
            PDFRead.outlineItems(outlines) { item in
                var st = OutlineStyle()
                let c = CGPDF.numbers(CGPDF.array(item, "C"))
                if c.count == 3, c != [0, 0, 0] { st.color = NSColor(srgbRed: c[0], green: c[1], blue: c[2], alpha: 1) }
                let raw = CGPDF.number(item, "F") ?? 0
                let f = raw.isFinite && raw >= 0 && raw <= 3 ? Int(raw) : 0
                st.italic = f & 1 != 0
                st.bold = f & 2 != 0
                styles.append(st)
            }
            let items = OutlineTree.flatten(root)
            if items.count == styles.count {
                for (item, st) in zip(items, styles) where !st.isPlain { x.outlineStyles[ObjectIdentifier(item)] = st }
            }
        }
        return x
    }
}

/// Reading helpers over CGPDF objects.
enum PDFRead {
    static func text(_ d: CGPDFDictionaryRef?, _ key: String) -> String? {
        guard let d else { return nil }
        var s: CGPDFStringRef?
        guard CGPDFDictionaryGetString(d, key, &s), let s, let t = CGPDFStringCopyTextString(s) else { return nil }
        return t as String
    }

    static func numberTree(_ node: CGPDFDictionaryRef, depth: Int, _ visit: (Int, CGPDFObjectRef) -> Void) {
        guard depth < 32 else { return }
        if let nums = CGPDF.array(node, "Nums") {
            var i = 0
            while i + 1 < CGPDFArrayGetCount(nums) {
                var k: CGPDFInteger = 0
                var obj: CGPDFObjectRef?
                if CGPDFArrayGetInteger(nums, i, &k), CGPDFArrayGetObject(nums, i + 1, &obj), let obj { visit(Int(k), obj) }
                i += 2
            }
        }
        if let kids = CGPDF.array(node, "Kids") {
            for i in 0..<CGPDFArrayGetCount(kids) {
                var kid: CGPDFDictionaryRef?
                if CGPDFArrayGetDictionary(kids, i, &kid), let kid { numberTree(kid, depth: depth + 1, visit) }
            }
        }
    }

    static func nameTree(_ node: CGPDFDictionaryRef, depth: Int, _ visit: (String, CGPDFObjectRef) -> Void) {
        guard depth < 32 else { return }
        if let names = CGPDF.array(node, "Names") {
            var i = 0
            while i + 1 < CGPDFArrayGetCount(names) {
                var s: CGPDFStringRef?
                var obj: CGPDFObjectRef?
                if CGPDFArrayGetString(names, i, &s), let s, CGPDFArrayGetObject(names, i + 1, &obj), let obj {
                    visit((CGPDFStringCopyTextString(s) as String?) ?? "", obj)
                }
                i += 2
            }
        }
        if let kids = CGPDF.array(node, "Kids") {
            for i in 0..<CGPDFArrayGetCount(kids) {
                var kid: CGPDFDictionaryRef?
                if CGPDFArrayGetDictionary(kids, i, &kid), let kid { nameTree(kid, depth: depth + 1, visit) }
            }
        }
    }

    /// File specification with an embedded stream (/EF /F or /UF).
    static func embeddedFile(_ spec: CGPDFDictionaryRef, fallbackName: String) -> EmbeddedFile? {
        guard let ef = CGPDF.dict(spec, "EF"), let stream = CGPDF.stream(ef, "F") ?? CGPDF.stream(ef, "UF") else { return nil }
        var format = CGPDFDataFormat.raw
        guard let data = CGPDFStreamCopyData(stream, &format) as Data? else { return nil }
        let name = [text(spec, "UF"), text(spec, "F")].compactMap { $0 }.first { !$0.isEmpty } ?? fallbackName
        let dict = CGPDFStreamGetDictionary(stream)
        let params = CGPDF.dict(dict, "Params")
        return EmbeddedFile(name: name, data: data, desc: text(spec, "Desc") ?? "",
                            modified: text(params, "ModDate").flatMap(PDFDate.parse),
                            mime: CGPDF.name(dict, "Subtype").map { AcroFormFixer.unescapeName($0) })
    }

    /// Outline items in document order (depth first).
    static func outlineItems(_ parent: CGPDFDictionaryRef, depth: Int = 0, _ visit: (CGPDFDictionaryRef) -> Void) {
        guard depth < 64 else { return }
        var item = CGPDF.dict(parent, "First")
        var guardCount = 0
        while let it = item, guardCount < 100_000 {
            visit(it)
            outlineItems(it, depth: depth + 1, visit)
            item = CGPDF.dict(it, "Next")
            guardCount += 1
        }
    }
}

/// PDF dates: "D:YYYYMMDDHHmmSSOHH'mm'".
enum PDFDate {
    static func parse(_ s: String) -> Date? {
        var t = s.trimmingCharacters(in: .whitespaces)
        if t.hasPrefix("D:") { t.removeFirst(2) }
        let digits = t.prefix { $0.isNumber }
        guard digits.count >= 4 else { return nil }
        func part(_ from: Int, _ len: Int, _ dflt: Int) -> Int {
            let a = Array(digits)
            guard a.count >= from + len else { return dflt }
            return Int(String(a[from..<from + len])) ?? dflt
        }
        var c = DateComponents()
        c.year = part(0, 4, 2000); c.month = part(4, 2, 1); c.day = part(6, 2, 1)
        c.hour = part(8, 2, 0); c.minute = part(10, 2, 0); c.second = part(12, 2, 0)
        let rest = t.dropFirst(digits.count)
        var offset = 0
        if let sign = rest.first, sign == "+" || sign == "-" {
            let nums = rest.dropFirst().split(whereSeparator: { !$0.isNumber }).compactMap { Int($0) }
            offset = ((nums.first ?? 0) * 3600 + (nums.count > 1 ? nums[1] : 0) * 60) * (sign == "-" ? -1 : 1)
        }
        c.timeZone = TimeZone(secondsFromGMT: offset)
        return Calendar(identifier: .gregorian).date(from: c)
    }

    static func pdf(_ date: Date) -> String {
        let tz = TimeZone.current.secondsFromGMT(for: date)
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.dateFormat = "yyyyMMddHHmmss"
        let sign = tz < 0 ? "-" : "+"
        return "D:" + f.string(from: date) + String(format: "%@%02d'%02d'", sign, abs(tz) / 3600, abs(tz) % 3600 / 60)
    }

    static func xmp(_ date: Date) -> String {
        let tz = TimeZone.current.secondsFromGMT(for: date)
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.dateFormat = "yyyy-MM-dd'T'HH:mm:ss"
        let sign = tz < 0 ? "-" : "+"
        return f.string(from: date) + String(format: "%@%02d:%02d", sign, abs(tz) / 3600, abs(tz) % 3600 / 60)
    }
}

/// Helpers on PDFKit's outline tree.
enum OutlineTree {
    /// Items in document order (depth first), without the root.
    static func flatten(_ root: PDFOutline) -> [PDFOutline] {
        var out: [PDFOutline] = []
        func walk(_ o: PDFOutline, _ depth: Int) {
            guard depth < 64 else { return }
            for i in 0..<o.numberOfChildren { if let c = o.child(at: i) { out.append(c); walk(c, depth + 1) } }
        }
        walk(root, 0)
        return out
    }

    /// Destination of an item: its own, or that of its GoTo action.
    static func destination(_ o: PDFOutline) -> PDFDestination? {
        o.destination ?? (o.action as? PDFActionGoTo)?.destination
    }
}

// MARK: - Writing

/// PDF/A requirements applied by `DocumentWriter` (see `PDFAExport`).
struct PDFAOptions {
    var removedAttachments = 0
}

enum DocumentWriter {
    /// `data` (as PDFKit and AcroFormFixer wrote it) with the document parts of `extras` written in,
    /// links with GoTo destinations repaired, and — for PDF/A — the archive requirements applied.
    /// The result is compacted (one cross-reference table, nothing unreachable left).
    static func apply(_ data: Data, extras: DocumentExtras, pdf: PDFDocument, pdfa: Bool = false) -> Data {
        guard let w = Writer(data) else { return data }
        w.write(extras: extras, pdf: pdf, pdfa: pdfa)
        let out = w.finish(data)
        guard w.changed else { return data }
        return PDFCompactor.compact(out) ?? out
    }
}

/// Incremental-update writer over a classic cross-reference file.
final class Writer {
    let file: PDFFileStructure
    let rootNum: Int
    var catalog: PDFObject
    var trailer: PDFObject
    var updates: [Int: [UInt8]] = [:]
    var nextNum: Int
    private(set) var pages: [Int] = []
    var changed = false
    let size: Int

    init?(_ data: Data) {
        guard let file = try? PDFFileStructure(data), case .ref(let root, _)? = file.trailer["Root"],
              let size = file.trailer["Size"]?.int, let catalog = try? file.object(root) else { return nil }
        self.file = file
        self.rootNum = root
        self.catalog = catalog
        self.trailer = file.trailer
        self.size = size
        self.nextNum = size
        pages = pageObjects()
    }

    func object(_ num: Int) -> PDFObject? { try? file.object(num) }
    func resolve(_ o: PDFObject?) -> PDFObject? { (try? file.resolve(o)) ?? nil }

    private func pageObjects() -> [Int] {
        guard case .ref(let pagesRoot, _)? = catalog["Pages"] else { return [] }
        var out: [Int] = []
        var seen = Set<Int>()
        func walk(_ num: Int, depth: Int) {
            guard depth < 64, !seen.contains(num), let node = object(num) else { return }
            seen.insert(num)
            if node["Type"]?.nameValue != "Page", let kids = resolve(node["Kids"])?.items {
                for case .ref(let k, _) in kids { walk(k, depth: depth + 1) }
            } else { out.append(num) }
        }
        walk(pagesRoot, depth: 0)
        return out
    }

    @discardableResult
    func add(_ object: PDFObject) -> Int {
        let num = nextNum
        nextNum += 1
        updates[num] = Array(object.serialized.utf8)
        changed = true
        return num
    }

    func set(_ num: Int, _ object: PDFObject) {
        updates[num] = Array(object.serialized.utf8)
        changed = true
    }

    func addStream(_ dict: PDFObject, _ bytes: [UInt8], compress: Bool = true) -> Int {
        var d = dict.removing("Length").removing("Filter").removing("DecodeParms")
        var body = bytes
        if compress, let z = zlibCompress(bytes) { body = z; d = d.setting("Filter", .name("FlateDecode")) }
        d = d.setting("Length", .raw("\(body.count)"))
        let num = nextNum
        nextNum += 1
        updates[num] = Array(d.serialized.utf8) + Array("\nstream\n".utf8) + body + Array("\nendstream".utf8)
        changed = true
        return num
    }

    /// Dictionary and raw (still encoded) bytes of stream object `num`.
    func stream(_ num: Int) -> (PDFObject, [UInt8])? {
        guard let entry = file.offsets[num], entry.offset > 0 else { return nil }
        var lexer = PDFLexer(bytes: file.bytes, pos: entry.offset)
        _ = lexer.word(); _ = lexer.word()
        guard (try? lexer.expect("obj")) != nil, let dict = try? lexer.parse(), case .dict = dict else { return nil }
        lexer.skipWhitespace()
        guard lexer.peek("stream") else { return nil }
        var start = lexer.pos + 6
        if start < file.bytes.count, file.bytes[start] == 0x0D { start += 1 }
        if start < file.bytes.count, file.bytes[start] == 0x0A { start += 1 }
        var length: Int?
        switch dict["Length"] {
        case .raw(let s)?: length = Int(s)
        case .ref(let n, _)?: length = object(n)?.int
        default: break
        }
        if let length, length >= 0, length <= file.bytes.count - start { return (dict, Array(file.bytes[start..<start + length])) }
        guard let end = AcroFormFixer.find("endstream", in: file.bytes, from: start) else { return nil }
        var stop = end
        if stop > start, file.bytes[stop - 1] == 0x0A { stop -= 1 }
        if stop > start, file.bytes[stop - 1] == 0x0D { stop -= 1 }
        return (dict, Array(file.bytes[start..<stop]))
    }

    /// Rewrites stream `num` with a changed dictionary, keeping its bytes.
    func setStreamDict(_ num: Int, _ change: (PDFObject) -> PDFObject) {
        guard let (dict, bytes) = stream(num) else { return }
        let d = change(dict).setting("Length", .raw("\(bytes.count)"))
        updates[num] = Array(d.serialized.utf8) + Array("\nstream\n".utf8) + bytes + Array("\nendstream".utf8)
        changed = true
    }

    /// Current version of an object: the one rewritten here, or the file's.
    func current(_ num: Int) -> PDFObject? {
        if let u = updates[num], let s = String(bytes: u, encoding: .isoLatin1), !s.contains("\nstream\n") {
            var lx = PDFLexer(bytes: u, pos: 0)
            return try? lx.parse()
        }
        return object(num)
    }

    func finish(_ data: Data) -> Data {
        guard changed else { return data }
        set(rootNum, catalog)
        var out = [UInt8](data)
        if out.last != 0x0A { out.append(0x0A) }
        var entries: [(num: Int, offset: Int)] = []
        for num in updates.keys.sorted() {
            entries.append((num, out.count))
            out += Array("\(num) \(num >= size ? 0 : file.gen(num)) obj\n".utf8) + updates[num]! + Array("\nendobj\n".utf8)
        }
        let xrefOffset = out.count
        var xref = "xref\n"
        for e in entries { xref += "\(e.num) 1\n" + String(format: "%010d %05d n\r\n", e.offset, e.num >= size ? 0 : file.gen(e.num)) }
        let t = trailer.removing("XRefStm").removing("Prev")
            .setting("Size", .raw("\(nextNum)")).setting("Prev", .raw("\(file.startxref)"))
        xref += "trailer\n\(t.serialized)\nstartxref\n\(xrefOffset)\n%%EOF\n"
        out += Array(xref.utf8)
        return Data(out)
    }

    // MARK: Parts

    /// Parts are written when edited, and also when PDFKit dropped them (it loses page labels, the
    /// initial view and attachments when pages are replaced, and leaves the outline on old pages).
    func write(extras x: DocumentExtras, pdf: PDFDocument, pdfa: Bool) {
        let names = resolve(catalog["Names"])
        if x.dirty.contains(.info) || pdfa || (trailer["Info"] == nil && x.info != DocumentInfo()) { writeInfo(x.info, pdfa: pdfa) }
        if x.dirty.contains(.labels) || (!x.labels.isEmpty && catalog["PageLabels"] == nil) { writeLabels(x.labels) }
        // PDFKit may discard OpenAction even when no PageLayout/PageMode was authored.
        if x.dirty.contains(.view) || x.view != InitialView() { writeView(x.view) }
        if x.dirty.contains(.files) || pdfa || (!x.files.isEmpty && names?["EmbeddedFiles"] == nil) { writeFiles(pdfa ? [] : x.files) }
        if x.dirty.contains(.outline) || needsOutline(pdf) { writeOutline(pdf, styles: x.outlineStyles) }
        repairLinks(pdf, fit: x.linkFit)
    }

    /// Outline in the model that the file lacks (PDFKit never writes a new outline), or whose
    /// entries point at pages that are no longer in the document.
    private func needsOutline(_ pdf: PDFDocument) -> Bool {
        guard let root = pdf.outlineRoot, root.numberOfChildren > 0 else { return false }
        guard let outlines = resolve(catalog["Outlines"]) else { return true }
        let pageSet = Set(pages)
        var stale = false
        var seen = Set<Int>()
        func walk(_ first: PDFObject?, depth: Int) {
            var next = first
            while case .ref(let n, _)? = next, !stale, depth < 64, seen.insert(n).inserted, let item = object(n) {
                let dest = resolve(item["Dest"]) ?? resolve(item["A"])?["D"]
                if let items = dest?.items, let head = items.first {
                    if case .ref(let p, _) = head { if !pageSet.contains(p) { stale = true } } else if case .name = head { stale = true }
                }
                walk(item["First"], depth: depth + 1)
                next = item["Next"]
            }
        }
        walk(outlines["First"], depth: 0)
        return stale
    }

    func writeInfo(_ info: DocumentInfo, pdfa: Bool) {
        let now = Date()
        let created = info.created ?? now
        var pairs: [(String, PDFObject)] = []
        func put(_ k: String, _ v: String) { if !v.isEmpty { pairs.append((k, .raw(pdfTextString(v)))) } }
        put("Title", info.title); put("Author", info.author); put("Subject", info.subject); put("Keywords", info.keywords)
        put("Creator", info.creator)
        let producer = info.producer.isEmpty ? "PDF42" : info.producer
        put("Producer", producer)
        pairs.append(("CreationDate", .raw("(\(PDFDate.pdf(created)))")))
        pairs.append(("ModDate", .raw("(\(PDFDate.pdf(now)))")))
        trailer = trailer.setting("Info", .ref(add(.dict(pairs)), 0))
        var withDates = info
        withDates.created = created
        withDates.modified = now
        withDates.producer = producer
        let xmp = XMP.packet(withDates, pdfa: pdfa, id: documentID)
        let meta = addStream(.dict([("Type", .name("Metadata")), ("Subtype", .name("XML"))]), Array(xmp.utf8), compress: false)
        catalog = catalog.setting("Metadata", .ref(meta, 0))
    }

    private var documentID: String {
        if let first = trailer["ID"]?.items?.first, case .raw(let s) = first, s.hasPrefix("<") {
            let hex = s.dropFirst().dropLast().lowercased()
            if hex.count == 32 {
                let a = Array(hex)
                return "uuid:" + [a[0..<8], a[8..<12], a[12..<16], a[16..<20], a[20..<32]].map { String($0) }.joined(separator: "-")
            }
        }
        return "uuid:" + UUID().uuidString.lowercased()
    }

    func writeLabels(_ labels: [PageLabelRange]) {
        guard !labels.isEmpty else { catalog = catalog.removing("PageLabels"); changed = true; return }
        var nums: [PDFObject] = []
        var sorted = labels.sorted { $0.start < $1.start }
        if sorted.first?.start != 0 { sorted.insert(PageLabelRange(start: 0), at: 0) }   // the tree must cover page 1
        for r in sorted {
            var d: [(String, PDFObject)] = [("Type", .name("PageLabel"))]
            if r.style != .none { d.append(("S", .name(r.style.rawValue))) }
            if !r.prefix.isEmpty { d.append(("P", .raw(pdfTextString(r.prefix)))) }
            if r.first != 1 { d.append(("St", .raw("\(r.first)"))) }
            nums += [.raw("\(r.start)"), .dict(d)]
        }
        catalog = catalog.setting("PageLabels", .dict([("Nums", .array(nums))]))
        changed = true
    }

    func writeView(_ v: InitialView) {
        catalog = v.pageLayout.isEmpty ? catalog.removing("PageLayout") : catalog.setting("PageLayout", .name(v.pageLayout))
        catalog = v.pageMode.isEmpty ? catalog.removing("PageMode") : catalog.setting("PageMode", .name(v.pageMode))
        var vp = resolve(catalog["ViewerPreferences"]) ?? .dict([])
        for (k, on) in [("FitWindow", v.fitWindow), ("CenterWindow", v.centerWindow), ("DisplayDocTitle", v.displayDocTitle),
                        ("HideToolbar", v.hideToolbar), ("HideMenubar", v.hideMenubar)] {
            vp = on ? vp.setting(k, .raw("true")) : vp.removing(k)
        }
        if case .dict(let pairs) = vp, pairs.isEmpty { catalog = catalog.removing("ViewerPreferences") }
        else { catalog = catalog.setting("ViewerPreferences", vp) }
        // Open at a page / magnification; a JavaScript or other action stays when nothing is chosen.
        if (v.openPage > 0 || !v.openZoom.isEmpty), v.openPage >= 0, v.openPage < pages.count {
            let page = PDFObject.ref(pages[v.openPage], file.gen(pages[v.openPage]))
            let dest: [PDFObject]
            switch v.openZoom {
            case "Fit": dest = [page, .name("Fit")]
            case "FitH": dest = [page, .name("FitH"), .raw("null")]
            case "FitV": dest = [page, .name("FitV"), .raw("null")]
            case "": dest = [page, .name("XYZ"), .raw("null"), .raw("null"), .raw("null")]
            default: dest = [page, .name("XYZ"), .raw("null"), .raw("null"), .raw(formatPDFNumber((Double(v.openZoom) ?? 100) / 100))]
            }
            catalog = catalog.setting("OpenAction", .array(dest))
        } else if resolve(catalog["OpenAction"])?.items != nil {
            catalog = catalog.removing("OpenAction")
        }
        changed = true
    }

    /// Names dictionary of the catalog (direct or indirect) with `change` applied; empty ones go.
    func changeNames(_ change: (PDFObject) -> PDFObject) {
        var names = resolve(catalog["Names"]) ?? .dict([])
        names = change(names)
        if case .dict(let pairs) = names, pairs.isEmpty { catalog = catalog.removing("Names") }
        else if case .ref(let n, _)? = catalog["Names"] { set(n, names) }
        else { catalog = catalog.setting("Names", names) }
        changed = true
    }

    func writeFiles(_ files: [EmbeddedFile]) {
        var entries: [PDFObject] = []
        var used = Set<String>()
        for f in files.sorted(by: { $0.name.compare($1.name, options: .literal) == .orderedAscending }) {
            var key = f.name, n = 2
            while used.contains(key) { key = "\(f.name) (\(n))"; n += 1 }
            used.insert(key)
            var params: [(String, PDFObject)] = [("Size", .raw("\(f.data.count)"))]
            params.append(("ModDate", .raw("(\(PDFDate.pdf(f.modified ?? Date())))")))
            var dict: [(String, PDFObject)] = [("Type", .name("EmbeddedFile")), ("Params", .dict(params))]
            if let mime = f.mime ?? Self.mime(for: f.name) { dict.append(("Subtype", .name(Self.escapeName(mime)))) }
            let stream = addStream(.dict(dict), [UInt8](f.data))
            var spec: [(String, PDFObject)] = [("Type", .name("Filespec")), ("F", .raw(pdfTextString(key))), ("UF", .raw(pdfTextString(key))),
                                              ("EF", .dict([("F", .ref(stream, 0)), ("UF", .ref(stream, 0))]))]
            if !f.desc.isEmpty { spec.append(("Desc", .raw(pdfTextString(f.desc)))) }
            spec.append(("AFRelationship", .name("Unspecified")))
            entries += [.raw(pdfTextString(key)), .ref(add(.dict(spec)), 0)]
        }
        changeNames { names in
            entries.isEmpty ? names.removing("EmbeddedFiles") : names.setting("EmbeddedFiles", .dict([("Names", .array(entries))]))
        }
        if entries.isEmpty, catalog["PageMode"]?.nameValue == "UseAttachments" { catalog = catalog.removing("PageMode") }
    }

    static func mime(for name: String) -> String? {
        let ext = (name as NSString).pathExtension.lowercased()
        guard !ext.isEmpty, let type = UTTypeLookup.mime(ext) else { return nil }
        return type
    }

    /// "text/plain" → "text#2Fplain" (a name may not contain "/").
    static func escapeName(_ s: String) -> String {
        var out = ""
        for b in s.utf8 {
            if b < 0x21 || b > 0x7E || "#/()<>[]{}%".utf8.contains(b) { out += String(format: "#%02X", b) } else { out.append(Character(UnicodeScalar(b))) }
        }
        return out
    }

    // MARK: Outline

    func destination(_ d: PDFDestination?, pdf: PDFDocument, fit: String? = nil) -> PDFObject? {
        guard let d, let page = d.page else { return nil }
        let i = pdf.index(for: page)
        guard i != NSNotFound, i < pages.count else { return nil }
        let ref = PDFObject.ref(pages[i], file.gen(pages[i]))
        let u = kPDFDestinationUnspecifiedValue
        func num(_ v: CGFloat) -> PDFObject { v == u || v.isNaN ? .raw("null") : .raw(formatPDFNumber(Double(v))) }
        switch fit {
        case "Fit"?: return .array([ref, .name("Fit")])
        case "FitH"?: return .array([ref, .name("FitH"), num(d.point.y)])
        default:
            let zoom: PDFObject = d.zoom == u || d.zoom <= 0 || d.zoom.isNaN ? .raw("null") : .raw(formatPDFNumber(Double(d.zoom)))
            return .array([ref, .name("XYZ"), num(d.point.x), num(d.point.y), zoom])
        }
    }

    func action(_ a: PDFAction?, pdf: PDFDocument) -> PDFObject? {
        switch a {
        case let url as PDFActionURL:
            guard let u = url.url?.absoluteString else { return nil }
            return .dict([("S", .name("URI")), ("URI", .raw(Self.literal(u)))])
        case let named as PDFActionNamed:
            let n: String
            switch named.name {
            case .nextPage: n = "NextPage"
            case .previousPage: n = "PrevPage"
            case .firstPage: n = "FirstPage"
            case .lastPage: n = "LastPage"
            default: return nil
            }
            return .dict([("S", .name("Named")), ("N", .name(n))])
        case let remote as PDFActionRemoteGoTo:
            return .dict([("S", .name("GoToR")), ("F", .raw(pdfTextString(remote.url.lastPathComponent))),
                          ("D", .array([.raw("\(remote.pageIndex)"), .name("Fit")]))])
        case let goTo as PDFActionGoTo:
            return destination(goTo.destination, pdf: pdf).map { .dict([("S", .name("GoTo")), ("D", $0)]) }
        default:
            return nil
        }
    }

    /// PDF literal string (ASCII URLs).
    static func literal(_ s: String) -> String {
        "(" + s.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "(", with: "\\(").replacingOccurrences(of: ")", with: "\\)") + ")"
    }

    func writeOutline(_ pdf: PDFDocument, styles: [ObjectIdentifier: OutlineStyle]) {
        guard let root = pdf.outlineRoot, root.numberOfChildren > 0 else {
            if catalog["Outlines"] != nil { catalog = catalog.removing("Outlines"); changed = true }
            if catalog["PageMode"]?.nameValue == "UseOutlines" { catalog = catalog.removing("PageMode") }
            return
        }
        // Numbers first, so items can point at each other.
        let rootNum = nextNum
        nextNum += 1
        var numbers: [ObjectIdentifier: Int] = [:]
        for item in OutlineTree.flatten(root) { numbers[ObjectIdentifier(item)] = nextNum; nextNum += 1 }
        func visibleCount(_ o: PDFOutline) -> Int {
            (0..<o.numberOfChildren).reduce(0) { sum, i in
                guard let c = o.child(at: i) else { return sum }
                return sum + 1 + (c.isOpen ? visibleCount(c) : 0)
            }
        }
        func children(_ o: PDFOutline) -> [PDFOutline] { (0..<o.numberOfChildren).compactMap { o.child(at: $0) } }
        func write(_ o: PDFOutline, parent: Int) {
            let kids = children(o)
            for (k, c) in kids.enumerated() {
                let num = numbers[ObjectIdentifier(c)]!
                var d: [(String, PDFObject)] = [("Title", .raw(pdfTextString(c.label ?? ""))), ("Parent", .ref(parent, 0))]
                if k > 0 { d.append(("Prev", .ref(numbers[ObjectIdentifier(kids[k - 1])]!, 0))) }
                if k + 1 < kids.count { d.append(("Next", .ref(numbers[ObjectIdentifier(kids[k + 1])]!, 0))) }
                let grand = children(c)
                if let f = grand.first, let l = grand.last {
                    d.append(("First", .ref(numbers[ObjectIdentifier(f)]!, 0)))
                    d.append(("Last", .ref(numbers[ObjectIdentifier(l)]!, 0)))
                    let n = visibleCount(c)
                    d.append(("Count", .raw("\(c.isOpen ? n : -grand.count)")))
                }
                if let dest = destination(c.destination, pdf: pdf) { d.append(("Dest", dest)) }
                else if let a = action(c.action, pdf: pdf) { d.append(("A", a)) }
                if let st = styles[ObjectIdentifier(c)] {
                    if let col = st.color?.usingColorSpace(.sRGB) {
                        d.append(("C", .array([col.redComponent, col.greenComponent, col.blueComponent].map { .raw(formatPDFNumber(Double($0))) })))
                    }
                    let flags = (st.italic ? 1 : 0) | (st.bold ? 2 : 0)
                    if flags != 0 { d.append(("F", .raw("\(flags)"))) }
                }
                updates[num] = Array(PDFObject.dict(d).serialized.utf8)
                write(c, parent: num)
            }
        }
        write(root, parent: rootNum)
        let top = children(root)
        updates[rootNum] = Array(PDFObject.dict([("Type", .name("Outlines")), ("First", .ref(numbers[ObjectIdentifier(top.first!)]!, 0)),
                                                 ("Last", .ref(numbers[ObjectIdentifier(top.last!)]!, 0)),
                                                 ("Count", .raw("\(visibleCount(root))"))]).serialized.utf8)
        catalog = catalog.setting("Outlines", .ref(rootNum, 0))
        changed = true
    }

    // MARK: Links

    /// PDFKit writes the page of a GoTo destination as a bogus name: links pointing into the document
    /// are rewritten from the model (matched by page and rectangle).
    func repairLinks(_ pdf: PDFDocument, fit: [ObjectIdentifier: String]) {
        for (i, pageNum) in pages.enumerated() where i < pdf.pageCount {
            guard let model = pdf.page(at: i) else { continue }
            let links = model.annotations.filter { $0.isKind(.link) }
            guard !links.isEmpty, let page = current(pageNum), let annots = resolve(page["Annots"])?.items else { continue }
            for case .ref(let num, _) in annots {
                guard let a = current(num), a["Subtype"]?.nameValue == "Link",
                      let r = a["Rect"]?.items?.compactMap({ Double($0.rawString ?? "") }), r.count == 4 else { continue }
                let rect = CGRect(x: min(r[0], r[2]), y: min(r[1], r[3]), width: abs(r[2] - r[0]), height: abs(r[3] - r[1]))
                guard let link = links.first(where: { abs($0.bounds.minX - rect.minX) < 0.6 && abs($0.bounds.minY - rect.minY) < 0.6
                    && abs($0.bounds.width - rect.width) < 0.6 && abs($0.bounds.height - rect.height) < 0.6 }) else { continue }
                let kind = fit[ObjectIdentifier(link)]
                let fileDest = a["Dest"] ?? resolve(a["A"])?["D"]
                let broken = Self.isBroken(fileDest)
                let dest = link.destination ?? (link.action as? PDFActionGoTo)?.destination
                guard let dest, let page = dest.page else {
                    if broken { set(num, a.removing("Dest").removing("A")) }
                    continue
                }
                let i = pdf.index(for: page)
                guard i != NSNotFound, i < pages.count else { continue }
                // PDFKit can also point a link at the wrong page (after pages were replaced) or write /Dest next to /A.
                var target: PDFObject?
                if broken || kind != nil {
                    target = destination(dest, pdf: pdf, fit: kind)
                } else if let items = fileDest?.items, case .ref(let n, _)? = items.first {
                    if n != pages[i] { target = .array([.ref(pages[i], file.gen(pages[i]))] + items.dropFirst()) }
                    else if a["Dest"] != nil, a["A"] != nil { target = fileDest }
                }
                guard let target else { continue }
                let fixed = a.removing("Dest").setting("A", .dict([("S", .name("GoTo")), ("D", target)]))
                set(num, fixed)
            }
        }
    }

    static func isBroken(_ dest: PDFObject?) -> Bool {
        guard let first = dest?.items?.first else { return false }
        if case .name = first { return true }   // a page must be a reference (or a number for remote files)
        return false
    }
}

extension PDFObject {
    var rawString: String? { if case .raw(let s) = self { return s } else { return nil } }
}

/// MIME types for embedded files.
enum UTTypeLookup {
    static func mime(_ ext: String) -> String? {
        let known = ["pdf": "application/pdf", "txt": "text/plain", "csv": "text/csv", "xml": "application/xml", "json": "application/json",
                     "png": "image/png", "jpg": "image/jpeg", "jpeg": "image/jpeg", "zip": "application/zip",
                     "docx": "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                     "xlsx": "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"]
        if let m = known[ext] { return m }
        return UTType(filenameExtension: ext)?.preferredMIMEType
    }
}

/// XMP packet matching the Info dictionary (dc:title ↔ Title, dc:creator ↔ Author, …).
enum XMP {
    static func escape(_ s: String) -> String {
        s.replacingOccurrences(of: "&", with: "&amp;").replacingOccurrences(of: "<", with: "&lt;")
            .replacingOccurrences(of: ">", with: "&gt;").replacingOccurrences(of: "\"", with: "&quot;")
    }

    static func packet(_ info: DocumentInfo, pdfa: Bool, id: String) -> String {
        var body = ""
        body += "   <dc:format>application/pdf</dc:format>\n"
        if !info.title.isEmpty {
            body += "   <dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">\(escape(info.title))</rdf:li></rdf:Alt></dc:title>\n"
        }
        if !info.author.isEmpty {
            body += "   <dc:creator><rdf:Seq><rdf:li>\(escape(info.author))</rdf:li></rdf:Seq></dc:creator>\n"
        }
        if !info.subject.isEmpty {
            body += "   <dc:description><rdf:Alt><rdf:li xml:lang=\"x-default\">\(escape(info.subject))</rdf:li></rdf:Alt></dc:description>\n"
        }
        if !info.keywords.isEmpty { body += "   <pdf:Keywords>\(escape(info.keywords))</pdf:Keywords>\n" }
        if !info.producer.isEmpty { body += "   <pdf:Producer>\(escape(info.producer))</pdf:Producer>\n" }
        if !info.creator.isEmpty { body += "   <xmp:CreatorTool>\(escape(info.creator))</xmp:CreatorTool>\n" }
        if let c = info.created { body += "   <xmp:CreateDate>\(PDFDate.xmp(c))</xmp:CreateDate>\n" }
        if let m = info.modified {
            body += "   <xmp:ModifyDate>\(PDFDate.xmp(m))</xmp:ModifyDate>\n   <xmp:MetadataDate>\(PDFDate.xmp(m))</xmp:MetadataDate>\n"
        }
        body += "   <xmpMM:DocumentID>\(id)</xmpMM:DocumentID>\n   <xmpMM:InstanceID>uuid:\(UUID().uuidString.lowercased())</xmpMM:InstanceID>\n"
        if pdfa { body += "   <pdfaid:part>2</pdfaid:part>\n   <pdfaid:conformance>B</pdfaid:conformance>\n" }
        let padding = String(repeating: String(repeating: " ", count: 99) + "\n", count: 20)
        return """
        <?xpacket begin="\u{FEFF}" id="W5M0MpCehiHzreSzNTczkc9d"?>
        <x:xmpmeta xmlns:x="adobe:ns:meta/">
         <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
          <rdf:Description rdf:about=""
            xmlns:dc="http://purl.org/dc/elements/1.1/"
            xmlns:xmp="http://ns.adobe.com/xap/1.0/"
            xmlns:pdf="http://ns.adobe.com/pdf/1.3/"
            xmlns:xmpMM="http://ns.adobe.com/xap/1.0/mm/"
            xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/">
        \(body)  </rdf:Description>
         </rdf:RDF>
        </x:xmpmeta>
        \(padding)<?xpacket end="w"?>
        """
    }
}
