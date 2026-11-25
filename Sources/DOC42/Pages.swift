import AppKit
import Kit42

/// Page size, margins, columns, header and footer of a document or section (points).
struct PageSetup: Equatable {
    var size = NSSize(width: 595, height: 842)
    var margins = NSEdgeInsets(top: 72, left: 72, bottom: 72, right: 72)
    var header = ""
    var footer = ""
    var pageNumbers = true
    var columns = 1
    var columnSpacing: CGFloat = 36
    /// Rich headers and footers of the document (nil: the plain `header`, `footer` and `pageNumbers`).
    var hf: DocHeaderFooter?
    /// The section's own headers and footers.
    var sectionHF: DocHeaderFooter?

    static func == (a: PageSetup, b: PageSetup) -> Bool {
        func same(_ x: DocHeaderFooter?, _ y: DocHeaderFooter?) -> Bool { x === y || (x != nil && y != nil && x!.isEqual(y!)) }
        return a.sameGeometry(b) && a.header == b.header && a.footer == b.footer && a.pageNumbers == b.pageNumbers && same(a.hf, b.hf) && same(a.sectionHF, b.sectionHF)
    }

    /// Same paper, margins and columns (what the layout depends on).
    func sameGeometry(_ b: PageSetup) -> Bool {
        size == b.size && columns == b.columns && abs(columnSpacing - b.columnSpacing) < 0.01 &&
            margins.top == b.margins.top && margins.left == b.margins.left && margins.bottom == b.margins.bottom && margins.right == b.margins.right
    }

    var textSize: NSSize { NSSize(width: size.width - margins.left - margins.right, height: size.height - margins.top - margins.bottom) }
    /// Keep at least the layout engine's minimum line width in every column.
    var minimumBodyWidth: CGFloat { CGFloat(max(1, columns)) * 36 + CGFloat(max(0, columns - 1)) * columnSpacing }
    var hasValidGeometry: Bool {
        let values = [size.width, size.height, margins.top, margins.left, margins.bottom, margins.right, columnSpacing]
        return values.allSatisfy { $0.isFinite && $0 >= 0 } && columns >= 1 && columns <= 64 &&
            textSize.width >= minimumBodyWidth && textSize.height >= 36
    }
    var columnWidth: CGFloat { max(36, (textSize.width - CGFloat(columns - 1) * columnSpacing) / CGFloat(max(1, columns))) }
    var isLandscape: Bool { size.width > size.height }

    /// The geometry part, as stored on a section break.
    var section: DocSection { DocSection(pageSize: size, margins: margins, columns: columns, columnSpacing: columnSpacing, headerFooter: sectionHF) }

    /// A section's geometry with the document's header and footer.
    func with(_ s: DocSection?) -> PageSetup {
        guard let s else { return self }
        var p = self
        p.size = s.pageSize
        p.margins = s.margins
        p.columns = s.columns
        p.columnSpacing = s.columnSpacing
        p.sectionHF = s.headerFooter
        return p
    }
}

/// Text flowing across real pages: one layout manager, one text container + text view per page column,
/// pages added and removed as the text grows (like TextEdit's "Wrap to Page"). Prints page by page.
/// Sections (form feeds carrying a DocSection) give their pages their own size, margins and columns;
/// footnotes take space at the bottom of the page that has their reference.
final class PagedView: NSView, NSLayoutManagerDelegate {
    let layoutManager = DocLayoutManager()
    /// A storage only this view uses (offscreen copies), kept alive with it.
    var ownedStorage: NSTextStorage?
    private(set) var cards: [PageCard] = []
    private let configure: (NSTextView) -> Void
    static let gap: CGFloat = 28
    /// Space on the right of the pages (comments).
    var sideWidth: CGFloat = 0 { didSet { if sideWidth != oldValue { relayout() } } }
    /// After pages were laid out or moved (comments follow their text).
    var onLayout: (() -> Void)?
    /// Double click on a footnote at the bottom of a page.
    var onNoteClick: ((DocNote) -> Void)?
    /// Mouse down on a floating picture.
    var onFloatMouseDown: ((FloatImageView, NSEvent) -> Void)?
    /// Double click on a page's header (true) or footer area.
    var onHeaderDoubleClick: ((PageCard, Bool) -> Void)?
    /// Title, author and file name for fields.
    var info: () -> DocInfo = { DocInfo() }
    /// Page colour and watermark text of every page.
    var background: NSColor? { didSet { cards.forEach { $0.needsDisplay = true } } }
    var watermark = "" { didSet { cards.forEach { $0.needsDisplay = true } } }

    /// Single pages, two side by side (book), as many as fit (multiple), or one endless page (web layout).
    enum ViewMode: Int { case single, book, multi, web }
    var viewMode: ViewMode = .single {
        didSet {
            guard viewMode != oldValue else { return }
            cards.forEach { $0.web = viewMode == .web }
            applySetup()
            relayout()
        }
    }
    /// Height of the web layout page (grows with the text).
    private var webHeight: CGFloat = 900
    private var webWidth: CGFloat { max(360, (enclosingScrollView?.contentView.bounds.width ?? 800) - Self.gap * 2 - sideWidth) }

    func fieldContext(page: Int) -> FieldContext {
        FieldContext(page: page, pages: cards.count, info: info(), storage: layoutManager.textStorage)
    }

    /// Value of a field in the body text (its page from the layout).
    func fieldValue(_ f: DocField, at index: Int) -> String {
        var page = 1
        if let storage = layoutManager.textStorage, index < storage.length, layoutManager.numberOfGlyphs > 0 {
            let g = layoutManager.glyphIndexForCharacter(at: index)
            if g < layoutManager.numberOfGlyphs, let c = layoutManager.textContainer(forGlyphAt: g, effectiveRange: nil, withoutAdditionalLayout: true),
               let i = cards.firstIndex(where: { $0.textViews.contains { $0.textContainer === c } }) {
                page = i + 1
            } else {
                page = cards.count
            }
        }
        return Fields.value(f, at: index, fieldContext(page: page))
    }

    /// Repaint changed fields; only a changed attachment width requires another layout pass.
    @discardableResult
    func refreshFields(force: Bool = false, schedule: Bool = true) -> Bool {
        guard let storage = layoutManager.textStorage else { return false }
        var changedGeometry: [NSRange] = []
        storage.enumerateAttribute(.docField, in: NSRange(location: 0, length: storage.length)) { v, r, _ in
            guard let f = v as? DocField, !f.fixed else { return }
            let value = self.fieldValue(f, at: r.location)
            let attributes = storage.attributes(at: r.location, effectiveRange: nil)
            let cell = (attributes[.attachment] as? NSTextAttachment)?.attachmentCell as? FieldCell
            let width = FieldCell.renderedWidth(value: value, attributes: attributes)
            let resized = cell?.laidOutWidth.map { abs($0 - width) > 0.01 } ?? true
            guard force || value != f.result || resized else { return }
            f.result = value
            self.layoutManager.invalidateDisplay(forCharacterRange: r)
            if resized { changedGeometry.append(r) }
        }
        for r in changedGeometry {
            let dirty = TableGrid(storage, at: r.location)?.range ?? (storage.string as NSString).paragraphRange(for: r)
            layoutManager.invalidateLayout(forCharacterRange: dirty, actualCharacterRange: nil)
        }
        if !changedGeometry.isEmpty, schedule { scheduleReconcile() }
        return !changedGeometry.isEmpty
    }

