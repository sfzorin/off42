import AppKit
import Kit42

/// Editing features beyond typing: fill handle, hiding, filters, names,
/// comments, conditional formatting, pictures and charts.
extension SheetWindowController {
    // MARK: - Fill handle

    /// Extends `source` over `target`: numbers and dates continue a series, text with a trailing
    /// number increments, formulas shift their relative references.
    func autoFill(from source: CellRange, to target: CellRange) {
        guard grid.commitEdit(move: nil), let doc, source != target,
              target.contains(source.start), target.contains(source.end),
              guardCellOperation(source), guardCellOperation(target),
              pasteTarget(rows: target.rows.count, cols: target.cols.count, at: target.start) != nil else { return }
        let vertical = target.cols == source.cols
        guard vertical ? (target.start.row == source.start.row || target.end.row == source.end.row)
            : (target.rows == source.rows && (target.start.col == source.start.col || target.end.col == source.end.col)) else { return }
        let forward = vertical ? target.end.row > source.end.row : target.end.col > source.end.col
        let destination: CellRange
        if vertical {
            destination = forward ? CellRange(CellRef(row: source.end.row + 1, col: source.start.col), target.end)
                : CellRange(target.start, CellRef(row: source.start.row - 1, col: source.end.col))
        } else {
            destination = forward ? CellRange(CellRef(row: source.start.row, col: source.end.col + 1), target.end)
                : CellRange(target.start, CellRef(row: source.end.row, col: source.start.col - 1))
        }
        guard guardEditable(destination), guardArrays(destination) else { return }
        doc.change(L("Fill")) { wb in
            let sheet = wb.sheets[sheetIndex]
            dropArrays(in: destination, of: &wb)
            let lines = vertical ? Array(source.cols) : Array(source.rows)
            for line in lines {
                let src: [CellRef] = vertical ? source.rows.map { CellRef(row: $0, col: line) } : source.cols.map { CellRef(row: line, col: $0) }
                let dst: [CellRef] = vertical
                    ? (forward ? Array(source.end.row + 1...target.end.row) : Array((target.start.row..<source.start.row).reversed())).map { CellRef(row: $0, col: line) }
                    : (forward ? Array(source.end.col + 1...target.end.col) : Array((target.start.col..<source.start.col).reversed())).map { CellRef(row: line, col: $0) }
                let cells = src.map { ref -> Cell in
                    var cell = sheet[ref]
                    if sheet.array(containing: ref) != nil || (cell.input.isEmpty && !cell.value.isEmpty) {
                        cell.input = XLSX.inputText(for: cell.value, style: cell.style, date1904: wb.date1904)
                    }
                    cell.array = false
                    return cell
                }
                let numbers = cells.compactMap { c -> Double? in if !c.isFormula, case .number(let d) = c.value { return d }; return nil }
                let isDate = cells.first.map { NumberFormat.isDateFormat($0.style.numberFormat) } ?? false
                var step: Double? = nil
                if numbers.count == cells.count, !numbers.isEmpty {
                    if numbers.count >= 2 { step = (numbers.last! - numbers.first!) / Double(numbers.count - 1) }
                    else if isDate { step = 1 }
                }
                for (k, ref) in dst.enumerated() {
                    let pattern = forward ? k % cells.count : (cells.count - 1 - k % cells.count)
                    var c = cells[pattern]
                    let from = src[pattern]
                    if c.isFormula {
                        c.input = Formula.shifted(c.input, rows: ref.row - from.row, cols: ref.col - from.col)
                    } else if let step {
                        let base = forward ? numbers.last! : numbers.first!
                        let v = base + step * Double(k + 1) * (forward ? 1 : -1)
                        c.value = .number(v)
                        c.input = isDate ? XLSX.inputText(for: c.value, style: c.style, date1904: wb.date1904) : Value.formatGeneral(v)
                    } else if case .string(let text) = c.value, let m = text.range(of: "\\d+$", options: .regularExpression), let n = Int(text[m]) {
                        let delta = (k / cells.count + 1) * (forward ? 1 : -1)
                        let next = String(text[..<m.lowerBound]) + String(max(0, n + delta))
                        c.input = next
                        c.value = .string(next)
                    }
                    wb.sheets[sheetIndex][ref] = c
                }
            }
        }
        grid.selection = CellRange(CellRef(row: min(source.start.row, target.start.row), col: min(source.start.col, target.start.col)),
                                   CellRef(row: max(source.end.row, target.end.row), col: max(source.end.col, target.end.col)))
        selectionDidChange()
    }

