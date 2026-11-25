import AppKit

// MARK: - Document objects carried in attributed strings (footnotes, comments, sections, bookmarks)

/// A footnote: set as `.docNote` on its reference character.
public final class DocNote: NSObject {
    public var text: String
    public init(_ text: String) { self.text = text }
}

/// A margin comment: set as `.docComment` on the commented range.
public final class DocComment: NSObject {
    public var author: String
    public var initials: String
    public var date: Date
    public var text: String
    public var resolved: Bool
    public init(author: String, initials: String? = nil, date: Date = Date(), text: String = "", resolved: Bool = false) {
        self.author = author
        self.initials = initials ?? String(author.split(separator: " ").compactMap(\.first))
        self.date = date
        self.text = text
        self.resolved = resolved
    }
}

/// Page layout of the section that ends at a section break: set as `.docSection` on the break character (form feed).
public final class DocSection: NSObject {
    public let pageSize: NSSize
    public let margins: NSEdgeInsets
    public let columns: Int
    public let columnSpacing: CGFloat
    /// The section's own headers and footers (nil: the document's).
    public var headerFooter: DocHeaderFooter?
    public init(pageSize: NSSize, margins: NSEdgeInsets, columns: Int = 1, columnSpacing: CGFloat = 36, headerFooter: DocHeaderFooter? = nil) {
        self.pageSize = pageSize
        self.margins = margins
        self.columns = max(1, columns)
        self.columnSpacing = columnSpacing
        self.headerFooter = headerFooter
    }
}

/// Headers and footers of a document or section: rich text with fields; a different first page and
/// different even pages (header1/2/3.xml: default, first, even; w:titlePg; w:evenAndOddHeaders).
public final class DocHeaderFooter: NSObject {
    public var header: NSAttributedString?
    public var footer: NSAttributedString?
    public var firstHeader: NSAttributedString?
    public var firstFooter: NSAttributedString?
    public var evenHeader: NSAttributedString?
    public var evenFooter: NSAttributedString?
    public var differentFirst = false
    public var differentOddEven = false

    public override init() {}

    public func duplicate() -> DocHeaderFooter {
        let c = DocHeaderFooter()
        c.header = header; c.footer = footer; c.firstHeader = firstHeader; c.firstFooter = firstFooter
        c.evenHeader = evenHeader; c.evenFooter = evenFooter
        c.differentFirst = differentFirst; c.differentOddEven = differentOddEven
        return c
    }

    public override func isEqual(_ object: Any?) -> Bool {
        guard let o = object as? DocHeaderFooter else { return false }
        func same(_ a: NSAttributedString?, _ b: NSAttributedString?) -> Bool { (a?.length ?? 0) == 0 && (b?.length ?? 0) == 0 || (a != nil && b != nil && a!.isEqual(to: b!)) }
        return differentFirst == o.differentFirst && differentOddEven == o.differentOddEven && same(header, o.header) && same(footer, o.footer)
            && same(firstHeader, o.firstHeader) && same(firstFooter, o.firstFooter) && same(evenHeader, o.evenHeader) && same(evenFooter, o.evenFooter)
    }

    public override var hash: Int { (header?.string.hashValue ?? 0) ^ (footer?.string.hashValue ?? 0) }

    /// The header or footer shown on a page (`page` from 1; `first`: the section's first page).
    public func part(header isHeader: Bool, page: Int, first: Bool) -> NSAttributedString? {
        if differentFirst && first { return isHeader ? firstHeader : firstFooter }
        if differentOddEven && page % 2 == 0 { return isHeader ? evenHeader : evenFooter }
        return isHeader ? header : footer
    }

    /// Parts that exist: (kind, header?, text).
    var parts: [(type: String, header: Bool, text: NSAttributedString)] {
        var out: [(String, Bool, NSAttributedString)] = []
        if let h = header { out.append(("default", true, h)) }
        if let f = footer { out.append(("default", false, f)) }
        if differentFirst { out.append(("first", true, firstHeader ?? NSAttributedString())); out.append(("first", false, firstFooter ?? NSAttributedString())) }
        if differentOddEven { out.append(("even", true, evenHeader ?? NSAttributedString())); out.append(("even", false, evenFooter ?? NSAttributedString())) }
        return out
    }
}

/// A tracked change: set as `.docRevision` on inserted text, or on deleted text (which stays in the
/// document until the deletion is accepted). Written as <w:ins>/<w:del>.
public final class DocRevision: NSObject {
    public enum Kind: String { case insertion, deletion }
    public let kind: Kind
    public let author: String
    public let date: Date
    public init(_ kind: Kind, author: String, date: Date = Date()) {
        self.kind = kind
        self.author = author
        self.date = date
    }
}

/// A field (page number, date, …): set as `.docField` on its character (an attachment that shows the value).
/// Written as <w:fldSimple w:instr="…"> with the current value as result; fixed fields are locked (w:fldLock).
public final class DocField: NSObject {
    public enum Kind: String, CaseIterable { case page, pageCount, date, time, title, author, fileName, chapter, sequence, formula }
    public let kind: Kind
    /// Date or time pattern ("dd.MM.yyyy", "HH:mm"); chapter: heading level ("1").
    public var format: String
    /// Sequence name ("Figure"), formula ("SUM(ABOVE)").
    public var argument: String
    /// Keeps `result` instead of updating (a fixed date).
    public var fixed: Bool
    /// The value last shown (what a fixed field shows).
    public var result: String

    public init(_ kind: Kind, format: String = "", argument: String = "", fixed: Bool = false, result: String = "") {
        self.kind = kind
        self.format = format
        self.argument = argument
        self.fixed = fixed
        self.result = result
    }

    public func copy(result: String? = nil) -> DocField {
        DocField(kind, format: format, argument: argument, fixed: fixed, result: result ?? self.result)
    }

    /// Word field code.
    public var instruction: String {
        switch kind {
        case .page: return " PAGE "
        case .pageCount: return " NUMPAGES "
        case .date: return format.isEmpty ? " DATE " : " DATE \\@ \"\(format)\" "
        case .time: return " TIME \\@ \"\(format.isEmpty ? "HH:mm" : format)\" "
        case .title: return " TITLE "
        case .author: return " AUTHOR "
        case .fileName: return " FILENAME "
        case .chapter: return " STYLEREF \"heading \(Int(format) ?? 1)\" "
        case .sequence: return " SEQ \(argument.isEmpty ? "Figure" : argument) \\* ARABIC "
        case .formula: return " =\(argument) "
        }
    }

    /// The field for a Word field code (nil: not one we show).
    public static func parse(_ instruction: String, result: String = "", locked: Bool = false) -> DocField? {
        let s = instruction.trimmingCharacters(in: .whitespaces)
        let word = s.split(separator: " ").first.map { String($0).uppercased() } ?? ""
        func picture() -> String {
            guard let r = s.range(of: "\\\\@\\s*\"([^\"]*)\"", options: .regularExpression) else {
                if let r = s.range(of: "\\\\@\\s*(\\S+)", options: .regularExpression) {
                    return String(s[r]).replacingOccurrences(of: "\\@", with: "").trimmingCharacters(in: .whitespaces)
                }
                return ""
            }
            return String(s[r]).replacingOccurrences(of: "^\\\\@\\s*\"", with: "", options: .regularExpression).replacingOccurrences(of: "\"", with: "")
        }
        switch word {
        case "PAGE": return DocField(.page, fixed: locked, result: result)
        case "NUMPAGES", "SECTIONPAGES": return DocField(.pageCount, fixed: locked, result: result)
        case "DATE", "CREATEDATE", "SAVEDATE", "PRINTDATE": return DocField(.date, format: picture(), fixed: locked || word != "DATE", result: result)
        case "TIME": return DocField(.time, format: picture(), fixed: locked, result: result)
        case "TITLE": return DocField(.title, fixed: locked, result: result)
        case "AUTHOR": return DocField(.author, fixed: locked, result: result)
        case "FILENAME": return DocField(.fileName, fixed: locked, result: result)
        case "STYLEREF":
            let level = s.range(of: "[0-9]", options: .regularExpression).map { String(s[$0]) } ?? "1"
            return DocField(.chapter, format: level, fixed: locked, result: result)
        case "SEQ":
            let parts = s.split(separator: " ")
            return DocField(.sequence, argument: parts.count > 1 ? String(parts[1]) : "Figure", fixed: locked, result: result)
        default:
            if s.hasPrefix("=") { return DocField(.formula, argument: String(s.dropFirst()).trimmingCharacters(in: .whitespaces), fixed: locked, result: result) }
            return nil
        }
    }
}

/// How a picture sits in the text: in the line, or floating on the page with text around it (square),
/// above and below it (top and bottom), or none (behind or in front of the text). Set as `.docImage` on the
/// picture's attachment character, which anchors a floating picture to its paragraph.
public final class DocImage: NSObject {
    public enum Wrap: String, CaseIterable { case inline, square, topBottom, behind, inFront }
    public var wrap: Wrap = .inline
    /// Floating: position of the picture's top-left from the top-left of the page's text area (points).
    public var offset: CGPoint = .zero
    /// Floating, read from a file: `offset.y` is from the anchor paragraph (converted once laid out).
    public var fromParagraph = false
    public var borderWidth: CGFloat = 0
    public var borderColor: NSColor = .black
    /// Cropped part of each side, as fractions of the picture (left, top, right, bottom).
    public var crop = NSEdgeInsets()
    /// Ink (signature, stamp): drawn with multiply, like ink on paper — text under it stays dark, overlapping inks
    /// darken each other, and the order of layers hardly matters.
    public var ink = false
    public override init() {}
    public func duplicate() -> DocImage {
        let d = DocImage()
        d.wrap = wrap; d.offset = offset; d.fromParagraph = fromParagraph
        d.borderWidth = borderWidth; d.borderColor = borderColor; d.crop = crop; d.ink = ink
        return d
    }
    public var isFloating: Bool { wrap != .inline }
    public var isCropped: Bool { crop.left > 0 || crop.top > 0 || crop.right > 0 || crop.bottom > 0 }
}