    private var fieldPasses = 0

    /// Setup of the last section (and header/footer of every page).
    var setup: PageSetup {
        didSet { if setup != oldValue { applySetup() } }
    }

    var textViews: [NSTextView] { cards.flatMap(\.textViews) }

    init(storage: NSTextStorage, setup: PageSetup, configure: @escaping (NSTextView) -> Void) {
        self.setup = setup
        self.configure = configure
        super.init(frame: NSRect(x: 0, y: 0, width: setup.size.width + Self.gap * 2, height: setup.size.height + Self.gap * 2))
        layoutManager.delegate = self
        layoutManager.allowsNonContiguousLayout = false
        layoutManager.fieldValue = { [weak self] f, i in self?.fieldValue(f, at: i) ?? f.result }
        storage.addLayoutManager(layoutManager)
        NotificationCenter.default.addObserver(self, selector: #selector(storageEdited(_:)), name: NSTextStorage.didProcessEditingNotification, object: storage)
        addPage(startingAt: 0)
    }

    required init?(coder: NSCoder) { fatalError("not supported") }

    func detach() {
        NotificationCenter.default.removeObserver(self)
        layoutManager.delegate = nil
        layoutManager.textStorage?.removeLayoutManager(layoutManager)
    }

    override var isFlipped: Bool { true }

    override func draw(_ dirtyRect: NSRect) {
        guard NSGraphicsContext.currentContextDrawingToScreen() else { return }
        NSColor.underPageBackgroundColor.setFill()
        dirtyRect.fill()
    }

    // MARK: Sections

    private var sectionCache: [(index: Int, section: DocSection)]?
    /// Row groups pushed to the next column: container → (first character of the group, room left empty).
    var keptGroups: [ObjectIdentifier: (start: Int, rect: NSRect)] = [:]

    /// Section breaks in text order.
    var sections: [(index: Int, section: DocSection)] {
        if let s = sectionCache { return s }
        var out: [(Int, DocSection)] = []
        if let storage = layoutManager.textStorage {
            storage.enumerateAttribute(.docSection, in: NSRange(location: 0, length: storage.length)) { v, r, _ in
                if let s = v as? DocSection { out.append((r.location, s)) }
            }
        }
        sectionCache = out
        return out
    }

    /// Setup of the section that contains a character (a break belongs to the section it ends).
    func setup(at index: Int) -> PageSetup {
        let s = setup.with(sections.first { $0.index >= index }?.section)
        guard viewMode == .web else { return s }
        var w = s
        w.size = NSSize(width: webWidth, height: webHeight)
        w.margins = NSEdgeInsets(top: 28, left: 36, bottom: 28, right: 36)
        w.columns = 1
        return w
    }

    @objc private func storageEdited(_ note: Notification) {
        guard let storage = note.object as? NSTextStorage, storage.editedMask.contains(.editedAttributes) || storage.editedMask.contains(.editedCharacters) else { return }
        sectionCache = nil
        keptGroups = [:]
        layoutManager.notesChanged()
        editCount += 1
        churn = 0
        scheduleReconcile()
    }

    // MARK: Pages

    // Adding a container re-enters layout (and this delegate); defer nested requests to keep page order.
    private var changingPages = false
    private var pendingAdd: Int?
    private var containerEnds: [ObjectIdentifier: Int] = [:]

    fileprivate func forgetContainer(_ container: NSTextContainer) {
        containerEnds[ObjectIdentifier(container)] = nil
    }

    /// TextKit can call its delegate while a column is inserted or removed. Keep
    /// page ownership stable until both the container and text-view lists agree.
    fileprivate func updateColumns(_ update: () -> Void) {
        let wasChanging = changingPages
        changingPages = true
        defer {
            changingPages = wasChanging
            if !wasChanging, let next = pendingAdd {
                pendingAdd = nil
                addPage(startingAt: next)
            }
        }
        update()
    }

    private func addPage(startingAt index: Int) {
        if changingPages { pendingAdd = index; return }
        changingPages = true
        defer {
            changingPages = false
            // Layout asked for another page while this one was being added.
            if let next = pendingAdd {
                pendingAdd = nil
                addPage(startingAt: next)
            }
        }
        let card = PageCard(setup: setup(at: index))
        card.characterStart = index
        card.paged = self
        card.web = viewMode == .web
        card.onNoteClick = { [weak self] in self?.onNoteClick?($0) }
        card.onHeaderDoubleClick = { [weak self] in self?.onHeaderDoubleClick?($0, $1) }
        cards.append(card)
        addSubview(card)
        card.setColumns(card.setup.columns, containerIndex: layoutManager.textContainers.count, in: self)
        relayout()
    }

    /// New text view for a column (configured like every other).
    fileprivate func makeColumn(size: NSSize, at index: Int) -> NSTextView {
        let container = NSTextContainer(size: size)
        forgetContainer(container) // ObjectIdentifier addresses can be reused after a column is removed.
        container.lineFragmentPadding = 0
        // The layout manager owns containers (text views don't retain theirs).
        layoutManager.insertTextContainer(container, at: index)
        let tv = PageTextView(frame: NSRect(origin: .zero, size: size), textContainer: container)
        tv.isVerticallyResizable = false
        tv.isHorizontallyResizable = false
        tv.textContainerInset = .zero
        configure(tv)
        return tv
    }

    private func removeLastPage() {
        guard cards.count > 1, !changingPages else { return }
        changingPages = true
        defer { changingPages = false }
        let card = cards.removeLast()
        if let responder = window?.firstResponder as? NSTextView, card.textViews.contains(where: { $0 === responder }) {
            window?.makeFirstResponder(cards.last?.textViews.first)
        }
        for _ in card.textViews {
            if let container = layoutManager.textContainers.last { forgetContainer(container) }
            layoutManager.removeTextContainer(at: layoutManager.textContainers.count - 1)
        }
        card.removeFromSuperview()
        relayout()
    }

    private func applySetup() {
        sectionCache = nil
        churn = 0
        for card in cards { card.setup.header = setup.header; card.setup.footer = setup.footer; card.setup.pageNumbers = setup.pageNumbers; card.setup.hf = setup.hf }
        layoutManager.invalidateLayout(forCharacterRange: NSRange(location: 0, length: layoutManager.textStorage?.length ?? 0), actualCharacterRange: nil)
        scheduleReconcile()
    }

    /// Index of a card's first container in the layout manager.
    private func firstContainerIndex(of cardIndex: Int) -> Int {
        cards[..<cardIndex].reduce(0) { $0 + $1.textViews.count }
    }

    func relayout() {
        let visibleWidth = enclosingScrollView?.contentView.bounds.width ?? 0
        if viewMode == .web, let c = cards.first, abs(c.setup.size.width - webWidth) > 1 { applySetup(); return }
        let pageWidth = cards.map(\.setup.size.width).max() ?? setup.size.width
        let breaks = sections
        for (i, card) in cards.enumerated() {
            card.number = i + 1
            card.total = cards.count
            card.sectionIndex = breaks.firstIndex { $0.index >= card.characterStart } ?? breaks.count
            card.firstOfSection = i == 0 || cards[i - 1].sectionIndex != card.sectionIndex
        }
        switch viewMode {
        case .single, .web:
            let width = max(visibleWidth, pageWidth + Self.gap * 2 + sideWidth)
            var y = Self.gap
            for card in cards {
                let s = card.setup.size
                card.frame = NSRect(x: floor((width - sideWidth - s.width) / 2), y: y, width: s.width, height: s.height)
                y += s.height + Self.gap
            }
            setFrameSize(NSSize(width: width, height: y))
        case .book, .multi:
            // Rows of pages: two (the first page alone on the right, like a book) or as many as fit.
            let perRow = viewMode == .book ? 2 : max(1, Int((visibleWidth - sideWidth - Self.gap) / (pageWidth + Self.gap)))
            let width = max(visibleWidth, CGFloat(perRow) * (pageWidth + Self.gap) + Self.gap + sideWidth)
            var slots: [[PageCard?]] = []
            var row: [PageCard?] = viewMode == .book ? [nil] : []
            for card in cards {
                row.append(card)
                if row.count == perRow { slots.append(row); row = [] }
            }
            if !row.isEmpty { slots.append(row) }
            let rowWidth = CGFloat(perRow) * pageWidth + CGFloat(perRow - 1) * Self.gap
            var y = Self.gap
            for r in slots {
                let h = r.compactMap { $0?.setup.size.height }.max() ?? 0
                var x = floor((width - sideWidth - rowWidth) / 2)
                for card in r {
                    if let card { card.frame = NSRect(x: x, y: y, width: card.setup.size.width, height: card.setup.size.height) }
                    x += pageWidth + Self.gap
                }
                y += h + Self.gap
            }
            setFrameSize(NSSize(width: width, height: y))
        }
        if !changingPages { onLayout?() }
    }

    /// Table rows joined by a merged cell (row span) can't be split over two pages (TextKit stretches the rows):
    /// when such a group starts in this column but doesn't end in it, the column ends above the group.
    func keepMergedRowsTogether(_ c: NSTextContainer) -> NSRect? {
        guard let storage = layoutManager.textStorage, viewMode != .web else { return nil }
        let g = layoutManager.glyphRange(for: c)
        guard g.length > 0 else { return nil }
        let chars = layoutManager.characterRange(forGlyphRange: g, actualGlyphRange: nil)
        // Already pushed: the group now starts the next column (keep the push, or it would come back and forth).
        if let kept = keptGroups[ObjectIdentifier(c)], kept.start == NSMaxRange(chars) { return kept.rect }
        var result: NSRect?
        storage.enumerateAttribute(.paragraphStyle, in: chars) { v, r, stop in
            guard let b = (v as? NSParagraphStyle)?.textBlocks.last as? NSTextTableBlock, b.rowSpan > 1,
                  let grid = TableGrid(storage, at: r.location) else { return }
            let rows = b.startingRow..<(b.startingRow + b.rowSpan)
            let group = grid.cells.filter { rows.contains($0.row) }
            guard let first = group.map(\.range.location).min(), let end = group.map({ NSMaxRange($0.range) }).max(), end > NSMaxRange(chars) else { return }
            let fg = layoutManager.glyphIndexForCharacter(at: first)
            guard NSLocationInRange(fg, g) else { return }
            let y = layoutManager.lineFragmentRect(forGlyphAt: fg, effectiveRange: nil, withoutAdditionalLayout: true).minY
            // A group taller than a page is left to split.
            guard y > 1 else { return }
            result = NSRect(x: -10, y: y, width: c.size.width + 20, height: c.size.height - y + 10)
            keptGroups[ObjectIdentifier(c)] = (first, result!)
            stop.pointee = true
        }
        if result == nil { keptGroups[ObjectIdentifier(c)] = nil }
        return result
    }

    /// Web layout: the page grows (or shrinks) with the text.
    private func fitWebHeight() -> Bool {
        guard viewMode == .web, let c = cards.first?.textViews.first?.textContainer else { return false }
        let used = layoutManager.usedRect(for: c).height
        let visible = (enclosingScrollView?.contentView.bounds.height ?? 600) - Self.gap * 2
        let wanted = max(visible, used + 56 + 300)
        guard cards.count > 1 || abs(wanted - webHeight) > 250 || used + 56 + 40 > webHeight else { return false }
        webHeight = max(wanted, webHeight * (cards.count > 1 ? 2 : 1))
        if abs(wanted - webHeight) > 250, cards.count == 1 { webHeight = wanted }
        sectionCache = nil
        return true
    }

    /// Width the view needs at least (widest page, gaps, side panel).
    var minimumWidth: CGFloat { (cards.map(\.setup.size.width).max() ?? setup.size.width) + Self.gap * 2 + sideWidth }

    override func resizeSubviews(withOldSize oldSize: NSSize) { relayout() }

    /// Lays out the whole text (adds all pages, places footnotes) — needed before printing offscreen.
    func layoutAll() {
        reconcile(full: true)
    }

    private func ensureFullLayout() {
        var count = -1
        while count != cards.count {
            count = cards.count
            // A balanced column or exclusion path can invalidate container layout
            // even when TextKit reports the full character range as laid out.
            for container in layoutManager.textContainers where container.layoutManager === layoutManager {
                layoutManager.ensureLayout(for: container)
            }
        }
    }

    func card(at characterIndex: Int) -> PageCard? {
        guard let storage = layoutManager.textStorage, storage.length > 0 else { return cards.first }
        let i = min(max(0, characterIndex), storage.length - 1)
        let g = layoutManager.glyphIndexForCharacter(at: i)
        guard let container = layoutManager.textContainer(forGlyphAt: g, effectiveRange: nil, withoutAdditionalLayout: true) else { return cards.last }
        return cards.first { $0.textViews.contains { $0.textContainer === container } }
    }

    func pageNumber(at characterIndex: Int) -> Int {
        guard let c = card(at: characterIndex), let i = cards.firstIndex(where: { $0 === c }) else { return 1 }
        return i + 1
    }

    func textView(at characterIndex: Int) -> NSTextView? {
        guard let storage = layoutManager.textStorage, storage.length > 0 else { return textViews.first }
        let g = layoutManager.glyphIndexForCharacter(at: min(max(0, characterIndex), storage.length - 1))
        guard let container = layoutManager.textContainer(forGlyphAt: g, effectiveRange: nil) else { return textViews.last }
        return textViews.first { $0.textContainer === container }
    }

    func scrollRangeToVisible(_ range: NSRange) {
        guard let tv = textView(at: range.location) else { return }
        tv.scrollRangeToVisible(range)
        tv.showFindIndicator(for: range)
    }

    /// Rectangle of a character range in this view's coordinates (first line).
    func rect(for range: NSRange) -> NSRect? {
        guard let tv = textView(at: range.location), let container = tv.textContainer else { return nil }
        let glyphs = layoutManager.glyphRange(forCharacterRange: NSRange(location: range.location, length: max(1, min(range.length, 1))), actualCharacterRange: nil)
        let r = layoutManager.boundingRect(forGlyphRange: glyphs, in: container)
        return convert(r.offsetBy(dx: tv.textContainerOrigin.x, dy: tv.textContainerOrigin.y), from: tv)
    }

    // MARK: Reconciling pages with sections, footnotes and column breaks

    private var reconcileScheduled = false
    private var reconciling = false
    /// Passes that changed something since the last edit: stops footnote ping-pong.
    private var churn = 0

    func scheduleReconcile() {
        guard !reconcileScheduled else { return }
        reconcileScheduled = true
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            self.reconcileScheduled = false
            self.reconcile(full: false)
        }
    }

