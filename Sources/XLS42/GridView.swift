import AppKit
import Kit42

/// The spreadsheet canvas: draws cells like Excel (fills, gridlines, overflow text, borders,
/// merged cells) and handles selection, keyboard navigation and in-cell editing.
final class GridView: NSView, NSTextFieldDelegate {
    weak var controller: SheetWindowController?

    /// User zoom (1 = 100 %). Excel's 100 % equals 4/3 of the point sizes stored in the file.
    var zoom: CGFloat = 1 { didSet { rebuildLayout(); controller?.zoomDidChange() } }
    var scale: CGFloat { zoom * 4 / 3 }

    private(set) var rowCount = 200
    private(set) var colCount = 40
    private var colOffsets: [CGFloat] = [0]
    private var rowOffsets: [CGFloat] = [0]

    var active = CellRef(row: 0, col: 0)
    var anchor = CellRef(row: 0, col: 0)
    enum SelectionAxis { case rows, columns }
    private(set) var selectionAxis: SelectionAxis?
    var selection = CellRange(CellRef(row: 0, col: 0), CellRef(row: 0, col: 0)) {
        didSet { selectionAxis = nil }
    }

    private let editor = CellEditorField()
    private(set) var isEditing = false
    private var editingCell: CellRef?
    /// While typing a formula, clicking cells inserts references at this range of the editor text.
    private var refInsertion: NSRange?
    private var refAnchor: CellRef?