public extension NSAttributedString.Key {
    /// DocImage on a picture's attachment character (wrap, position, border, crop).
    static let docImage = NSAttributedString.Key("Kit42.image")
    /// DocField on a field character.
    static let docField = NSAttributedString.Key("Kit42.field")
    /// DocNote on a footnote reference character.
    static let docNote = NSAttributedString.Key("Kit42.note")
    /// DocComment on a commented range.
    static let docComment = NSAttributedString.Key("Kit42.comment")
    /// Bookmark name (String) on a paragraph: target of "#name" links.
    static let docBookmark = NSAttributedString.Key("Kit42.bookmark")
    /// DocSection on a section break character.
    static let docSection = NSAttributedString.Key("Kit42.section")
    /// "dot" on a tab character: drawn and saved with a dot leader (tables of contents).
    static let docTabLeader = NSAttributedString.Key("Kit42.leader")
    /// DocRevision on tracked inserted or deleted text.
    static let docRevision = NSAttributedString.Key("Kit42.revision")
    /// Character style id (String) on a range: written as w:rStyle.
    static let docCharStyle = NSAttributedString.Key("Kit42.charStyle")
}

/// Writes an attributed string as .docx. Replaces the system writer, which drops tables.
/// Covers paragraphs, runs (font, size, bold/italic/underline/strike, colour, highlight),
/// alignment, spacing, indents, tabs, page breaks, tables (NSTextTable), images, hyperlinks,
/// bookmarks, footnotes, comments, sections (page size, orientation, margins, columns) and TOC fields.
public enum DocxWriter {
    public struct Options {
        public var pageSize = NSSize(width: 595, height: 842)
        public var margins = NSEdgeInsets(top: 72, left: 72, bottom: 72, right: 72)
        public var header = ""
        public var footer = ""
        public var pageNumbers = false
        /// Text columns of the last section.
        public var columns = 1
        public var columnSpacing: CGFloat = 36
        /// Word style id for a paragraph (e.g. "Heading1"), so outlines and tables of contents work in Word.
        public var paragraphStyle: ((NSAttributedString, NSRange) -> String?)?
        /// Track changes is on (w:trackRevisions in settings.xml).
        public var trackRevisions = false
        /// Style definitions for styles.xml (empty: a basic built-in set). Run properties equal to the
        /// paragraph's style are then left out of document.xml, so the styles really apply.
        public var styles: [Style] = []
        /// Current value of a field at a character index (-1: in a header or footer); default: its `result`.
        public var fieldResult: ((DocField, Int) -> String)?
        /// Rich headers and footers of the document (replace `header`, `footer` and `pageNumbers`).
        public var headerFooter: DocHeaderFooter?
        /// Page colour (w:background) and a diagonal text watermark (a VML shape in the headers).
        public var background: NSColor?
        public var watermark = ""
        /// Automatic hyphenation (w:autoHyphenation).
        public var autoHyphenation = false
        /// Document properties (docProps/core.xml).
        public var title = ""
        public var author = ""
        public init() {}
    }

    /// A paragraph or character style for styles.xml (nil: not set).
    public struct Style {
        public var id: String
        public var name: String
        public var character: Bool
        public var basedOn: String?
        public var next: String?
        public var custom = false
        public var family: String?
        public var size: CGFloat?
        public var bold: Bool?
        public var italic: Bool?
        public var underline: Bool?
        public var color: NSColor?
        public var spaceBefore: CGFloat?
        public var spaceAfter: CGFloat?
        public var lineHeightMultiple: CGFloat?
        public var headIndent: CGFloat?
        public var firstLineHeadIndent: CGFloat?
        public var tailIndent: CGFloat?
        public var alignment: NSTextAlignment?
        public var outlineLevel: Int?
        public init(id: String, name: String, character: Bool = false) {
            self.id = id
            self.name = name
            self.character = character
        }

        /// This style's set properties over `base`.
        func over(_ base: Style?) -> Style {
            guard let base else { return self }
            var s = self
            s.family = family ?? base.family
            s.size = size ?? base.size
            s.bold = bold ?? base.bold
            s.italic = italic ?? base.italic
            s.underline = underline ?? base.underline
            s.color = color ?? base.color
            return s
        }
    }

    public static func data(_ text: NSAttributedString, pageSize: NSSize) -> Data {
        var o = Options()
        o.pageSize = pageSize
        return data(text, options: o)
    }

    /// State of one write: parts collected while the body is written.
    private final class Context {
        var media: [(name: String, data: Data)] = []
        var links: [String] = []                    // external hyperlink targets → rIdLinkN
        var notes: [String] = []                    // footnote texts, ids from 1
        var comments: [DocComment] = []
        var commentIds: [ObjectIdentifier: Int] = [:]
        var commentStarts: [Int: [Int]] = [:]       // character index → comment ids starting there
        var commentEnds: [Int: [Int]] = [:]
        var bookmarks = 0
        var revisions = 0                           // w:ins / w:del ids
        var sectPr: (DocSection) -> String = { _ in "" }
        var styles: [String: Style] = [:]
        var fieldResult: ((DocField, Int) -> String)?
        var textWidth: CGFloat = 451
        var mediaPrefix = ""
        /// Lists: numId of each list (by its outermost NSTextList), level lists, paragraph styles per level.
        var numIds: [ObjectIdentifier: Int] = [:]
        var levels: [Int: [Int: NSTextList]] = [:]
        var levelStyles: [Int: [Int: Set<String>]] = [:]
    }

    public static func data(_ text: NSAttributedString, options: Options = Options()) -> Data {
        styleProvider = options.paragraphStyle
        let ctx = Context()
        for st in options.styles { ctx.styles[st.id] = st }
        ctx.fieldResult = options.fieldResult
        ctx.textWidth = options.pageSize.width - options.margins.left - options.margins.right
        var body = ""
        let ns = text.string as NSString
        let full = NSRange(location: 0, length: ns.length)

        // Comment ranges, start and end moved off paragraph marks.
        text.enumerateAttribute(.docComment, in: full) { v, r, _ in
            guard let c = v as? DocComment else { return }
            let key = ObjectIdentifier(c)
            if ctx.commentIds[key] == nil {
                ctx.comments.append(c)
                ctx.commentIds[key] = ctx.comments.count - 1
            }
        }
        for (k, c) in ctx.comments.enumerated() {
            var start = Int.max, end = 0
            text.enumerateAttribute(.docComment, in: full) { v, r, _ in
                if (v as? DocComment) === c { start = min(start, r.location); end = max(end, NSMaxRange(r)) }
            }
            while start < end, isParagraphMark(ns.character(at: start)) { start += 1 }
            while end > start, isParagraphMark(ns.character(at: end - 1)) { end -= 1 }
            ctx.commentStarts[start, default: []].append(k)
            ctx.commentEnds[end, default: []].append(k)
        }

        // Paragraph ranges; a section break ends its paragraph.
        var paragraphs: [NSRange] = []
        ns.enumerateSubstrings(in: full, options: [.byParagraphs, .substringNotRequired]) { _, r, _, _ in
            var start = r.location
            text.enumerateAttribute(.docSection, in: r) { v, sr, _ in
                guard v is DocSection else { return }
                paragraphs.append(NSRange(location: start, length: NSMaxRange(sr) - start))
                start = NSMaxRange(sr)
            }
            if start < NSMaxRange(r) || start == r.location { paragraphs.append(NSRange(location: start, length: NSMaxRange(r) - start)) }
        }
        if paragraphs.isEmpty { paragraphs = [NSRange(location: 0, length: 0)] }
        // Lists → numbering.xml (one abstractNum and num per list).
        for r in paragraphs where text.length > 0 {
            guard let p = text.attribute(.paragraphStyle, at: min(r.location, text.length - 1), effectiveRange: nil) as? NSParagraphStyle,
                  let root = p.textLists.first else { continue }
            let key = ObjectIdentifier(root)
            let id = ctx.numIds[key] ?? { ctx.numIds[key] = ctx.numIds.count + 1; return ctx.numIds.count }()
            for (k, l) in p.textLists.enumerated() where ctx.levels[id]?[k] == nil { ctx.levels[id, default: [:]][k] = l }
            ctx.levelStyles[id, default: [:]][p.textLists.count - 1, default: []].insert(options.paragraphStyle?(text, r) ?? "Normal")
        }

        func style(at r: NSRange) -> NSParagraphStyle? {
            guard text.length > 0 else { return nil }
            return text.attribute(.paragraphStyle, at: min(r.location, text.length - 1), effectiveRange: nil) as? NSParagraphStyle
        }

        let twip = { (pt: CGFloat) in Int((pt * 20).rounded()) }
        // Header and footer parts: one set per DocHeaderFooter (the document's and sections' own).
        let documentHF = options.headerFooter ?? legacyHeaderFooter(options)
        var hfParts: [(file: String, rId: String, xml: String, rels: String?)] = []
        var hfRefs: [ObjectIdentifier: String] = [:]
        var evenOdd = false
        func references(_ hf: DocHeaderFooter) -> String {
            if let r = hfRefs[ObjectIdentifier(hf)] { return r }
            var refs = ""
            evenOdd = evenOdd || hf.differentOddEven
            var parts = hf.parts
            if !options.watermark.isEmpty, !parts.contains(where: { $0.header && $0.type == "default" }) { parts.insert(("default", true, NSAttributedString()), at: 0) }
            for part in parts {
                let n = hfParts.filter { $0.file.hasPrefix(part.header ? "header" : "footer") }.count + 1
                let file = (part.header ? "header" : "footer") + "\(n).xml"
                let rId = "rId" + (part.header ? "Hdr" : "Ftr") + "\(n)"
                let (xml, rels) = headerFooterPart(part.header ? "hdr" : "ftr", part.text, options: options, prefix: "hf\(hfParts.count + 1)",
                                                   watermark: part.header ? options.watermark : "")
                hfParts.append((file, rId, xml, rels))
                refs += "<w:\(part.header ? "header" : "footer")Reference w:type=\"\(part.type)\" r:id=\"\(rId)\"/>"
            }
            hfRefs[ObjectIdentifier(hf)] = refs
            return refs
        }
        ctx.sectPr = { s in
            var x = "<w:sectPr>"
            let hf = s.headerFooter ?? documentHF
            x += references(hf)
            let m = s.margins
            x += "<w:pgSz w:w=\"\(twip(s.pageSize.width))\" w:h=\"\(twip(s.pageSize.height))\"\(s.pageSize.width > s.pageSize.height ? " w:orient=\"landscape\"" : "")/>"
            x += "<w:pgMar w:top=\"\(twip(m.top))\" w:right=\"\(twip(m.right))\" w:bottom=\"\(twip(m.bottom))\" w:left=\"\(twip(m.left))\" w:header=\"567\" w:footer=\"567\" w:gutter=\"0\"/>"
            if s.columns > 1 { x += "<w:cols w:num=\"\(s.columns)\" w:space=\"\(twip(s.columnSpacing))\"/>" }
            if hf.differentFirst { x += "<w:titlePg/>" }
            return x + "</w:sectPr>"
        }