    // MARK: - Hide / unhide

    /// Whole selection (not clipped to data), unless it spans the entire sheet.
    private var selectedRows: ClosedRange<Int> { grid.selection.rows.count < 100_000 ? grid.selection.rows : clampedSelection.rows }

    @objc func hideRowsAction(_ sender: Any?) {
        guard guardEditable(nil) else { return }
        let rows = selectedRows
        doc?.change(L("Hide Rows"), recalc: false) { wb in rows.forEach { wb.sheets[sheetIndex].rowHeights[$0] = 0 } }
    }

    @objc func unhideRowsAction(_ sender: Any?) {
        guard guardEditable(nil) else { return }
        let rows = grid.selection.rows.count > 1 ? Array(selectedRows) : [grid.active.row - 1, grid.active.row, grid.active.row + 1]
        doc?.change(L("Unhide Rows"), recalc: false) { wb in
            for r in rows where wb.sheets[sheetIndex].rowHeights[r] == 0 { wb.sheets[sheetIndex].rowHeights[r] = nil }
        }
    }

    @objc func hideColumnsAction(_ sender: Any?) {
        guard guardEditable(nil) else { return }
        let cols = grid.selection.cols
        doc?.change(L("Hide Columns"), recalc: false) { wb in cols.forEach { wb.sheets[sheetIndex].columnWidths[$0] = 0 } }
    }

    @objc func unhideColumnsAction(_ sender: Any?) {
        guard guardEditable(nil) else { return }
        let cols = grid.selection.cols.count > 1 ? Array(grid.selection.cols) : [grid.active.col - 1, grid.active.col, grid.active.col + 1]
        doc?.change(L("Unhide Columns"), recalc: false) { wb in
            for c in cols where wb.sheets[sheetIndex].columnWidths[c] == 0 { wb.sheets[sheetIndex].columnWidths[c] = nil }
        }
    }

    // MARK: - AutoFilter

    @objc func toggleFilterAction(_ sender: Any?) {
        guard let doc else { return }
        if sheet.autoFilter != nil {
            doc.change(L("Filter"), recalc: false) { wb in
                wb.sheets[sheetIndex].autoFilter = nil
                wb.sheets[sheetIndex].filterValues = [:]
                wb.sheets[sheetIndex].filterHidden = []
            }
            return
        }
        var range = clampedSelection
        if range.rows.count < 2, let used = sheet.usedRange {
            // Current table: from the active row down to the end of the used area.
            range = CellRange(CellRef(row: grid.active.row, col: used.start.col), used.end)
        }
        doc.change(L("Filter"), recalc: false) { $0.sheets[sheetIndex].autoFilter = range }
    }

    /// Display texts of a filter column (below the header), in order of first appearance.
    func filterItems(_ col: Int) -> [String] {
        guard let f = sheet.autoFilter else { return [] }
        var seen = Set<String>(), out: [String] = []
        for r in (f.start.row + 1)...max(f.start.row + 1, f.end.row) {
            let c = sheet[CellRef(row: r, col: col)]
            let text = NumberFormat.format(c.value, c.style.numberFormat, date1904: doc?.workbook.date1904 ?? false).text
            if seen.insert(text).inserted { out.append(text) }
        }
        return out.sorted { $0.localizedStandardCompare($1) == .orderedAscending }
    }

    func applyFilter(column: Int, allowed: Set<String>?) {
        guard let doc else { return }
        doc.change(L("Filter"), recalc: false) { wb in
            var s = wb.sheets[sheetIndex]
            s.filterValues[column] = allowed
            s.filterHidden = []
            if let f = s.autoFilter, f.end.row > f.start.row {
                for r in (f.start.row + 1)...f.end.row {
                    for (col, values) in s.filterValues {
                        let c = s[CellRef(row: r, col: col)]
                        if !values.contains(NumberFormat.format(c.value, c.style.numberFormat, date1904: wb.date1904).text) { s.filterHidden.insert(r); break }
                    }
                }
            }
            wb.sheets[sheetIndex] = s
        }
    }