    var printMode = false

    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }
    override var isOpaque: Bool { true }

    /// Sheet drawn when printing a sheet other than the one on screen.
    var sheetOverride: Sheet?
    var workbookOverride: Workbook?
    var sheetIndexOverride: Int?
    var workbook: Workbook { workbookOverride ?? controller?.doc?.workbook ?? Workbook() }
    var sheetIndex: Int { sheetIndexOverride ?? controller?.sheetIndex ?? 0 }
    var sheet: Sheet { sheetOverride ?? controller?.sheet ?? Sheet(name: "") }

    // Printing
    var printGridlines = true
    var printRange: CellRange?
    private var pageRects: [NSRect] = []

    override init(frame: NSRect) {
        super.init(frame: frame)
        editor.delegate = self
        editor.isHidden = true
        addSubview(editor)
    }

    required init?(coder: NSCoder) { fatalError("not supported") }

    // MARK: - Layout

    func rebuildLayout() {
        let used = sheet.usedRange
        rowCount = min(RefPart.maxRow + 1, max(rowCount, (used?.end.row ?? 0) + 100, selection.end.row + 50))
        colCount = min(RefPart.maxCol + 1, max(colCount, (used?.end.col ?? 0) + 20, selection.end.col + 10))
        if printMode {
            let range = printRange ?? used
            rowCount = (range?.end.row ?? 0) + 1
            colCount = (range?.end.col ?? 0) + 1
        }
        colOffsets = [0]
        colOffsets.reserveCapacity(colCount + 1)
        for c in 0..<colCount {
            let width = sheet.width(c) * scale
            colOffsets.append(colOffsets[c] + (printMode ? width : floor(width)))
        }
        rowOffsets = [0]
        rowOffsets.reserveCapacity(rowCount + 1)
        for r in 0..<rowCount {
            let height = sheet.height(r) * scale
            rowOffsets.append(rowOffsets[r] + (printMode ? height : floor(height)))
        }
        setFrameSize(NSSize(width: colOffsets[colCount] + 1, height: rowOffsets[rowCount] + 1))
        removeAllToolTips()
        if !printMode, !sheet.comments.isEmpty { addToolTip(bounds, owner: self, userData: nil) }
        needsDisplay = true
        controller?.headersNeedDisplay()
        if isEditing, let ref = editingCell { positionEditor(ref) }
    }

    /// Grows the grid when the user scrolls near its end.
    func extendIfNeeded() {
        let visible = visibleRect
        var changed = false
        if visible.maxY > frame.height - 400, rowCount <= RefPart.maxRow { rowCount = min(RefPart.maxRow + 1, rowCount + 300); changed = true }
        if visible.maxX > frame.width - 300, colCount <= RefPart.maxCol { colCount = min(RefPart.maxCol + 1, colCount + 20); changed = true }
        if changed { rebuildLayout() }
    }

    func x(_ col: Int) -> CGFloat { col < colOffsets.count ? colOffsets[col] : colOffsets.last! + CGFloat(col - colOffsets.count + 1) * sheet.defaultColumnWidth * scale }
    func y(_ row: Int) -> CGFloat { row < rowOffsets.count ? rowOffsets[row] : rowOffsets.last! + CGFloat(row - rowOffsets.count + 1) * sheet.defaultRowHeight * scale }

    func col(at px: CGFloat) -> Int { index(in: colOffsets, px) }
    func row(at py: CGFloat) -> Int { index(in: rowOffsets, py) }

    private func index(in offsets: [CGFloat], _ v: CGFloat) -> Int {
        var lo = 0, hi = offsets.count - 2
        guard hi >= 0 else { return 0 }
        if v <= 0 { return 0 }
        if v >= offsets[hi + 1] { return hi }
        while lo < hi {
            let mid = (lo + hi + 1) / 2
            if offsets[mid] <= v { lo = mid } else { hi = mid - 1 }
        }
        return lo
    }

    func rect(_ ref: CellRef) -> NSRect {
        if let m = sheet.merge(containing: ref) { return rect(m) }
        return NSRect(x: x(ref.col), y: y(ref.row), width: x(ref.col + 1) - x(ref.col), height: y(ref.row + 1) - y(ref.row))
    }

    func rect(_ range: CellRange) -> NSRect {
        NSRect(x: x(range.start.col), y: y(range.start.row),
               width: x(range.end.col + 1) - x(range.start.col), height: y(range.end.row + 1) - y(range.start.row))
    }

    func cell(at point: NSPoint) -> CellRef { CellRef(row: row(at: point.y), col: col(at: point.x)) }

    // MARK: - Drawing

    /// Colours for the current appearance (always light when printing).
    var palette: SheetPalette { SheetPalette.for(self, print: printMode) }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        needsDisplay = true
    }

    override var needsDisplay: Bool {
        didSet { if needsDisplay { overlay?.panesNeedDisplay() } }
    }

    override func draw(_ dirty: NSRect) { drawContent(dirty) }

    /// Draws the cells in `dirty` (grid coordinates). Also used by the frozen panes.
    func drawContent(_ dirty: NSRect) {
        let palette = self.palette
        palette.background.setFill()
        dirty.fill()
        guard colOffsets.count > 1, rowOffsets.count > 1 else { return }
        let r0 = row(at: dirty.minY), r1 = min(rowCount - 1, row(at: dirty.maxY))
        let c0 = col(at: dirty.minX), c1 = min(colCount - 1, col(at: dirty.maxX))
        let sheet = self.sheet

        // Gridlines
        if printMode ? printGridlines : sheet.showGridlines {
            palette.grid.setFill()
            for c in c0...c1 + 1 { NSRect(x: x(c) - 0.5, y: dirty.minY, width: 1, height: dirty.height).fill() }
            for r in r0...r1 + 1 { NSRect(x: dirty.minX, y: y(r) - 0.5, width: dirty.width, height: 1).fill() }
        }

        let merges = sheet.merges.filter { rect($0).intersects(dirty) }
        let merged = Set(merges.flatMap { $0.count < 5000 ? $0.cells : [] })
        let rules = sheet.conditionalFormats.isEmpty ? nil : ConditionalEngine(sheet: sheet, workbook: workbook, sheetIndex: sheetIndex)
        func display(_ ref: CellRef) -> Cell? {
            guard var cell = sheet.cells[ref] else { return nil }
            rules?.apply(&cell, at: ref)
            return cell
        }

        // Fills (cover the gridlines under them, like Excel)
        for r in r0...r1 where !sheet.isRowHidden(r) {
            for c in c0...c1 {
                let ref = CellRef(row: r, col: c)
                guard let fill = display(ref)?.style.fill, !merged.contains(ref) else { continue }
                fill.setFill()
                rect(ref).insetBy(dx: -0.5, dy: -0.5).fill()
            }
        }
        // Data bars
        if let rules {
            for r in r0...r1 where !sheet.isRowHidden(r) {
                for c in c0...c1 {
                    let ref = CellRef(row: r, col: c)
                    guard let (fraction, color) = rules.bar(at: ref) else { continue }
                    let cr = rect(ref).insetBy(dx: 2, dy: 2)
                    let bar = NSRect(x: cr.minX, y: cr.minY, width: cr.width * fraction, height: cr.height)
                    NSGradient(starting: color.withAlphaComponent(0.85), ending: color.withAlphaComponent(0.25))?.draw(in: bar, angle: 0)
                }
            }
        }
        for m in merges {
            (sheet.cells[m.start]?.style.fill ?? palette.background).setFill()
            rect(m).insetBy(dx: 0.5, dy: 0.5).fill()
        }

        // Text (with overflow into empty neighbours)
        let scanFrom = max(0, c0 - 30)
        for r in r0...r1 where !sheet.isRowHidden(r) {
            for c in scanFrom...c1 {
                let ref = CellRef(row: r, col: c)
                guard let cell = display(ref), !cell.value.isEmpty || cell.isFormula else { continue }
                if merged.contains(ref) && !merges.contains(where: { $0.start == ref }) { continue }
                if isEditing, editingCell == ref { continue }
                drawText(cell, at: ref, row: r, visibleMaxCol: c1)
            }
        }
        // Merged cells whose top-left lies outside the dirty rows/cols
        for m in merges where !(r0...r1).contains(m.start.row) || m.start.col < scanFrom {
            if let cell = sheet.cells[m.start] { drawText(cell, at: m.start, row: m.start.row, visibleMaxCol: c1) }
        }

        // Borders
        for r in max(0, r0 - 1)...r1 {
            for c in max(0, c0 - 1)...c1 {
                guard let style = sheet.cells[CellRef(row: r, col: c)]?.style,
                      style.left != nil || style.right != nil || style.top != nil || style.bottom != nil else { continue }
                drawBorders(style, rect(CellRef(row: r, col: c)), palette)
            }
        }

        // Comment markers, filter buttons, floating objects
        if !printMode {
            NSColor.systemRed.setFill()
            for (ref, _) in sheet.comments where rect(ref).intersects(dirty) {
                let r = rect(ref)
                let tri = NSBezierPath()
                tri.move(to: NSPoint(x: r.maxX - 6, y: r.minY))
                tri.line(to: NSPoint(x: r.maxX, y: r.minY))
                tri.line(to: NSPoint(x: r.maxX, y: r.minY + 6))
                tri.close()
                tri.fill()
            }
            if let f = sheet.autoFilter, f.start.row >= r0 - 1, f.start.row <= r1 {
                for c in f.cols { drawFilterButton(c, row: f.start.row, active: sheet.filterValues[c] != nil) }
            }
            if hasListDropdown(active), rect(active).intersects(dirty) || listButtonRect(active).intersects(dirty) { drawListButton(active) }
        }
        drawObjects(dirty)
        if !printMode { drawSelection() }
        if let target = fillTarget {
            let path = NSBezierPath(rect: rect(target).insetBy(dx: 0.5, dy: 0.5))
            path.setLineDash([4, 3], count: 2, phase: 0)
            path.lineWidth = 1.5
            NSColor.controlAccentColor.setStroke()
            path.stroke()
        }
    }

    // MARK: - Filter buttons, objects

    func filterButtonRect(_ col: Int, row: Int) -> NSRect {
        let r = rect(CellRef(row: row, col: col))
        let size = min(r.height - 2, 16)
        return NSRect(x: r.maxX - size - 2, y: r.maxY - size - 1, width: size, height: size)
    }

    private func drawFilterButton(_ col: Int, row: Int, active: Bool) {
        let b = filterButtonRect(col, row: row)
        let path = NSBezierPath(roundedRect: b, xRadius: 3, yRadius: 3)
        let palette = self.palette
        (active ? NSColor.controlAccentColor : palette.buttonFill).setFill()
        path.fill()
        palette.buttonStroke.setStroke()
        path.lineWidth = 0.5
        path.stroke()
        let symbol = active ? "line.3.horizontal.decrease" : "chevron.down"
        if let img = NSImage(systemSymbolName: symbol, accessibilityDescription: nil)?.withSymbolConfiguration(.init(pointSize: b.height * 0.5, weight: .bold)) {
            let tinted = NSImage(size: img.size, flipped: false) { r in
                img.draw(in: r)
                (active ? NSColor.white : palette.buttonGlyph).set()
                r.fill(using: .sourceAtop)
                return true
            }
            tinted.draw(in: NSRect(x: b.midX - img.size.width / 2, y: b.midY - img.size.height / 2, width: img.size.width, height: img.size.height))
        }
    }

    /// Validation list arrow, just right of the active cell (as in Excel).
    func listButtonRect(_ ref: CellRef) -> NSRect {
        let r = rect(ref)
        let size = min(max(r.height, 14), 18)
        return NSRect(x: r.maxX + 1, y: r.maxY - size, width: size, height: size)
    }

    func hasListDropdown(_ ref: CellRef) -> Bool {
        guard !printMode, let v = sheet.validation(at: ref) else { return false }
        return v.kind == .list && v.showDropdown
    }

    private func drawListButton(_ ref: CellRef) {
        let b = listButtonRect(ref)
        let palette = self.palette
        palette.buttonFill.setFill()
        b.fill()
        palette.buttonStroke.setStroke()
        NSBezierPath(rect: b.insetBy(dx: 0.5, dy: 0.5)).stroke()
        let tri = NSBezierPath()
        tri.move(to: NSPoint(x: b.midX - 3.5, y: b.midY - 1.5))
        tri.line(to: NSPoint(x: b.midX + 3.5, y: b.midY - 1.5))
        tri.line(to: NSPoint(x: b.midX, y: b.midY + 2.5))
        tri.close()
        palette.buttonGlyph.setFill()
        tri.fill()
    }

    enum FloatingObject: Equatable { case image(Int), chart(Int) }
    var selectedObject: FloatingObject? { didSet { needsDisplay = true } }

    func objectRect(_ o: FloatingObject) -> NSRect {
        switch o {
        case .image(let i):
            let img = sheet.images[i]
            let a = rect(img.anchor).origin
            return NSRect(x: a.x + img.offset.x * scale, y: a.y + img.offset.y * scale, width: img.size.width * scale, height: img.size.height * scale)
        case .chart(let i):
            let ch = sheet.charts[i]
            let a = rect(ch.anchor).origin
            return NSRect(x: a.x, y: a.y, width: ch.size.width * scale, height: ch.size.height * scale)
        }
    }

    var objects: [FloatingObject] { sheet.images.indices.map { .image($0) } + sheet.charts.indices.map { .chart($0) } }

    private func drawObjects(_ dirty: NSRect) {
        for o in objects {
            let r = objectRect(o)
            guard r.intersects(dirty) else { continue }
            switch o {
            case .image(let i):
                NSImage(data: sheet.images[i].data)?.draw(in: r, from: .zero, operation: .sourceOver, fraction: 1, respectFlipped: true, hints: nil)
            case .chart(let i):
                ChartRenderer.draw(sheet.charts[i], in: r, workbook: workbook, sheet: sheetIndex, scale: scale * 0.75, palette: palette)
            }
            if o == selectedObject, !printMode {
                NSColor.controlAccentColor.setStroke()
                let border = NSBezierPath(rect: r.insetBy(dx: -1, dy: -1))
                border.lineWidth = 1.5
                border.stroke()
                NSColor.controlAccentColor.setFill()
                NSRect(x: r.maxX - 4, y: r.maxY - 4, width: 8, height: 8).fill()
            }
        }
    }

    private func displayAlignment(_ cell: Cell) -> NSTextAlignment {
        switch cell.style.hAlign {
        case "center", "centerContinuous": return .center
        case "right": return .right
        case "left": return .left
        case "justify", "distributed": return .justified
        default:
            switch cell.value {
            case .number: return .right
            case .bool, .error: return .center
            default: return .left
            }
        }
    }

    private func drawText(_ cell: Cell, at ref: CellRef, row r: Int, visibleMaxCol: Int) {
        let formatted = NumberFormat.format(cell.value, cell.style.numberFormat, date1904: workbook.date1904)
        var text = formatted.text
        guard !text.isEmpty else { return }
        var font = cell.style.font(scale: scale)
        let isLink = sheet.hyperlinks[ref] != nil || (cell.isFormula && cell.input.uppercased().contains("HYPERLINK("))
        var color = palette.textColor(formatted.color ?? cell.style.textColor, fill: cell.style.fill)
        if isLink, cell.style.textColor == nil, formatted.color == nil { color = printMode ? .systemBlue : .linkColor }
        let align = displayAlignment(cell)
        var attrs: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: color]
        if cell.style.underline || (isLink && cell.style.textColor == nil) { attrs[.underlineStyle] = NSUnderlineStyle.single.rawValue }
        if cell.style.doubleUnderline { attrs[.underlineStyle] = NSUnderlineStyle.double.rawValue }
        if cell.style.strike { attrs[.strikethroughStyle] = NSUnderlineStyle.single.rawValue }

        let cellRect = rect(ref)
        let pad = 2 * scale + CGFloat(cell.style.indent) * 9 * scale
        var clip = cellRect

        let isNumber: Bool = { if case .number = cell.value { return true }; return false }()
        var size = (text as NSString).size(withAttributes: attrs)
        // Shrink to fit: a smaller font instead of overflowing.
        if cell.style.shrink, !cell.style.wrap, size.width > cellRect.width - pad * 2, size.width > 0 {
            font = NSFont(descriptor: font.fontDescriptor, size: max(4, font.pointSize * (cellRect.width - pad * 2) / size.width)) ?? font
            attrs[.font] = font
            size = (text as NSString).size(withAttributes: attrs)
        }
        // Rotated or stacked text, centred in the cell.
        if cell.style.rotation != 0, let ctx = NSGraphicsContext.current?.cgContext {
            NSGraphicsContext.saveGraphicsState()
            NSBezierPath(rect: cellRect).addClip()
            if cell.style.rotation == 255 {
                let stacked = text.map(String.init).joined(separator: "\n")
                let para = NSMutableParagraphStyle()
                para.alignment = .center
                attrs[.paragraphStyle] = para
                let b = (stacked as NSString).boundingRect(with: NSSize(width: cellRect.width, height: .greatestFiniteMagnitude), options: [.usesLineFragmentOrigin], attributes: attrs)
                let top = cell.style.vAlign == "top" ? cellRect.minY : cell.style.vAlign == "center" ? cellRect.midY - b.height / 2 : cellRect.maxY - b.height
                (stacked as NSString).draw(with: NSRect(x: cellRect.minX, y: top, width: cellRect.width, height: b.height), options: [.usesLineFragmentOrigin], attributes: attrs)
            } else {
                let angle = CGFloat(cell.style.rotation) * .pi / 180
                ctx.translateBy(x: cellRect.midX, y: cellRect.midY)
                ctx.rotate(by: -angle)
                (text as NSString).draw(at: NSPoint(x: -size.width / 2, y: -size.height / 2), withAttributes: attrs)
            }
            NSGraphicsContext.restoreGraphicsState()
            return
        }

        if cell.style.wrap {
            let para = NSMutableParagraphStyle()
            para.alignment = align
            para.lineBreakMode = .byWordWrapping
            attrs[.paragraphStyle] = para
            let box = cellRect.insetBy(dx: pad, dy: 1)
            let bounds = (text as NSString).boundingRect(with: NSSize(width: box.width, height: .greatestFiniteMagnitude),
                                                         options: [.usesLineFragmentOrigin], attributes: attrs)
            var drawRect = box
            switch cell.style.vAlign {
            case "top": drawRect.size.height = bounds.height
            case "center": drawRect.origin.y += max(0, (box.height - bounds.height) / 2)
            default: drawRect.origin.y += max(0, box.height - bounds.height)
            }
            drawRect.size.height = min(box.maxY - drawRect.minY, bounds.height)
            NSGraphicsContext.saveGraphicsState()
            NSBezierPath(rect: cellRect).addClip()
            (text as NSString).draw(with: drawRect, options: [.usesLineFragmentOrigin, .truncatesLastVisibleLine], attributes: attrs)
            NSGraphicsContext.restoreGraphicsState()
            return
        }

        var squeeze: CGFloat = 1
        if isNumber, size.width > cellRect.width - 2, size.width <= (cellRect.width - 2) * 1.12, sheet.merge(containing: ref) == nil {
            // Slightly too wide only because Mac fonts aren't pixel-hinted like Excel's: compress a little.
            squeeze = (cellRect.width - 2) / size.width
            size.width = cellRect.width - 2
        } else if isNumber, size.width > cellRect.width - 2, sheet.merge(containing: ref) == nil {
            // Numbers never overflow: Excel shows ###.
            let hashWidth = ("#" as NSString).size(withAttributes: attrs).width
            text = String(repeating: "#", count: max(1, Int((cellRect.width - 2) / max(1, hashWidth))))
            size = (text as NSString).size(withAttributes: attrs)
        } else if !isNumber, size.width > cellRect.width - pad * 2, sheet.merge(containing: ref) == nil {
            // Text overflows into empty neighbours.
            if align == .left || align == .justified {
                var c = ref.col + 1
                while clip.width < size.width + pad * 2, c <= visibleMaxCol + 30, sheet.cells[CellRef(row: r, col: c)]?.value.isEmpty ?? true {
                    clip.size.width = x(c + 1) - clip.minX
                    c += 1
                }
            } else if align == .right {
                var c = ref.col - 1
                while clip.width < size.width + pad * 2, c >= 0, sheet.cells[CellRef(row: r, col: c)]?.value.isEmpty ?? true {
                    clip.size.width = clip.maxX - x(c)
                    clip.origin.x = x(c)
                    c -= 1
                }
            } else if align == .center {
                let extra = (size.width + pad * 2 - cellRect.width) / 2
                clip = cellRect.insetBy(dx: -extra, dy: 0)
            }
        }

        var origin = NSPoint(x: cellRect.minX + pad, y: cellRect.minY)
        switch align {
        case .right: origin.x = cellRect.maxX - pad - size.width
        case .center: origin.x = cellRect.midX - size.width / 2
        default: break
        }
        switch cell.style.vAlign {
        case "top": origin.y = cellRect.minY + 1
        case "center": origin.y = cellRect.midY - size.height / 2
        default: origin.y = cellRect.maxY - size.height - 1 * scale
        }
        NSGraphicsContext.saveGraphicsState()
        NSBezierPath(rect: clip.insetBy(dx: 0.5, dy: 0)).addClip()
        if squeeze < 1, let ctx = NSGraphicsContext.current?.cgContext {
            ctx.translateBy(x: origin.x, y: 0)
            ctx.scaleBy(x: squeeze, y: 1)
            (text as NSString).draw(at: NSPoint(x: 0, y: origin.y), withAttributes: attrs)
        } else {
            (text as NSString).draw(at: origin, withAttributes: attrs)
        }
        NSGraphicsContext.restoreGraphicsState()
    }

    private func drawBorders(_ style: CellStyle, _ r: NSRect, _ palette: SheetPalette) {
        func line(_ side: BorderSide?, _ from: NSPoint, _ to: NSPoint) {
            guard let side else { return }
            let horizontal = from.y == to.y
            let width = side.width * zoom
            let offsets: [CGFloat] = side.style == "double" ? [-width / 3, width / 3] : [0]
            palette.borderColor(side.color).setStroke()
            for offset in offsets {
                let path = NSBezierPath()
                let shift = horizontal ? NSPoint(x: 0, y: offset) : NSPoint(x: offset, y: 0)
                path.move(to: NSPoint(x: from.x + shift.x, y: from.y + shift.y))
                path.line(to: NSPoint(x: to.x + shift.x, y: to.y + shift.y))
                path.lineWidth = side.style == "double" ? width / 3 : width
                let kind = side.style.lowercased()
                if kind.contains("dashdotdot") { path.setLineDash([4 * zoom, 2 * zoom, zoom, 2 * zoom, zoom, 2 * zoom], count: 6, phase: 0) }
                else if kind.contains("dashdot") { path.setLineDash([4 * zoom, 2 * zoom, zoom, 2 * zoom], count: 4, phase: 0) }
                else if kind.contains("dash") { path.setLineDash([3 * zoom, 2 * zoom], count: 2, phase: 0) }
                else if kind == "dotted" { path.setLineDash([zoom, zoom], count: 2, phase: 0) }
                path.stroke()
            }
        }
        line(style.left, NSPoint(x: r.minX, y: r.minY), NSPoint(x: r.minX, y: r.maxY))
        line(style.right, NSPoint(x: r.maxX, y: r.minY), NSPoint(x: r.maxX, y: r.maxY))
        line(style.top, NSPoint(x: r.minX, y: r.minY), NSPoint(x: r.maxX, y: r.minY))
        line(style.bottom, NSPoint(x: r.minX, y: r.maxY), NSPoint(x: r.maxX, y: r.maxY))
    }

    private func drawSelection() {
        let accent = NSColor.controlAccentColor
        let selRect = rect(expandedSelection)
        if selection.count > 1 {
            accent.withAlphaComponent(0.10).setFill()
            selRect.fill()
            palette.background.setFill()
            rect(active).insetBy(dx: 1, dy: 1).fill()
            if let cell = sheet.cells[active] {
                cell.style.fill?.setFill()
                if cell.style.fill != nil { rect(active).insetBy(dx: 1, dy: 1).fill() }
                drawText(cell, at: active, row: active.row, visibleMaxCol: active.col + 20)
            }
        }
        accent.setStroke()
        let border = NSBezierPath(rect: selRect.insetBy(dx: 0.5, dy: 0.5))
        border.lineWidth = 2
        border.stroke()
        // Fill handle
        accent.setFill()
        NSRect(x: selRect.maxX - 3, y: selRect.maxY - 3, width: 6, height: 6).fill()
        palette.background.setStroke()
        let ring = NSBezierPath(rect: NSRect(x: selRect.maxX - 3.5, y: selRect.maxY - 3.5, width: 7, height: 7))
        ring.lineWidth = 1
        ring.stroke()
    }

    /// Selection grown to whole merged areas it touches.
    var expandedSelection: CellRange {
        var s = selection
        for m in sheet.merges where rect(m).intersects(rect(s)) {
            s = CellRange(CellRef(row: min(s.start.row, m.start.row), col: min(s.start.col, m.start.col)),
                          CellRef(row: max(s.end.row, m.end.row), col: max(s.end.col, m.end.col)))
        }
        return s
    }

    // MARK: - Printing

    override func knowsPageRange(_ range: NSRangePointer) -> Bool {
        guard printMode else { return false }
        let info = NSPrintOperation.current?.printInfo ?? NSPrintInfo.shared
        let scaleFactor = max(0.1, info.scalingFactor)
        let page = NSSize(width: (info.paperSize.width - info.leftMargin - info.rightMargin) / scaleFactor,
                          height: (info.paperSize.height - info.topMargin - info.bottomMargin) / scaleFactor)
        pageRects = pages(for: page)
        range.pointee = NSRange(location: 1, length: max(1, pageRects.count))
        return true
    }

    override func rectForPage(_ page: Int) -> NSRect {
        guard printMode, page >= 1, page <= pageRects.count else { return bounds }
        return pageRects[page - 1]
    }

    /// Page rectangles broken at row and column boundaries, down then across (like Excel).
    func pages(for page: NSSize) -> [NSRect] {
        let range = printRange ?? sheet.usedRange ?? CellRange(CellRef(row: 0, col: 0), CellRef(row: 0, col: 0))
        func breaks(_ start: Int, _ end: Int, _ offset: (Int) -> CGFloat, _ limit: CGFloat, manual: Set<Int> = []) -> [(CGFloat, CGFloat)] {
            var out: [(CGFloat, CGFloat)] = []
            var from = start
            while from <= end {
                var to = from
                // Manual page breaks (Subtotals › page break between groups) start a new page.
                while to + 1 <= end, offset(to + 2) - offset(from) <= limit, !manual.contains(to + 1) { to += 1 }
                out.append((offset(from), offset(to + 1)))
                from = to + 1
            }
            return out
        }
        let cols = breaks(range.start.col, range.end.col, x, page.width)
        let rows = breaks(range.start.row, range.end.row, y, page.height, manual: sheet.rowBreaks)
        var rects: [NSRect] = []
        for c in cols {
            for r in rows { rects.append(NSRect(x: c.0, y: r.0, width: c.1 - c.0 + 1, height: r.1 - r.0 + 1)) }
        }
        return rects
    }

    // MARK: - Frozen panes

    weak var overlay: FreezeOverlay?

    /// View › Split: position of the split lines in the visible area, and the scroll offsets of the left/top panes.
    var split: NSPoint?
    var splitScroll = NSPoint.zero

    /// Width/height of the left/top panes: frozen rows/columns, or the split position.
    var frozenWidth: CGFloat {
        if let split, !printMode { return split.x }
        return printMode || sheet.frozenCols == 0 ? 0 : x(sheet.frozenCols)
    }
    var frozenHeight: CGFloat {
        if let split, !printMode { return split.y }
        return printMode || sheet.frozenRows == 0 ? 0 : y(sheet.frozenRows)
    }
    /// Grid offset of the left/top panes (frozen panes start at A1).
    var paneOffset: NSPoint { split != nil && !printMode ? splitScroll : .zero }
    var scrollOrigin: NSPoint { enclosingScrollView?.contentView.bounds.origin ?? .zero }
    var viewportSize: NSSize { enclosingScrollView?.contentView.bounds.size ?? bounds.size }

    /// Grid point → point in the visible area (frozen rows/columns don't scroll).
    func toViewport(_ p: NSPoint) -> NSPoint {
        let o = scrollOrigin, po = paneOffset, fw = frozenWidth, fh = frozenHeight
        if split != nil {
            // Shown in the main pane when it's there, else in the left/top pane.
            return NSPoint(x: p.x >= o.x + fw ? p.x - o.x : p.x - po.x, y: p.y >= o.y + fh ? p.y - o.y : p.y - po.y)
        }
        return NSPoint(x: p.x < fw ? p.x : p.x - o.x, y: p.y < fh ? p.y : p.y - o.y)
    }

    func fromViewport(_ v: NSPoint) -> NSPoint {
        let o = scrollOrigin, po = paneOffset
        return NSPoint(x: v.x < frozenWidth ? v.x + po.x : v.x + o.x, y: v.y < frozenHeight ? v.y + po.y : v.y + o.y)
    }

    /// Grid point under the mouse, honouring frozen panes.
    func gridPoint(_ event: NSEvent) -> NSPoint {
        guard let overlay, frozenWidth > 0 || frozenHeight > 0 else { return convert(event.locationInWindow, from: nil) }
        return fromViewport(overlay.convert(event.locationInWindow, from: nil))
    }

    /// Scrolls so the cell is visible below/right of the frozen panes.
    func reveal(_ ref: CellRef) {
        guard let scroll = enclosingScrollView else { return }
        let r = rect(ref).insetBy(dx: -2, dy: -2)
        let clip = scroll.contentView
        var o = clip.bounds.origin
        let size = clip.bounds.size
        // Split panes: a cell visible in the left/top pane needn't scroll the main pane.
        let po = paneOffset
        let inLeftPane = split != nil && r.minX >= po.x && r.maxX <= po.x + frozenWidth
        let inTopPane = split != nil && r.minY >= po.y && r.maxY <= po.y + frozenHeight
        if ref.col >= sheet.frozenCols, !inLeftPane {
            if r.minX - o.x < frozenWidth { o.x = r.minX - frozenWidth } else if r.maxX - o.x > size.width { o.x = r.maxX - size.width }
        }
        if ref.row >= sheet.frozenRows, !inTopPane {
            if r.minY - o.y < frozenHeight { o.y = r.minY - frozenHeight } else if r.maxY - o.y > size.height { o.y = r.maxY - size.height }
        }
        o.x = max(0, min(o.x, frame.width - size.width))
        o.y = max(0, min(o.y, frame.height - size.height))
        if o != clip.bounds.origin {
            clip.scroll(to: o)
            scroll.reflectScrolledClipView(clip)
        }
    }

    // MARK: - Selection

    func select(_ ref: CellRef, extend: Bool = false) {
        let ref = CellRef(row: min(RefPart.maxRow, max(0, ref.row)), col: min(RefPart.maxCol, max(0, ref.col)))
        if extend {
            selection = CellRange(anchor, ref)
        } else {
            active = sheet.merge(containing: ref)?.start ?? ref
            anchor = active
            selection = CellRange(active, active)
        }
        if ref.row >= rowCount - 20 || ref.col >= colCount - 5 { rebuildLayout() }
        reveal(extend ? ref : active)
        needsDisplay = true
        controller?.selectionDidChange()
    }

    func selectAllCells() {
        anchor = CellRef(row: 0, col: 0)
        selection = CellRange(anchor, CellRef(row: max(0, (sheet.usedRange?.end.row ?? 0)), col: max(0, (sheet.usedRange?.end.col ?? 0))))
        needsDisplay = true
        controller?.selectionDidChange()
    }

    func selectColumns(_ a: Int, _ b: Int) {
        anchor = CellRef(row: 0, col: a)
        active = anchor
        selection = CellRange(CellRef(row: 0, col: min(a, b)), CellRef(row: rowCount - 1, col: max(a, b)))
        selectionAxis = .columns
        needsDisplay = true
        controller?.selectionDidChange()
    }

    func selectRows(_ a: Int, _ b: Int) {
        anchor = CellRef(row: a, col: 0)
        active = anchor
        selection = CellRange(CellRef(row: min(a, b), col: 0), CellRef(row: max(a, b), col: colCount - 1))
        selectionAxis = .rows
        needsDisplay = true
        controller?.selectionDidChange()
    }

    // MARK: - Mouse

    private var dragging = false

    private enum Drag { case none, select, fill, moveObject(NSPoint, NSRect), resizeObject(NSRect) }
    private var drag: Drag = .none
    private(set) var fillTarget: CellRange?

    private var handleRect: NSRect {
        let r = rect(expandedSelection)
        return NSRect(x: r.maxX - 6, y: r.maxY - 6, width: 12, height: 12)
    }

    override func mouseDown(with event: NSEvent) {
        let point = gridPoint(event)
        let ref = cell(at: point)
        if !isEditing {
            // Floating objects (topmost first)
            if let o = objects.reversed().first(where: { objectRect($0).insetBy(dx: -4, dy: -4).contains(point) }) {
                selectedObject = o
                if event.clickCount == 2, controller?.selectedTechnicalElement != nil {
                    controller?.editTechnicalElementAction(nil)
                    return
                }
                let r = objectRect(o)
                drag = NSRect(x: r.maxX - 8, y: r.maxY - 8, width: 14, height: 14).contains(point) ? .resizeObject(r) : .moveObject(point, r)
                window?.makeFirstResponder(self)
                dragging = true
                return
            }
            selectedObject = nil
            if let f = sheet.autoFilter, ref.row == f.start.row, f.cols.contains(ref.col), filterButtonRect(ref.col, row: ref.row).contains(point) {
                controller?.showFilter(column: ref.col, from: toViewport(filterButtonRect(ref.col, row: ref.row).origin))
                return
            }
            if hasListDropdown(active), listButtonRect(active).contains(point) {
                controller?.showValidationList(at: active)
                return
            }
            if handleRect.contains(point) {
                drag = .fill
                dragging = true
                return
            }
        }
        drag = .select
        if !isEditing, !event.modifierFlags.contains(.shift), event.clickCount == 1, let target = controller?.hyperlinkTarget(at: ref),
           event.modifierFlags.contains(.command) || ref == active {
            select(ref)
            controller?.openHyperlink(target)
            return
        }
        if isEditing, editor.stringValue.hasPrefix("="), canInsertReference {
            refAnchor = ref
            insertReference(CellRange(ref, ref))
            dragging = true
            return
        }
        if isEditing, !commitEdit(move: nil) { return }
        window?.makeFirstResponder(self)
        if event.clickCount == 2 {
            select(ref)
            beginEdit(initial: nil)
            return
        }
        select(ref, extend: event.modifierFlags.contains(.shift))
        dragging = true
    }

    override func mouseDragged(with event: NSEvent) {
        guard dragging else { return }
        autoscroll(with: event)
        let point = gridPoint(event)
        switch drag {
        case .fill:
            let ref = cell(at: point)
            let s = selection
            var t = s
            let down = ref.row - s.end.row, up = s.start.row - ref.row, right = ref.col - s.end.col, left = s.start.col - ref.col
            let best = max(down, up, right, left)
            if best <= 0 { t = s }
            else if best == down { t = CellRange(s.start, CellRef(row: ref.row, col: s.end.col)) }
            else if best == up { t = CellRange(CellRef(row: ref.row, col: s.start.col), s.end) }
            else if best == right { t = CellRange(s.start, CellRef(row: s.end.row, col: ref.col)) }
            else { t = CellRange(CellRef(row: s.start.row, col: ref.col), s.end) }
            fillTarget = t == s ? nil : t
            needsDisplay = true
            return
        case .moveObject(let start, let original):
            controller?.previewObject(selectedObject, frame: original.offsetBy(dx: point.x - start.x, dy: point.y - start.y))
            return
        case .resizeObject(let original):
            var w = max(20, point.x - original.minX), h = max(20, point.y - original.minY)
            if case .image = selectedObject, original.height > 0 {
                let ratio = original.width / original.height
                if w / h > ratio { h = w / ratio } else { w = h * ratio }
            }
            controller?.previewObject(selectedObject, frame: NSRect(x: original.minX, y: original.minY, width: w, height: h))
            return
        default: break
        }
        let ref = cell(at: point)
        if isEditing, let a = refAnchor {
            insertReference(CellRange(a, ref))
            return
        }
        if CellRange(anchor, ref) != selection { select(ref, extend: true) }
    }

    override func mouseUp(with event: NSEvent) {
        dragging = false
        switch drag {
        case .fill:
            if let t = fillTarget { controller?.autoFill(from: selection, to: t) }
            fillTarget = nil
            needsDisplay = true
        case .moveObject(_, let original), .resizeObject(let original):
            controller?.commitObject(selectedObject, original: original)
        default: break
        }
        drag = .none
        if isEditing { window?.makeFirstResponder(editor); refAnchor = nil }
    }

    override func menu(for event: NSEvent) -> NSMenu? {
        let ref = cell(at: gridPoint(event))
        if isEditing, !commitEdit(move: nil) { return nil }
        if !expandedSelection.contains(ref) { select(ref) }
        window?.makeFirstResponder(self)
        return Cmd.contextMenu
    }

    // MARK: - Keyboard

    override func keyDown(with event: NSEvent) {
        let shift = event.modifierFlags.contains(.shift)
        let cmd = event.modifierFlags.contains(.command)
        guard let scalar = event.charactersIgnoringModifiers?.unicodeScalars.first else { return }
        let key = Int(scalar.value)
        let from = shift ? lastExtent : active
        switch key {
        case NSUpArrowFunctionKey: move(to: cmd ? edge(from, dr: -1, dc: 0) : CellRef(row: from.row - 1, col: from.col), extend: shift)
        case NSDownArrowFunctionKey where event.modifierFlags.contains(.option) && hasListDropdown(active):
            controller?.showValidationList(at: active)
        case NSDownArrowFunctionKey: move(to: cmd ? edge(from, dr: 1, dc: 0) : CellRef(row: from.row + 1, col: from.col), extend: shift)
        case NSLeftArrowFunctionKey: move(to: cmd ? edge(from, dr: 0, dc: -1) : CellRef(row: from.row, col: from.col - 1), extend: shift)
        case NSRightArrowFunctionKey: move(to: cmd ? edge(from, dr: 0, dc: 1) : CellRef(row: from.row, col: from.col + 1), extend: shift)
        case NSPageDownFunctionKey: move(to: CellRef(row: from.row + visibleRows, col: from.col), extend: shift)
        case NSPageUpFunctionKey: move(to: CellRef(row: from.row - visibleRows, col: from.col), extend: shift)
        case NSHomeFunctionKey: move(to: CellRef(row: cmd ? 0 : from.row, col: 0), extend: shift)
        case 13, 3: advance(rows: shift ? -1 : 1, cols: 0)
        case 9: advance(rows: 0, cols: shift ? -1 : 1)
        case 25: advance(rows: 0, cols: -1)
        case 127, NSDeleteFunctionKey:
            if let o = selectedObject { controller?.deleteObject(o); selectedObject = nil } else { controller?.clearContents(nil) }
        case NSF2FunctionKey: beginEdit(initial: nil)
        case 27: break
        default:
            if !cmd, let chars = event.characters, !chars.isEmpty, chars.unicodeScalars.allSatisfy({ !CharacterSet.controlCharacters.contains($0) && $0.value < 0xF700 }) {
                beginEdit(initial: chars)
            } else {
                super.keyDown(with: event)
            }
        }
    }

    private var lastExtent: CellRef {
        CellRef(row: selection.start.row == anchor.row ? selection.end.row : selection.start.row,
                col: selection.start.col == anchor.col ? selection.end.col : selection.start.col)
    }

    private var visibleRows: Int { max(1, Int(visibleRect.height / (sheet.defaultRowHeight * scale)) - 1) }

    private func move(to ref: CellRef, extend: Bool) { select(ref, extend: extend) }

    /// Enter/Tab movement inside a multi-cell selection wraps like Excel.
    private func advance(rows dr: Int, cols dc: Int) {
        if selection.count > 1 {
            var next = CellRef(row: active.row + dr, col: active.col + dc)
            if dr != 0, next.row > selection.end.row { next = CellRef(row: selection.start.row, col: active.col + 1) }
            if dc != 0, next.col > selection.end.col { next = CellRef(row: active.row + 1, col: selection.start.col) }
            if dr != 0, next.row < selection.start.row { next = CellRef(row: selection.end.row, col: active.col - 1) }
            if dc != 0, next.col < selection.start.col { next = CellRef(row: active.row - 1, col: selection.end.col) }
            if next.row > selection.end.row || next.col > selection.end.col { next = selection.start }
            if next.row < selection.start.row || next.col < selection.start.col { next = selection.end }
            active = next
            needsDisplay = true
            controller?.selectionDidChange()
        } else {
            select(CellRef(row: active.row + dr, col: active.col + dc))
        }
    }

    /// Ctrl/⌘+arrow: jump to the edge of the data block.
    private func edge(_ from: CellRef, dr: Int, dc: Int) -> CellRef {
        func filled(_ r: CellRef) -> Bool { !(sheet.cells[r]?.value.isEmpty ?? true) }
        var cur = from
        let limitR = max(rowCount, (sheet.usedRange?.end.row ?? 0) + 1), limitC = max(colCount, (sheet.usedRange?.end.col ?? 0) + 1)
        func step(_ r: CellRef) -> CellRef? {
            let n = CellRef(row: r.row + dr, col: r.col + dc)
            return n.row < 0 || n.col < 0 || n.row >= limitR || n.col >= limitC ? nil : n
        }
        guard var next = step(cur) else { return cur }
        if filled(cur) && filled(next) {
            while let n = step(next), filled(n) { next = n }
            return next
        }
        while !filled(next), let n = step(next) { next = n }
        cur = next
        return cur
    }

    // MARK: - Editing

    func beginEdit(initial: String?) {
        guard !isEditing else { return }
        if let controller, !controller.guardEditable(CellRange(active, active)) { return }
        isEditing = true
        editingCell = active
        let cell = sheet[active]
        editor.stringValue = initial ?? controller?.editText(at: active) ?? cell.input
        editor.font = cell.style.font(scale: scale)
        editor.alignment = initial == nil && !cell.input.hasPrefix("=") ? displayAlignment(cell) : .left
        positionEditor(active)
        editor.isHidden = false
        window?.makeFirstResponder(editor)
        if let text = editor.currentEditor() {
            text.selectedRange = NSRange(location: editor.stringValue.utf16.count, length: 0)
        }
        needsDisplay = true
        controller?.editorTextChanged(editor.stringValue)
    }

    /// Puts the in-cell editor in the overlay so it stays visible over frozen panes.
    func hostEditor(in view: NSView) {
        editor.removeFromSuperview()
        view.addSubview(editor)
    }

    func repositionEditor() {
        if isEditing, let ref = editingCell { positionEditor(ref) }
    }

    private func positionEditor(_ ref: CellRef) {
        var r = rect(ref).insetBy(dx: 1, dy: 1)
        if editor.superview !== self { r.origin = toViewport(r.origin) }
        let width = (editor.stringValue as NSString).size(withAttributes: [.font: editor.font ?? NSFont.systemFont(ofSize: 13)]).width + 16
        r.size.width = max(r.width, min(width, 600))
        editor.frame = r
    }

    @discardableResult
    func commitEdit(move: (Int, Int)?) -> Bool {
        guard isEditing, let ref = editingCell else { return true }
        let text = editor.stringValue
        guard controller?.setInput(text, at: ref) != false else { return false }
        endEditing()
        controller?.selectionDidChange()
        if let (dr, dc) = move { advance(rows: dr, cols: dc) }
        return true
    }

    func cancelEdit() {
        endEditing()
        controller?.selectionDidChange()
    }

    private func endEditing() {
        isEditing = false
        editingCell = nil
        refInsertion = nil
        editor.isHidden = true
        window?.makeFirstResponder(self)
        needsDisplay = true
    }

    var editorText: String { editor.stringValue }
    var pendingEdit: (cell: CellRef, text: String)? {
        guard isEditing, let editingCell else { return nil }
        return (editingCell, editor.stringValue)
    }
    /// Caret in the cell editor (or the formula bar) when a dialog took the focus.
    var savedCaret: NSRange?
    var editorCaret: NSRange? { editor.currentEditor()?.selectedRange }

    /// Inserts text at the caret of the cell editor (function wizard while typing a formula).
    func insertIntoEditor(_ text: String) {
        guard isEditing else { return }
        let current = editor.stringValue as NSString
        // Making the editor first responder selects everything, so use the caret saved when the dialog opened.
        var caret = savedCaret ?? NSRange(location: current.length, length: 0)
        savedCaret = nil
        if NSMaxRange(caret) > current.length { caret = NSRange(location: current.length, length: 0) }
        window?.makeFirstResponder(editor)
        editor.stringValue = current.replacingCharacters(in: caret, with: text)
        editor.currentEditor()?.selectedRange = NSRange(location: caret.location + (text as NSString).length, length: 0)
        positionEditor(editingCell ?? active)
        controller?.editorTextChanged(editor.stringValue)
    }

    /// Text typed in the formula bar is mirrored into the cell editor.
    func setEditorText(_ text: String) {
        if !isEditing { beginEdit(initial: text) } else { editor.stringValue = text }
        positionEditor(active)
    }

    private var canInsertReference: Bool {
        guard let fieldEditor = editor.currentEditor() else { return false }
        let text = editor.stringValue as NSString
        let caret = fieldEditor.selectedRange.location
        if let r = refInsertion, NSMaxRange(r) == caret { return true }
        guard caret > 0, caret <= text.length else { return false }
        let prev = text.substring(with: NSRange(location: caret - 1, length: 1))
        return "=(,;+-*/^&<>:".contains(prev)
    }

    private func insertReference(_ range: CellRange) {
        guard let fieldEditor = editor.currentEditor() else { return }
        let text = range.a1
        let target = refInsertion ?? fieldEditor.selectedRange
        let s = (editor.stringValue as NSString).replacingCharacters(in: target, with: text)
        editor.stringValue = s
        refInsertion = NSRange(location: target.location, length: (text as NSString).length)
        window?.makeFirstResponder(editor)
        editor.currentEditor()?.selectedRange = NSRange(location: NSMaxRange(refInsertion!), length: 0)
        positionEditor(editingCell ?? active)
        controller?.editorTextChanged(s)
    }

    func controlTextDidChange(_ obj: Notification) {
        refInsertion = nil
        positionEditor(editingCell ?? active)
        controller?.editorTextChanged(editor.stringValue)
    }

    func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        switch selector {
        case #selector(insertNewline(_:)): commitEdit(move: (1, 0)); return true
        case #selector(insertTab(_:)): commitEdit(move: (0, 1)); return true
        case #selector(insertBacktab(_:)): commitEdit(move: (0, -1)); return true
        case #selector(cancelOperation(_:)): cancelEdit(); return true
        case #selector(insertNewlineIgnoringFieldEditor(_:)):
            textView.insertText("\n", replacementRange: textView.selectedRange())
            return true
        default: return false
        }
    }
}