        func isTOCEntry(_ r: NSRange) -> Bool {
            guard let s = styleProvider?(text, r) else { return false }
            return s.hasPrefix("TOC") && s != "TOCHeading"
        }

        var i = 0
        while i < paragraphs.count {
            let r = paragraphs[i]
            if let block = style(at: r)?.textBlocks.last as? NSTextTableBlock {
                // Collect the whole table
                let table = block.table
                var cells: [[Int: [NSRange]]] = []   // row → col → paragraphs
                var columns = table.numberOfColumns
                while i < paragraphs.count, let b = style(at: paragraphs[i])?.textBlocks.last as? NSTextTableBlock, b.table === table {
                    while cells.count <= b.startingRow { cells.append([:]) }
                    cells[b.startingRow][b.startingColumn, default: []].append(paragraphs[i])
                    columns = max(columns, b.startingColumn + b.columnSpan)
                    i += 1
                }
                body += tableXML(cells, columns: columns, text: text, ctx: ctx, border: block)
                continue
            }
            // A table of contents is one field around its entries.
            let toc = isTOCEntry(r)
            let begin = toc && (i == 0 || !isTOCEntry(paragraphs[i - 1]))
            let end = toc && (i + 1 == paragraphs.count || !isTOCEntry(paragraphs[i + 1]))
            body += paragraphXML(r, text: text, ctx: ctx, fieldBegin: begin ? " TOC \\o \"1-3\" \\h \\z \\u " : nil, fieldEnd: end)
            i += 1
        }

        body += ctx.sectPr(DocSection(pageSize: options.pageSize, margins: options.margins, columns: options.columns, columnSpacing: options.columnSpacing))

        let document = """
        <?xml version="1.0" encoding="UTF-8" standalone="yes"?>
        <w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" \
        xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" \
        xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" \
        xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" \
        xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture">\(options.background.map { "<w:background w:color=\"\(hex($0))\"/>" } ?? "")<w:body>\(body)</w:body></w:document>
        """

