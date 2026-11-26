import AppKit
import Kit42
import PDFKit

@objc(TextDocument)
final class TextDocument: NSDocument {
    let storage = NSTextStorage()
    /// Page size, margins, header and footer (saved to .docx).
    var setup = PageSetup() { didSet { editor?.setupDidChange() } }
    /// Track changes is on: edits are recorded as insertions and deletions (saved to .docx).
    var trackChanges = false
    /// Paragraph and character styles (saved to .docx styles.xml and .odt styles.xml).
    let styles = StyleSheet()
    /// Title and author (docProps/core.xml, PDF metadata, fields).
    var docInfo = DocInfo()
    /// Page colour and watermark text (Format › Page), automatic hyphenation.
    var background: NSColor? { didSet { editor?.pageLookDidChange() } }
    var watermark = "" { didSet { editor?.pageLookDidChange() } }
    var hyphenate = false { didSet { editor?.pageLookDidChange() } }

    /// Document facts for fields: the file name follows the file.
    var fieldInfo: DocInfo {
        var i = docInfo
        i.fileName = fileURL?.lastPathComponent ?? displayName ?? ""
        return i
    }

    override init() {
        super.init()
        storage.setAttributedString(NSAttributedString(string: "", attributes: Styles.normal.attributes))
    }

    override class var autosavesInPlace: Bool { false }

    override var isDocumentEdited: Bool {
        super.isDocumentEdited || editor?.headerEditor?.hasChanges == true
    }

    override func canClose(withDelegate delegate: Any, shouldClose shouldCloseSelector: Selector?, contextInfo: UnsafeMutableRawPointer?) {
        commitPendingEdits()
        super.canClose(withDelegate: delegate, shouldClose: shouldCloseSelector, contextInfo: contextInfo)
    }

    var editor: DocWindowController? { windowControllers.first as? DocWindowController }

    override func makeWindowControllers() {
        let controller = DocWindowController(storage: storage)
        addWindowController(controller)
        if let root = controller.window?.contentView { signatureState.install(in: root) }
    }

    // MARK: Digital signatures (Office/OpenDocument XMLDSig, Kit42)

    lazy var signatureState = DocumentSignatureState(document: self)

    override func updateChangeCount(_ change: NSDocument.ChangeType) {
        super.updateChangeCount(change)
        if change == .changeDone { signatureState.didEdit() }
    }

    override func save(_ sender: Any?) {
        guard signatureState.confirmSave() else { return }
        super.save(sender)
    }

    override func save(to url: URL, ofType typeName: String, for saveOperation: NSDocument.SaveOperationType,
                       completionHandler: @escaping (Error?) -> Void) {
        super.save(to: url, ofType: typeName, for: saveOperation) { [weak self] error in
            if error == nil, saveOperation == .saveOperation || saveOperation == .saveAsOperation { self?.signatureState.didSave() }
            completionHandler(error)
        }
    }

    @objc func signDocumentAction(_ sender: Any?) { signatureState.sign() }
    @objc func showSignaturesAction(_ sender: Any?) { signatureState.showDetails() }

    static func documentType(for typeName: String) -> NSAttributedString.DocumentType {
        let t = typeName.lowercased()
        if t.contains("wordprocessingml") || t.hasSuffix("docx") { return .officeOpenXML }
        if t.contains("word.doc") || t == "doc" { return .docFormat }
        if t.contains("opendocument") || t.hasSuffix("odt") { return .openDocument }
        if t.contains("rtf") { return t.contains("rtfd") ? .rtfd : .rtf }
        if t.contains("html") { return .html }
        return .plain
    }

    static func isMarkdown(_ typeName: String) -> Bool {
        let t = typeName.lowercased()
        return t.contains("markdown") || t.hasSuffix(".md") || t == "md"
    }

    /// Encoding of an opened plain-text file: saved back the same way.
    var textEncoding: String.Encoding = .utf8
    /// Folder of the file being read: Markdown pictures are relative to it.
    private var readingFrom: URL?