/// Borderless in-cell editor.
final class CellEditorField: NSTextField {
    override init(frame: NSRect) {
        super.init(frame: frame)
        isBordered = false
        drawsBackground = true
        backgroundColor = .textBackgroundColor
        textColor = .textColor
        focusRingType = .none
        cell?.wraps = false
        cell?.isScrollable = true
        wantsLayer = true
        layer?.borderColor = NSColor.controlAccentColor.cgColor
        layer?.borderWidth = 2
    }

    required init?(coder: NSCoder) { fatalError("not supported") }
}

extension GridView: NSViewToolTipOwner {
    func view(_ view: NSView, stringForToolTip tag: NSView.ToolTipTag, point: NSPoint, userData data: UnsafeMutableRawPointer?) -> String {
        sheet.comments[cell(at: point)] ?? ""
    }
}

// MARK: - Headers

/// Column letters / row numbers beside the scroll view, with selection highlight and resizing.
final class HeaderView: NSView {
    weak var grid: GridView?
    let isColumns: Bool
    private var resizing: Int?
    private var resizeStart: CGFloat = 0
    private var originalSize: CGFloat = 0
    private var selectingFrom: Int?

    init(grid: GridView, columns: Bool) {
        self.grid = grid
        isColumns = columns
        super.init(frame: .zero)
        clipsToBounds = true
    }