        let hasNotes = !ctx.notes.isEmpty, hasComments = !ctx.comments.isEmpty
        let hasResolved = ctx.comments.contains(where: \.resolved)
        var zip = ZipArchive()
        var types = """
        <?xml version="1.0" encoding="UTF-8" standalone="yes"?>
        <Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
        <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
        <Default Extension="xml" ContentType="application/xml"/>
        <Default Extension="png" ContentType="image/png"/>
        <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
        <Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>
        <Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/>
        <Override PartName="/word/footer1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/>
        """
        let wml = "application/vnd.openxmlformats-officedocument.wordprocessingml."
        if hasNotes { types += "<Override PartName=\"/word/footnotes.xml\" ContentType=\"\(wml)footnotes+xml\"/>" }
        let hasSettings = hasNotes || options.trackRevisions || evenOdd || options.background != nil || options.autoHyphenation
        if hasSettings { types += "<Override PartName=\"/word/settings.xml\" ContentType=\"\(wml)settings+xml\"/>" }
        for part in hfParts where !["header1.xml", "footer1.xml"].contains(part.file) {
            types += "<Override PartName=\"/word/\(part.file)\" ContentType=\"\(wml)\(part.file.hasPrefix("header") ? "header" : "footer")+xml\"/>"
        }
        if hasComments { types += "<Override PartName=\"/word/comments.xml\" ContentType=\"\(wml)comments+xml\"/>" }
        if !ctx.numIds.isEmpty { types += "<Override PartName=\"/word/numbering.xml\" ContentType=\"\(wml)numbering+xml\"/>" }
        if hasResolved { types += "<Override PartName=\"/word/commentsExtended.xml\" ContentType=\"\(wml)commentsExtended+xml\"/>" }
        let hasCore = !options.title.isEmpty || !options.author.isEmpty
        if hasCore { types += "<Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>" }
        zip["[Content_Types].xml"] = Data((types + "\n</Types>").utf8)
        if hasCore {
            let iso = ISO8601DateFormatter()
            zip["docProps/core.xml"] = Data("""
            <?xml version="1.0" encoding="UTF-8" standalone="yes"?>
            <cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" \
            xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" \
            xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><dc:title>\(xmlEscape(options.title))</dc:title>\
            <dc:creator>\(xmlEscape(options.author))</dc:creator>\
            <dcterms:modified xsi:type="dcterms:W3CDTF">\(iso.string(from: Date()))</dcterms:modified></cp:coreProperties>
            """.utf8)
        }
        zip["_rels/.rels"] = Data("""
        <?xml version="1.0" encoding="UTF-8" standalone="yes"?>
        <Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
        <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>\(hasCore ? "\n<Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/>" : "")
        </Relationships>
        """.utf8)
        let relType = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/"
        var rels = """
        <?xml version="1.0" encoding="UTF-8" standalone="yes"?>
        <Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
        <Relationship Id="rIdStyles" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
        """
        for (n, m) in ctx.media.enumerated() {
            rels += "<Relationship Id=\"rIdImg\(n + 1)\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/\(m.name)\"/>"
            zip["word/media/\(m.name)"] = m.data
        }
        for (n, url) in ctx.links.enumerated() {
            rels += "<Relationship Id=\"rIdLink\(n + 1)\" Type=\"\(relType)hyperlink\" Target=\"\(xmlEscape(url))\" TargetMode=\"External\"/>"
        }
        for part in hfParts {
            let kind = part.file.hasPrefix("header") ? "header" : "footer"
            rels += "<Relationship Id=\"\(part.rId)\" Type=\"\(relType)\(kind)\" Target=\"\(part.file)\"/>"
            zip["word/\(part.file)"] = Data(part.xml.utf8)
            if let r = part.rels { zip["word/_rels/\(part.file).rels"] = Data(r.utf8) }
        }
        for (name, data) in hfMedia { zip["word/media/\(name)"] = data }
        hfMedia = []
        let wNS = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\""
        if hasNotes {
            rels += "<Relationship Id=\"rIdNotes\" Type=\"\(relType)footnotes\" Target=\"footnotes.xml\"/>"
            var x = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:footnotes \(wNS)>"
            x += "<w:footnote w:type=\"separator\" w:id=\"-1\"><w:p><w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr><w:r><w:separator/></w:r></w:p></w:footnote>"
            x += "<w:footnote w:type=\"continuationSeparator\" w:id=\"0\"><w:p><w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr><w:r><w:continuationSeparator/></w:r></w:p></w:footnote>"
            for (k, note) in ctx.notes.enumerated() {
                x += "<w:footnote w:id=\"\(k + 1)\">"
                for (line, s) in note.components(separatedBy: "\n").enumerated() {
                    x += "<w:p><w:pPr><w:pStyle w:val=\"FootnoteText\"/></w:pPr>"
                    if line == 0 { x += "<w:r><w:rPr><w:rStyle w:val=\"FootnoteReference\"/><w:vertAlign w:val=\"superscript\"/></w:rPr><w:footnoteRef/></w:r>" }
                    x += "<w:r><w:t xml:space=\"preserve\">\(line == 0 ? " " : "")\(xmlEscape(s))</w:t></w:r></w:p>"
                }
                x += "</w:footnote>"
            }
            zip["word/footnotes.xml"] = Data((x + "</w:footnotes>").utf8)
        }
        if hasSettings {
            rels += "<Relationship Id=\"rIdSettings\" Type=\"\(relType)settings\" Target=\"settings.xml\"/>"
            let track = options.trackRevisions ? "<w:trackRevisions/>" : ""
            let notes = hasNotes ? "<w:footnotePr><w:footnote w:id=\"-1\"/><w:footnote w:id=\"0\"/></w:footnotePr>" : ""
            let shape = options.background != nil ? "<w:displayBackgroundShape/>" : ""
            let hyphen = options.autoHyphenation ? "<w:autoHyphenation/>" : ""
            let even = evenOdd ? "<w:evenAndOddHeaders/>" : ""
            zip["word/settings.xml"] = Data("""
            <?xml version="1.0" encoding="UTF-8" standalone="yes"?>
            <w:settings \(wNS)>\(shape)\(track)\(hyphen)\(even)\(notes)</w:settings>
            """.utf8)
        }
        if hasComments {
            rels += "<Relationship Id=\"rIdComments\" Type=\"\(relType)comments\" Target=\"comments.xml\"/>"
            let iso = ISO8601DateFormatter()
            iso.formatOptions = [.withInternetDateTime]
            var x = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:comments \(wNS) xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\">"
            var extended = ""
            for (k, c) in ctx.comments.enumerated() {
                x += "<w:comment w:id=\"\(k)\" w:author=\"\(xmlEscape(c.author))\" w:date=\"\(iso.string(from: c.date))\" w:initials=\"\(xmlEscape(c.initials))\">"
                let lines = c.text.components(separatedBy: "\n")
                let paraId = String(format: "%08X", 0x10000000 + k)
                for (line, s) in lines.enumerated() {
                    x += "<w:p\(line == lines.count - 1 ? " w14:paraId=\"\(paraId)\"" : "")><w:pPr><w:pStyle w:val=\"CommentText\"/></w:pPr>"
                    if line == 0 { x += "<w:r><w:rPr><w:rStyle w:val=\"CommentReference\"/></w:rPr><w:annotationRef/></w:r>" }
                    x += "<w:r><w:t xml:space=\"preserve\">\(xmlEscape(s))</w:t></w:r></w:p>"
                }
                x += "</w:comment>"
                extended += "<w15:commentEx w15:paraId=\"\(paraId)\" w15:done=\"\(c.resolved ? 1 : 0)\"/>"
            }
            zip["word/comments.xml"] = Data((x + "</w:comments>").utf8)
            if hasResolved {
                rels += "<Relationship Id=\"rIdCommentsEx\" Type=\"http://schemas.microsoft.com/office/2011/relationships/commentsExtended\" Target=\"commentsExtended.xml\"/>"
                zip["word/commentsExtended.xml"] = Data("""
                <?xml version="1.0" encoding="UTF-8" standalone="yes"?>
                <w15:commentsEx xmlns:w15="http://schemas.microsoft.com/office/word/2012/wordml">\(extended)</w15:commentsEx>
                """.utf8)
            }
        }
        if !ctx.numIds.isEmpty {
            rels += "<Relationship Id=\"rIdNumbering\" Type=\"\(relType)numbering\" Target=\"numbering.xml\"/>"
            zip["word/numbering.xml"] = Data(numberingXML(ctx).utf8)
        }
        rels += "</Relationships>"
        zip["word/_rels/document.xml.rels"] = Data(rels.utf8)
        zip["word/styles.xml"] = !options.styles.isEmpty ? Data(stylesXML(options.styles).utf8) : Data("""
        <?xml version="1.0" encoding="UTF-8" standalone="yes"?>
        <w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
        <w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Helvetica Neue" w:hAnsi="Helvetica Neue" w:cs="Helvetica Neue" w:eastAsia="Helvetica Neue"/>\
        <w:sz w:val="24"/><w:szCs w:val="24"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults>
        <w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style>
        <w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/></w:style>
        <w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:outlineLvl w:val="0"/></w:pPr></w:style>
        <w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:outlineLvl w:val="1"/></w:pPr></w:style>
        <w:style w:type="paragraph" w:styleId="Heading3"><w:name w:val="heading 3"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:outlineLvl w:val="2"/></w:pPr></w:style>
        <w:style w:type="paragraph" w:styleId="Quote"><w:name w:val="Quote"/><w:basedOn w:val="Normal"/><w:qFormat/></w:style>
        <w:style w:type="paragraph" w:styleId="TOCHeading"><w:name w:val="TOC Heading"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/></w:style>
        <w:style w:type="paragraph" w:styleId="TOC1"><w:name w:val="toc 1"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/></w:style>
        <w:style w:type="paragraph" w:styleId="TOC2"><w:name w:val="toc 2"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/></w:style>
        <w:style w:type="paragraph" w:styleId="TOC3"><w:name w:val="toc 3"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/></w:style>
        <w:style w:type="paragraph" w:styleId="FootnoteText"><w:name w:val="footnote text"/><w:basedOn w:val="Normal"/><w:rPr><w:sz w:val="18"/><w:szCs w:val="18"/></w:rPr></w:style>
        <w:style w:type="paragraph" w:styleId="CommentText"><w:name w:val="annotation text"/><w:basedOn w:val="Normal"/><w:rPr><w:sz w:val="20"/><w:szCs w:val="20"/></w:rPr></w:style>
        <w:style w:type="character" w:default="1" w:styleId="DefaultParagraphFont"><w:name w:val="Default Paragraph Font"/></w:style>
        <w:style w:type="character" w:styleId="Hyperlink"><w:name w:val="Hyperlink"/><w:rPr><w:color w:val="0563C1"/><w:u w:val="single"/></w:rPr></w:style>
        <w:style w:type="character" w:styleId="FootnoteReference"><w:name w:val="footnote reference"/><w:rPr><w:vertAlign w:val="superscript"/></w:rPr></w:style>
        <w:style w:type="character" w:styleId="CommentReference"><w:name w:val="annotation reference"/><w:rPr><w:sz w:val="16"/><w:szCs w:val="16"/></w:rPr></w:style>
        <w:style w:type="table" w:default="1" w:styleId="TableNormal"><w:name w:val="Normal Table"/><w:tblPr><w:tblCellMar>\
        <w:top w:w="0" w:type="dxa"/><w:left w:w="108" w:type="dxa"/><w:bottom w:w="0" w:type="dxa"/><w:right w:w="108" w:type="dxa"/></w:tblCellMar></w:tblPr></w:style>
        </w:styles>
        """.utf8)
        zip["word/document.xml"] = Data(document.utf8)
        return zip.data()
    }

    private static func isParagraphMark(_ c: unichar) -> Bool { c == 10 || c == 13 || c == 0x2029 }

    // MARK: - Paragraphs

    private static var styleProvider: ((NSAttributedString, NSRange) -> String?)?

    /// Media of header and footer parts (collected while writing them).
    private static var hfMedia: [(String, Data)] = []

    /// The plain header, footer and page number options as rich text (centred, 9 pt, grey; a PAGE field).
    static func legacyHeaderFooter(_ o: Options) -> DocHeaderFooter {
        let hf = DocHeaderFooter()
        let p = NSMutableParagraphStyle()
        p.alignment = .center
        let font = NSFont(name: "Helvetica Neue", size: 9) ?? .systemFont(ofSize: 9)
        let attrs: [NSAttributedString.Key: Any] = [.font: font, .paragraphStyle: p, .foregroundColor: NSColor(white: 0.4, alpha: 1)]
        if !o.header.isEmpty { hf.header = NSAttributedString(string: o.header, attributes: attrs) }
        if !o.footer.isEmpty || o.pageNumbers {
            let f = NSMutableAttributedString(string: o.footer + (o.pageNumbers && !o.footer.isEmpty ? "   " : ""), attributes: attrs)
            if o.pageNumbers {
                var a = attrs
                a[.docField] = DocField(.page, result: "1")
                a[.attachment] = NSTextAttachment()
                f.append(NSAttributedString(string: "\u{FFFC}", attributes: a))
            }
            hf.footer = f
        }
        return hf
    }

    /// A header or footer part (and its relationships, for links and pictures).
    private static func headerFooterPart(_ tag: String, _ text: NSAttributedString, options: Options, prefix: String, watermark: String) -> (String, String?) {
        let ctx = Context()
        for st in options.styles { ctx.styles[st.id] = st }
        ctx.fieldResult = { f, _ in options.fieldResult?(f, -1) ?? f.result }
        ctx.textWidth = options.pageSize.width - options.margins.left - options.margins.right
        ctx.mediaPrefix = prefix
        var x = ""
        let ns = text.string as NSString
        var paragraphs: [NSRange] = []
        ns.enumerateSubstrings(in: NSRange(location: 0, length: ns.length), options: [.byParagraphs, .substringNotRequired]) { _, r, _, _ in paragraphs.append(r) }
        for (i, r) in paragraphs.enumerated() {
            var p = paragraphXML(r, text: text, ctx: ctx)
            if i == 0, !watermark.isEmpty, let end = p.range(of: "</w:p>", options: .backwards) { p.insert(contentsOf: watermarkXML(watermark), at: end.lowerBound) }
            x += p
        }
        if paragraphs.isEmpty { x = "<w:p>" + (watermark.isEmpty ? "" : watermarkXML(watermark)) + "</w:p>" }
        let ns2 = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" "
            + "xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" "
            + "xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\" xmlns:v=\"urn:schemas-microsoft-com:vml\" xmlns:o=\"urn:schemas-microsoft-com:office:office\" "
            + "xmlns:w10=\"urn:schemas-microsoft-com:office:word\""
        let xml = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:\(tag) \(ns2)>\(x)</w:\(tag)>"
        guard !ctx.media.isEmpty || !ctx.links.isEmpty else { return (xml, nil) }
        let relType = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/"
        var rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">"
        for (n, m) in ctx.media.enumerated() {
            rels += "<Relationship Id=\"rIdImg\(n + 1)\" Type=\"\(relType)image\" Target=\"media/\(m.name)\"/>"
            hfMedia.append((m.name, m.data))
        }
        for (n, url) in ctx.links.enumerated() {
            rels += "<Relationship Id=\"rIdLink\(n + 1)\" Type=\"\(relType)hyperlink\" Target=\"\(xmlEscape(url))\" TargetMode=\"External\"/>"
        }
        return (xml, rels + "</Relationships>")
    }

    /// Word's text watermark: a light grey WordArt shape, rotated, centred on the page, behind the text.
    private static func watermarkXML(_ text: String) -> String {
        "<w:r><w:pict><v:shapetype id=\"_x0000_t136\" coordsize=\"21600,21600\" o:spt=\"136\" adj=\"10800\" path=\"m@7,l@8,m@5,21600l@6,21600e\">"
            + "<v:path textpathok=\"t\" o:connecttype=\"custom\"/><v:textpath on=\"t\" fitshape=\"t\"/></v:shapetype>"
            + "<v:shape id=\"PowerPlusWaterMarkObject1\" o:spid=\"_x0000_s2049\" type=\"#_x0000_t136\" "
            + "style=\"position:absolute;margin-left:0;margin-top:0;width:468pt;height:117pt;rotation:315;z-index:-251657216;"
            + "mso-position-horizontal:center;mso-position-horizontal-relative:margin;mso-position-vertical:center;mso-position-vertical-relative:margin\" "
            + "o:allowincell=\"f\" fillcolor=\"silver\" stroked=\"f\"><v:fill opacity=\".5\"/>"
            + "<v:textpath style=\"font-family:&quot;Helvetica Neue&quot;;font-size:1pt\" string=\"\(xmlEscape(text))\"/>"
            + "<w10:wrap anchorx=\"margin\" anchory=\"margin\"/></v:shape></w:pict></w:r>"
    }

    private static func headerFooterXML(_ tag: String, text: String, pageNumber: Bool) -> String {
        var p = "<w:p><w:pPr><w:jc w:val=\"center\"/></w:pPr>"
        if !text.isEmpty { p += "<w:r><w:rPr><w:sz w:val=\"18\"/></w:rPr><w:t xml:space=\"preserve\">\(xmlEscape(text))\(pageNumber ? "  " : "")</w:t></w:r>" }
        if pageNumber { p += "<w:fldSimple w:instr=\" PAGE \"><w:r><w:rPr><w:sz w:val=\"18\"/></w:rPr><w:t>1</w:t></w:r></w:fldSimple>" }
        p += "</w:p>"
        return "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:\(tag) xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\">\(p)</w:\(tag)>"
    }

    private static func paragraphXML(_ r: NSRange, text: NSAttributedString, ctx: Context, fieldBegin: String? = nil, fieldEnd: Bool = false) -> String {
        var x = "<w:p>"
        let paragraphBase = ctx.styles.isEmpty ? nil : ctx.styles[styleProvider?(text, r) ?? "Normal"] ?? ctx.styles["Normal"]
        var bookmark: (Int, String)?
        if text.length > 0 {
            let at = min(r.location, text.length - 1)
            let p = text.attribute(.paragraphStyle, at: at, effectiveRange: nil) as? NSParagraphStyle
            var leader = false
            if r.length > 0 { text.enumerateAttribute(.docTabLeader, in: r) { v, _, stop in if v != nil { leader = true; stop.pointee = true } } }
            // A section break at the end of the paragraph: its sectPr closes the section.
            var sect: String?
            if r.length > 0, let s = text.attribute(.docSection, at: NSMaxRange(r) - 1, effectiveRange: nil) as? DocSection { sect = ctx.sectPr(s) }
            // A tracked paragraph mark (the newline after the paragraph).
            var mark: String?
            let end = NSMaxRange(r)
            if end < text.length, isParagraphMark((text.string as NSString).character(at: end)),
               let rev = text.attribute(.docRevision, at: end, effectiveRange: nil) as? DocRevision {
                mark = "<w:rPr>" + revisionTag(rev, ctx: ctx, empty: true) + "</w:rPr>"
            }
            var numPr: String?
            if let root = p?.textLists.first, let id = ctx.numIds[ObjectIdentifier(root)] {
                numPr = "<w:numPr><w:ilvl w:val=\"\(p!.textLists.count - 1)\"/><w:numId w:val=\"\(id)\"/></w:numPr>"
            }
            x += paragraphProperties(p, style: styleProvider?(text, r), leader: leader, sectPr: sect, markRPr: mark, numPr: numPr)
            if let name = text.attribute(.docBookmark, at: at, effectiveRange: nil) as? String, !name.isEmpty {
                ctx.bookmarks += 1
                bookmark = (ctx.bookmarks, name)
                x += "<w:bookmarkStart w:id=\"\(ctx.bookmarks)\" w:name=\"\(xmlEscape(name))\"/>"
            }
        }
        if let instr = fieldBegin {
            x += "<w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:instrText xml:space=\"preserve\">\(xmlEscape(instr))</w:instrText></w:r><w:r><w:fldChar w:fldCharType=\"separate\"/></w:r>"
        }
        func commentMarks(_ at: Int, end: Bool) -> String {
            let ids = (end ? ctx.commentEnds : ctx.commentStarts)[at] ?? []
            return ids.map { id in
                end ? "<w:commentRangeEnd w:id=\"\(id)\"/><w:r><w:rPr><w:rStyle w:val=\"CommentReference\"/></w:rPr><w:commentReference w:id=\"\(id)\"/></w:r>"
                    : "<w:commentRangeStart w:id=\"\(id)\"/>"
            }.joined()
        }
        // Tracked changes: runs of one revision share a <w:ins>/<w:del>, inside any hyperlink.
        var openRevision: DocRevision?
        func setRevision(_ rev: DocRevision?) {
            guard rev !== openRevision else { return }
            if let old = openRevision { x += old.kind == .insertion ? "</w:ins>" : "</w:del>" }
            openRevision = rev
            if let rev { x += revisionTag(rev, ctx: ctx) }
        }
        var openLink: String?
        func setLink(_ link: String?) {
            guard link != openLink else { return }
            setRevision(nil)
            if openLink != nil { x += "</w:hyperlink>" }
            openLink = link
            guard let link else { return }
            if link.hasPrefix("#") {
                x += "<w:hyperlink w:anchor=\"\(xmlEscape(String(link.dropFirst())))\" w:history=\"1\">"
            } else {
                let n = (ctx.links.firstIndex(of: link) ?? { ctx.links.append(link); return ctx.links.count - 1 }()) + 1
                x += "<w:hyperlink r:id=\"rIdLink\(n)\" w:history=\"1\">"
            }
        }
        // The list marker ("\t1.\t") is numbering.xml's job.
        var body = r
        if r.length > 0, let p = text.attribute(.paragraphStyle, at: r.location, effectiveRange: nil) as? NSParagraphStyle, !p.textLists.isEmpty,
           let m = try? NSRegularExpression(pattern: "^\t[^\t\n]*\t").firstMatch(in: text.string, options: .anchored, range: r) {
            for k in r.location..<NSMaxRange(m.range) { x += commentMarks(k, end: false) }
            body = NSRange(location: NSMaxRange(m.range), length: NSMaxRange(r) - NSMaxRange(m.range))
        }
        // A drop cap: its letter in a framed paragraph of its own, before this one (as Word writes it).
        var dropCap = ""
        if body.length > 1, let lines = text.attribute(.docDropCap, at: body.location, effectiveRange: nil) as? Int, lines > 1 {
            let attrs = text.attributes(at: body.location, effectiveRange: nil)
            let font = attrs[.font] as? NSFont ?? .systemFont(ofSize: 12)
            let p = attrs[.paragraphStyle] as? NSParagraphStyle
            let line = font.pointSize * 1.2 * max(1, p?.lineHeightMultiple ?? 1)
            let size = (line * CGFloat(lines - 1) + font.capHeight) / max(0.5, font.capHeight / font.pointSize)
            var a = attrs
            a[.font] = NSFontManager.shared.convert(font, toSize: size)
            a[.docDropCap] = nil
            let letter = (text.string as NSString).substring(with: NSRange(location: body.location, length: 1))
            dropCap = "<w:p><w:pPr><w:keepNext/><w:framePr w:dropCap=\"drop\" w:lines=\"\(lines)\" w:wrap=\"around\" w:vAnchor=\"text\" w:hAnchor=\"text\"/>"
                + "<w:spacing w:before=\"0\" w:after=\"0\" w:line=\"\(Int((line * CGFloat(lines) * 20).rounded()))\" w:lineRule=\"exact\"/><w:textAlignment w:val=\"baseline\"/></w:pPr>"
                + runXML(letter, a) + "</w:p>"
            body = NSRange(location: body.location + 1, length: body.length - 1)
        }
        if body.length == 0 {
            x += commentMarks(body.location, end: false) + commentMarks(body.location, end: true)
        } else {
            text.enumerateAttributes(in: body) { attrs, run, _ in
                let link = (attrs[.link] as? URL)?.absoluteString ?? (attrs[.link] as? String)
                if attrs[.docSection] != nil { return }   // the break itself is the sectPr
                if ctx.commentStarts[run.location] != nil { setRevision(nil); setLink(nil); x += commentMarks(run.location, end: false) }
                setLink(link)
                let rev = attrs[.docRevision] as? DocRevision
                setRevision(rev)
                if let note = attrs[.docNote] as? DocNote {
                    ctx.notes.append(note.text)
                    x += "<w:r><w:rPr><w:rStyle w:val=\"FootnoteReference\"/><w:vertAlign w:val=\"superscript\"/></w:rPr><w:footnoteReference w:id=\"\(ctx.notes.count)\"/></w:r>"
                } else if let field = attrs[.docField] as? DocField {
                    for k in 0..<run.length {
                        let value = ctx.fieldResult?(field, run.location + k) ?? field.result
                        var a = attrs
                        a[.attachment] = nil
                        x += "<w:fldSimple w:instr=\"\(xmlEscape(field.instruction))\"\(field.fixed ? " w:fldLock=\"1\"" : "")>"
                            + runXML(value.isEmpty ? " " : value, a, style: attrs[.docCharStyle] as? String) + "</w:fldSimple>"
                    }
                } else if let attachment = attrs[.attachment] as? NSTextAttachment {
                    x += imageXML(attachment, ctx: ctx, props: attrs[.docImage] as? DocImage)
                } else {
                    let s = (text.string as NSString).substring(with: run)
                    let internalLink = link == nil || link!.hasPrefix("#")
                    let charStyle = attrs[.docCharStyle] as? String
                    let base = ctx.styles.isEmpty || !internalLink ? nil : (charStyle.flatMap { ctx.styles[$0] }?.over(paragraphBase) ?? paragraphBase)
                    x += runXML(s, attrs, style: internalLink ? charStyle : "Hyperlink", deleted: rev?.kind == .deletion, base: base)
                }
                if ctx.commentEnds[NSMaxRange(run)] != nil { setRevision(nil); setLink(nil); x += commentMarks(NSMaxRange(run), end: true) }
            }
            setLink(nil)
            setRevision(nil)
        }
        if fieldEnd { x += "<w:r><w:fldChar w:fldCharType=\"end\"/></w:r>" }
        if let (id, _) = bookmark { x += "<w:bookmarkEnd w:id=\"\(id)\"/>" }
        return dropCap + x + "</w:p>"
    }

    private static func paragraphProperties(_ p: NSParagraphStyle?, style: String? = nil, leader: Bool = false, sectPr: String? = nil,
                                            markRPr: String? = nil, numPr: String? = nil) -> String {
        guard let p else {
            if style == nil && sectPr == nil && markRPr == nil { return "" }
            return "<w:pPr>" + (style.map { "<w:pStyle w:val=\"\($0)\"/>" } ?? "") + (markRPr ?? "") + (sectPr ?? "") + "</w:pPr>"
        }
        var x = "<w:pPr>"
        if let style { x += "<w:pStyle w:val=\"\(xmlEscape(style))\"/>" }
        if let numPr { x += numPr }
        // Paragraph borders and background: a text block that is not a table cell.
        if let b = p.textBlocks.last, !(b is NSTextTableBlock) {
            let pad = b.width(for: .padding, edge: .minY)
            var edges = ""
            for (name, e) in [("top", NSRectEdge.minY), ("left", .minX), ("bottom", .maxY), ("right", .maxX)] {
                let w = b.width(for: .border, edge: e)
                guard w > 0 else { continue }
                edges += "<w:\(name) w:val=\"single\" w:sz=\"\(max(2, Int((w * 8).rounded())))\" w:space=\"\(Int(pad.rounded()))\" w:color=\"\(hex(b.borderColor(for: e) ?? .black))\"/>"
            }
            if !edges.isEmpty { x += "<w:pBdr>\(edges)</w:pBdr>" }
            if let bg = b.backgroundColor, bg.alphaComponent > 0 { x += "<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"\(hex(bg))\"/>" }
        }
        let twip = { (pt: CGFloat) in Int((pt * 20).rounded()) }
        if leader, let stop = p.tabStops.last(where: { $0.alignment == .right }) ?? p.tabStops.last {
            x += "<w:tabs><w:tab w:val=\"\(stop.alignment == .right ? "right" : "left")\" w:leader=\"dot\" w:pos=\"\(twip(stop.location))\"/></w:tabs>"
        } else if p.textLists.isEmpty, !p.tabStops.isEmpty, p.tabStops != NSParagraphStyle.default.tabStops {
            // Tab stops set on the paragraph (headers and footers: centre and right).
            x += "<w:tabs>" + p.tabStops.map { t -> String in
                let kind = t.alignment == .center ? "center" : t.alignment == .right ? "right" : t.options[.columnTerminators] != nil ? "decimal" : "left"
                return "<w:tab w:val=\"\(kind)\" w:pos=\"\(twip(t.location))\"/>"
            }.joined() + "</w:tabs>"
        }
        if p.minimumLineHeight > 0, p.minimumLineHeight == p.maximumLineHeight {
            x += "<w:spacing w:before=\"\(twip(p.paragraphSpacingBefore))\" w:after=\"\(twip(p.paragraphSpacing))\" w:line=\"\(twip(p.minimumLineHeight))\" w:lineRule=\"exact\"/>"
        } else {
            let line = p.lineHeightMultiple > 0 ? Int((p.lineHeightMultiple * 240).rounded()) : 240
            x += "<w:spacing w:before=\"\(twip(p.paragraphSpacingBefore))\" w:after=\"\(twip(p.paragraphSpacing))\" w:line=\"\(line)\" w:lineRule=\"auto\"/>"
        }
        if p.headIndent > 0 || p.firstLineHeadIndent > 0 || p.tailIndent < 0 {
            let left = twip(p.headIndent)
            let first = twip(p.firstLineHeadIndent) - left
            x += "<w:ind w:left=\"\(left)\"" + (p.tailIndent < 0 ? " w:right=\"\(twip(-p.tailIndent))\"" : "")
                + (first < 0 ? " w:hanging=\"\(-first)\"" : first > 0 ? " w:firstLine=\"\(first)\"" : "") + "/>"
        }
        switch p.alignment {
        case .center: x += "<w:jc w:val=\"center\"/>"
        case .right: x += "<w:jc w:val=\"right\"/>"
        case .justified: x += "<w:jc w:val=\"both\"/>"
        default: break
        }
        return x + (markRPr ?? "") + (sectPr ?? "") + "</w:pPr>"
    }

    private static let revisionDate: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime]
        return f
    }()

    /// <w:ins …> / <w:del …> opening tag (or empty element, for paragraph marks).
    private static func revisionTag(_ rev: DocRevision, ctx: Context, empty: Bool = false) -> String {
        ctx.revisions += 1
        let tag = rev.kind == .insertion ? "ins" : "del"
        return "<w:\(tag) w:id=\"\(ctx.revisions)\" w:author=\"\(xmlEscape(rev.author))\" w:date=\"\(revisionDate.string(from: rev.date))\"\(empty ? "/" : "")>"
    }

    private static func hex(_ c: NSColor) -> String {
        let rgb = c.usingColorSpace(.sRGB) ?? c
        return String(format: "%02X%02X%02X", Int(rgb.redComponent * 255), Int(rgb.greenComponent * 255), Int(rgb.blueComponent * 255))
    }

    /// `base`: the paragraph's (and character) style; properties equal to it are left to the style.
    private static func runXML(_ s: String, _ attrs: [NSAttributedString.Key: Any], style: String? = nil, deleted: Bool = false, base: Style? = nil) -> String {
        var pr = style.map { "<w:rStyle w:val=\"\(xmlEscape($0))\"/>" } ?? ""
        if let font = attrs[.font] as? NSFont {
            let family = xmlEscape(font.familyName ?? font.fontName)
            if base == nil || base?.family != font.familyName {
                pr += "<w:rFonts w:ascii=\"\(family)\" w:hAnsi=\"\(family)\" w:cs=\"\(family)\" w:eastAsia=\"\(family)\"/>"
            }
            let traits = NSFontManager.shared.traits(of: font)
            let bold = traits.contains(.boldFontMask), italic = traits.contains(.italicFontMask)
            if let base {
                if bold != (base.bold ?? false) { pr += bold ? "<w:b/><w:bCs/>" : "<w:b w:val=\"0\"/><w:bCs w:val=\"0\"/>" }
                if italic != (base.italic ?? false) { pr += italic ? "<w:i/><w:iCs/>" : "<w:i w:val=\"0\"/><w:iCs w:val=\"0\"/>" }
            } else {
                if bold { pr += "<w:b/><w:bCs/>" }
                if italic { pr += "<w:i/><w:iCs/>" }
            }
        }
        pr += effectsXML(attrs)
        if let c = attrs[.foregroundColor] as? NSColor, base == nil || hex(c) != hex(base?.color ?? .black) { pr += "<w:color w:val=\"\(hex(c))\"/>" }
        if let k = attrs[.kern] as? NSNumber, abs(k.doubleValue) > 0.01 { pr += "<w:spacing w:val=\"\(Int((k.doubleValue * 20).rounded()))\"/>" }
        if let font = attrs[.font] as? NSFont, base == nil || abs(font.pointSize - (base?.size ?? 12)) > 0.01 {
            let half = Int((font.pointSize * 2).rounded())
            pr += "<w:sz w:val=\"\(half)\"/><w:szCs w:val=\"\(half)\"/>"
        }
        let underline = attrs[.underlineStyle] as? Int ?? 0
        if underline != 0 {
            if base?.underline != true || underline != NSUnderlineStyle.single.rawValue { pr += "<w:u w:val=\"\(underlineValue(underline))\"/>" }
        } else if base?.underline == true { pr += "<w:u w:val=\"none\"/>" }
        if let bg = attrs[.backgroundColor] as? NSColor { pr += "<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"\(hex(bg))\"/>" }
        if let sup = attrs[.superscript] as? Int, sup != 0 { pr += "<w:vertAlign w:val=\"\(sup > 0 ? "superscript" : "subscript")\"/>" }

        var x = ""
        var chunk = ""
        func flush() {
            guard !chunk.isEmpty else { return }
            let t = deleted ? "w:delText" : "w:t"
            x += "<w:r>\(pr.isEmpty ? "" : "<w:rPr>\(pr)</w:rPr>")<\(t) xml:space=\"preserve\">\(xmlEscape(chunk))</\(t)></w:r>"
            chunk = ""
        }
        func special(_ xml: String) {
            flush()
            x += "<w:r>\(pr.isEmpty ? "" : "<w:rPr>\(pr)</w:rPr>")\(xml)</w:r>"
        }
        for ch in s {
            switch ch {
            case "\t": special("<w:tab/>")
            case "\u{2028}": special("<w:br/>")
            case "\u{000C}": special("<w:br w:type=\"page\"/>")
            case "\n", "\r", "\u{2029}", "\u{FFFC}": break
            default: chunk.append(ch)
            }
        }
        flush()
        return x
    }

    // MARK: - Images

    private static func imageXML(_ attachment: NSTextAttachment, ctx: Context, props: DocImage? = nil) -> String {
        var image = attachment.image
        if image == nil, let data = attachment.fileWrapper?.regularFileContents { image = NSImage(data: data) }
        guard let image else { return "" }
        let png: Data
        if let original = attachment.fileWrapper?.regularFileContents, TechnicalElement.fromPNG(original) != nil {
            // Keep the code/symbol recipe embedded in the PNG when saving DOCX.
            png = original
        } else {
            guard let tiff = image.tiffRepresentation,
                  let encoded = NSBitmapImageRep(data: tiff)?.representation(using: .png, properties: [:]) else { return "" }
            png = encoded
        }
        ctx.media.append((name: "\(ctx.mediaPrefix)image\(ctx.media.count + 1).png", data: png))
        let n = ctx.media.count
        var size = attachment.bounds.size
        if size.width <= 0 || size.height <= 0 { size = image.size }
        let emu = { (pt: CGFloat) in Int((pt * 12700).rounded()) }
        let cx = emu(size.width), cy = emu(size.height)
        var crop = ""
        if let c = props?.crop, props?.isCropped == true {
            crop = "<a:srcRect l=\"\(Int(c.left * 100_000))\" t=\"\(Int(c.top * 100_000))\" r=\"\(Int(c.right * 100_000))\" b=\"\(Int(c.bottom * 100_000))\"/>"
        }
        var line = ""
        if let p = props, p.borderWidth > 0 { line = "<a:ln w=\"\(emu(p.borderWidth))\"><a:solidFill><a:srgbClr val=\"\(hex(p.borderColor))\"/></a:solidFill></a:ln>" }
        let graphic = """
        <wp:docPr id="\(n)" name="Image \(n)"/><wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect="1"/></wp:cNvGraphicFramePr>\
        <a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture">\
        <pic:pic><pic:nvPicPr><pic:cNvPr id="\(n)" name="image\(n).png"/><pic:cNvPicPr/></pic:nvPicPr>\
        <pic:blipFill><a:blip r:embed="rIdImg\(n)"/>\(crop)<a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
        <pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="\(cx)" cy="\(cy)"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom>\(line)</pic:spPr>\
        </pic:pic></a:graphicData></a:graphic>
        """
        guard let p = props, p.isFloating else {
            return "<w:r><w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\"><wp:extent cx=\"\(cx)\" cy=\"\(cy)\"/>\(graphic)</wp:inline></w:drawing></w:r>"
        }
        let wrap: String
        switch p.wrap {
        case .square: wrap = "<wp:wrapSquare wrapText=\"bothSides\"/>"
        case .topBottom: wrap = "<wp:wrapTopAndBottom/>"
        default: wrap = "<wp:wrapNone/>"
        }
        return """
        <w:r><w:drawing><wp:anchor distT="76200" distB="76200" distL="114300" distR="114300" simplePos="0" relativeHeight="\(251_658_240 + n)" \
        behindDoc="\(p.wrap == .behind ? 1 : 0)" locked="0" layoutInCell="1" allowOverlap="1"><wp:simplePos x="0" y="0"/>\
        <wp:positionH relativeFrom="margin"><wp:posOffset>\(emu(p.offset.x))</wp:posOffset></wp:positionH>\
        <wp:positionV relativeFrom="margin"><wp:posOffset>\(emu(p.offset.y))</wp:posOffset></wp:positionV>\
        <wp:extent cx="\(cx)" cy="\(cy)"/><wp:effectExtent l="0" t="0" r="0" b="0"/>\(wrap)\(graphic)</wp:anchor></w:drawing></w:r>
        """
    }

    // MARK: - Tables

    private static func tableXML(_ rows: [[Int: [NSRange]]], columns: Int, text: NSAttributedString,
                                 ctx: Context, border: NSTextTableBlock) -> String {
        let twip = { (pt: CGFloat) in Int((pt * 20).rounded()) }
        func edge(_ b: NSTextTableBlock, _ e: NSRectEdge) -> String {
            let w = b.width(for: .border, edge: e)
            guard w > 0 else { return "w:val=\"nil\"" }
            return "w:val=\"single\" w:sz=\"\(max(2, Int((w * 8).rounded())))\" w:space=\"0\" w:color=\"\(hex(b.borderColor(for: e) ?? .gray))\""
        }
        // Table borders from the first cell (outer edges and the lines inside).
        let edges = [("top", NSRectEdge.minY), ("left", .minX), ("bottom", .maxY), ("right", .maxX), ("insideH", .maxY), ("insideV", .maxX)]
            .map { "<w:\($0.0) \(edge(border, $0.1))/>" }.joined()
        let table = border.table
        let headerRows = (table as? DocTextTable)?.headerRows ?? 0
        let pad = border.width(for: .padding, edge: .minX)
        // Blocks by position, spans and column widths (percent of the table, from cells that span one column).
        var blocks: [[Int: NSTextTableBlock]] = []
        for (r, row) in rows.enumerated() {
            blocks.append([:])
            for (c, paragraphs) in row {
                if let first = paragraphs.first, let b = (text.attribute(.paragraphStyle, at: first.location, effectiveRange: nil) as? NSParagraphStyle)?.textBlocks.last as? NSTextTableBlock {
                    blocks[r][c] = b
                }
            }
        }
        var widths = [CGFloat](repeating: 0, count: columns)
        for row in blocks {
            for (c, b) in row where b.columnSpan == 1 && c < columns && widths[c] == 0 {
                let v = b.value(for: .width)
                if v > 0 { widths[c] = b.valueType(for: .width) == .percentageValueType ? v : -v }   // negative: points
            }
        }
        let textWidth = max(72, ctx.textWidth)
        let pcts: [CGFloat] = widths.map { w in w > 0 ? w : w < 0 ? -w / textWidth * 100 : 0 }
        let known = pcts.reduce(0, +), unknown = pcts.filter { $0 == 0 }.count
        let filled = pcts.map { $0 > 0 ? $0 : max(5, (100 - known) / CGFloat(max(1, unknown))) }
        let total = filled.reduce(0, +)
        let pct = filled.map { $0 / max(1, total) * 100 }
        // Cells covered by a cell above (rowSpan): row → columns → span width.
        var covered: [Int: [Int: (Int, NSTextTableBlock)]] = [:]
        for (r, row) in blocks.enumerated() {
            for (c, b) in row where b.rowSpan > 1 {
                for k in 1..<b.rowSpan { covered[r + k, default: [:]][c] = (max(1, b.columnSpan), b) }
            }
        }
        var x = "<w:tbl><w:tblPr><w:tblW w:w=\"5000\" w:type=\"pct\"/><w:tblBorders>\(edges)</w:tblBorders>"
        x += "<w:tblLayout w:type=\"fixed\"/><w:tblCellMar><w:top w:w=\"\(twip(pad / 2))\" w:type=\"dxa\"/><w:left w:w=\"\(twip(pad))\" w:type=\"dxa\"/>"
        x += "<w:bottom w:w=\"\(twip(pad / 2))\" w:type=\"dxa\"/><w:right w:w=\"\(twip(pad))\" w:type=\"dxa\"/></w:tblCellMar></w:tblPr><w:tblGrid>"
        for p in pct { x += "<w:gridCol w:w=\"\(twip(textWidth * p / 100))\"/>" }
        x += "</w:tblGrid>"
        let rowCount = max(rows.count, (covered.keys.max() ?? -1) + 1)
        for r in 0..<rowCount {
            let row = r < rows.count ? rows[r] : [:]
            x += "<w:tr>" + (r < headerRows ? "<w:trPr><w:tblHeader/></w:trPr>" : "")
            var c = 0
            while c < columns {
                func cellPr(_ b: NSTextTableBlock?, span: Int, merge: String?) -> String {
                    let w = pct[c..<min(columns, c + span)].reduce(0, +)
                    var pr = "<w:tcPr><w:tcW w:w=\"\(Int((w * 50).rounded()))\" w:type=\"pct\"/>" + (span > 1 ? "<w:gridSpan w:val=\"\(span)\"/>" : "")
                    if let merge { pr += merge }
                    if let b {
                        pr += "<w:tcBorders>" + [("top", NSRectEdge.minY), ("left", .minX), ("bottom", .maxY), ("right", .maxX)].map { "<w:\($0.0) \(edge(b, $0.1))/>" }.joined() + "</w:tcBorders>"
                        if let bg = b.backgroundColor, bg.alphaComponent > 0 { pr += "<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"\(hex(bg))\"/>" }
                        switch b.verticalAlignment {
                        case .middleAlignment: pr += "<w:vAlign w:val=\"center\"/>"
                        case .bottomAlignment: pr += "<w:vAlign w:val=\"bottom\"/>"
                        default: break
                        }
                    }
                    return pr + "</w:tcPr>"
                }
                if let (span, b) = covered[r]?[c] {
                    x += "<w:tc>" + cellPr(b, span: span, merge: "<w:vMerge/>") + "<w:p/></w:tc>"
                    c += span
                    continue
                }
                let paragraphs = row[c] ?? []
                let b = blocks.indices.contains(r) ? blocks[r][c] : nil
                let span = max(1, b?.columnSpan ?? 1)
                x += "<w:tc>" + cellPr(b, span: span, merge: (b?.rowSpan ?? 1) > 1 ? "<w:vMerge w:val=\"restart\"/>" : nil)
                if paragraphs.isEmpty { x += "<w:p/>" }
                for p in paragraphs { x += paragraphXML(p, text: text, ctx: ctx) }
                x += "</w:tc>"
                c += span
            }
            x += "</w:tr>"
        }
        return x + "</w:tbl>"
    }
}