    func showFilter(column: Int, from viewportPoint: NSPoint) {
        let vc = FilterPopover(items: filterItems(column), allowed: sheet.filterValues[column]) { [weak self] allowed, sort in
            guard let self else { return }
            if let sort {
                self.grid.select(CellRef(row: (self.sheet.autoFilter?.start.row ?? 0) + 1, col: column))
                if let f = self.sheet.autoFilter { self.grid.select(CellRef(row: f.end.row, col: f.end.col), extend: true) }
                self.grid.active = CellRef(row: (self.sheet.autoFilter?.start.row ?? 0) + 1, col: column)
                sort ? self.sortAscending(nil) : self.sortDescending(nil)
            } else {
                self.applyFilter(column: column, allowed: allowed)
            }
        }
        let popover = NSPopover()
        popover.contentViewController = vc
        popover.behavior = .transient
        vc.popover = popover
        let anchor = NSRect(x: viewportPoint.x, y: viewportPoint.y, width: 16, height: 16)
        popover.show(relativeTo: anchor, of: grid.overlay ?? grid, preferredEdge: .maxY)
    }

    // MARK: - Print area & names

    @objc func setPrintAreaAction(_ sender: Any?) {
        let range = clampedSelection
        doc?.change(L("Set Print Area"), recalc: false) { $0.sheets[sheetIndex].printArea = range }
    }

    @objc func clearPrintAreaAction(_ sender: Any?) {
        doc?.change(L("Clear Print Area"), recalc: false) { $0.sheets[sheetIndex].printArea = nil }
    }

    @objc func defineNameAction(_ sender: Any?) {
        guard let doc else { return }
        let r = clampedSelection
        let absolute = "$\(CellRef.columnName(r.start.col))$\(r.start.row + 1)" + (r.count > 1 ? ":$\(CellRef.columnName(r.end.col))$\(r.end.row + 1)" : "")
        let name = NSTextField(string: "")
        name.placeholderString = L("e.g. Rate")
        let refers = NSTextField(string: Formula.sheetPrefix(sheet.name) + absolute)
        let existing = doc.workbook.names.sorted { $0.key < $1.key }.map { "\($0.key) = \($0.value)" }.joined(separator: "\n")
        let alert = NSAlert.form(L("Define Name"), info: existing.isEmpty ? L("Names can be used in formulas instead of references.") : existing,
                                 ok: L("Define"), rows: [(L("Name:"), name), (L("Refers to:"), refers)])
        alert.window.initialFirstResponder = name
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        let key = name.stringValue.trimmingCharacters(in: .whitespaces)
        guard !key.isEmpty, key.first?.isLetter == true, Formula.refPart(key) == nil else { NSSound.beep(); return }
        let value = refers.stringValue.trimmingCharacters(in: .whitespaces).trimmingCharacters(in: CharacterSet(charactersIn: "="))
        doc.change(L("Define Name")) { $0.names[key] = value.isEmpty ? nil : value }
    }

    // MARK: - Comments

    @objc func editCommentAction(_ sender: Any?) {
        guard let doc, let window else { return }
        let ref = grid.active
        let field = NSTextView(frame: NSRect(x: 0, y: 0, width: 300, height: 90))
        field.string = sheet.comments[ref] ?? ""
        field.font = .systemFont(ofSize: 13)
        let scroll = NSScrollView(frame: field.frame)
        scroll.documentView = field
        scroll.borderType = .bezelBorder
        let alert = NSAlert()
        alert.messageText = L("Comment for %@", ref.a1)
        alert.accessoryView = scroll
        alert.addButton(withTitle: L("Save"))
        alert.addButton(withTitle: L("Cancel"))
        alert.window.initialFirstResponder = field
        alert.beginSheetModal(for: window) { response in
            guard response == .alertFirstButtonReturn else { return }
            let text = field.string.trimmingCharacters(in: .whitespacesAndNewlines)
            doc.change(L("Comment"), recalc: false) { $0.sheets[self.sheetIndex].comments[ref] = text.isEmpty ? nil : text }
        }
    }

    @objc func deleteCommentAction(_ sender: Any?) {
        let range = clampedSelection
        doc?.change(L("Delete Comment"), recalc: false) { wb in
            for ref in wb.sheets[sheetIndex].comments.keys where range.contains(ref) { wb.sheets[sheetIndex].comments[ref] = nil }
        }
    }

    // MARK: - Conditional formatting