    required init?(coder: NSCoder) { fatalError("not supported") }

    override var isFlipped: Bool { true }

    override func draw(_ rect: NSRect) {
        guard let grid else { return }
        let palette = SheetPalette.for(self)
        palette.headerFill.setFill()
        bounds.fill()
        let sel = grid.expandedSelection
        let font = NSFont.systemFont(ofSize: 11 * min(1.2, max(0.8, grid.zoom)))
        let line = palette.headerLine
        let fw = grid.frozenWidth, fh = grid.frozenHeight
        let o = grid.scrollOrigin, size = grid.viewportSize
        if isColumns {
            // Frozen columns stay put; the rest scroll and are clipped right of the freeze line.
            let po = grid.paneOffset
            var cols: [(Int, Bool)] = grid.split != nil && fw > 0
                ? Array(grid.col(at: po.x)...grid.col(at: po.x + fw)).map { ($0, true) }
                : (0..<grid.sheet.frozenCols).map { ($0, true) }
            let c0 = grid.col(at: o.x + fw), c1 = grid.col(at: o.x + size.width)
            if c1 >= c0 { cols += (max(c0, grid.sheet.frozenCols)...max(c1, grid.sheet.frozenCols)).map { ($0, false) } }
            for (c, frozen) in cols {
                let x0 = frozen ? grid.x(c) - po.x : grid.x(c) - o.x
                let r = NSRect(x: x0, y: 0, width: grid.x(c + 1) - grid.x(c), height: bounds.height)
                NSGraphicsContext.saveGraphicsState()
                if !frozen { NSBezierPath(rect: NSRect(x: fw, y: 0, width: bounds.width, height: bounds.height)).addClip() }
                else { NSBezierPath(rect: NSRect(x: 0, y: 0, width: fw, height: bounds.height)).addClip() }
                let selected = sel.cols.contains(c)
                if selected {
                    NSColor.controlAccentColor.withAlphaComponent(0.18).setFill()
                    r.fill()
                    NSColor.controlAccentColor.setFill()
                    NSRect(x: r.minX, y: r.maxY - 2, width: r.width, height: 2).fill()
                }
                line.setFill()
                NSRect(x: r.maxX - 0.5, y: 0, width: 1, height: bounds.height).fill()
                label(CellRef.columnName(c), in: r, font: font, selected: selected)
                NSGraphicsContext.restoreGraphicsState()
            }
            if fw > 0 { palette.frozenLine.setFill(); NSRect(x: fw - 0.5, y: 0, width: 1, height: bounds.height).fill() }
            line.setFill()
            NSRect(x: 0, y: bounds.maxY - 1, width: bounds.width, height: 1).fill()
        } else {
            let po = grid.paneOffset
            var rows: [(Int, Bool)] = grid.split != nil && fh > 0
                ? Array(grid.row(at: po.y)...grid.row(at: po.y + fh)).map { ($0, true) }
                : (0..<grid.sheet.frozenRows).map { ($0, true) }
            let r0 = grid.row(at: o.y + fh), r1 = grid.row(at: o.y + size.height)
            if r1 >= r0 { rows += (max(r0, grid.sheet.frozenRows)...max(r1, grid.sheet.frozenRows)).map { ($0, false) } }
            for (row, frozen) in rows {
                let y0 = frozen ? grid.y(row) - po.y : grid.y(row) - o.y
                let r = NSRect(x: 0, y: y0, width: bounds.width, height: grid.y(row + 1) - grid.y(row))
                NSGraphicsContext.saveGraphicsState()
                if !frozen { NSBezierPath(rect: NSRect(x: 0, y: fh, width: bounds.width, height: bounds.height)).addClip() }
                else { NSBezierPath(rect: NSRect(x: 0, y: 0, width: bounds.width, height: fh)).addClip() }
                let selected = sel.rows.contains(row)
                if selected {
                    NSColor.controlAccentColor.withAlphaComponent(0.18).setFill()
                    r.fill()
                    NSColor.controlAccentColor.setFill()
                    NSRect(x: r.maxX - 2, y: r.minY, width: 2, height: r.height).fill()
                }
                line.setFill()
                NSRect(x: 0, y: r.maxY - 0.5, width: bounds.width, height: 1).fill()
                if r.height > 6 { label("\(row + 1)", in: r, font: font, selected: selected) }
                NSGraphicsContext.restoreGraphicsState()
            }
            if fh > 0 { palette.frozenLine.setFill(); NSRect(x: 0, y: fh - 0.5, width: bounds.width, height: 1).fill() }
            line.setFill()
            NSRect(x: bounds.maxX - 1, y: 0, width: 1, height: bounds.height).fill()
        }
    }