    /// Gives every laid-out page its section's geometry, room for its footnotes and empty columns after a page break.
    func reconcile(full: Bool) {
        guard !reconciling, let storage = layoutManager.textStorage else { return }
        reconciling = true
        defer { reconciling = false }
        // A cleared document can have many obsolete sections with different geometries.
        // Trim them before the bounded geometry passes rather than spending a pass per page.
        if storage.length == 0 {
            while !changingPages, cards.count > 1 { removeLastPage() }
        }
        for _ in 0..<12 {
            if full { ensureFullLayout() }
            guard churn < 40 else { break }
            let laidOut = full ? storage.length : layoutManager.firstUnlaidCharacterIndex()
            var changed = false
            var containerIndex = 0
            var i = 0
            while i < cards.count {
                let card = cards[i]
                let first = containerIndex
                containerIndex += card.textViews.count
                guard let firstContainer = card.textViews.first?.textContainer else { i += 1; continue }
                let glyphs = layoutManager.glyphRange(for: firstContainer)
                let start = layoutManager.characterRange(forGlyphRange: glyphs, actualGlyphRange: nil).location
                card.characterStart = start
                if start > laidOut { break }
                // 1. Section geometry.
                let wanted = setup(at: start)
                if !card.setup.sameGeometry(wanted) {
                    card.setup = wanted
                    card.balanceKey = ""
                    card.balancedHeight = nil
                    card.setColumns(wanted.columns, containerIndex: first, in: self)
                    card.setNotes([])
                    card.noteHeight = 0
                    changed = true
                    break
                }
                card.setup.header = setup.header; card.setup.footer = setup.footer; card.setup.pageNumbers = setup.pageNumbers; card.setup.hf = setup.hf
                if card.setup.sectionHF !== wanted.sectionHF { card.setup.sectionHF = wanted.sectionHF }
                card.sectionIndex = sections.firstIndex { $0.index >= start } ?? sections.count
                // Characters on the page.
                var end = start
                for tv in card.textViews {
                    let g = layoutManager.glyphRange(for: tv.textContainer!)
                    end = max(end, NSMaxRange(layoutManager.characterRange(forGlyphRange: g, actualGlyphRange: nil)))
                }
                // 2. Footnotes of the page.
                var notes: [(Int, DocNote)] = []
                if end > start {
                    storage.enumerateAttribute(.docNote, in: NSRange(location: start, length: end - start)) { v, r, _ in
                        if let n = v as? DocNote { notes.append((self.layoutManager.noteNumber(at: r.location), n)) }
                    }
                }
                card.setNotes(notes)
                let reserve = card.measureNotes()
                // Shrinking can move a reference back and forth between pages: after a few passes only grow.
                if abs(reserve - card.noteHeight) > 0.5, reserve > card.noteHeight || churn < 6 {
                    card.noteHeight = reserve
                    card.positionColumns()
                    changed = true
                    break
                }
                // 3. A page or section break in a column leaves the page's other columns empty.
                var blockFrom = Int.max
                for (j, tv) in card.textViews.enumerated() where j < card.textViews.count - 1 {
                    let g = layoutManager.glyphRange(for: tv.textContainer!)
                    let r = layoutManager.characterRange(forGlyphRange: g, actualGlyphRange: nil)
                    if r.length > 0, (storage.string as NSString).character(at: NSMaxRange(r) - 1) == 0x0C { blockFrom = j + 1; break }
                }
                // Drop caps and floating pictures: text flows around them.
                let floats = placeFloats(card, start: start, end: end)
                for (j, tv) in card.textViews.enumerated() {
                    let blocked = j >= blockFrom
                    guard let c = tv.textContainer else { continue }
                    var wanted: [NSRect] = []
                    if blocked {
                        wanted = [NSRect(x: -10, y: -10, width: c.size.width + 20, height: c.size.height + 20)]
                    } else {
                        wanted = dropCaps(in: c).map(\.rect)
                        if let k = keepMergedRowsTogether(c) { wanted.append(k) }
                        let origin = tv.frame.origin
                        for f in floats where f.wrap != .behind && f.wrap != .inFront && f.wrap != .inline {
                            var r = f.rect.offsetBy(dx: -origin.x, dy: -origin.y)
                            if f.wrap == .topBottom { r = NSRect(x: -10, y: r.minY - 6, width: c.size.width + 20, height: r.height + 12) } else { r = r.insetBy(dx: -8, dy: -6) }
                            if r.intersects(NSRect(origin: .zero, size: c.size)) { wanted.append(r) }
                        }
                    }
                    let current = c.exclusionPaths.map(\.bounds)
                    let same = current.count == wanted.count && zip(current, wanted).allSatisfy { abs($0.minX - $1.minX) < 0.5 && abs($0.minY - $1.minY) < 0.5 && abs($0.width - $1.width) < 0.5 && abs($0.height - $1.height) < 0.5 }
                    if !same {
                        c.exclusionPaths = wanted.map { NSBezierPath(rect: $0) }
                        changed = true
                    }
                }
                if changed { break }
                // 4. The last page of a multi-column section balances its columns.
                if balanceColumns(card, start: start, end: end) { changed = true; break }
                i += 1
            }
            // Empty pages left at the end (a removal asked for while pages were being added).
            if !changed, layoutManager.firstUnlaidCharacterIndex() >= storage.length {
                while !changingPages, cards.count > 1, let c = cards.last?.textViews.first?.textContainer, layoutManager.glyphRange(for: c).length == 0 {
                    removeLastPage()
                    changed = true
                }
            }
            if !changed, fitWebHeight() { changed = true }
            if !changed { break }
            churn += 1
        }
        // The last geometry pass can resize a container. Flush that layout before
        // exposing pages to printing, export or callers inspecting their ranges.
        if full { ensureFullLayout() }
        relayout()
        if full {
            // Offscreen pages (PDF, printing) are drawn right after: settle page numbers now.
            if fieldPasses < 3, refreshFields(schedule: false) {
                fieldPasses += 1
                reconciling = false
                reconcile(full: true)
                fieldPasses = 0
            }
        } else {
            refreshFields()
        }
    }

