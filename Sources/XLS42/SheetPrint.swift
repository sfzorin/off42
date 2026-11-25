import AppKit
import Kit42

extension Sheet {
    /// Floating objects can be the only content on a sheet, and can extend beyond populated cells.
    /// Keep this separate from usedRange so CSV and cell-selection behavior stay cell-based.
    var printableRange: CellRange? {
        var result = usedRange
        func include(anchor: CellRef, offset: CGPoint, size: CGSize) {
            guard anchor.row >= 0, anchor.col >= 0, anchor.row <= RefPart.maxRow, anchor.col <= RefPart.maxCol,
                  size.width.isFinite, size.height.isFinite, offset.x.isFinite, offset.y.isFinite,
                  size.width > 0, size.height > 0 else { return }
            var column = anchor.col, row = anchor.row
            var remainingX = max(0, offset.x) + size.width, remainingY = max(0, offset.y) + size.height
            while column < RefPart.maxCol && remainingX > width(column) {
                remainingX -= width(column); column += 1
            }
            while row < RefPart.maxRow && remainingY > height(row) {
                remainingY -= height(row); row += 1
            }
            let first = result.map { CellRef(row: min($0.start.row, anchor.row), col: min($0.start.col, anchor.col)) } ?? anchor
            let last = CellRef(row: max(result?.end.row ?? row, row), col: max(result?.end.col ?? column, column))
            result = CellRange(first, last)
        }
        for image in images { include(anchor: image.anchor, offset: image.offset, size: image.size) }
        for chart in charts { include(anchor: chart.anchor, offset: .zero, size: chart.size) }
        return result
    }
}

/// Print layout for sheets: each page shows a block of cells, the repeated title rows
/// (frozen rows) and a footer with the sheet name and page number.
final class SheetPrintView: NSView {
    struct Page {
        let grid: GridView
        let sheetName: String
        let cols: (CGFloat, CGFloat)     // grid x range
        let rows: (CGFloat, CGFloat)     // grid y range of the body
        let titleHeight: CGFloat
    }

    let workbook: Workbook
    let activeSheet: Int
    let selection: CellRange?
    let options: SheetExport.Options
    private var pages: [Page] = []
    private var slot = NSSize(width: 500, height: 700)
    private let footerHeight: CGFloat = 18

    init(workbook: Workbook, activeSheet: Int, selection: CellRange?, options: SheetExport.Options) {
        self.workbook = workbook
        self.activeSheet = activeSheet
        self.selection = selection
        self.options = options
        super.init(frame: NSRect(x: 0, y: 0, width: 500, height: 700))
    }

    required init?(coder: NSCoder) { fatalError("not supported") }

    override var isFlipped: Bool { true }

    private func ranges() -> [(Int, CellRange)] {
        switch options.scope {
        case .selection:
            guard let selection, workbook.sheets.indices.contains(activeSheet) else { return [] }
            return [(activeSheet, selection)]
        case .workbook:
            return workbook.sheets.indices.filter { !workbook.sheets[$0].hidden }.compactMap { i in (workbook.sheets[i].printArea ?? workbook.sheets[i].printableRange).map { (i, $0) } }
        case .sheet:
            guard workbook.sheets.indices.contains(activeSheet) else { return [] }
            let s = workbook.sheets[activeSheet]
            return (s.printArea ?? s.printableRange).map { [(activeSheet, $0)] } ?? []
        }
    }

    /// Lays out pages for the paper and margins in `info` (orientation may be chosen here).
    func prepare(_ info: NSPrintInfo) {
        if options.orientation != .auto { info.orientation = options.orientation == .landscape ? .landscape : .portrait }
        else if let (i, r) = ranges().first {
            // Landscape when the table is noticeably wider than a portrait page.
            let probe = makeGrid(workbook.sheets[i], index: i, range: r, zoom: 0.75)
            let width = probe.x(r.end.col + 1) - probe.x(r.start.col)
            let portrait = min(info.paperSize.width, info.paperSize.height) - info.leftMargin - info.rightMargin
            info.orientation = width > portrait * 1.1 ? .landscape : .portrait
        }
        layout(info)
    }