    /// Header point → grid coordinate along this header's axis.
    private func gridValue(_ p: NSPoint) -> CGFloat {
        guard let grid else { return 0 }
        let g = grid.fromViewport(isColumns ? NSPoint(x: p.x, y: 0) : NSPoint(x: 0, y: p.y))
        return isColumns ? g.x : g.y
    }

    private func label(_ s: String, in r: NSRect, font: NSFont, selected: Bool) {
        let attrs: [NSAttributedString.Key: Any] = [
            .font: selected ? NSFont.systemFont(ofSize: font.pointSize, weight: .semibold) : font,
            .foregroundColor: selected ? NSColor.controlAccentColor : SheetPalette.for(self).headerText,
        ]
        let size = (s as NSString).size(withAttributes: attrs)
        (s as NSString).draw(at: NSPoint(x: r.midX - size.width / 2, y: r.midY - size.height / 2), withAttributes: attrs)
    }

    /// Index of a boundary near the point (for resizing), in grid coordinates.
    private func boundary(at p: NSPoint) -> Int? {
        guard let grid else { return nil }
        let v = gridValue(p)
        let i = isColumns ? grid.col(at: v) : grid.row(at: v)
        let start = isColumns ? grid.x(i) : grid.y(i)
        let end = isColumns ? grid.x(i + 1) : grid.y(i + 1)
        if abs(v - end) <= 4 { return i }
        if abs(v - start) <= 4, i > 0 { return i - 1 }
        return nil
    }

