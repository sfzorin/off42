import AppKit
import Kit42

/// Where the user is: slide and selected objects (saved with each undo step).
struct EditState: Equatable {
    var slide = 0
    var selection: [Int] = []
}

@objc(PresentationDocument)
final class PresentationDocument: NSDocument {
    private(set) var deck = Deck.makeDefault()
    /// Called after every change of the deck (undo/redo too).
    var onChange: ((_ old: Deck) -> Void)?
    /// Asked for the current state before a change is recorded.
    var currentState: (() -> EditState)?
    /// Restores slide/selection after undo or redo.
    var restoreState: ((EditState) -> Void)?
    private var silentStart: (Deck, EditState)?

    static var testMode: Bool { ProcessInfo.processInfo.environment["K42_SNAPSHOT"] != nil || ProcessInfo.processInfo.environment["K42_TEST"] != nil }

    override init() {
        super.init()
        hasUndoManager = true
    }

    override class var autosavesInPlace: Bool { false }

    // Debug runs never ask to save (they quit on their own).
    var hasUnsavedChanges: Bool {
        super.isDocumentEdited || controller?.canvas.editor.hasPendingChanges == true || controller?.outline.hasPendingChanges == true
    }
    override var isDocumentEdited: Bool { Self.testMode ? false : hasUnsavedChanges }

    var controller: DeckWindowController? { windowControllers.first as? DeckWindowController }

    override func makeWindowControllers() {
        addWindowController(DeckWindowController())
    }

    /// Sets the deck of a new document (before its window opens).
    func replaceDeck(_ d: Deck) { deck = d }

    // MARK: - Changes

    /// Records one undoable change of the deck.
    func change(_ name: String, state: EditState? = nil, _ body: (inout Deck) -> Void) {
        endSilentEdit()
        let old = deck
        let before = currentState?() ?? EditState()
        var new = deck
        body(&new)
        guard new != old || new.media.count != old.media.count else { return }
        set(new, undo: old, state: before, name: name)
        if let state { restoreState?(state) }
    }

    private func set(_ new: Deck, undo old: Deck, state: EditState, name: String) {
        undoManager?.registerUndo(withTarget: self) { doc in
            let inverseState = doc.currentState?() ?? EditState()
            doc.set(old, undo: new, state: inverseState, name: name)
            doc.restoreState?(state)
        }
        undoManager?.setActionName(name)
        deck = new
        onChange?(old)
    }

    /// Typing in notes or the outline: the deck follows every keystroke, one undo step for the burst.
    func silentChange(_ body: (inout Deck) -> Void) {
        if silentStart == nil {
            silentStart = (deck, currentState?() ?? EditState())
            updateChangeCount(.changeDone)
        }
        let old = deck
        body(&deck)
        onChange?(old)
    }

    func endSilentEdit(_ name: String = L("Typing")) {
        guard let (start, state) = silentStart else { return }
        silentStart = nil
        guard start != deck else { updateChangeCount(.changeUndone); return }
        let new = deck
        undoManager?.registerUndo(withTarget: self) { doc in
            let inverseState = doc.currentState?() ?? EditState()
            doc.set(start, undo: new, state: inverseState, name: name)
            doc.restoreState?(state)
        }
        undoManager?.setActionName(name)
        // The change count was already raised while typing; the registration raised it once more.
        updateChangeCount(.changeUndone)
    }

    // MARK: - Reading and writing