    override func read(from url: URL, ofType typeName: String) throws {
        readingFrom = url.deletingLastPathComponent()
        defer { readingFrom = nil }
        try super.read(from: url, ofType: typeName)
        signatureState.load(url)
    }

    override func read(from data: Data, ofType typeName: String) throws {
        if Self.isMarkdown(typeName) {
            let (md, encoding) = TextDecoding.decode(data)
            let text = MarkdownReader.read(md, baseURL: readingFrom ?? fileURL?.deletingLastPathComponent())
            applyImportedSettings(DocxStyles.Extras())
            textEncoding = encoding
            storage.setAttributedString(text)
            return
        }
        let type = Self.documentType(for: typeName)
        if type == .plain {
            let (text, encoding) = TextDecoding.decode(data)
            applyImportedSettings(DocxStyles.Extras())
            textEncoding = encoding
            storage.setAttributedString(NSAttributedString(string: text, attributes: Styles.normal.attributes))
            return
        }
        let options: [NSAttributedString.DocumentReadingOptionKey: Any] = [.documentType: type]
        if type == .officeOpenXML {
            let (text, extras) = try DocxStyles.read(data)
            applyImportedSettings(extras)
            storage.setAttributedString(text)
            return
        }
        var source = data
        var extras = DocxStyles.Extras()
        switch type {
        case .openDocument: (source, extras) = OdtFormat.prepare(data)
        case .rtf: (source, extras) = RtfFormat.prepare(data)
        default: break
        }
        var attributes: NSDictionary?
        let text = NSMutableAttributedString(attributedString: try NSAttributedString(data: source, options: options, documentAttributes: &attributes))
        DocxStyles.restoreImages(text, extras.images)
        DocxStyles.restoreStyles(text)
        DocxStyles.restoreLists(text, extras.lists)
        DocxStyles.restoreMarks(text, extras)
        if type == .rtf, let a = attributes as? [NSAttributedString.DocumentAttributeKey: Any],
           let paper = a[.paperSize] as? NSSize, paper.width > 0, paper.height > 0 {
            extras.hasSetup = true
            extras.setup.size = paper
            func m(_ k: NSAttributedString.DocumentAttributeKey, _ d: CGFloat) -> CGFloat { (a[k] as? NSNumber).map { CGFloat($0.doubleValue) } ?? d }
            let d = extras.setup.margins
            extras.setup.margins = NSEdgeInsets(top: m(.topMargin, d.top), left: m(.leftMargin, d.left), bottom: m(.bottomMargin, d.bottom), right: m(.rightMargin, d.right))
        }
        applyImportedSettings(extras)
        if type == .officeOpenXML || type == .docFormat || type == .openDocument || type == .rtf { ListMarkers.insert(into: text) }
        storage.setAttributedString(text)
    }

    /// Reading also implements Revert: replace document-level state only after parsing succeeded.
    private func applyImportedSettings(_ extras: DocxStyles.Extras) {
        if let editor, let pending = editor.headerEditor {
            editor.headerEditor = nil
            pending.card.editing = nil
            pending.onCommit = nil
            pending.commit()
        }
        setup = extras.hasSetup ? extras.setup : PageSetup()
        styles.setAll(StyleSheet.builtins)
        styles.merge(extras.styles)
        trackChanges = extras.trackRevisions
        docInfo = extras.info
        background = extras.background
        watermark = extras.watermark
        hyphenate = extras.hyphenate
        textEncoding = .utf8
        editor?.rebuildStylePopup()
    }

    override func data(ofType typeName: String) throws -> Data {
        commitPendingEdits()
        if Self.isMarkdown(typeName) { return markdownData() }
        return try exportData(Self.documentType(for: typeName))
    }

    /// Plain text in the file's own encoding when every character fits, else UTF-8.
    private func encode(_ s: String) -> Data {
        if textEncoding != .utf8, let d = s.data(using: textEncoding, allowLossyConversion: false) {
            if textEncoding == .utf16LittleEndian { return Data([0xFF, 0xFE]) + d }
            if textEncoding == .utf16BigEndian { return Data([0xFE, 0xFF]) + d }
            return d
        }
        return Data(s.utf8)
    }