    /// Edits so far (column balance is redone after each).
    private var editCount = 0

    /// Columns of equal height on the page that ends a multi-column section (as Word and Writer do before a
    /// continuous break or at the end): the smallest height that still keeps the rest of the section on the page.
    /// Returns whether the page changed.
    private func balanceColumns(_ card: PageCard, start: Int, end: Int) -> Bool {
        guard let storage = layoutManager.textStorage else { return false }
        let old = card.balancedHeight
        let sectionEnd = sections.first { $0.index >= start }?.index ?? storage.length
        // Last character of the section's text (not the break, not trailing paragraph marks).
        let ns = storage.string as NSString
        var lastChar = min(sectionEnd, storage.length) - 1
        while lastChar >= start, [10, 13, 0x0C, 0x2029].contains(ns.character(at: lastChar)) { lastChar -= 1 }
        guard card.textViews.count > 1, lastChar >= start, end > sectionEnd || end >= storage.length else {
            card.balanceKey = ""
            card.balancedHeight = nil
            return old != nil
        }
        let full = card.fullColumnHeight
        let key = "\(start):\(sectionEnd):\(editCount):\(full):\(card.setup.columnWidth)"
        if card.balanceKey == key { return false }
        card.balanceKey = key
        guard let last = card.textViews.last?.textContainer else { return false }
        for tv in card.textViews { tv.textContainer?.exclusionPaths = [] }
        func fits(_ h: CGFloat) -> Bool {
            card.balancedHeight = h
            layoutManager.ensureLayout(for: last)
            let g = layoutManager.glyphRange(for: last)
            guard NSMaxRange(layoutManager.characterRange(forGlyphRange: g, actualGlyphRange: nil)) > lastChar else { return false }
            // The text ends within `h` (the last column's extra room is for the break only).
            let glyph = layoutManager.glyphIndexForCharacter(at: lastChar)
            guard layoutManager.textContainer(forGlyphAt: glyph, effectiveRange: nil, withoutAdditionalLayout: true) === last else { return true }
            return layoutManager.lineFragmentUsedRect(forGlyphAt: glyph, effectiveRange: nil, withoutAdditionalLayout: true).maxY <= h + 0.5
        }
        guard fits(full) else {
            card.balancedHeight = nil
            return true
        }
        var lo: CGFloat = 8, hi = full
        while hi - lo > 1 {
            let mid = (lo + hi) / 2
            if fits(mid) { hi = mid } else { lo = mid }
        }
        let h = hi + 1 >= full ? nil : hi + 0.5
        if ProcessInfo.processInfo.environment["K42_DEBUG"] != nil { Swift.print("balance", start, sectionEnd, full, h as Any) }
        _ = fits(h ?? full)
        card.balancedHeight = h
        return true
    }