    override func resetCursorRects() {
        addCursorRect(bounds, cursor: .arrow)
    }

    override func mouseMoved(with event: NSEvent) {
        if boundary(at: convert(event.locationInWindow, from: nil)) != nil {
            (isColumns ? NSCursor.resizeLeftRight : NSCursor.resizeUpDown).set()
        } else { NSCursor.arrow.set() }
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        trackingAreas.forEach(removeTrackingArea)
        addTrackingArea(NSTrackingArea(rect: bounds, options: [.mouseMoved, .activeInKeyWindow, .inVisibleRect], owner: self))
    }

    override func mouseDown(with event: NSEvent) {
        guard let grid else { return }
        let p = convert(event.locationInWindow, from: nil)
        if let b = boundary(at: p) {
            guard grid.commitEdit(move: nil), grid.controller?.guardEditable(nil) == true else { return }
            if event.clickCount == 2 {
                grid.controller?.autofit(isColumns ? .column(b) : .row(b))
                return
            }
            resizing = b
            resizeStart = isColumns ? p.x : p.y
            originalSize = isColumns ? grid.sheet.width(b) : grid.sheet.height(b)
            return
        }
        let v = gridValue(p)
        let i = isColumns ? grid.col(at: v) : grid.row(at: v)
        if grid.isEditing, !grid.commitEdit(move: nil) { return }
        let from = event.modifierFlags.contains(.shift) ? (isColumns ? grid.anchor.col : grid.anchor.row) : i
        selectingFrom = from
        if isColumns { grid.selectColumns(from, i) } else { grid.selectRows(from, i) }
        window?.makeFirstResponder(grid)
    }