    /// Save/export/print must include text still being edited in a header or footer.
    func commitPendingEdits() {
        editor?.headerEditor?.commit()
        editor?.textView.breakUndoCoalescing()
    }

    func markdownData() -> Data {
        commitPendingEdits()
        return encode(MarkdownWriter.write(Fields.flatten(Revisions.accepted(storage), info: fieldInfo)))
    }   // footnotes as [^n]

    /// Header and footer as plain text and page numbers, for formats without rich headers (ODT, RTF).
    var plainSetup: PageSetup {
        var s = setup
        guard let hf = setup.hf else { return s }
        s.header = HeaderFooter.plain(hf.header).0
        let (footer, page) = HeaderFooter.plain(hf.footer)
        s.footer = footer
        s.pageNumbers = page || HeaderFooter.plain(hf.header).1
        return s
    }

    func exportData(_ type: NSAttributedString.DocumentType) throws -> Data {
        commitPendingEdits()
        if type == .plain { return encode(Notes.flatten(Fields.flatten(Revisions.accepted(storage), info: fieldInfo)).string) }
        if type == .officeOpenXML {
            var o = DocxWriter.Options()
            o.pageSize = setup.size
            o.margins = setup.margins
            o.header = setup.header
            o.footer = setup.footer
            o.pageNumbers = setup.pageNumbers
            o.columns = setup.columns
            o.columnSpacing = setup.columnSpacing
            o.paragraphStyle = Styles.wordStyle
            o.styles = styles.docxStyles()
            o.headerFooter = setup.hf
            o.title = docInfo.title
            o.author = docInfo.author
            o.background = background
            o.watermark = watermark
            o.autoHyphenation = hyphenate
            let pages = offscreenPages()
            let fieldInfo = fieldInfo
            o.fieldResult = { f, i in
                i < 0 ? Fields.value(f, at: i, FieldContext(page: 1, pages: pages.cards.count, info: fieldInfo, storage: self.storage))
                    : Fields.value(f, at: i, pages.fieldContext(page: pages.pageNumber(at: i)))
            }
            defer { pages.detach() }
            o.trackRevisions = trackChanges
            return DocxWriter.data(storage, options: o)
        }
        // Tracked changes are accepted. Other formats have no footnotes (ODT has): references become numbers,
        // notes a list at the end. Comments go to ODT, RTF and HTML through markers (Annotations.swift).
        let accepted = Fields.flatten(Revisions.accepted(self.storage), info: fieldInfo)
        let setup = plainSetup
        var comments: [DocComment] = [], notes: [DocNote] = []
        var storage: NSAttributedString
        switch type {
        case .openDocument:
            (storage, comments, notes) = CommentMarkers.insert(accepted, notes: true)
            storage = OdtStyles.markParagraphs(Notes.flatten(storage))   // (only section geometry left to drop)
        case .rtf, .html:
            (storage, comments, _) = CommentMarkers.insert(Notes.flatten(accepted), notes: false, clamp: type == .html)
        default:
            storage = Notes.flatten(accepted)
        }
        let range = NSRange(location: 0, length: storage.length)
        if type == .html { return CommentMarkers.finishHTML(try storage.data(from: range, documentAttributes: [.documentType: type, .characterEncoding: String.Encoding.utf8.rawValue]), comments: comments) }
        var attrs: [NSAttributedString.DocumentAttributeKey: Any] = [.documentType: type]
        if type == .html { attrs[.characterEncoding] = String.Encoding.utf8.rawValue }
        if type == .rtf || type == .openDocument {
            // Pictures, page setup, header and footer are added to the system writer's output.
            let (text, pictures) = ImageMarkers.extract(storage)
            attrs[.paperSize] = setup.size
            attrs[.topMargin] = setup.margins.top
            attrs[.bottomMargin] = setup.margins.bottom
            attrs[.leftMargin] = setup.margins.left
            attrs[.rightMargin] = setup.margins.right
            let data = try text.data(from: NSRange(location: 0, length: text.length), documentAttributes: attrs)
            if type == .rtf {
                let rtf = RtfFormat.finish(data, pictures: pictures, setup: setup)
                return comments.isEmpty ? rtf : Data(RtfFormat.finishAnnotations(String(decoding: rtf, as: UTF8.self), comments: comments).utf8)
            }
            var borders = false
            storage.enumerateAttribute(.paragraphStyle, in: range) { v, _, stop in
                if let b = (v as? NSParagraphStyle)?.textBlocks.last as? NSTextTableBlock, b.width(for: .border, edge: .minX) > 0 { borders = true; stop.pointee = true }
            }
            return OdtFormat.finish(data, pictures: pictures, setup: setup, tableBorders: borders, comments: comments, notes: notes, styles: styles)
        }
        return try storage.data(from: range, documentAttributes: attrs)
    }