    // MARK: NSLayoutManagerDelegate

    func layoutManager(_ lm: NSLayoutManager, didCompleteLayoutFor textContainer: NSTextContainer?, atEnd layoutFinished: Bool) {
        guard let storage = lm.textStorage, storage.length > 0 else { return }
        let containers = lm.textContainers
        if !layoutFinished || textContainer == nil {
            // Text left over: add a page after the last one.
            if let last = containers.last, textContainer === last || textContainer == nil {
                // (A page that took nothing would be followed by endless empty ones.)
                // (Containers rather than cards: this runs while a card is still adding its columns.)
                let recent = containers.suffix(6)
                if let end = recent.reversed().compactMap({ containerEnds[ObjectIdentifier($0)] }).first {
                    addPage(startingAt: end)
                }
            }
        }
        // Remove unused pages in reconcile, after TextKit leaves its layout callback.
        // Removing containers here invalidates the layout being completed (notably tables).
        if !reconciling { scheduleReconcile() }
    }

    /// The empty line right after a section break takes no room at the top of the next page.
    func layoutManager(_ lm: NSLayoutManager, shouldSetLineFragmentRect lineFragmentRect: UnsafeMutablePointer<NSRect>,
                       lineFragmentUsedRect: UnsafeMutablePointer<NSRect>, baselineOffset: UnsafeMutablePointer<CGFloat>,
                       in textContainer: NSTextContainer, forGlyphRange glyphRange: NSRange) -> Bool {
        // Cache progress while TextKit supplies a laid-out line. Querying a container's
        // glyph range from its completion callback can recursively request that same layout.
        if glyphRange.length > 0 {
            containerEnds[ObjectIdentifier(textContainer)] = NSMaxRange(lm.characterRange(forGlyphRange: glyphRange, actualGlyphRange: nil))
        }
        guard let storage = lm.textStorage, glyphRange.length == 1 else { return false }
        let c = lm.characterIndexForGlyph(at: glyphRange.location)
        let ns = storage.string as NSString
        guard c > 0, c < storage.length else { return false }
        let afterBreak = ns.character(at: c) == 10 && storage.attribute(.docSection, at: c - 1, effectiveRange: nil) != nil
        // A section break alone on its line takes no room either (balanced columns end with the text).
        let loneBreak = ns.character(at: c) == 0x0C && ns.character(at: c - 1) == 10 && storage.attribute(.docSection, at: c, effectiveRange: nil) != nil
        guard afterBreak || loneBreak else { return false }
        lineFragmentRect.pointee.size.height = 0.01
        lineFragmentUsedRect.pointee.size.height = 0.01
        baselineOffset.pointee = 0
        return true
    }

    /// Final view of tracked changes: deleted text takes no room.
    var hidesDeletions = false {
        didSet {
            guard hidesDeletions != oldValue, let storage = layoutManager.textStorage, Revisions.has(storage) else { return }
            let full = NSRange(location: 0, length: storage.length)
            layoutManager.invalidateGlyphs(forCharacterRange: full, changeInLength: 0, actualCharacterRange: nil)
            layoutManager.invalidateLayout(forCharacterRange: full, actualCharacterRange: nil)
            scheduleReconcile()
        }
    }

