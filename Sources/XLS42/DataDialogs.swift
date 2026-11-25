import AppKit
import Kit42

/// Data and Tools menu dialogs (Sort…, Standard/Advanced Filter…, Subtotals…, Remove Duplicates…, Text to Columns…,
/// Fill › Series…, Goal Seek…, Solver…, Statistics). Each dialog collects its settings and calls an `apply…`
/// method that makes one undoable change (the UI tests call those directly).
extension SheetWindowController {
    // MARK: - Helpers

    /// The selection, or the block of filled cells around the active cell (Excel's current region).
    func currentRegion() -> CellRange {
        let sel = clampedSelection
        if sel.count > 1 { return sel }
        let s = sheet
        func filled(_ r: Int, _ c: Int) -> Bool { r >= 0 && c >= 0 && !(s.cells[CellRef(row: r, col: c)]?.value.isEmpty ?? true) }
        var r = CellRange(grid.active, grid.active)
        var changed = true
        while changed {
            changed = false
            let top = r.start.row - 1, bottom = r.end.row + 1, left = r.start.col - 1, right = r.end.col + 1
            if (left...right).contains(where: { filled(top, $0) }) { r = CellRange(CellRef(row: top, col: r.start.col), r.end); changed = true }
            if (left...right).contains(where: { filled(bottom, $0) }) { r = CellRange(r.start, CellRef(row: bottom, col: r.end.col)); changed = true }
            if (r.start.row - 1...r.end.row + 1).contains(where: { filled($0, left) }) { r = CellRange(CellRef(row: r.start.row, col: left), r.end); changed = true }
            if (r.start.row - 1...r.end.row + 1).contains(where: { filled($0, right) }) { r = CellRange(r.start, CellRef(row: r.end.row, col: right)); changed = true }
            r = CellRange(CellRef(row: max(0, r.start.row), col: max(0, r.start.col)), r.end)
            if r.count > 5_000_000 { break }
        }
        return r
    }

    /// Column titles of a range: header texts, or "Column B".
    func columnTitles(_ range: CellRange, header: Bool) -> [String] {
        range.cols.map { c in
            let h = header ? NumberFormat.format(sheet[CellRef(row: range.start.row, col: c)].value, "General").text : ""
            return h.isEmpty ? L("Column %@", CellRef.columnName(c)) : h
        }
    }

    /// "B3", "$B$3:$D$9", "Sheet2!A1" → sheet index and range.
    func parseRange(_ text: String) -> (sheet: Int, range: CellRange)? {
        var t = text.trimmingCharacters(in: .whitespaces)
        if t.hasPrefix("=") { t.removeFirst() }
        var si = sheetIndex
        if let bang = t.lastIndex(of: "!") {
            var name = String(t[..<bang])
            if name.hasPrefix("'"), name.hasSuffix("'"), name.count >= 2 { name = String(name.dropFirst().dropLast()).replacingOccurrences(of: "''", with: "'") }
            guard let i = doc?.workbook.sheetIndex(named: name) else { return nil }
            si = i
            t = String(t[t.index(after: bang)...])
        }
        guard let r = CellRange(t.replacingOccurrences(of: "$", with: "")) else { return nil }
        return (si, r)
    }

    func popup(_ titles: [String], selected: Int = 0) -> NSPopUpButton {
        let p = NSPopUpButton(frame: .zero, pullsDown: false)
        p.addItems(withTitles: titles)
        if selected < titles.count { p.selectItem(at: selected) }
        return p
    }

    func field(_ text: String, width: CGFloat = 160, placeholder: String? = nil) -> NSTextField {
        let f = NSTextField(string: text)
        f.placeholderString = placeholder
        f.translatesAutoresizingMaskIntoConstraints = false
        f.widthAnchor.constraint(equalToConstant: width).isActive = true
        return f
    }

    func checkbox(_ title: String, _ on: Bool) -> NSButton {
        let b = NSButton(checkboxWithTitle: title, target: nil, action: nil)
        b.state = on ? .on : .off
        return b
    }

    /// Alert with a grid of rows as accessory; returns the button pressed.
    func runDialog(_ title: String, info: String? = nil, ok: String, extra: String? = nil, rows: [[NSView]]) -> NSApplication.ModalResponse {
        let alert = NSAlert()
        alert.messageText = title
        if let info { alert.informativeText = info }
        alert.addButton(withTitle: ok)
        alert.addButton(withTitle: L("Cancel"))
        if let extra { alert.addButton(withTitle: extra) }
        let width = rows.map(\.count).max() ?? 1
        let grid = NSGridView(views: rows.map { $0 + Array(repeating: NSGridCell.emptyContentView, count: width - $0.count) })
        grid.rowSpacing = 7
        grid.columnSpacing = 8
        grid.column(at: 0).xPlacement = .trailing
        grid.frame.size = grid.fittingSize
        alert.accessoryView = grid
        return alert.runModal()
    }

    func label(_ s: String) -> NSTextField { NSTextField(labelWithString: s) }

    func hstack(_ views: [NSView]) -> NSStackView {
        let s = NSStackView(views: views)
        s.spacing = 8
        return s
    }

    func report(_ text: String) {
        lastAlertMessage = text
        guard !suppressAlerts else { return }
        let a = NSAlert()
        a.messageText = text
        a.runModal()
    }

    // MARK: - Sort