    override func mouseDragged(with event: NSEvent) {
        guard let grid else { return }
        let p = convert(event.locationInWindow, from: nil)
        if let b = resizing {
            let delta = ((isColumns ? p.x : p.y) - resizeStart) / grid.scale
            grid.controller?.previewSize(isColumns ? .column(b) : .row(b), max(0, originalSize + delta))
            return
        }
        guard let from = selectingFrom else { return }
        let v = gridValue(p)
        let i = isColumns ? grid.col(at: v) : grid.row(at: v)
        if isColumns { grid.selectColumns(from, i) } else { grid.selectRows(from, i) }
    }

    override func mouseUp(with event: NSEvent) {
        if let b = resizing, let grid {
            let current = isColumns ? grid.sheet.width(b) : grid.sheet.height(b)
            grid.controller?.commitSize(isColumns ? .column(b) : .row(b), current, original: originalSize)
        }
        resizing = nil
        selectingFrom = nil
    }
}

/// Top-left corner above the row numbers: click selects everything.
final class CornerView: NSView {
    var onClick: (() -> Void)?

    override func draw(_ dirtyRect: NSRect) {
        let palette = SheetPalette.for(self)
        palette.headerFill.setFill()
        bounds.fill()
        palette.headerLine.setFill()
        NSRect(x: bounds.maxX - 1, y: 0, width: 1, height: bounds.height).fill()
        NSRect(x: 0, y: 0, width: bounds.width, height: 1).fill()
        let tri = NSBezierPath()
        tri.move(to: NSPoint(x: bounds.maxX - 4, y: 4))
        tri.line(to: NSPoint(x: bounds.maxX - 4, y: bounds.height - 4))
        tri.line(to: NSPoint(x: bounds.maxX - 4 - (bounds.height - 8), y: 4))
        tri.close()
        palette.cornerGlyph.setFill()
        tri.fill()
    }