    func layoutManager(_ lm: NSLayoutManager, shouldGenerateGlyphs glyphs: UnsafePointer<CGGlyph>, properties props: UnsafePointer<NSLayoutManager.GlyphProperty>,
                       characterIndexes charIndexes: UnsafePointer<Int>, font aFont: NSFont, forGlyphRange glyphRange: NSRange) -> Int {
        guard let storage = lm.textStorage, glyphRange.length > 0 else { return 0 }
        let count = glyphRange.length
        let first = charIndexes[0], last = charIndexes[count - 1]
        guard first < storage.length else { return 0 }
        let span = NSRange(location: first, length: min(storage.length, last + 1) - first)
        var deletions = false, effects = false
        if hidesDeletions {
            storage.enumerateAttribute(.docRevision, in: span) { v, _, stop in
                if (v as? DocRevision)?.kind == .deletion { deletions = true; stop.pointee = true }
            }
        }
        for key in [NSAttributedString.Key.docAllCaps, .docSmallCaps, .docDropCap] where !effects {
            storage.enumerateAttribute(key, in: span) { v, _, stop in if v != nil { effects = true; stop.pointee = true } }
        }
        guard deletions || effects else { return 0 }
        var out = Array(UnsafeBufferPointer(start: props, count: count))
        if deletions {
            for i in 0..<count where charIndexes[i] < storage.length {
                if (storage.attribute(.docRevision, at: charIndexes[i], effectiveRange: nil) as? DocRevision)?.kind == .deletion { out[i] = .null }
            }
        }
        let newGlyphs = effects ? Self.capitalGlyphs(storage, glyphs, &out, charIndexes, count, aFont) : nil
        out.withUnsafeBufferPointer { pp in
            if let g = newGlyphs {
                g.withUnsafeBufferPointer { lm.setGlyphs($0.baseAddress!, properties: pp.baseAddress!, characterIndexes: charIndexes, font: aFont, forGlyphRange: glyphRange) }
            } else {
                lm.setGlyphs(glyphs, properties: pp.baseAddress!, characterIndexes: charIndexes, font: aFont, forGlyphRange: glyphRange)
            }
        }
        return count
    }

    // MARK: Printing (one sheet per page card)

    private var calibratedPageContext: CGContext?

    override func beginPage(in rect: NSRect, atPlacement location: NSPoint) {
        super.beginPage(in: rect, atPlacement: location)
        guard let context = NSGraphicsContext.current?.cgContext else { return }
        context.saveGState()
        calibratedPageContext = context
        ProductionPrint.concatenate(in: context, pageRect: rect, flipped: isFlipped)
    }

    override func endPage() {
        calibratedPageContext?.restoreGState()
        calibratedPageContext = nil
        super.endPage()
    }

    override func knowsPageRange(_ range: NSRangePointer) -> Bool {
        range.pointee = NSRange(location: 1, length: cards.count)
        return true
    }

    override func rectForPage(_ page: Int) -> NSRect { cards[max(0, min(cards.count - 1, page - 1))].frame }

    /// Pages differ in size (sections with another orientation or paper).
    var hasMixedPageSizes: Bool { Set(cards.map { "\($0.setup.size)" }).count > 1 }

    /// One PDF page per card, each in its own size.
    func pdfData() -> Data {
        let data = NSMutableData()
        guard let consumer = CGDataConsumer(data: data as CFMutableData) else { return Data() }
        var box = CGRect(origin: .zero, size: cards.first?.setup.size ?? setup.size)
        guard let ctx = CGContext(consumer: consumer, mediaBox: &box, nil) else { return Data() }
        for card in cards {
            var media = CGRect(origin: .zero, size: card.bounds.size)
            let page = NSData(data: card.dataWithPDF(inside: card.bounds))
            ctx.beginPage(mediaBox: &media)
            if let provider = CGDataProvider(data: page), let doc = CGPDFDocument(provider), let p = doc.page(at: 1) {
                ctx.drawPDFPage(p)
            }
            ctx.endPage()
        }
        ctx.closePDF()
        return data as Data
    }
}

/// White sheet with column text views inside the margins, header, footer and footnotes.
final class PageCard: NSView {
    /// Updated during reconciliation; drawing and frame layout never request text layout.
    var characterStart = 0
    private(set) var textViews: [NSTextView] = []
    var setup: PageSetup { didSet { if !setup.sameGeometry(oldValue) { positionColumns() }; needsDisplay = true } }
    var number = 1 { didSet { if number != oldValue { needsDisplay = true } } }
    var total = 1 { didSet { if total != oldValue { needsDisplay = true } } }
    weak var paged: PagedView?
    /// Index of the card's section (sections are numbered by their breaks).
    var sectionIndex = 0
    var firstOfSection = true { didSet { if firstOfSection != oldValue { needsDisplay = true } } }
    /// Web layout: no shadow, header, footer or page edge.
    var web = false { didSet { layer?.shadowOpacity = web ? 0 : 0.16; needsDisplay = true } }
    /// The header or footer is being edited on the page.
    enum Editing { case header, footer }
    var editing: Editing? { didSet { needsDisplay = true } }
    var onHeaderDoubleClick: ((PageCard, Bool) -> Void)?
    /// Footnotes whose references are on this page, with their numbers.
    private(set) var notes: [(Int, DocNote)] = []
    private var notesKey = ""

    func setNotes(_ n: [(Int, DocNote)]) {
        let key = n.map { "\($0.0):\(ObjectIdentifier($0.1).hashValue):\($0.1.text.hashValue)" }.joined(separator: ",")
        notes = n
        if key != notesKey { notesKey = key; needsDisplay = true }
    }
    /// Room taken by the footnotes at the bottom of the text area.
    var noteHeight: CGFloat = 0
    var onNoteClick: ((DocNote) -> Void)?
    private var noteRects: [(NSRect, DocNote)] = []

    static let noteFont = NSFont(name: "Helvetica Neue", size: 9) ?? .systemFont(ofSize: 9)

    init(setup: PageSetup) {
        self.setup = setup
        super.init(frame: NSRect(origin: .zero, size: setup.size))
        // Paper stays white with black text in dark mode (like Pages and Word): text views, selection, caret.
        appearance = NSAppearance(named: .aqua)
        wantsLayer = true
        layer?.shadowColor = NSColor.black.cgColor
        layer?.shadowOpacity = 0.16
        layer?.shadowRadius = 6
        layer?.shadowOffset = CGSize(width: 0, height: -2)
    }

    required init?(coder: NSCoder) { fatalError("not supported") }

    override var isFlipped: Bool { true }

    /// Adds or removes column text views (and their containers, from `containerIndex` in the layout manager).
    func setColumns(_ n: Int, containerIndex: Int, in paged: PagedView) {
        paged.updateColumns {
            let lm = paged.layoutManager
            while textViews.count > n {
                let tv = textViews.removeLast()
                if let responder = window?.firstResponder as? NSTextView, responder === tv { window?.makeFirstResponder(textViews.first) }
                if let container = tv.textContainer { paged.forgetContainer(container) }
                lm.removeTextContainer(at: containerIndex + textViews.count)
                tv.removeFromSuperview()
            }
            while textViews.count < n {
                let tv = paged.makeColumn(size: NSSize(width: setup.columnWidth, height: setup.textSize.height), at: containerIndex + textViews.count)
                textViews.append(tv)
                addSubview(tv)
            }
            positionColumns()
        }
    }

    /// Column height on the last page of a multi-column section (columns balanced), nil: full height.
    var balancedHeight: CGFloat? { didSet { if balancedHeight != oldValue { positionColumns() } } }
    var balanceKey = ""

    /// Text height available on the page (footnotes taken off).
    var fullColumnHeight: CGFloat { max(36, setup.textSize.height - noteHeight) }