    static let highlightStyles: [(title: String, fill: NSColor, text: NSColor)] = [
        (L("Light red fill with dark red text"), NSColor(srgbRed: 1, green: 0.78, blue: 0.81, alpha: 1), NSColor(srgbRed: 0.61, green: 0, blue: 0.02, alpha: 1)),
        (L("Yellow fill with dark yellow text"), NSColor(srgbRed: 1, green: 0.92, blue: 0.61, alpha: 1), NSColor(srgbRed: 0.61, green: 0.4, blue: 0, alpha: 1)),
        (L("Green fill with dark green text"), NSColor(srgbRed: 0.78, green: 0.94, blue: 0.81, alpha: 1), NSColor(srgbRed: 0, green: 0.38, blue: 0, alpha: 1)),
    ]

    @objc func conditionalRuleAction(_ sender: Any?) {
        guard let doc else { return }
        let kind = commandTag(of: sender)   // 0 >, 1 <, 2 between, 3 equal, 4 text contains, 5 duplicates
        let range = clampedSelection
        guard guardCellOperation(range) else { return }
        let a = NSTextField(string: ""), b = NSTextField(string: "")
        let style = NSPopUpButton(frame: .zero, pullsDown: false)
        style.addItems(withTitles: Self.highlightStyles.map(\.title))
        var rows: [(String, NSView)] = []
        switch kind {
        case 2: rows = [(L("From:"), a), (L("To:"), b)]
        case 4: rows = [(L("Text:"), a)]
        case 5: rows = []
        default: rows = [(L("Value:"), a)]
        }
        rows.append((L("Format:"), style))
        let titles = [L("Greater Than…"), L("Less Than…"), L("Between…"), L("Equal To…"), L("Text Contains…"), L("Duplicate Values")]
        let alert = NSAlert.form(titles[min(kind, titles.count - 1)].replacingOccurrences(of: "…", with: ""), info: range.a1, ok: L("Apply"), rows: rows)
        alert.window.initialFirstResponder = a
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        let ops = ["greaterThan", "lessThan", "between", "equal"]
        let ruleKind: ConditionalFormat.Kind
        switch kind {
        case 0, 1, 3: ruleKind = .cellIs(op: ops[kind], values: [a.stringValue])
        case 2: ruleKind = .cellIs(op: "between", values: [a.stringValue, b.stringValue])
        case 4: ruleKind = .containsText(a.stringValue)
        default: ruleKind = .duplicates
        }
        let s = Self.highlightStyles[max(0, style.indexOfSelectedItem)]
        doc.change(L("Conditional Formatting"), recalc: false) {
            $0.sheets[sheetIndex].conditionalFormats.append(ConditionalFormat(range: range, kind: ruleKind, fill: s.fill, textColor: s.text))
        }
    }

    @objc func colorScaleAction(_ sender: Any?) {
        let range = clampedSelection
        guard guardCellOperation(range) else { return }
        let colors = [NSColor(srgbRed: 0.97, green: 0.41, blue: 0.42, alpha: 1), NSColor(srgbRed: 1, green: 0.92, blue: 0.52, alpha: 1),
                      NSColor(srgbRed: 0.39, green: 0.75, blue: 0.48, alpha: 1)]
        doc?.change(L("Conditional Formatting"), recalc: false) {
            $0.sheets[sheetIndex].conditionalFormats.append(ConditionalFormat(range: range, kind: .colorScale(colors)))
        }
    }

    @objc func dataBarAction(_ sender: Any?) {
        let range = clampedSelection
        guard guardCellOperation(range) else { return }
        doc?.change(L("Conditional Formatting"), recalc: false) {
            $0.sheets[sheetIndex].conditionalFormats.append(ConditionalFormat(range: range, kind: .dataBar(NSColor(srgbRed: 0.39, green: 0.56, blue: 0.85, alpha: 1))))
        }
    }

    @objc func clearRulesAction(_ sender: Any?) {
        let range = clampedSelection
        doc?.change(L("Clear Rules"), recalc: false) { wb in
            wb.sheets[sheetIndex].conditionalFormats.removeAll { $0.range.intersects(range) }
        }
    }

    // MARK: - Pictures & charts