/// A table that knows how many of its first rows repeat as a header on each page (w:tblHeader).
open class DocTextTable: NSTextTable {
    open var headerRows = 0
}

// MARK: - Character effects

public extension NSAttributedString.Key {
    /// true: shown in capitals (w:caps); the text keeps its case.
    static let docAllCaps = NSAttributedString.Key("Kit42.caps")
    /// true: small capitals (w:smallCaps).
    static let docSmallCaps = NSAttributedString.Key("Kit42.smallCaps")
    /// Drop cap on a paragraph's first letter: the number of lines it spans (Int); a w:framePr dropCap paragraph.
    static let docDropCap = NSAttributedString.Key("Kit42.dropCap")
}

extension DocxWriter {
    /// Caps, small caps, strikethrough (single or double).
    static func effectsXML(_ attrs: [NSAttributedString.Key: Any]) -> String {
        var x = ""
        if attrs[.docAllCaps] as? Bool == true { x += "<w:caps/>" }
        if attrs[.docSmallCaps] as? Bool == true { x += "<w:smallCaps/>" }
        let strike = attrs[.strikethroughStyle] as? Int ?? 0
        if strike != 0 { x += strike & 0xF == NSUnderlineStyle.double.rawValue ? "<w:dstrike/>" : "<w:strike/>" }
        return x
    }