    private func layout(_ info: NSPrintInfo) {
        slot = NSSize(width: info.paperSize.width - info.leftMargin - info.rightMargin,
                      height: info.paperSize.height - info.topMargin - info.bottomMargin)
        let body = slot.height - (options.pageNumbers ? footerHeight : 0)
        pages = []
        for (i, range) in ranges() {
            let sheet = workbook.sheets[i]
            var grid = makeGrid(sheet, index: i, range: range, zoom: 0.75)
            let width = grid.x(range.end.col + 1) - grid.x(range.start.col)
            if options.fitWidth, width > slot.width {
                grid = makeGrid(sheet, index: i, range: range, zoom: 0.75 * slot.width / width)
            }
            // Title rows: frozen rows above the printed block repeat on every page.
            var titleRows = options.repeatTitles ? min(sheet.frozenRows, range.start.row + (range.rows.count > sheet.frozenRows ? sheet.frozenRows : 0)) : 0
            // Oversized frozen regions must not consume the entire body on every page.
            if titleRows > 0, grid.y(titleRows) >= body * 0.8 { titleRows = 0 }
            let titleHeight = titleRows > 0 ? grid.y(titleRows) : 0
            let firstBodyRow = titleRows > 0 ? max(range.start.row, titleRows) : range.start.row

            func breaks(_ start: Int, _ end: Int, _ offset: (Int) -> CGFloat, _ limit: CGFloat) -> [(CGFloat, CGFloat)] {
                var out: [(CGFloat, CGFloat)] = []
                var from = start
                while from <= end {
                    var to = from
                    while to + 1 <= end, offset(to + 2) - offset(from) <= limit { to += 1 }
                    out.append((offset(from), offset(to + 1)))
                    from = to + 1
                }
                return out
            }
            let cols = options.fitWidth ? [(grid.x(range.start.col), grid.x(range.end.col + 1))] : breaks(range.start.col, range.end.col, grid.x, slot.width)
            let rows = firstBodyRow <= range.end.row ? breaks(firstBodyRow, range.end.row, grid.y, body - titleHeight) : [(grid.y(firstBodyRow), grid.y(firstBodyRow))]
            for c in cols {
                for r in rows {
                    pages.append(Page(grid: grid, sheetName: sheet.name, cols: c, rows: r, titleHeight: titleHeight))
                }
            }
        }
        if pages.isEmpty { pages = [] }
        setFrameSize(NSSize(width: slot.width, height: slot.height * CGFloat(max(1, pages.count))))
        needsDisplay = true
    }

    private func makeGrid(_ sheet: Sheet, index: Int, range: CellRange, zoom: CGFloat) -> GridView {
        let g = GridView(frame: .zero)
        g.sheetOverride = sheet
        g.workbookOverride = workbook
        g.sheetIndexOverride = index
        g.printMode = true
        g.printGridlines = options.gridlines
        g.printRange = range
        g.zoom = zoom
        g.rebuildLayout()
        return g
    }

    // MARK: Printing

    override func knowsPageRange(_ range: NSRangePointer) -> Bool {
        if let info = NSPrintOperation.current?.printInfo { layout(info) }
        range.pointee = NSRange(location: 1, length: max(1, pages.count))
        return true
    }

    override func rectForPage(_ page: Int) -> NSRect {
        NSRect(x: 0, y: CGFloat(page - 1) * slot.height, width: slot.width, height: slot.height)
    }