    override func read(from url: URL, ofType typeName: String) throws {
        guard Self.extensionFor(typeName) == "key" || url.pathExtension.lowercased() == "key" else {
            try super.read(from: url, ofType: typeName); return
        }
        let archive = try KeynoteArchive(url: url)
        var importer = KeynoteImport(archive)
        let imported = try importer.read()
        installReadDeck(imported.deck)
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            guard !Self.testMode else { return }
            let alert = NSAlert()
            alert.messageText = L("Keynote import limitations")
            alert.informativeText = imported.warnings.joined(separator: "\n") + "\n" + L("Save this imported presentation as PPTX or ODP.")
            alert.addButton(withTitle: L("OK"))
            if let window = self.windowForSheet { alert.beginSheetModal(for: window) } else { alert.runModal() }
        }
    }

    override func read(from data: Data, ofType typeName: String) throws {
        let ext = Self.extensionFor(typeName)
        let restored = try DeckIO.read(data, ext: ext)
        installReadDeck(restored)
        // Opening a template starts a new, untitled presentation from it.
        if ext == "potx" || ext == "otp" { DispatchQueue.main.async { [weak self] in self?.fileURL = nil; self?.fileType = ext == "otp" ? "org.oasis-open.opendocument.presentation" : "org.openxmlformats.presentationml.presentation"; self?.updateChangeCount(.changeDone) } }
    }

    private func installReadDeck(_ restored: Deck) {
        let old = deck
        // Parse first: failed Revert must preserve both the model and the visible draft.
        silentStart = nil
        controller?.canvas.editor.cancel()
        controller?.outline.discardPendingEdits()
        if let controller { controller.window?.makeFirstResponder(controller.canvas) }
        silentStart = nil
        deck = restored
        undoManager?.removeAllActions()
        updateChangeCount(.changeCleared)
        onChange?(old)
        restoreState?(EditState())
        controller?.notesView.resetUndoHistory()
        controller?.notesPage.resetUndoHistory()
        controller?.outline.resetUndoHistory()
    }

    override func data(ofType typeName: String) throws -> Data {
        controller?.commitEditing()
        endSilentEdit()
        return try DeckIO.write(deck, ext: Self.extensionFor(typeName))
    }

    static func extensionFor(_ typeName: String) -> String {
        let t = typeName.lowercased()
        if t == "com.apple.iwork.keynote.key" || t == "key" || t.hasSuffix(".key") { return "key" }
        if t.contains("presentation-template") || t.hasSuffix("otp") { return "otp" }
        if t.contains("presentationml.template") || t.hasSuffix("potx") { return "potx" }
        if t.contains("opendocument") || t.hasSuffix("odp") { return "odp" }
        if t.contains("powerpoint.ppt") || t == "ppt" || t.hasSuffix(".ppt") { return "ppt" }
        return "pptx"
    }

    override class var writableTypes: [String] {
        ["org.openxmlformats.presentationml.presentation", "org.oasis-open.opendocument.presentation",
         "org.openxmlformats.presentationml.template", "org.oasis-open.opendocument.presentation-template"]
    }

    override class func isNativeType(_ type: String) -> Bool { type == "org.openxmlformats.presentationml.presentation" }

    override func writableTypes(for saveOperation: NSDocument.SaveOperationType) -> [String] { Self.writableTypes }

    override func prepareSavePanel(_ savePanel: NSSavePanel) -> Bool {
        savePanel.isExtensionHidden = false
        return true
    }

    /// Opening a legacy .ppt: explain instead of a generic error.
    override func presentedItemDidChange() {}

    // MARK: - Printing

    override func printOperation(withSettings printSettings: [NSPrintInfo.AttributeKey: Any]) throws -> NSPrintOperation {
        controller?.commitEditing()
        let info = printInfo.copy() as! NSPrintInfo
        info.topMargin = 24; info.bottomMargin = 24; info.leftMargin = 24; info.rightMargin = 24
        info.horizontalPagination = .fit
        info.verticalPagination = .fit
        let view = PrintView(deck: deck, info: info)
        let op = NSPrintOperation(view: view, printInfo: info)
        op.jobTitle = displayName
        let accessory = PrintAccessory(view: view)
        op.printPanel.addAccessoryController(accessory)
        op.printPanel.options.formUnion([.showsPaperSize, .showsOrientation, .showsScaling, .showsPreview, .showsPageRange, .showsCopies])
        return op
    }
}

/// Paginated printing of slides, handouts or notes pages.
final class PrintView: NSView {
    enum Mode: Int { case slides, handouts, notes }
    let deck: Deck
    var mode: Mode = .slides { didSet { needsDisplay = true } }
    var perPage = 6 { didSet { if perPage < 1 { perPage = 1 }; needsDisplay = true } }
    var includeHidden = false
    private let info: NSPrintInfo

    init(deck: Deck, info: NSPrintInfo) {
        self.deck = deck
        self.info = info
        super.init(frame: NSRect(origin: .zero, size: info.paperSize))
    }

    required init?(coder: NSCoder) { fatalError("not supported") }

    override var isFlipped: Bool { true }

    private var slides: [(Int, Slide)] { deck.slides.enumerated().filter { includeHidden || !$0.element.hidden }.map { ($0.offset, $0.element) } }

    private var pageSize: NSSize {
        let p = NSPrintOperation.current?.printInfo ?? info
        return NSSize(width: p.paperSize.width - p.leftMargin - p.rightMargin, height: p.paperSize.height - p.topMargin - p.bottomMargin)
    }

    private var pageCount: Int {
        switch mode {
        case .slides: return max(1, slides.count)
        case .notes: return max(1, notesPages.count)
        case .handouts: return max(1, Int(ceil(Double(slides.count) / Double(perPage))))
        }
    }