    /// w:u value for an NSUnderlineStyle.
    static func underlineValue(_ v: Int) -> String {
        let style = v & 0xF, pattern = v & 0x700
        if v & NSUnderlineStyle.byWord.rawValue != 0 { return "words" }
        if style == NSUnderlineStyle.double.rawValue { return "double" }
        if style == NSUnderlineStyle.thick.rawValue { return pattern == NSUnderlineStyle.patternDot.rawValue ? "dottedHeavy" : "thick" }
        switch pattern {
        case NSUnderlineStyle.patternDot.rawValue: return "dotted"
        case NSUnderlineStyle.patternDash.rawValue: return "dash"
        case NSUnderlineStyle.patternDashDot.rawValue: return "dotDash"
        case NSUnderlineStyle.patternDashDotDot.rawValue: return "dotDotDash"
        default: return "single"
        }
    }

    /// NSUnderlineStyle for a w:u value.
    public static func underlineStyle(_ val: String) -> Int {
        switch val {
        case "none", "0", "false": return 0
        case "double": return NSUnderlineStyle.double.rawValue
        case "thick": return NSUnderlineStyle.thick.rawValue
        case "dotted": return NSUnderlineStyle.single.rawValue | NSUnderlineStyle.patternDot.rawValue
        case "dottedHeavy": return NSUnderlineStyle.thick.rawValue | NSUnderlineStyle.patternDot.rawValue
        case "dash", "dashLong", "dashedHeavy": return NSUnderlineStyle.single.rawValue | NSUnderlineStyle.patternDash.rawValue
        case "dotDash": return NSUnderlineStyle.single.rawValue | NSUnderlineStyle.patternDashDot.rawValue
        case "dotDotDash": return NSUnderlineStyle.single.rawValue | NSUnderlineStyle.patternDashDotDot.rawValue
        case "words": return NSUnderlineStyle.single.rawValue | NSUnderlineStyle.byWord.rawValue
        default: return NSUnderlineStyle.single.rawValue
        }
    }
}