    @objc func sortDialogAction(_ sender: Any?) {
        guard grid.commitEdit(move: nil) else { return }
        let range = currentRegion()
        guard guardCellOperation(range) else { return }
        let header = checkbox(L("Range contains column labels"), DataTools.detectHeader(sheet, range))
        let direction = popup([L("Top to bottom (sort rows)"), L("Left to right (sort columns)")])
        let lists = DataTools.builtinLists + DataTools.userLists
        let order = popup([L("None")] + lists.map { $0.prefix(3).joined(separator: ", ") + ", …" })
        let caseSensitive = checkbox(L("Case sensitive"), false)
        let natural = checkbox(L("Natural sort (2 before 10)"), false)
        var keys: [(NSPopUpButton, NSPopUpButton)] = []
        func titles() -> [String] {
            direction.indexOfSelectedItem == 1
                ? range.rows.map { r in header.state == .on ? NumberFormat.format(sheet[CellRef(row: r, col: range.start.col)].value, "General").text : L("Row %d", r + 1) }
                : columnTitles(range, header: header.state == .on)
        }
        for k in 0..<3 {
            let key = popup((k == 0 ? [] : [L("(none)")]) + titles(), selected: k == 0 ? max(0, grid.active.col - range.start.col) : 0)
            keys.append((key, popup([L("Ascending"), L("Descending")])))
        }
        let refresher = DialogActions { [weak self] in
            guard self != nil else { return }
            let t = titles()
            for (k, pair) in keys.enumerated() {
                let sel = pair.0.indexOfSelectedItem
                pair.0.removeAllItems()
                pair.0.addItems(withTitles: (k == 0 ? [] : [L("(none)")]) + t)
                pair.0.selectItem(at: min(max(0, sel), pair.0.numberOfItems - 1))
            }
        }
        for c in [header, direction] { c.target = refresher; c.action = #selector(DialogActions.changed(_:)) }
        for k in keys { k.0.widthAnchor.constraint(greaterThanOrEqualToConstant: 180).isActive = true }
        let rows: [[NSView]] = [
            [label(L("Sort key 1:")), hstack([keys[0].0, keys[0].1])], [label(L("Then by:")), hstack([keys[1].0, keys[1].1])],
            [label(L("Then by:")), hstack([keys[2].0, keys[2].1])],
            [label(L("Direction:")), direction], [label(L("Custom order:")), order],
            [NSGridCell.emptyContentView, header], [NSGridCell.emptyContentView, caseSensitive], [NSGridCell.emptyContentView, natural],
        ]
        guard runDialog(L("Sort"), info: L("Range %@", range.a1), ok: L("Sort"), rows: rows) == .alertFirstButtonReturn else { return }
        withExtendedLifetime(refresher) {}
        let leftToRight = direction.indexOfSelectedItem == 1
        var sortKeys: [DataTools.SortKey] = []
        for (k, pair) in keys.enumerated() {
            let i = pair.0.indexOfSelectedItem - (k == 0 ? 0 : 1)
            guard i >= 0 else { continue }
            sortKeys.append(.init(index: (leftToRight ? range.start.row : range.start.col) + i, ascending: pair.1.indexOfSelectedItem == 0))
        }
        let options = DataTools.SortOptions(header: header.state == .on, caseSensitive: caseSensitive.state == .on, natural: natural.state == .on,
                                            leftToRight: leftToRight, customList: order.indexOfSelectedItem > 0 ? lists[order.indexOfSelectedItem - 1] : nil)
        applySort(range, keys: sortKeys, options: options)
    }

    func applySort(_ range: CellRange, keys: [DataTools.SortKey], options: DataTools.SortOptions) {
        guard grid.commitEdit(move: nil), let doc, guardCellOperation(range), guardEditable(range) else { return }
        guard !sheet.arrays.values.contains(where: { $0.intersects(range) }) else { report(Self.partOfArrayMessage); return }
        doc.change(L("Sort")) { DataTools.sort(&$0, sheet: sheetIndex, range: range, keys: keys, options: options) }
    }

    /// Data › Sort Lists…: user orders for Sort (one list per line, items separated by commas).
    @objc func sortListsAction(_ sender: Any?) {
        let text = NSTextView(frame: NSRect(x: 0, y: 0, width: 360, height: 140))
        text.string = DataTools.userLists.map { $0.joined(separator: ", ") }.joined(separator: "\n")
        text.font = .systemFont(ofSize: 13)
        let scroll = NSScrollView(frame: NSRect(x: 0, y: 0, width: 360, height: 140))
        scroll.documentView = text
        scroll.hasVerticalScroller = true
        scroll.borderType = .bezelBorder
        let alert = NSAlert()
        alert.messageText = L("Sort Lists")
        alert.informativeText = L("One list per line, items separated by commas. Built-in lists: weekdays and months.")
        alert.addButton(withTitle: L("Save"))
        alert.addButton(withTitle: L("Cancel"))
        alert.accessoryView = scroll
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        DataTools.userLists = text.string.split(separator: "\n").map { $0.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) } }.filter { !$0.isEmpty }
    }

    // MARK: - Filters

    static let filterOps: [(FilterCondition.Op, String)] = [
        (.equal, "="), (.notEqual, "<>"), (.greaterThan, ">"), (.lessThan, "<"), (.greaterThanOrEqual, ">="), (.lessThanOrEqual, "<="),
        (.contains, L("Contains")), (.notContains, L("Does not contain")), (.beginsWith, L("Begins with")), (.endsWith, L("Ends with")),
        (.top, L("Largest")), (.bottom, L("Smallest")), (.topPercent, L("Largest %")), (.bottomPercent, L("Smallest %")),
        (.empty, L("Empty")), (.notEmpty, L("Not empty")),
    ]

    @objc func standardFilterAction(_ sender: Any?) {
        guard grid.commitEdit(move: nil) else { return }
        let range = sheet.autoFilter ?? currentRegion()
        guard guardCellOperation(range) else { return }
        let header = checkbox(L("Range contains column labels"), true)
        let titles = columnTitles(range, header: true)
        var rows: [[NSView]] = []
        var lines: [(NSPopUpButton?, NSPopUpButton, NSPopUpButton, NSComboBox)] = []
        for k in 0..<4 {
            let connector = k == 0 ? nil : popup([L("—"), L("AND"), L("OR")])
            let f = popup((k == 0 ? [] : [L("(none)")]) + titles, selected: k == 0 ? max(0, grid.active.col - range.start.col) : 0)
            let op = popup(Self.filterOps.map(\.1))
            let value = NSComboBox()
            value.translatesAutoresizingMaskIntoConstraints = false
            value.widthAnchor.constraint(equalToConstant: 160).isActive = true
            value.addItems(withObjectValues: Array(Set(range.rows.dropFirst().map { NumberFormat.format(sheet[CellRef(row: $0, col: range.start.col)].value, "General").text })).sorted().prefix(200).map { $0 })
            lines.append((connector, f, op, value))
            f.widthAnchor.constraint(greaterThanOrEqualToConstant: 150).isActive = true
            rows.append([connector ?? label(L("Field:")), hstack([f, op, value])])
        }
        let caseSensitive = checkbox(L("Case sensitive"), false)
        let unique = checkbox(L("No duplications"), false)
        let copy = checkbox(L("Copy results to:"), false)
        let copyTo = field("", width: 120, placeholder: "$H$1")
        rows += [[NSGridCell.emptyContentView, header], [NSGridCell.emptyContentView, caseSensitive], [NSGridCell.emptyContentView, unique], [copy, copyTo]]
        guard runDialog(L("Standard Filter"), info: L("Range %@", range.a1), ok: L("Filter"), rows: rows) == .alertFirstButtonReturn else { return }
        var criteria: [DataTools.Criterion] = []
        for (k, line) in lines.enumerated() {
            let fi = line.1.indexOfSelectedItem - (k == 0 ? 0 : 1)
            guard fi >= 0, k == 0 || (line.0?.indexOfSelectedItem ?? 0) > 0 || criteria.isEmpty else { continue }
            let op = Self.filterOps[max(0, line.2.indexOfSelectedItem)].0
            criteria.append(.init(field: range.start.col + fi, condition: .init(op: op, value: line.3.stringValue), or: line.0?.indexOfSelectedItem == 2))
        }
        var target: (sheet: Int, ref: CellRef)?
        if copy.state == .on {
            guard let t = parseRange(copyTo.stringValue) else { NSSound.beep(); return }
            target = (t.sheet, t.range.start)
        }
        applyStandardFilter(range, criteria: criteria, caseSensitive: caseSensitive.state == .on, unique: unique.state == .on,
                            header: header.state == .on, copyTo: target)
    }

    func applyStandardFilter(_ range: CellRange, criteria: [DataTools.Criterion], caseSensitive: Bool, unique: Bool, header: Bool,
                             copyTo: (sheet: Int, ref: CellRef)?) {
        guard grid.commitEdit(move: nil), let doc, guardCellOperation(range) else { return }
        var output = Set<CellRef>()
        if let copyTo {
            guard let target = pasteTarget(rows: range.rows.count, cols: range.cols.count, at: copyTo.ref) else { return }
            output = Set(target.cells)
            guard guardDataWrites(output, sheet: copyTo.sheet) else { return }
        } else if !guardEditable(nil) { return }
        doc.change(L("Standard Filter"), recalc: copyTo != nil) { wb in
            if let copyTo { dropArrays(replacedBy: output, of: &wb, sheet: copyTo.sheet) }
            DataTools.standardFilter(&wb, sheet: sheetIndex, range: range, criteria: criteria, caseSensitive: caseSensitive,
                                     unique: unique, header: header, copyTo: copyTo)
        }
    }

    @objc func advancedFilterAction(_ sender: Any?) {
        guard grid.commitEdit(move: nil) else { return }
        let range = currentRegion()
        guard guardCellOperation(range) else { return }
        let criteria = field("", width: 180, placeholder: "$H$1:$I$3")
        let copy = checkbox(L("Copy results to:"), false)
        let copyTo = field("", width: 180, placeholder: "$K$1")
        let unique = checkbox(L("No duplications"), false)
        let rows: [[NSView]] = [[label(L("Criteria range:")), criteria], [copy, copyTo], [NSGridCell.emptyContentView, unique]]
        guard runDialog(L("Advanced Filter"), info: L("List range %@. The criteria range has the column labels in its first row; conditions in a row must all hold, rows are alternatives.", range.a1),
                        ok: L("Filter"), rows: rows) == .alertFirstButtonReturn else { return }
        guard let crit = parseRange(criteria.stringValue) else { NSSound.beep(); return }
        var target: (sheet: Int, ref: CellRef)?
        if copy.state == .on {
            guard let t = parseRange(copyTo.stringValue) else { NSSound.beep(); return }
            target = (t.sheet, t.range.start)
        }
        if !applyAdvancedFilter(range, criteria: crit, unique: unique.state == .on, copyTo: target) {
            report(L("The criteria range must have the column labels of the list in its first row."))
        }
    }

    @discardableResult
    func applyAdvancedFilter(_ range: CellRange, criteria: (sheet: Int, range: CellRange), unique: Bool, copyTo: (sheet: Int, ref: CellRef)?) -> Bool {
        guard grid.commitEdit(move: nil), let doc, guardCellOperation(range), guardCellOperation(criteria.range),
              doc.workbook.sheets.indices.contains(criteria.sheet) else { return false }
        var output = Set<CellRef>()
        if let copyTo {
            guard let target = pasteTarget(rows: range.rows.count, cols: range.cols.count, at: copyTo.ref) else { return false }
            output = Set(target.cells)
            guard guardDataWrites(output, sheet: copyTo.sheet) else { return false }
        } else if !guardEditable(nil) { return false }
        var test = doc.workbook
        if let copyTo { dropArrays(replacedBy: output, of: &test, sheet: copyTo.sheet) }
        guard DataTools.advancedFilter(&test, sheet: sheetIndex, list: range, criteria: criteria, unique: unique, copyTo: copyTo) else { return false }
        doc.change(L("Advanced Filter"), recalc: copyTo != nil) { $0 = test }
        return true
    }

    @objc func resetFilterAction(_ sender: Any?) {
        doc?.change(L("Reset Filter"), recalc: false) { wb in
            wb.sheets[sheetIndex].filterValues = [:]
            wb.sheets[sheetIndex].customFilters = [:]
            wb.sheets[sheetIndex].filterHidden = []
        }
    }

    // MARK: - Subtotals

    @objc func subtotalsAction(_ sender: Any?) {
        guard grid.commitEdit(move: nil) else { return }
        let range = currentRegion()
        guard guardCellOperation(range) else { return }
        let titles = columnTitles(range, header: true)
        let groupBy = popup(titles)
        let function = popup(DataTools.subtotalFunctions.map { L($0.name) })
        let checks = titles.enumerated().map { checkbox($0.element, $0.offset == titles.count - 1) }
        let columns = NSStackView(views: checks)
        columns.orientation = .vertical
        columns.alignment = .leading
        columns.spacing = 3
        let scroll = NSScrollView(frame: NSRect(x: 0, y: 0, width: 220, height: min(160, CGFloat(checks.count) * 20 + 8)))
        let doc = FlippedView(frame: NSRect(origin: .zero, size: NSSize(width: 200, height: CGFloat(checks.count) * 20 + 8)))
        columns.frame = doc.bounds
        doc.addSubview(columns)
        scroll.documentView = doc
        scroll.hasVerticalScroller = true
        scroll.borderType = .bezelBorder
        scroll.translatesAutoresizingMaskIntoConstraints = false
        scroll.widthAnchor.constraint(equalToConstant: 220).isActive = true
        scroll.heightAnchor.constraint(equalToConstant: scroll.frame.height).isActive = true
        let replace = checkbox(L("Replace current subtotals"), true)
        let breaks = checkbox(L("Page break between groups"), false)
        let rows: [[NSView]] = [[label(L("Group by:")), groupBy], [label(L("Calculate:")), function], [label(L("Subtotal for:")), scroll],
                                [NSGridCell.emptyContentView, replace], [NSGridCell.emptyContentView, breaks]]
        switch runDialog(L("Subtotals"), info: L("Range %@ (sorted by the group column)", range.a1), ok: L("OK"), extra: L("Remove All"), rows: rows) {
        case .alertFirstButtonReturn:
            let cols = checks.enumerated().filter { $0.element.state == .on }.map { range.start.col + $0.offset }
            guard !cols.isEmpty else { NSSound.beep(); return }
            applySubtotals(range, groupBy: range.start.col + groupBy.indexOfSelectedItem, function: DataTools.subtotalFunctions[function.indexOfSelectedItem].code,
                           columns: cols, replace: replace.state == .on, pageBreaks: breaks.state == .on)
        case .alertThirdButtonReturn:
            removeSubtotals(range)
        default: break
        }
    }

    func applySubtotals(_ range: CellRange, groupBy: Int, function: Int, columns: [Int], replace: Bool, pageBreaks: Bool) {
        guard let doc, guardCellOperation(range), guardEditable(nil) else { return }
        var result = range
        doc.change(L("Subtotals")) { wb in
            result = DataTools.subtotals(&wb, sheet: sheetIndex, range: range, groupBy: groupBy, function: function, columns: columns,
                                         replace: replace, pageBreaks: pageBreaks)
        }
        updateOutlineGutters()
        grid.select(result.start)
        grid.select(result.end, extend: true)
    }

    func removeSubtotals(_ range: CellRange) {
        guard let doc, guardEditable(nil) else { return }
        doc.change(L("Remove Subtotals")) { _ = DataTools.removeSubtotals(&$0, sheet: sheetIndex, range: range) }
        updateOutlineGutters()
    }

    // MARK: - Remove duplicates, text to columns

    @objc func removeDuplicatesAction(_ sender: Any?) {
        guard grid.commitEdit(move: nil) else { return }
        let range = currentRegion()
        guard guardCellOperation(range) else { return }
        let header = checkbox(L("Data has headers"), DataTools.detectHeader(sheet, range))
        let checks = columnTitles(range, header: true).map { checkbox($0, true) }
        let stack = NSStackView(views: checks)
        stack.orientation = .vertical
        stack.alignment = .leading
        let rows: [[NSView]] = [[label(L("Columns:")), stack], [NSGridCell.emptyContentView, header]]
        guard runDialog(L("Remove Duplicates"), info: L("Range %@", range.a1), ok: L("Remove Duplicates"), rows: rows) == .alertFirstButtonReturn else { return }
        let cols = checks.enumerated().filter { $0.element.state == .on }.map { range.start.col + $0.offset }
        guard !cols.isEmpty else { NSSound.beep(); return }
        let n = applyRemoveDuplicates(range, columns: cols, header: header.state == .on)
        report(n == 0 ? L("No duplicate values found.") : L("%d duplicate rows removed.", n))
    }

    @discardableResult
    func applyRemoveDuplicates(_ range: CellRange, columns: [Int], header: Bool) -> Int {
        guard grid.commitEdit(move: nil), let doc, guardCellOperation(range), guardEditable(range) else { return 0 }
        guard !sheet.arrays.values.contains(where: { $0.intersects(range) }) else { report(Self.partOfArrayMessage); return 0 }
        var test = doc.workbook
        let n = DataTools.removeDuplicates(&test, sheet: sheetIndex, range: range, columns: columns, header: header)
        if n > 0 { doc.change(L("Remove Duplicates")) { $0 = test } }
        return n
    }

    @objc func textToColumnsAction(_ sender: Any?) {
        guard grid.commitEdit(move: nil) else { return }
        var range = clampedSelection
        range = CellRange(range.start, CellRef(row: range.end.row, col: range.start.col))
        if range.count == 1, let used = sheet.usedRange {
            // The column down to the last filled cell.
            var end = range.start.row
            while end < used.end.row, !sheet[CellRef(row: end + 1, col: range.start.col)].value.isEmpty { end += 1 }
            range = CellRange(range.start, CellRef(row: end, col: range.start.col))
        }
        let mode = popup([L("Delimited"), L("Fixed width")])
        let tab = checkbox(L("Tab"), true), comma = checkbox(L("Comma"), false), semicolon = checkbox(L("Semicolon"), true)
        let space = checkbox(L("Space"), false), other = checkbox(L("Other:"), false)
        let otherText = field("", width: 40)
        let merge = checkbox(L("Merge delimiters"), false)
        let quote = popup(["\"", "'", L("None")])
        let widths = field("", width: 160, placeholder: L("e.g. 5, 12"))
        let types = (0..<5).map { _ -> NSPopUpButton in
            let p = popup([L("Standard"), L("Text"), L("Date (DMY)"), L("Date (MDY)"), L("Date (YMD)"), L("Skip")])
            p.controlSize = .small
            p.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
            return p
        }
        let typeRow = NSStackView(views: types)
        typeRow.spacing = 4
        let delimiters = NSStackView(views: [tab, comma, semicolon, space, other, otherText])
        delimiters.spacing = 8
        let rows: [[NSView]] = [[label(L("Type:")), mode], [label(L("Separated by:")), delimiters], [NSGridCell.emptyContentView, merge],
                                [label(L("Text delimiter:")), quote], [label(L("Column starts:")), widths], [label(L("Column types:")), typeRow]]
        guard runDialog(L("Text to Columns"), info: L("Splits %@ into columns to the right.", range.a1), ok: L("OK"), rows: rows) == .alertFirstButtonReturn else { return }
        var o = DataTools.SplitOptions()
        o.delimiters = []
        if tab.state == .on { o.delimiters.insert("\t") }
        if comma.state == .on { o.delimiters.insert(",") }
        if semicolon.state == .on { o.delimiters.insert(";") }
        if space.state == .on { o.delimiters.insert(" ") }
        if other.state == .on { o.delimiters.formUnion(otherText.stringValue) }
        o.consecutiveAsOne = merge.state == .on
        o.quote = quote.indexOfSelectedItem == 0 ? "\"" : quote.indexOfSelectedItem == 1 ? "'" : nil
        if mode.indexOfSelectedItem == 1 { o.fixedBreaks = widths.stringValue.split(whereSeparator: { ",; ".contains($0) }).compactMap { Int($0) } }
        o.types = types.map { DataTools.ColumnType(rawValue: $0.indexOfSelectedItem) ?? .general }
        applyTextToColumns(range, options: o)
    }

    func applyTextToColumns(_ range: CellRange, options: DataTools.SplitOptions) {
        guard grid.commitEdit(move: nil), let doc, guardCellOperation(range) else { return }
        let calc = Calculator(doc.workbook)
        var output = Set<CellRef>()
        for row in range.rows {
            let source = sheet[CellRef(row: row, col: range.start.col)]
            guard !source.isFormula else { continue }
            let text = calc.text(source.value)
            guard !text.isEmpty else { continue }
            let parts = DataTools.split(text, options)
            let count = parts.indices.filter { $0 >= options.types.count || options.types[$0] != .skip }.count
            guard count > 0 else { continue }
            guard let target = pasteTarget(rows: 1, cols: count, at: CellRef(row: row, col: range.start.col)),
                  output.count <= Self.maximumCellOperationCount - count else {
                if output.count > Self.maximumCellOperationCount - count { report(L("This operation supports up to %d cells. Select a smaller range.", Self.maximumCellOperationCount)) }
                return
            }
            output.formUnion(target.cells)
        }
        guard !output.isEmpty, guardDataWrites(output, sheet: sheetIndex) else { return }
        doc.change(L("Text to Columns")) { wb in
            dropArrays(replacedBy: output, of: &wb)
            DataTools.textToColumns(&wb, sheet: sheetIndex, range: range, options: options)
        }
    }

    /// Data commands may copy to another sheet or write a ragged set of columns.
    private func guardDataWrites(_ refs: Set<CellRef>, sheet si: Int) -> Bool {
        guard let doc, doc.workbook.sheets.indices.contains(si) else { return false }
        let target = doc.workbook.sheets[si]
        if target.isProtected, refs.contains(where: { target[$0].style.locked }) { refuseProtected(); return false }
        return guardArrayEdits(refs, in: target)
    }

    // MARK: - Fill

    @objc func fillUpAction(_ sender: Any?) { applyFill(clampedSelection, .up) }
    @objc func fillLeftAction(_ sender: Any?) { applyFill(clampedSelection, .left) }

    func applyFill(_ range: CellRange, _ direction: DataTools.Direction) {
        guard grid.commitEdit(move: nil), let doc, guardCellOperation(range) else { return }
        let vertical = direction == .down || direction == .up
        guard vertical ? range.rows.count > 1 : range.cols.count > 1 else { return }
        let forward = direction == .down || direction == .right
        let target: CellRange
        if vertical {
            target = forward ? CellRange(CellRef(row: range.start.row + 1, col: range.start.col), range.end)
                : CellRange(range.start, CellRef(row: range.end.row - 1, col: range.end.col))
        } else {
            target = forward ? CellRange(CellRef(row: range.start.row, col: range.start.col + 1), range.end)
                : CellRange(range.start, CellRef(row: range.end.row, col: range.end.col - 1))
        }
        guard guardEditable(target), guardArrays(target) else { return }
        let names: [DataTools.Direction: String] = [.down: L("Fill Down"), .right: L("Fill Right"), .up: L("Fill Up"), .left: L("Fill Left")]
        doc.change(names[direction] ?? L("Fill")) { wb in
            let original = wb.sheets[sheetIndex]
            dropArrays(in: target, of: &wb)
            for ref in target.cells {
                let source = vertical ? CellRef(row: forward ? range.start.row : range.end.row, col: ref.col)
                    : CellRef(row: ref.row, col: forward ? range.start.col : range.end.col)
                var cell = original[source]
                if original.array(containing: source) != nil || (cell.input.isEmpty && !cell.value.isEmpty) {
                    cell.input = XLSX.inputText(for: cell.value, style: cell.style, date1904: wb.date1904)
                } else if cell.isFormula { cell.input = Formula.shifted(cell.input, rows: ref.row - source.row, cols: ref.col - source.col) }
                cell.array = false
                wb.sheets[sheetIndex][ref] = cell
            }
        }
    }

    @objc func fillSeriesAction(_ sender: Any?) {
        guard grid.commitEdit(move: nil) else { return }
        let range = clampedSelection
        guard guardCellOperation(range) else { return }
        let direction = popup([L("Down"), L("Right"), L("Up"), L("Left")], selected: range.cols.count > range.rows.count ? 1 : 0)
        let type = popup([L("Linear"), L("Growth"), L("Date"), L("AutoFill"), L("Serial Numbers")])
        type.identifier = NSUserInterfaceItemIdentifier("fill-series-type")
        let unit = popup([L("Day"), L("Weekday"), L("Month"), L("Year")])
        let step = field("1", width: 100)
        let stop = field("", width: 100)
        let rows: [[NSView]] = [[label(L("Direction:")), direction], [label(L("Series type:")), type], [label(L("Time unit:")), unit],
                                [label(L("Increment:")), step], [label(L("End value:")), stop]]
        guard runDialog(L("Fill Series"), info: L("Range %@", range.a1), ok: L("OK"), rows: rows) == .alertFirstButtonReturn else { return }
        if type.indexOfSelectedItem == 4 { showSerialSeriesDialog(reverse: direction.indexOfSelectedItem >= 2); return }
        let parse: (String) -> Double? = { s in Double(s.replacingOccurrences(of: ",", with: ".")) ?? DateMath.parse(s) }
        guard let inc = parse(step.stringValue) else { NSSound.beep(); return }
        let t = DataTools.SeriesType(rawValue: type.indexOfSelectedItem) ?? .linear
        if t == .autofill {
            // AutoFill: continue the pattern of the first cells, like dragging the fill handle.
            let dir = DataTools.Direction(rawValue: direction.indexOfSelectedItem) ?? .down
            let source = dir == .down ? CellRange(range.start, CellRef(row: range.start.row, col: range.end.col))
                : dir == .right ? CellRange(range.start, CellRef(row: range.end.row, col: range.start.col))
                : dir == .up ? CellRange(CellRef(row: range.end.row, col: range.start.col), range.end)
                : CellRange(CellRef(row: range.start.row, col: range.end.col), range.end)
            autoFill(from: source, to: range)
            return
        }
        applySeries(range, direction: DataTools.Direction(rawValue: direction.indexOfSelectedItem) ?? .down, type: t,
                    unit: DataTools.DateUnit(rawValue: unit.indexOfSelectedItem) ?? .day, step: inc, stop: parse(stop.stringValue))
    }

    func applySeries(_ range: CellRange, direction: DataTools.Direction, type: DataTools.SeriesType, unit: DataTools.DateUnit, step: Double, stop: Double?) {
        guard grid.commitEdit(move: nil), let doc, guardCellOperation(range), step.isFinite, stop?.isFinite != false else { return }
        // Calculate the intended writes first: a stop value can shorten the destination.
        var proposed = doc.workbook
        DataTools.fillSeries(&proposed, sheet: sheetIndex, range: range, direction: direction, type: type, unit: unit, step: step, stop: stop)
        let writes = Set(range.cells.filter { proposed.sheets[sheetIndex][$0] != sheet[$0] })
        guard !writes.isEmpty, guardArrayEdits(writes) else { return }
        guard writes.allSatisfy({ !sheet.isProtected || !sheet[$0].style.locked }) else { refuseProtected(); return }
        doc.change(L("Fill Series")) { wb in
            dropArrays(replacedBy: writes, of: &wb)
            for ref in writes {
                var cell = proposed.sheets[sheetIndex][ref]
                cell.array = false
                wb.sheets[sheetIndex][ref] = cell
            }
        }
    }

    // MARK: - Goal Seek, Solver

    @objc func goalSeekAction(_ sender: Any?) {
        guard grid.commitEdit(move: nil) else { return }
        let formula = field(sheet[grid.active].isFormula ? grid.active.a1 : "", width: 120)
        let target = field("", width: 120)
        let variable = field("", width: 120)
        let rows: [[NSView]] = [[label(L("Formula cell:")), formula], [label(L("Target value:")), target], [label(L("Variable cell:")), variable]]
        guard runDialog(L("Goal Seek"), ok: L("OK"), rows: rows) == .alertFirstButtonReturn else { return }
        guard let f = CellRef(formula.stringValue), let v = CellRef(variable.stringValue),
              let t = Double(target.stringValue.replacingOccurrences(of: ",", with: ".")) else { NSSound.beep(); return }
        guard let x = goalSeek(formula: f, target: t, variable: v) else {
            report(L("Goal Seek could not find a value for %@ that gives %@.", v.a1, NumberFormat.general(t)))
            return
        }
        guard !suppressAlerts else { return }
        let alert = NSAlert()
        alert.messageText = L("Goal Seek found a solution")
        alert.informativeText = L("%@ = %@. Keep the result?", v.a1, NumberFormat.general(x))
        alert.addButton(withTitle: L("Keep"))
        alert.addButton(withTitle: L("Undo"))
        if alert.runModal() != .alertFirstButtonReturn { doc?.undoManager?.undo() }
    }

    /// Sets `variable` so that `formula` gives `target`; nil (and nothing changed) when there's no solution.
    func goalSeek(formula: CellRef, target: Double, variable: CellRef) -> Double? {
        guard let doc, sheet[formula].isFormula, !sheet[variable].isFormula, guardEditable(CellRange(variable, variable)) else { return nil }
        guard let x = DataTools.goalSeek(doc.workbook, sheet: sheetIndex, formula: formula, target: target, variable: variable) else { return nil }
        setVariables([variable], [x], name: L("Goal Seek"))
        return x
    }

    func setVariables(_ refs: [CellRef], _ values: [Double], name: String, solver: SolverModel? = nil) {
        doc?.change(name) { wb in
            if let solver { wb.sheets[sheetIndex].solver = solver }
            for (ref, x) in zip(refs, values) {
                var c = wb.sheets[sheetIndex][ref]
                let v = abs(x - x.rounded()) < 1e-9 * max(1, abs(x)) ? x.rounded() : x
                c.value = .number(v)
                c.input = Value.formatGeneral(v)
                wb.sheets[sheetIndex][ref] = c
            }
        }
    }

    @objc func solverAction(_ sender: Any?) {
        guard grid.commitEdit(move: nil) else { return }
        // The sheet's saved model (from the file or the last run) fills the dialog, as in Excel and Calc.
        let saved = sheet.solver
        func absolute(_ r: CellRange) -> String {
            let a = "$\(CellRef.columnName(r.start.col))$\(r.start.row + 1)"
            return r.count == 1 ? a : a + ":$\(CellRef.columnName(r.end.col))$\(r.end.row + 1)"
        }
        let objective = field(saved.map { absolute(CellRange($0.objective, $0.objective)) } ?? (sheet[grid.active].isFormula ? grid.active.a1 : ""), width: 120)
        var goalIndex = 0, goalText = ""
        switch saved?.goal {
        case .min?: goalIndex = 1
        case .value(let v)?: goalIndex = 2; goalText = NumberFormat.general(v)
        default: break
        }
        let goal = popup([L("Maximum"), L("Minimum"), L("Value of")], selected: goalIndex)
        let goalValue = field(goalText, width: 80)
        let variables = field(saved.map { ODS.ranges($0.variables).map(absolute).joined(separator: ";") } ?? "", width: 160, placeholder: "$B$1:$B$3")
        let ops = SolverModel.Op.allCases
        let savedConstraints = saved?.constraints ?? []
        let constraints = (0..<max(5, min(30, savedConstraints.count + 1))).map { k -> (NSTextField, NSPopUpButton, NSTextField) in
            let c = k < savedConstraints.count ? savedConstraints[k] : nil
            return (field(c.map { absolute($0.lhs) } ?? "", width: 110, placeholder: L("Cell")),
                    popup(ops.map { $0 == .int ? L("Integer") : $0 == .bin ? L("Binary") : $0.rawValue }, selected: c.flatMap { ops.firstIndex(of: $0.op) } ?? 0),
                    field(c?.rhs ?? "", width: 110, placeholder: L("Value")))
        }
        let nonNegative = checkbox(L("Assume non-negative variables"), saved?.nonNegative ?? true)
        let engine = popup([L("Linear (simplex)"), L("Nonlinear (Nelder–Mead)")], selected: saved?.linear == false ? 1 : 0)
        var rows: [[NSView]] = [[label(L("Target cell:")), objective], [label(L("Optimize result to:")), hstack([goal, goalValue])],
                                [label(L("By changing cells:")), variables]]
        for (k, c) in constraints.enumerated() { rows.append([label(k == 0 ? L("Limiting conditions:") : ""), hstack([c.0, c.1, c.2])]) }
        rows += [[NSGridCell.emptyContentView, nonNegative], [label(L("Solving method:")), engine]]
        guard runDialog(L("Solver"), ok: L("Solve"), rows: rows) == .alertFirstButtonReturn else { return }
        let varRanges = ODS.splitArguments(variables.stringValue).map { parseRange($0)?.range }
        guard let obj = CellRef(objective.stringValue), !varRanges.isEmpty, !varRanges.contains(where: { $0 == nil }) else { NSSound.beep(); return }
        var model = SolverModel(objective: obj, variables: varRanges.compactMap { $0 }.flatMap(\.cells))
        model.extra = saved?.extra ?? [:]
        switch goal.indexOfSelectedItem {
        case 1: model.goal = .min
        case 2: model.goal = .value(Double(goalValue.stringValue.replacingOccurrences(of: ",", with: ".")) ?? 0)
        default: model.goal = .max
        }
        for c in constraints where !c.0.stringValue.isEmpty {
            guard let lhs = parseRange(c.0.stringValue)?.range else { NSSound.beep(); return }
            model.constraints.append(.init(lhs: lhs, op: ops[c.1.indexOfSelectedItem], rhs: c.2.stringValue))
        }
        model.nonNegative = nonNegative.state == .on
        model.linear = engine.indexOfSelectedItem == 0
        guard let result = solve(model) else { report(L("The model can't be solved: check the cells and conditions.")); return }
        guard !suppressAlerts else { return }
        let alert = NSAlert()
        switch result.status {
        case .optimal, .approximate:
            alert.messageText = L("Solver found a solution")
            alert.informativeText = L("Result: %@", NumberFormat.general(result.objective)) + "\n"
                + zip(model.variables, result.values).map { "\($0.a1) = \(NumberFormat.general($1))" }.joined(separator: "\n")
            alert.addButton(withTitle: L("Keep"))
            alert.addButton(withTitle: L("Undo"))
            if alert.runModal() != .alertFirstButtonReturn {
                // Original values back; the model stays with the sheet (Excel's Restore Original Values).
                doc?.undoManager?.undo()
                if let doc, sheetIndex < doc.workbook.sheets.count { doc.workbook.sheets[sheetIndex].solver = model }
            }
        case .infeasible: report(L("No solution satisfies all the conditions."))
        case .unbounded: report(L("The result can grow without limit (unbounded)."))
        case .failed: report(L("The model can't be solved: check the cells and conditions."))
        }
    }

    /// Solves the model and writes the variable values (one undo step); nil when the model is invalid.
    /// The model is kept with the sheet (and saved in the file) either way.
    func solve(_ model: SolverModel) -> SolverResult? {
        guard let doc, guardEditable(nil) else { return nil }
        let result = Solver.solve(doc.workbook, sheet: sheetIndex, model: model)
        if let result, result.status == .optimal || result.status == .approximate {
            setVariables(model.variables, result.values, name: L("Solver"), solver: model)
        } else if sheet.solver != model {
            doc.change(L("Solver"), recalc: false) { $0.sheets[sheetIndex].solver = model }
        }
        return result
    }

    // MARK: - Statistics

    @objc func descriptiveStatisticsAction(_ sender: Any?) { statisticsDialog(correlation: false) }
    @objc func correlationAction(_ sender: Any?) { statisticsDialog(correlation: true) }

    private func statisticsDialog(correlation: Bool) {
        guard grid.commitEdit(move: nil) else { return }
        let range = currentRegion()
        let input = field(Formula.sheetPrefix(sheet.name) + range.a1, width: 200)
        let grouped = popup([L("Columns"), L("Rows")])
        let labels = checkbox(L("Labels in first row/column"), DataTools.detectHeader(sheet, range))
        let rows: [[NSView]] = [[label(L("Input range:")), input], [label(L("Grouped by:")), grouped], [NSGridCell.emptyContentView, labels]]
        guard runDialog(correlation ? L("Correlation") : L("Descriptive Statistics"), info: L("The results go to a new sheet."), ok: L("OK"), rows: rows) == .alertFirstButtonReturn,
              let source = parseRange(input.stringValue) else { return }
        applyStatistics(source, byRows: grouped.indexOfSelectedItem == 1, labels: labels.state == .on, correlation: correlation)
    }

    /// Writes formulas (MEAN, STDEV…, or CORREL matrix) for each column/row of `source` on a new sheet.
    func applyStatistics(_ source: (sheet: Int, range: CellRange), byRows: Bool, labels: Bool, correlation: Bool) {
        guard let doc else { return }
        let src = doc.workbook.sheets[source.sheet]
        let r = source.range
        let groups = byRows ? Array(r.rows) : Array(r.cols)
        func data(_ g: Int) -> String {
            let range = byRows ? CellRange(CellRef(row: g, col: r.start.col + (labels ? 1 : 0)), CellRef(row: g, col: r.end.col))
                               : CellRange(CellRef(row: r.start.row + (labels ? 1 : 0), col: g), CellRef(row: r.end.row, col: g))
            return Formula.sheetPrefix(src.name) + "$" + CellRef.columnName(range.start.col) + "$\(range.start.row + 1):$" + CellRef.columnName(range.end.col) + "$\(range.end.row + 1)"
        }
        func title(_ g: Int) -> String {
            if labels {
                let ref = byRows ? CellRef(row: g, col: r.start.col) : CellRef(row: r.start.row, col: g)
                let t = NumberFormat.format(src[ref].value, "General").text
                if !t.isEmpty { return t }
            }
            return byRows ? L("Row %d", g + 1) : L("Column %@", CellRef.columnName(g))
        }
        var out = Sheet(name: "")
        func put(_ row: Int, _ col: Int, _ input: String, bold: Bool = false) {
            var c = Cell()
            c.input = input
            c.value = input.hasPrefix("=") ? .empty : XLSX.constant(input, date1904: doc.workbook.date1904)
            c.style.bold = bold
            out[CellRef(row: row, col: col)] = c
        }
        if correlation {
            for (i, g) in groups.enumerated() {
                put(0, i + 1, title(g), bold: true)
                put(i + 1, 0, title(g), bold: true)
                for (j, h) in groups.enumerated() where j <= i { put(i + 1, j + 1, "=CORREL(\(data(g)),\(data(h)))") }
            }
        } else {
            let stats: [(String, String)] = [
                ("Mean", "AVERAGE"), ("Standard Error", "SE"), ("Mode", "MODE"), ("Median", "MEDIAN"), ("Variance", "VAR"),
                ("Standard Deviation", "STDEV"), ("Kurtosis", "KURT"), ("Skewness", "SKEW"), ("Range", "RANGE"), ("Minimum", "MIN"),
                ("Maximum", "MAX"), ("Sum", "SUM"), ("Count", "COUNT"),
            ]
            for (k, s) in stats.enumerated() { put(k + 1, 0, L(s.0), bold: true) }
            for (i, g) in groups.enumerated() {
                put(0, i + 1, title(g), bold: true)
                let d = data(g)
                for (k, s) in stats.enumerated() {
                    let f: String
                    switch s.1 {
                    case "SE": f = "=STDEV(\(d))/SQRT(COUNT(\(d)))"
                    case "RANGE": f = "=MAX(\(d))-MIN(\(d))"
                    default: f = "=\(s.1)(\(d))"
                    }
                    put(k + 1, i + 1, f)
                }
            }
        }
        out.columnWidths[0] = 120
        var n = 1
        let base = correlation ? L("Correlation") : L("Statistics")
        var name = base
        while doc.workbook.sheetIndex(named: name) != nil { n += 1; name = "\(base) \(n)" }
        out.name = name
        doc.change(correlation ? L("Correlation") : L("Descriptive Statistics")) { $0.sheets.insert(out, at: sheetIndex + 1) }
        selectSheet(sheetIndex + 1)
    }
}

/// Target for dialog controls that refresh other controls (lives while the dialog runs).
final class DialogActions: NSObject {
    let handler: () -> Void
    init(_ handler: @escaping () -> Void) { self.handler = handler }
    @objc func changed(_ sender: Any?) { handler() }
}

final class FlippedView: NSView {
    override var isFlipped: Bool { true }
}