    private var notesPages: [(index: Int, slide: Slide, range: NSRange, continued: Bool)] {
        slides.flatMap { index, slide in
            Renderer.notesPageRanges(slide, deck: deck, page: pageSize).enumerated().map { (index, slide, $0.element, $0.offset > 0) }
        }
    }

    override func knowsPageRange(_ range: NSRangePointer) -> Bool {
        let s = pageSize
        setFrameSize(NSSize(width: s.width, height: s.height * CGFloat(pageCount)))
        range.pointee = NSRange(location: 1, length: pageCount)
        return true
    }

    override func rectForPage(_ page: Int) -> NSRect {
        let s = pageSize
        return NSRect(x: 0, y: CGFloat(page - 1) * s.height, width: s.width, height: s.height)
    }

    override func draw(_ dirtyRect: NSRect) {
        guard let ctx = NSGraphicsContext.current?.cgContext else { return }
        let s = pageSize
        for page in 0..<pageCount {
            let rect = CGRect(x: 0, y: CGFloat(page) * s.height, width: s.width, height: s.height)
            guard rect.intersects(dirtyRect) else { continue }
            ctx.saveGState()
            ctx.translateBy(x: rect.minX, y: rect.minY)
            switch mode {
            case .slides:
                if slides.indices.contains(page) {
                    let (i, slide) = slides[page]
                    var w = s.width, h = w * deck.size.height / deck.size.width
                    if h > s.height { h = s.height; w = h * deck.size.width / deck.size.height }
                    Renderer.drawSlide(slide, index: i, deck: deck, in: CGRect(x: (s.width - w) / 2, y: (s.height - h) / 2, width: w, height: h), ctx: ctx)
                }
            case .notes:
                if notesPages.indices.contains(page) {
                    let item = notesPages[page]
                    Renderer.drawNotesPage(item.slide, index: item.index, deck: deck, page: s, ctx: ctx, textRange: item.range, continued: item.continued)
                }
            case .handouts:
                let chunk = Array(slides.dropFirst(page * perPage).prefix(perPage)).map { (offset: $0.0, element: $0.1) }
                Renderer.drawHandout(chunk, perPage: perPage, deck: deck, page: s, ctx: ctx)
            }
            ctx.restoreGState()
        }
    }
}

/// Print panel options: what to print.
final class PrintAccessory: NSViewController, NSPrintPanelAccessorizing {
    private let printView: PrintView
    @objc dynamic var mode = 0 { didSet { printView.mode = PrintView.Mode(rawValue: mode) ?? .slides } }
    @objc dynamic var perPage = 2 { didSet { printView.perPage = [1, 2, 3, 4, 6, 9][min(5, max(0, perPage))] } }
    @objc dynamic var hidden = false { didSet { printView.includeHidden = hidden } }

    init(view: PrintView) {
        printView = view
        super.init(nibName: nil, bundle: nil)
        title = L("Presentation")
    }

    required init?(coder: NSCoder) { fatalError("not supported") }

    override func loadView() {
        let kind = NSPopUpButton()
        kind.addItems(withTitles: [L("Slides"), L("Handouts"), L("Notes Pages")])
        kind.bind(.selectedIndex, to: self, withKeyPath: "mode")
        let per = NSPopUpButton()
        per.addItems(withTitles: ["1", "2", "3", "4", "6", "9"].map { L("%@ slides per page", $0) })
        per.bind(.selectedIndex, to: self, withKeyPath: "perPage")
        per.selectItem(at: 4)
        perPage = 4
        let hiddenBox = NSButton(checkboxWithTitle: L("Include hidden slides"), target: nil, action: nil)
        hiddenBox.bind(.value, to: self, withKeyPath: "hidden")
        let grid = NSGridView(views: [[NSTextField(labelWithString: L("Print:")), kind], [NSTextField(labelWithString: L("Handouts:")), per], [NSGridCell.emptyContentView, hiddenBox]])
        grid.rowSpacing = 8
        grid.column(at: 0).xPlacement = .trailing
        let box = NSView(frame: NSRect(x: 0, y: 0, width: 420, height: 100))
        grid.translatesAutoresizingMaskIntoConstraints = false
        box.addSubview(grid)
        NSLayoutConstraint.activate([grid.centerXAnchor.constraint(equalTo: box.centerXAnchor), grid.centerYAnchor.constraint(equalTo: box.centerYAnchor)])
        view = box
    }

    func localizedSummaryItems() -> [[NSPrintPanel.AccessorySummaryKey: String]] {
        [[.itemName: L("Print:"), .itemDescription: [L("Slides"), L("Handouts"), L("Notes Pages")][mode]]]
    }

    func keyPathsForValuesAffectingPreview() -> Set<String> { ["mode", "perPage", "hidden"] }
}