// MARK: - styles.xml

extension DocxWriter {
    static func stylesXML(_ styles: [Style]) -> String {
        let twip = { (pt: CGFloat) in Int((pt * 20).rounded()) }
        let normal = styles.first { $0.id == "Normal" }
        let family = xmlEscape(normal?.family ?? "Helvetica Neue")
        let size = Int(((normal?.size ?? 12) * 2).rounded())
        var x = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">"
        x += "<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"\(family)\" w:hAnsi=\"\(family)\" w:cs=\"\(family)\" w:eastAsia=\"\(family)\"/>"
        x += "<w:sz w:val=\"\(size)\"/><w:szCs w:val=\"\(size)\"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults>"
        var ids = Set<String>()
        for s in styles {
            ids.insert(s.id)
            let type = s.character ? "character" : "paragraph"
            x += "<w:style w:type=\"\(type)\"\(s.id == "Normal" ? " w:default=\"1\"" : "")\(s.custom ? " w:customStyle=\"1\"" : "") w:styleId=\"\(xmlEscape(s.id))\">"
            x += "<w:name w:val=\"\(xmlEscape(s.name))\"/>"
            if let b = s.basedOn { x += "<w:basedOn w:val=\"\(xmlEscape(b))\"/>" }
            if let n = s.next { x += "<w:next w:val=\"\(xmlEscape(n))\"/>" }
            x += "<w:qFormat/>"
            if !s.character {
                var p = ""
                if s.outlineLevel != nil { p += "<w:keepNext/>" }
                if s.spaceBefore != nil || s.spaceAfter != nil || s.lineHeightMultiple != nil {
                    let line = Int(((s.lineHeightMultiple ?? 1) * 240).rounded())
                    p += "<w:spacing w:before=\"\(twip(s.spaceBefore ?? 0))\" w:after=\"\(twip(s.spaceAfter ?? 0))\" w:line=\"\(line)\" w:lineRule=\"auto\"/>"
                }
                let head = s.headIndent ?? 0, first = (s.firstLineHeadIndent ?? head) - head, tail = -(s.tailIndent ?? 0)
                if head > 0 || first != 0 || tail > 0 {
                    p += "<w:ind w:left=\"\(twip(head))\"" + (tail > 0 ? " w:right=\"\(twip(tail))\"" : "")
                        + (first < 0 ? " w:hanging=\"\(twip(-first))\"" : first > 0 ? " w:firstLine=\"\(twip(first))\"" : "") + "/>"
                }
                switch s.alignment {
                case .center: p += "<w:jc w:val=\"center\"/>"
                case .right: p += "<w:jc w:val=\"right\"/>"
                case .justified: p += "<w:jc w:val=\"both\"/>"
                case .left: p += "<w:jc w:val=\"left\"/>"
                default: break
                }
                if let o = s.outlineLevel { p += "<w:outlineLvl w:val=\"\(o)\"/>" }
                if !p.isEmpty { x += "<w:pPr>\(p)</w:pPr>" }
            }
            var r = ""
            if let f = s.family.map(xmlEscape) { r += "<w:rFonts w:ascii=\"\(f)\" w:hAnsi=\"\(f)\" w:cs=\"\(f)\" w:eastAsia=\"\(f)\"/>" }
            if let b = s.bold { r += b ? "<w:b/><w:bCs/>" : "<w:b w:val=\"0\"/><w:bCs w:val=\"0\"/>" }
            if let i = s.italic { r += i ? "<w:i/><w:iCs/>" : "<w:i w:val=\"0\"/><w:iCs w:val=\"0\"/>" }
            if let c = s.color { r += "<w:color w:val=\"\(hex(c))\"/>" }
            if let sz = s.size { let h = Int((sz * 2).rounded()); r += "<w:sz w:val=\"\(h)\"/><w:szCs w:val=\"\(h)\"/>" }
            if let u = s.underline { r += "<w:u w:val=\"\(u ? "single" : "none")\"/>" }
            if !r.isEmpty { x += "<w:rPr>\(r)</w:rPr>" }
            x += "</w:style>"
        }
        // Technical styles the writer refers to.
        func para(_ id: String, _ name: String, _ rest: String = "") -> (String, String) {
            (id, "<w:style w:type=\"paragraph\" w:styleId=\"\(id)\"><w:name w:val=\"\(name)\"/><w:basedOn w:val=\"Normal\"/>\(rest)</w:style>")
        }
        func char(_ id: String, _ name: String, _ rPr: String, def: Bool = false) -> (String, String) {
            (id, "<w:style w:type=\"character\"\(def ? " w:default=\"1\"" : "") w:styleId=\"\(id)\"><w:name w:val=\"\(name)\"/>\(rPr.isEmpty ? "" : "<w:rPr>\(rPr)</w:rPr>")</w:style>")
        }
        let tabs = "<w:pPr><w:tabs><w:tab w:val=\"center\" w:pos=\"4680\"/><w:tab w:val=\"right\" w:pos=\"9360\"/></w:tabs></w:pPr>"
        let extra: [(String, String)] = [
            para("TOCHeading", "TOC Heading"), para("TOC1", "toc 1"), para("TOC2", "toc 2"), para("TOC3", "toc 3"),
            para("FootnoteText", "footnote text", "<w:rPr><w:sz w:val=\"18\"/><w:szCs w:val=\"18\"/></w:rPr>"),
            para("CommentText", "annotation text", "<w:rPr><w:sz w:val=\"20\"/><w:szCs w:val=\"20\"/></w:rPr>"),
            para("Header", "header", tabs), para("Footer", "footer", tabs),
            char("DefaultParagraphFont", "Default Paragraph Font", "", def: true),
            char("Hyperlink", "Hyperlink", "<w:color w:val=\"0563C1\"/><w:u w:val=\"single\"/>"),
            char("FootnoteReference", "footnote reference", "<w:vertAlign w:val=\"superscript\"/>"),
            char("CommentReference", "annotation reference", "<w:sz w:val=\"16\"/><w:szCs w:val=\"16\"/>"),
            ("TableNormal", "<w:style w:type=\"table\" w:default=\"1\" w:styleId=\"TableNormal\"><w:name w:val=\"Normal Table\"/><w:tblPr><w:tblCellMar><w:top w:w=\"0\" w:type=\"dxa\"/><w:left w:w=\"108\" w:type=\"dxa\"/><w:bottom w:w=\"0\" w:type=\"dxa\"/><w:right w:w=\"108\" w:type=\"dxa\"/></w:tblCellMar></w:tblPr></w:style>"),
        ]
        for (id, xml) in extra where !ids.contains(id) { x += xml }
        return x + "</w:styles>"
    }
}