    override func mouseDown(with event: NSEvent) { onClick?() }
}

/// Transparent layer over the scroll view holding the frozen panes and the cell editor.
/// Clicks outside the panes fall through to the grid.
final class FreezeOverlay: NSView {
    weak var grid: GridView?
    private let corner = FrozenPane(kind: .corner)
    private let top = FrozenPane(kind: .top)
    private let left = FrozenPane(kind: .left)

    init(grid: GridView) {
        self.grid = grid
        super.init(frame: .zero)
        for pane in [left, top, corner] {
            pane.grid = grid
            addSubview(pane)
        }
    }

    required init?(coder: NSCoder) { fatalError("not supported") }

    override var isFlipped: Bool { true }

    override func hitTest(_ point: NSPoint) -> NSView? {
        let hit = super.hitTest(point)
        return hit === self ? nil : hit
    }

    /// Re-lays out panes after scrolling, resizing or changing the freeze.
    func panesNeedDisplay() {
        guard let grid else { return }
        let fw = grid.frozenWidth, fh = grid.frozenHeight
        let size = grid.viewportSize
        corner.isHidden = fw == 0 || fh == 0
        top.isHidden = fh == 0
        left.isHidden = fw == 0
        corner.frame = NSRect(x: 0, y: 0, width: fw, height: fh)
        top.frame = NSRect(x: fw, y: 0, width: max(0, size.width - fw), height: fh)
        left.frame = NSRect(x: 0, y: fh, width: fw, height: max(0, size.height - fh))
        for pane in [corner, top, left] where !pane.isHidden { pane.needsDisplay = true }
        needsDisplay = true
    }

    override func draw(_ dirtyRect: NSRect) {
        guard let grid else { return }
        let fw = grid.frozenWidth, fh = grid.frozenHeight
        SheetPalette.for(self).frozenLine.setFill()
        let w: CGFloat = grid.split != nil ? 3 : 1
        if fw > 0 { NSRect(x: fw - w / 2, y: 0, width: w, height: bounds.height).fill() }
        if fh > 0 { NSRect(x: 0, y: fh - w / 2, width: bounds.width, height: w).fill() }
    }
}

final class FrozenPane: NSView {
    enum Kind { case corner, top, left }
    let kind: Kind
    weak var grid: GridView?

    init(kind: Kind) {
        self.kind = kind
        super.init(frame: .zero)
        clipsToBounds = true
    }

    required init?(coder: NSCoder) { fatalError("not supported") }

    override var isFlipped: Bool { true }

    /// Local point + offset = grid point.
    private var offset: NSPoint {
        guard let grid else { return .zero }
        let o = grid.scrollOrigin
        let po = grid.paneOffset
        switch kind {
        case .corner: return po
        case .top: return NSPoint(x: grid.frozenWidth + o.x, y: po.y)
        case .left: return NSPoint(x: po.x, y: grid.frozenHeight + o.y)
        }
    }

    /// Split panes scroll on their own along their axis; the other direction moves the main pane.
    override func scrollWheel(with event: NSEvent) {
        guard let grid, grid.split != nil else { grid?.enclosingScrollView?.scrollWheel(with: event); return }
        var s = grid.splitScroll
        var dx = -event.scrollingDeltaX, dy = -event.scrollingDeltaY
        if !event.hasPreciseScrollingDeltas { dx *= 10; dy *= 10 }
        if kind != .top { s.x = max(0, min(grid.frame.width - grid.frozenWidth, s.x + dx)); dx = 0 }
        if kind != .left { s.y = max(0, min(grid.frame.height - grid.frozenHeight, s.y + dy)); dy = 0 }
        grid.splitScroll = s
        if dx != 0 || dy != 0, let clip = grid.enclosingScrollView?.contentView {
            var o = clip.bounds.origin
            o.x = max(0, min(grid.frame.width - clip.bounds.width, o.x + dx))
            o.y = max(0, min(grid.frame.height - clip.bounds.height, o.y + dy))
            clip.scroll(to: o)
            grid.enclosingScrollView?.reflectScrolledClipView(clip)
        }
        grid.overlay?.panesNeedDisplay()
        grid.controller?.headersNeedDisplay()
    }

    override func draw(_ dirtyRect: NSRect) {
        guard let grid, let ctx = NSGraphicsContext.current?.cgContext else { return }
        let o = offset
        ctx.saveGState()
        ctx.translateBy(x: -o.x, y: -o.y)
        grid.drawContent(dirtyRect.offsetBy(dx: o.x, dy: o.y))
        ctx.restoreGState()
    }

    override func mouseDown(with event: NSEvent) { grid?.mouseDown(with: event) }
    override func mouseDragged(with event: NSEvent) { grid?.mouseDragged(with: event) }
    override func mouseUp(with event: NSEvent) { grid?.mouseUp(with: event) }
    override func menu(for event: NSEvent) -> NSMenu? { grid?.menu(for: event) }
}