    func positionColumns() {
        let w = setup.columnWidth
        for (j, tv) in textViews.enumerated() {
            // Balanced: the last column has room below for the (zero-height) section break line.
            let h = min(fullColumnHeight, balancedHeight.map { $0 + (j == textViews.count - 1 ? 40 : 0) } ?? .greatestFiniteMagnitude)
            let size = NSSize(width: w, height: h)
            if let container = tv.textContainer, container.size != size {
                container.size = size
            }
            tv.frame = NSRect(x: setup.margins.left + CGFloat(j) * (w + setup.columnSpacing), y: setup.margins.top, width: w, height: h)
        }
    }

    private func noteText(_ n: Int, _ note: DocNote) -> NSAttributedString {
        let s = NSMutableAttributedString(string: "\(n)", attributes: [.font: Self.noteFont, .superscript: 1, .foregroundColor: NSColor.black])
        s.append(NSAttributedString(string: " " + (note.text.isEmpty ? " " : note.text), attributes: [.font: Self.noteFont, .foregroundColor: NSColor.black]))
        return s
    }

    /// Height the footnotes need (separator included).
    func measureNotes() -> CGFloat {
        guard !notes.isEmpty else { return 0 }
        let width = setup.textSize.width
        let h = notes.reduce(CGFloat(0)) { sum, n in
            sum + ceil(noteText(n.0, n.1).boundingRect(with: NSSize(width: width, height: 10_000), options: [.usesLineFragmentOrigin, .usesFontLeading]).height) + 2
        }
        return min(setup.textSize.height / 2, h + 14)
    }

    override func draw(_ dirtyRect: NSRect) {
        (paged?.background ?? .white).setFill()
        bounds.fill()
        if let w = paged?.watermark, !w.isEmpty, !web { drawWatermark(w) }
        if !web { drawHeadersAndFooters() }

        // Footnotes under a short rule.
        noteRects = []
        guard !notes.isEmpty, noteHeight > 0 else { return }
        let x = setup.margins.left, width = setup.textSize.width
        var y = setup.margins.top + setup.textSize.height - noteHeight + 6
        NSColor(white: 0.55, alpha: 1).setFill()
        NSRect(x: x, y: y, width: min(144, width / 3), height: 0.5).fill()
        y += 6
        for (n, note) in notes {
            let s = noteText(n, note)
            let h = ceil(s.boundingRect(with: NSSize(width: width, height: 10_000), options: [.usesLineFragmentOrigin, .usesFontLeading]).height)
            let r = NSRect(x: x, y: y, width: width, height: h)
            s.draw(with: r, options: [.usesLineFragmentOrigin, .usesFontLeading])
            noteRects.append((r, note))
            y += h + 2
        }
    }

    override func mouseDown(with event: NSEvent) {
        let p = convert(event.locationInWindow, from: nil)
        if let hit = noteRects.first(where: { $0.0.insetBy(dx: 0, dy: -1).contains(p) }) {
            onNoteClick?(hit.1)
            return
        }
        if event.clickCount == 2, !web {
            if p.y < setup.margins.top { onHeaderDoubleClick?(self, true); return }
            if p.y > bounds.height - setup.margins.bottom { onHeaderDoubleClick?(self, false); return }
        }
        super.mouseDown(with: event)
    }

    /// The header and footer of this page, fields filled in.
    private func drawHeadersAndFooters() {
        let hf = setup.effectiveHeaderFooter
        var context = paged?.fieldContext(page: number) ?? FieldContext(page: number, pages: total, info: DocInfo(), storage: nil)
        context.page = number
        context.pageStart = characterStart
        for header in [true, false] {
            if editing == (header ? .header : .footer) {
                // Word's look while editing: a dashed line between header and text, the part named.
                let r = HeaderFooter.rect(setup, header: header, height: 18)
                let y = header ? setup.margins.top - 4 : bounds.height - setup.margins.bottom + 4
                let line = NSBezierPath()
                line.move(to: NSPoint(x: 8, y: y)); line.line(to: NSPoint(x: bounds.width - 8, y: y))
                line.setLineDash([4, 3], count: 2, phase: 0)
                NSColor.systemBlue.setStroke()
                line.lineWidth = 0.8
                line.stroke()
                let part = HeaderFooter.part(hf, page: number, first: firstOfSection)
                let name = (header ? L("Header") : L("Footer")) + (part == .first ? " (" + L("first page") + ")" : part == .even ? " (" + L("even page") + ")" : "")
                (name as NSString).draw(at: NSPoint(x: 8, y: header ? y + 2 : y - 14), withAttributes: [.font: NSFont.systemFont(ofSize: 9), .foregroundColor: NSColor.systemBlue])
                _ = r
                continue
            }
            guard let text = hf.part(header: header, page: number, first: firstOfSection), text.length > 0 else { continue }
            let s = Fields.resolve(text, context)
            let h = HeaderFooter.height(s, width: setup.textSize.width)
            s.draw(with: HeaderFooter.rect(setup, header: header, height: h), options: [.usesLineFragmentOrigin, .usesFontLeading])
        }
    }

    private func drawWatermark(_ text: String) {
        guard let ctx = NSGraphicsContext.current?.cgContext else { return }
        let diagonal = hypot(bounds.width, bounds.height)
        var size: CGFloat = 120
        let font = { (s: CGFloat) in NSFont(name: "Helvetica Neue Bold", size: s) ?? .boldSystemFont(ofSize: s) }
        var str = NSAttributedString(string: text, attributes: [.font: font(size)])
        while str.size().width > diagonal * 0.75, size > 12 { size -= 6; str = NSAttributedString(string: text, attributes: [.font: font(size)]) }
        str = NSAttributedString(string: text, attributes: [.font: font(size), .foregroundColor: NSColor(white: 0.75, alpha: 0.45)])
        ctx.saveGState()
        ctx.translateBy(x: bounds.midX, y: bounds.midY)
        ctx.rotate(by: -atan2(bounds.height, bounds.width))
        let sz = str.size()
        str.draw(at: NSPoint(x: -sz.width / 2, y: -sz.height / 2))
        ctx.restoreGState()
    }
}

/// Text view of one page column: find commands go to the window's finder (all pages at once),
/// the format painter applies on mouse up.
final class PageTextView: NSTextView {
    private var editor: DocWindowController? { window?.windowController as? DocWindowController }

    override func performTextFinderAction(_ sender: Any?) {
        if let editor { editor.performFinderAction(sender) } else { super.performTextFinderAction(sender) }
    }

    override func performFindPanelAction(_ sender: Any?) {
        if let editor { editor.performFinderAction(sender) } else { super.performFindPanelAction(sender) }
    }