    override func draw(_ dirtyRect: NSRect) {
        NSColor.white.setFill()
        dirtyRect.fill()
        guard let ctx = NSGraphicsContext.current?.cgContext else { return }
        for (i, page) in pages.enumerated() {
            let origin = CGFloat(i) * slot.height
            let pageRect = NSRect(x: 0, y: origin, width: slot.width, height: slot.height)
            guard pageRect.intersects(dirtyRect) else { continue }
            ctx.saveGState()
            ProductionPrint.concatenate(in: ctx, pageRect: pageRect, flipped: true)
            defer { ctx.restoreGState() }
            let w = page.cols.1 - page.cols.0 + 1
            // Title rows
            if page.titleHeight > 0 {
                ctx.saveGState()
                ctx.clip(to: NSRect(x: 0, y: origin, width: w, height: page.titleHeight))
                ctx.translateBy(x: -page.cols.0, y: origin)
                page.grid.drawContent(NSRect(x: page.cols.0, y: 0, width: w, height: page.titleHeight))
                ctx.restoreGState()
            }
            // Body
            let h = page.rows.1 - page.rows.0 + 1
            ctx.saveGState()
            ctx.clip(to: NSRect(x: 0, y: origin + page.titleHeight, width: w, height: h))
            ctx.translateBy(x: -page.cols.0, y: origin + page.titleHeight - page.rows.0)
            page.grid.drawContent(NSRect(x: page.cols.0, y: page.rows.0, width: w, height: h))
            ctx.restoreGState()
            // Footer
            if options.pageNumbers {
                let attrs: [NSAttributedString.Key: Any] = [.font: NSFont(name: "Helvetica Neue", size: 8) ?? .systemFont(ofSize: 8),
                                                            .foregroundColor: NSColor(white: 0.35, alpha: 1)]
                let y = origin + slot.height - 11
                (page.sheetName as NSString).draw(at: NSPoint(x: 0, y: y), withAttributes: attrs)
                let number = L("Page %d of %d", i + 1, pages.count) as NSString
                let size = number.size(withAttributes: attrs)
                number.draw(at: NSPoint(x: slot.width - size.width, y: y), withAttributes: attrs)
            }
        }
    }
}

/// Print-panel accessory for sheets: what to print, fitting, gridlines, titles, page numbers.
final class SheetPrintAccessory: NSViewController, NSPrintPanelAccessorizing {
    let options: SheetExport.Options

    init(options: SheetExport.Options) {
        self.options = options
        super.init(nibName: nil, bundle: nil)
        title = "XLS42"
    }

    required init?(coder: NSCoder) { fatalError("not supported") }

    override func loadView() {
        let scope = NSPopUpButton(frame: .zero, pullsDown: false)
        scope.addItems(withTitles: [L("Current sheet"), L("All sheets"), L("Selection")])
        scope.bind(.selectedIndex, to: options, withKeyPath: "scopeIndex")
        func check(_ title: String, _ key: String) -> NSButton {
            let b = NSButton(checkboxWithTitle: title, target: nil, action: nil)
            b.bind(.value, to: options, withKeyPath: key)
            return b
        }
        let grid = NSGridView(views: [
            [NSTextField(labelWithString: L("Print:")), scope],
            [NSGridCell.emptyContentView, check(L("Fit all columns to page width"), "fitWidth")],
            [NSGridCell.emptyContentView, check(L("Print gridlines"), "gridlines")],
            [NSGridCell.emptyContentView, check(L("Repeat frozen rows on every page"), "repeatTitles")],
            [NSGridCell.emptyContentView, check(L("Page numbers"), "pageNumbers")],
        ])
        grid.rowSpacing = 8
        grid.column(at: 0).xPlacement = .trailing
        let box = NSView()
        grid.translatesAutoresizingMaskIntoConstraints = false
        box.addSubview(grid)
        NSLayoutConstraint.activate([
            grid.leadingAnchor.constraint(equalTo: box.leadingAnchor, constant: 20),
            grid.trailingAnchor.constraint(lessThanOrEqualTo: box.trailingAnchor, constant: -20),
            grid.topAnchor.constraint(equalTo: box.topAnchor, constant: 12),
            grid.bottomAnchor.constraint(equalTo: box.bottomAnchor, constant: -12),
        ])
        view = box
    }

    func localizedSummaryItems() -> [[NSPrintPanel.AccessorySummaryKey: String]] {
        [[.itemName: L("Print:"), .itemDescription: [L("Current sheet"), L("All sheets"), L("Selection")][options.scopeIndex]]]
    }

    func keyPathsForValuesAffectingPreview() -> Set<String> {
        Set(SheetExport.Options.keys.map { "options." + $0 })
    }
}