    /// Paginated PDF of the document: exactly the pages shown on screen.
    /// (Sections in another size or orientation: each page as its own sheet; see pdfData(_:).)
    func pdfData() -> Data { (try? pdfData(PDFOptions())) ?? Data() }

    /// Paper = page size, no extra margins: each page card is one sheet.
    func pageInfo() -> NSPrintInfo {
        let info = printInfo.copy() as! NSPrintInfo
        info.paperSize = setup.size
        info.orientation = setup.size.width > setup.size.height ? .landscape : .portrait
        info.leftMargin = 0; info.rightMargin = 0; info.topMargin = 0; info.bottomMargin = 0
        info.horizontalPagination = .clip
        info.verticalPagination = .clip
        info.isHorizontallyCentered = false
        info.isVerticallyCentered = false
        return info
    }

    func debugPages() {
        let v = offscreenPages()
        for (i, c) in v.cards.enumerated() {
            for (j, tv) in c.textViews.enumerated() {
                let g = v.layoutManager.glyphRange(for: tv.textContainer!)
                let chars = v.layoutManager.characterRange(forGlyphRange: g, actualGlyphRange: nil)
                Swift.print(i, j, c.frame, "chars", chars, "notes", c.notes.map(\.0), (storage.string as NSString).substring(with: NSRange(location: chars.location, length: min(20, chars.length))).debugDescription)
                if ProcessInfo.processInfo.environment["K42_DEBUG"] == "lines" {
                    v.layoutManager.enumerateLineFragments(forGlyphRange: g) { rect, _, _, glyphs, _ in
                        let cr = v.layoutManager.characterRange(forGlyphRange: glyphs, actualGlyphRange: nil)
                        Swift.print("   line", rect, (self.storage.string as NSString).substring(with: NSRange(location: cr.location, length: min(12, cr.length))).debugDescription)
                    }
                }
            }
        }
        v.detach()
    }

    /// Pages of the document laid out offscreen; `accepted`: with every tracked change accepted (PDF, printing).
    func offscreenPages(accepted: Bool = false) -> PagedView {
        commitPendingEdits()
        // An export layout must not attach another manager to the live editor's storage.
        // Attributed-string copying is shallow: fields also need independent result caches
        // and attachment cells, otherwise pagination during export invalidates the editor.
        let source: NSAttributedString = accepted && Revisions.has(storage) ? Revisions.accepted(storage) : storage
        let text = NSTextStorage(attributedString: source)
        source.enumerateAttribute(.docField, in: NSRange(location: 0, length: source.length)) { value, range, _ in
            guard let field = value as? DocField else { return }
            let attachment = NSTextAttachment()
            attachment.attachmentCell = FieldCell()
            text.addAttributes([.docField: field.copy(result: field.result), .attachment: attachment], range: range)
        }
        let view = PagedView(storage: text, setup: setup) { tv in
            tv.backgroundColor = .white
            tv.drawsBackground = false
        }
        view.ownedStorage = text   // layout managers don't retain their storage
        let info = fieldInfo
        view.info = { info }
        view.background = background
        view.watermark = watermark
        view.layoutManager.hyphenationFactor = hyphenate ? 0.9 : 0
        view.layoutAll()
        return view
    }