    override func validateUserInterfaceItem(_ item: NSValidatedUserInterfaceItem) -> Bool {
        if item.action == #selector(performTextFinderAction(_:)) || item.action == #selector(performFindPanelAction(_:)), let editor {
            return editor.validateFinderAction(item)
        }
        return super.validateUserInterfaceItem(item)
    }

    override func mouseDown(with event: NSEvent) {
        super.mouseDown(with: event)
        editor?.textViewMouseUp(self)
    }

    /// Tab at the start of a list item demotes it (Shift-Tab promotes); in a table it goes to the next cell.
    override func insertTab(_ sender: Any?) {
        if let editor, editor.changeListLevel(by: 1) { return }
        if let editor, editor.moveToCell(forward: true) { return }
        super.insertTab(sender)
    }

    override func insertBacktab(_ sender: Any?) {
        if let editor, editor.changeListLevel(by: -1) { return }
        if let editor, editor.moveToCell(forward: false) { return }
        super.insertBacktab(sender)
    }

    override func cancelOperation(_ sender: Any?) {
        if editor?.cancelFormatPainter() == true { return }
        super.cancelOperation(sender)
    }

    // Track changes applies to the user's own edits (typing, deleting, pasting, dropping), not to commands.
    static var userEditing = 0
    /// The edit deletes forward (the cursor stays after a tracked deletion).
    static var forwardDelete = false

    static func userEdit(forward: Bool = false, _ body: () -> Void) {
        userEditing += 1
        let wasForward = forwardDelete
        forwardDelete = forward
        defer { userEditing -= 1; forwardDelete = wasForward }
        body()
    }

    override func keyDown(with event: NSEvent) { Self.userEdit { super.keyDown(with: event) } }
    override func insertText(_ string: Any, replacementRange: NSRange) { Self.userEdit { super.insertText(string, replacementRange: replacementRange) } }
    override func deleteBackward(_ sender: Any?) { Self.userEdit { super.deleteBackward(sender) } }
    override func deleteForward(_ sender: Any?) { Self.userEdit(forward: true) { super.deleteForward(sender) } }
    override func deleteWordForward(_ sender: Any?) { Self.userEdit(forward: true) { super.deleteWordForward(sender) } }
    override func deleteToEndOfLine(_ sender: Any?) { Self.userEdit(forward: true) { super.deleteToEndOfLine(sender) } }
    override func deleteToEndOfParagraph(_ sender: Any?) { Self.userEdit(forward: true) { super.deleteToEndOfParagraph(sender) } }
    override func cut(_ sender: Any?) { Self.userEdit { super.cut(sender) } }
    override func paste(_ sender: Any?) { Self.userEdit { super.paste(sender) } }
    override func pasteAsPlainText(_ sender: Any?) { Self.userEdit { super.pasteAsPlainText(sender) } }
    override func pasteAsRichText(_ sender: Any?) { Self.userEdit { super.pasteAsRichText(sender) } }
    override func delete(_ sender: Any?) { Self.userEdit(forward: true) { super.delete(sender) } }
    override func performDragOperation(_ sender: NSDraggingInfo) -> Bool {
        var ok = false
        Self.userEdit { ok = super.performDragOperation(sender) }
        return ok
    }
}

/// Draws dot leaders on tab characters marked `.docTabLeader` (tables of contents) and numbers footnote references.
final class DocLayoutManager: NSLayoutManager {
    private var numbers: [Int: Int]?
    /// Value of a field at a character index (set by the pages).
    var fieldValue: ((DocField, Int) -> String)?
    /// Fields have a grey background on screen.
    var shadesFields = false

    func notesChanged() { numbers = nil }

    /// A drop cap's room in its container, its letter, and where the letter's top goes.
    func dropCap(at index: Int, lines: Int) -> (rect: NSRect, letter: NSAttributedString, top: CGFloat)? {
        guard let storage = textStorage, index < storage.length else { return nil }
        let g = glyphIndexForCharacter(at: index)
        guard g < numberOfGlyphs else { return nil }
        // The letter's own glyph is null (not shown): the line is the one of the next glyph.
        let next = g + 1 < numberOfGlyphs ? g + 1 : g
        let line = lineFragmentRect(forGlyphAt: next, effectiveRange: nil, withoutAdditionalLayout: true)
        let font = storage.attribute(.font, at: index, effectiveRange: nil) as? NSFont ?? Styles.normal.font
        let p = storage.attribute(.paragraphStyle, at: index, effectiveRange: nil) as? NSParagraphStyle
        let lineHeight = defaultLineHeight(for: font) * max(1, p?.lineHeightMultiple ?? 1)
        let capFont = PagedView.dropCapFont(font, lines: lines, lineHeight: lineHeight)
        var attrs: [NSAttributedString.Key: Any] = [.font: capFont]
        attrs[.foregroundColor] = storage.attribute(.foregroundColor, at: index, effectiveRange: nil) ?? NSColor.black
        let letter = NSAttributedString(string: (storage.string as NSString).substring(with: NSRange(location: index, length: 1)), attributes: attrs)
        let baseline = line.minY + location(forGlyphAt: next).y
        let top = baseline - font.capHeight - capFont.ascender + capFont.capHeight
        return (NSRect(x: p?.firstLineHeadIndent ?? 0, y: line.minY, width: ceil(letter.size().width) + 4, height: lineHeight * CGFloat(lines) - 2), letter, top)
    }

    /// Footnote number of the reference at a character index (1-based, in text order).
    func noteNumber(at index: Int) -> Int {
        if numbers == nil {
            var out: [Int: Int] = [:]
            if let storage = textStorage {
                var n = 0
                storage.enumerateAttribute(.docNote, in: NSRange(location: 0, length: storage.length)) { v, r, _ in
                    guard v is DocNote else { return }
                    for k in 0..<r.length { n += 1; out[r.location + k] = n }
                }
            }
            numbers = out
        }
        return numbers?[index] ?? 1
    }

    override func drawGlyphs(forGlyphRange glyphsToShow: NSRange, at origin: NSPoint) {
        super.drawGlyphs(forGlyphRange: glyphsToShow, at: origin)
        guard let storage = textStorage else { return }
        let chars = characterRange(forGlyphRange: glyphsToShow, actualGlyphRange: nil)
        storage.enumerateAttribute(.docDropCap, in: chars) { v, r, _ in
            guard let lines = v as? Int, lines > 1, let cap = dropCap(at: r.location, lines: lines) else { return }
            cap.letter.draw(at: NSPoint(x: origin.x + cap.rect.minX, y: origin.y + cap.top))
        }
        storage.enumerateAttribute(.docTabLeader, in: chars) { v, r, _ in
            guard v != nil else { return }
            for c in r.location..<NSMaxRange(r) where (storage.string as NSString).character(at: c) == 9 {
                let g = glyphIndexForCharacter(at: c)
                guard let container = textContainer(forGlyphAt: g, effectiveRange: nil) else { continue }
                let rect = boundingRect(forGlyphRange: NSRange(location: g, length: 1), in: container)
                var line = NSRange()
                let fragment = lineFragmentRect(forGlyphAt: g, effectiveRange: &line)
                let baseline = fragment.minY + location(forGlyphAt: g).y
                let color = (storage.attribute(.foregroundColor, at: c, effectiveRange: nil) as? NSColor) ?? .black
                color.setFill()
                var x = ceil((rect.minX + 3) / 4) * 4
                while x < rect.maxX - 4 {
                    NSBezierPath(ovalIn: NSRect(x: origin.x + x, y: origin.y + baseline - 1.6, width: 1.3, height: 1.3)).fill()
                    x += 4
                }
            }
        }
    }
}