// MARK: - numbering.xml

extension DocxWriter {
    /// numFmt and lvlText of an NSTextList level ("{decimal}." → decimal, "%1."; with enclosing markers "%1.%2.").
    static func numberFormat(_ lists: [Int: NSTextList], level: Int) -> (fmt: String, text: String, start: Int) {
        func parts(_ l: NSTextList, _ n: Int) -> (String, String) {
            let raw = l.markerFormat.rawValue
            let tokens: [(String, String)] = [("{decimal}", "decimal"), ("{lower-alpha}", "lowerLetter"), ("{lower-latin}", "lowerLetter"),
                                              ("{upper-alpha}", "upperLetter"), ("{upper-latin}", "upperLetter"), ("{lower-roman}", "lowerRoman"),
                                              ("{upper-roman}", "upperRoman"), ("{octal}", "decimal"), ("{lower-hexadecimal}", "decimal"), ("{upper-hexadecimal}", "decimal")]
            for (t, f) in tokens where raw.contains(t) { return (f, raw.replacingOccurrences(of: t, with: "%\(n)")) }
            let bullets = ["{disc}": "•", "{circle}": "◦", "{square}": "▪", "{box}": "☐", "{check}": "✓", "{diamond}": "◆", "{hyphen}": "–"]
            for (t, b) in bullets where raw.contains(t) { return ("bullet", raw.replacingOccurrences(of: t, with: b)) }
            return ("bullet", raw.isEmpty ? "•" : raw)
        }
        guard let own = lists[level] else { return ("bullet", "•", 1) }
        var (fmt, text) = parts(own, level + 1)
        if own.listOptions.contains(.prependEnclosingMarker), fmt != "bullet" {
            var prefix = ""
            for k in 0..<level { if let l = lists[k] { prefix += parts(l, k + 1).1 } }
            text = prefix + text
        }
        return (fmt, text, own.startingItemNumber)
    }

    private static func numberingXML(_ ctx: Context) -> String {
        var x = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:numbering xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">"
        let ids = ctx.numIds.values.sorted()
        for id in ids {
            let lists = ctx.levels[id] ?? [:]
            x += "<w:abstractNum w:abstractNumId=\"\(id)\"><w:multiLevelType w:val=\"\((lists.keys.max() ?? 0) > 0 ? "multilevel" : "hybridMultilevel")\"/>"
            for k in 0...max(0, lists.keys.max() ?? 0) {
                let (fmt, text, start) = numberFormat(lists, level: k)
                let styles = ctx.levelStyles[id]?[k] ?? []
                let heading = styles.count == 1 && styles.first!.hasPrefix("Heading") ? styles.first : nil
                x += "<w:lvl w:ilvl=\"\(k)\"><w:start w:val=\"\(start)\"/><w:numFmt w:val=\"\(fmt)\"/>"
                if let heading { x += "<w:pStyle w:val=\"\(heading)\"/>" }
                if heading != nil { x += "<w:suff w:val=\"space\"/>" }
                x += "<w:lvlText w:val=\"\(xmlEscape(text))\"/><w:lvlJc w:val=\"left\"/>"
                x += heading != nil ? "<w:pPr><w:ind w:left=\"0\" w:firstLine=\"0\"/></w:pPr>" : "<w:pPr><w:ind w:left=\"\(720 * (k + 1))\" w:hanging=\"360\"/></w:pPr>"
                x += "</w:lvl>"
            }
            x += "</w:abstractNum>"
        }
        for id in ids { x += "<w:num w:numId=\"\(id)\"><w:abstractNumId w:val=\"\(id)\"/></w:num>" }
        return x + "</w:numbering>"
    }
}