    override func printOperation(withSettings printSettings: [NSPrintInfo.AttributeKey: Any]) throws -> NSPrintOperation {
        let view = offscreenPages(accepted: true)
        if view.hasMixedPageSizes {
            defer { view.detach() }
            let op = try ProductionPrint.operation(pdfData: view.pdfData(), printInfo: pageInfo())
            op.jobTitle = displayName
            return op
        }
        let op = NSPrintOperation(view: view, printInfo: pageInfo())
        op.jobTitle = displayName
        op.printPanel.options.formUnion([.showsPaperSize, .showsOrientation, .showsScaling, .showsPreview, .showsPageRange])
        ProductionPrint.install(on: op)
        return op
    }
}

/// Paragraph styles offered in DOC42; saved as direct formatting plus the matching Word style.
struct Styles {
    /// Word style id for a paragraph: from the explicit style mark, else by its look.
    static func wordStyle(_ text: NSAttributedString, _ range: NSRange) -> String? {
        guard text.length > 0 else { return nil }
        let at = min(range.location, text.length - 1)
        if let name = text.attribute(styleKey, at: at, effectiveRange: nil) as? String { return name }
        guard let font = text.attribute(.font, at: at, effectiveRange: nil) as? NSFont else { return nil }
        let bold = NSFontManager.shared.traits(of: font).contains(.boldFontMask)
        switch font.pointSize {
        case 24...: return "Title"
        case 18..<24 where bold: return "Heading1"
        case 15..<18 where bold: return "Heading2"
        case 13..<15 where bold: return "Heading3"
        default: return nil
        }
    }

    static let styleKey = NSAttributedString.Key("DOC42.style")
    static let wordIds = ["Title", "Heading1", "Heading2", "Heading3", nil, "Quote"]

    let name: String
    let size: CGFloat
    let weight: NSFont.Weight
    let spacingBefore: CGFloat
    let spacingAfter: CGFloat
    let color: NSColor
    let italic: Bool

    static let normal = Styles(name: L("Body"), size: 12, weight: .regular, spacingBefore: 0, spacingAfter: 6, color: .black, italic: false)
    static let all: [Styles] = [
        Styles(name: L("Title"), size: 26, weight: .bold, spacingBefore: 0, spacingAfter: 12, color: .black, italic: false),
        Styles(name: L("Heading 1"), size: 20, weight: .bold, spacingBefore: 18, spacingAfter: 6, color: .black, italic: false),
        Styles(name: L("Heading 2"), size: 16, weight: .semibold, spacingBefore: 14, spacingAfter: 4, color: .black, italic: false),
        Styles(name: L("Heading 3"), size: 13, weight: .semibold, spacingBefore: 10, spacingAfter: 2, color: NSColor(white: 0.25, alpha: 1), italic: false),
        normal,
        Styles(name: L("Quote"), size: 12, weight: .regular, spacingBefore: 6, spacingAfter: 6, color: NSColor(white: 0.35, alpha: 1), italic: true),
    ]

    static var defaultFamily: String { NSFont(name: "Helvetica Neue", size: 12) != nil ? "Helvetica Neue" : NSFont.systemFont(ofSize: 12).familyName ?? "Helvetica" }

    var font: NSFont {
        var font = NSFontManager.shared.font(withFamily: Self.defaultFamily, traits: [], weight: weight == .bold ? 9 : weight == .semibold ? 8 : 5, size: size)
            ?? NSFont.systemFont(ofSize: size, weight: weight)
        if italic { font = NSFontManager.shared.convert(font, toHaveTrait: .italicFontMask) }
        return font
    }

    var paragraph: NSParagraphStyle {
        let p = NSMutableParagraphStyle()
        p.paragraphSpacingBefore = spacingBefore
        p.paragraphSpacing = spacingAfter
        p.lineHeightMultiple = 1.15
        if italic { p.headIndent = 24; p.firstLineHeadIndent = 24 }
        return p
    }

    var attributes: [NSAttributedString.Key: Any] {
        [.font: font, .paragraphStyle: paragraph, .foregroundColor: color]
    }
}