    @objc func insertImageAction(_ sender: Any?) {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.image]
        guard panel.runModal() == .OK, let url = panel.url, let image = NSImage(contentsOf: url),
              let tiff = image.tiffRepresentation, let png = NSBitmapImageRep(data: tiff)?.representation(using: .png, properties: [:]) else { return }
        insertPicture(png, size: image.size)
    }

    func insertPicture(_ png: Data, size: NSSize) {
        let maxSide: CGFloat = 320
        let k = min(1, maxSide / max(size.width, size.height, 1))
        let image = SheetImage(anchor: grid.active, size: CGSize(width: size.width * k, height: size.height * k), data: png)
        doc?.change(L("Insert Picture"), recalc: false) { $0.sheets[sheetIndex].images.append(image) }
        grid.selectedObject = .image(sheet.images.count - 1)
    }

    @objc func showStampsAction(_ sender: Any?) {
        let popover = NSPopover()
        let vc = StampLibraryViewController { [weak self] cg in
            let rep = NSBitmapImageRep(cgImage: cg)
            guard let png = rep.representation(using: .png, properties: [:]) else { return }
            // Its physical size, whatever resolution it was drawn at.
            let aspect = CGFloat(cg.width) / CGFloat(max(1, cg.height))
            let width = MarkPlacement.standard(aspect: aspect).widthPoints
            self?.insertPicture(png, size: NSSize(width: width, height: width / aspect))
        }
        vc.popover = popover
        popover.contentViewController = vc
        popover.behavior = .transient
        popover.show(relativeTo: grid.visibleRect, of: grid, preferredEdge: .minY)
    }

    @objc func insertChartAction(_ sender: Any?) {
        guard let doc else { return }
        let kinds: [SheetChart.Kind] = [.column, .bar, .line, .area, .pie, .scatter]
        let kind = kinds[max(0, min(kinds.count - 1, commandTag(of: sender)))]
        var range = clampedSelection
        if range.count == 1, let used = sheet.usedRange { range = used }
        guard guardCellOperation(range) else { return }
        // First column with text → categories; first row with text → series names.
        let firstColText = range.rows.dropFirst().contains { r -> Bool in if case .string = sheet[CellRef(row: r, col: range.start.col)].value { return true }; return false }
        let firstRowText = range.cols.contains { c -> Bool in if case .string = sheet[CellRef(row: range.start.row, col: c)].value { return true }; return false }
        let dataRows = (range.start.row + (firstRowText ? 1 : 0))...range.end.row
        let catCol = firstColText ? range.start.col : nil
        let valueCols = (range.start.col + (firstColText ? 1 : 0))...max(range.start.col + (firstColText ? 1 : 0), range.end.col)
        let p = Formula.sheetPrefix(sheet.name)
        func ref(_ c: Int) -> String { "\(p)$\(CellRef.columnName(c))$\(dataRows.lowerBound + 1):$\(CellRef.columnName(c))$\(dataRows.upperBound + 1)" }
        let series = valueCols.map { c in
            SheetChart.Series(name: firstRowText ? "=\(p)$\(CellRef.columnName(c))$\(range.start.row + 1)" : L("Series %d", c - valueCols.lowerBound + 1),
                              categories: catCol.map(ref), values: ref(c))
        }
        let anchor = CellRef(row: range.start.row, col: range.end.col + 2)
        let title = firstRowText && !firstColText ? "" : sheet[range.start].value.plain
        let chart = SheetChart(kind: kind, title: title, series: series, anchor: anchor, size: CGSize(width: 360, height: 220))
        doc.change(L("Insert Chart"), recalc: false) { $0.sheets[sheetIndex].charts.append(chart) }
        grid.selectedObject = .chart(sheet.charts.count - 1)
    }

    /// Live move/resize while dragging (no undo step).
    func previewObject(_ o: GridView.FloatingObject?, frame: NSRect) {
        guard let o else { return }
        doc?.preview { wb in Self.place(o, frame: frame, in: &wb.sheets[sheetIndex], grid: grid) }
    }

    func commitObject(_ o: GridView.FloatingObject?, original: NSRect) {
        guard let o, let doc else { return }
        let final = grid.objectRect(o)
        guard final != original else { return }
        doc.preview { wb in Self.place(o, frame: original, in: &wb.sheets[sheetIndex], grid: grid) }
        doc.change(L("Move Object"), recalc: false) { wb in Self.place(o, frame: final, in: &wb.sheets[sheetIndex], grid: grid) }
    }

    func deleteObject(_ o: GridView.FloatingObject) {
        doc?.change(L("Delete Object"), recalc: false) { wb in
            switch o {
            case .image(let i): if i < wb.sheets[sheetIndex].images.count { wb.sheets[sheetIndex].images.remove(at: i) }
            case .chart(let i): if i < wb.sheets[sheetIndex].charts.count { wb.sheets[sheetIndex].charts.remove(at: i) }
            }
        }
    }

    static func place(_ o: GridView.FloatingObject, frame: NSRect, in sheet: inout Sheet, grid: GridView) {
        let anchor = grid.cell(at: NSPoint(x: max(0, frame.minX), y: max(0, frame.minY)))
        let origin = grid.rect(anchor).origin
        let s = grid.scale
        switch o {
        case .image(let i):
            guard i < sheet.images.count else { return }
            sheet.images[i].anchor = anchor
            sheet.images[i].offset = CGPoint(x: (frame.minX - origin.x) / s, y: (frame.minY - origin.y) / s)
            sheet.images[i].size = CGSize(width: frame.width / s, height: frame.height / s)
        case .chart(let i):
            guard i < sheet.charts.count else { return }
            sheet.charts[i].anchor = anchor
            sheet.charts[i].size = CGSize(width: frame.width / s, height: frame.height / s)
        }
    }
}

/// Filter drop-down: sort buttons and a checklist of the column's values.
final class FilterPopover: NSViewController {
    weak var popover: NSPopover?
    private let items: [String]
    private var checked: Set<String>
    private let done: (Set<String>?, Bool?) -> Void
    private var boxes: [NSButton] = []

    init(items: [String], allowed: Set<String>?, done: @escaping (Set<String>?, Bool?) -> Void) {
        self.items = items
        checked = allowed ?? Set(items)
        self.done = done
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) { fatalError("not supported") }

    override func loadView() {
        let asc = NSButton(title: L("Sort A→Z"), target: self, action: #selector(sortAsc))
        let desc = NSButton(title: L("Sort Z→A"), target: self, action: #selector(sortDesc))
        [asc, desc].forEach { $0.bezelStyle = .accessoryBarAction; $0.controlSize = .small }
        let all = NSButton(checkboxWithTitle: L("(Select All)"), target: self, action: #selector(toggleAll(_:)))
        all.state = checked.count == items.count ? .on : .off
        let list = NSStackView()
        list.orientation = .vertical
        list.alignment = .leading
        list.spacing = 4
        for item in items.prefix(500) {
            let b = NSButton(checkboxWithTitle: item.isEmpty ? L("(Blanks)") : item, target: self, action: #selector(toggled(_:)))
            b.identifier = NSUserInterfaceItemIdentifier(item)
            b.state = checked.contains(item) ? .on : .off
            boxes.append(b)
            list.addArrangedSubview(b)
        }
        let scroll = NSScrollView()
        scroll.documentView = list
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = false
        list.translatesAutoresizingMaskIntoConstraints = false
        scroll.translatesAutoresizingMaskIntoConstraints = false
        scroll.heightAnchor.constraint(equalToConstant: min(220, CGFloat(items.count) * 22 + 8)).isActive = true
        scroll.widthAnchor.constraint(equalToConstant: 220).isActive = true
        list.widthAnchor.constraint(equalTo: scroll.widthAnchor).isActive = true
        let ok = NSButton(title: L("OK"), target: self, action: #selector(apply))
        ok.keyEquivalent = "\r"
        let clear = NSButton(title: L("Clear Filter"), target: self, action: #selector(clearFilter))
        let buttons = NSStackView(views: [clear, NSView(), ok])
        let root = NSStackView(views: [NSStackView(views: [asc, desc]), all, scroll, buttons])
        root.orientation = .vertical
        root.alignment = .leading
        root.spacing = 8
        root.edgeInsets = NSEdgeInsets(top: 12, left: 12, bottom: 12, right: 12)
        buttons.translatesAutoresizingMaskIntoConstraints = false
        buttons.widthAnchor.constraint(equalTo: scroll.widthAnchor).isActive = true
        view = root
    }

    @objc private func toggled(_ sender: NSButton) {
        guard let v = sender.identifier?.rawValue else { return }
        if sender.state == .on { checked.insert(v) } else { checked.remove(v) }
    }

    @objc private func toggleAll(_ sender: NSButton) {
        checked = sender.state == .on ? Set(items) : []
        boxes.forEach { $0.state = sender.state }
    }

    @objc private func apply() {
        popover?.performClose(nil)
        done(checked.count == items.count ? nil : checked, nil)
    }

    @objc private func clearFilter() {
        popover?.performClose(nil)
        done(nil, nil)
    }

    @objc private func sortAsc() { popover?.performClose(nil); done(nil, true) }
    @objc private func sortDesc() { popover?.performClose(nil); done(nil, false) }
}
