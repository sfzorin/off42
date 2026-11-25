import AppKit
import Kit42

/// Static technical objects and label output use the current cell values as a snapshot.
/// They do not create a hidden link between a floating picture and a sortable row.
extension SheetWindowController {
    var selectedTechnicalElement: (index: Int, element: TechnicalElement)? {
        guard case .image(let index) = grid.selectedObject, sheet.images.indices.contains(index),
              var element = TechnicalElement.fromPNG(sheet.images[index].data) else { return nil }
        element.widthMM = Double(sheet.images[index].size.width) * 25.4 / 72
        return (index, element)
    }

    @objc func insertTechnicalElementAction(_ sender: Any?) {
        guard grid.commitEdit(move: nil), guardEditable(nil), let window else { return }
        let targetSheet = sheetIndex, anchor = grid.active
        let initialValue = labelText(at: anchor)
        if suppressAlerts, let element = (sender as? NSMenuItem)?.representedObject as? TechnicalElement {
            do { applyTechnicalElement(element, render: try element.render(), anchor: anchor) }
            catch { report(error.localizedDescription) }
            return
        }
        TechnicalElementDialog.present(in: window, initialValue: initialValue) { [weak self] element, render in
            guard let self, self.sheetIndex == targetSheet else { return }
            self.applyTechnicalElement(element, render: render, anchor: anchor)
        }
    }

    @objc func editTechnicalElementAction(_ sender: Any?) {
        guard grid.commitEdit(move: nil), guardEditable(nil), let selected = selectedTechnicalElement, let window else { return }
        let targetSheet = sheetIndex, original = sheet.images[selected.index]
        if suppressAlerts, let element = (sender as? NSMenuItem)?.representedObject as? TechnicalElement {
            do { applyTechnicalElement(element, render: try element.render(), replacing: selected.index) }
            catch { report(error.localizedDescription) }
            return
        }
        TechnicalElementDialog.present(in: window, initial: selected.element) { [weak self] element, render in
            guard let self, self.sheetIndex == targetSheet, self.sheet.images.indices.contains(selected.index),
                  self.sheet.images[selected.index].data == original.data else { return }
            self.applyTechnicalElement(element, render: render, replacing: selected.index)
        }
    }

    func applyTechnicalElement(_ element: TechnicalElement, render: TechnicalRender, replacing index: Int? = nil, anchor: CellRef? = nil) {
        guard let doc, guardEditable(nil), render.sizePoints.width > 0, render.sizePoints.height > 0 else { return }
        let si = sheetIndex
        if let index {
            guard sheet.images.indices.contains(index) else { return }
            if sheet.images[index].data == render.pngData && sheet.images[index].size == render.sizePoints { return }
            doc.change(L("Edit Symbol or Code"), recalc: false) { wb in
                wb.sheets[si].images[index].data = render.pngData
                wb.sheets[si].images[index].size = render.sizePoints
            }
            grid.selectedObject = .image(index)
        } else {
            let picture = SheetImage(anchor: anchor ?? grid.active, size: render.sizePoints, data: render.pngData)
            doc.change(L("Insert Symbol or Code"), recalc: false) { $0.sheets[si].images.append(picture) }
            grid.selectedObject = .image(sheet.images.count - 1)
        }
        grid.needsDisplay = true
    }

    func labelText(at ref: CellRef) -> String {
        let cell = sheet[ref]
        return NumberFormat.format(cell.value, cell.style.numberFormat, date1904: doc?.workbook.date1904 ?? false).text
    }

    /// The complete selected rectangle is bounded before allocating rows or cell references.
    func makeLabelSource() -> LabelSource? {
        let range = clampedSelection
        guard range.rows.count <= 5_000, range.cols.count <= 200, range.count <= 500_000 else {
            report(L("Select at most 5,000 rows, 200 columns and 500,000 cells for labels."))
            return nil
        }
        let rows = range.rows.map { row in range.cols.map { labelText(at: CellRef(row: row, col: $0)) } }
        return LabelSource(title: "\(sheet.name)!\(range.a1)", headers: range.cols.map { L("Column %@", CellRef.columnName($0)) },
                           rows: rows, rowNumbers: range.rows.map { $0 + 1 }, visible: range.rows.map { !sheet.isRowHidden($0) },
                           firstRowIsHeader: range.rows.count > 1)
    }

    @objc func createLabelsAction(_ sender: Any?) {
        guard grid.commitEdit(move: nil), let window, let source = makeLabelSource() else { return }
        if suppressAlerts { lastCreatedLabelSource = source; return }
        LabelSheetDialog.present(in: window, sources: [source])
    }

    func showSerialSeriesDialog(reverse: Bool = false) {
        let prefix = field(""), suffix = field(""), start = field("1"), step = field("1"), digits = field("6")
        for (name, control) in [("prefix", prefix), ("suffix", suffix), ("start", start), ("step", step), ("digits", digits)] {
            control.identifier = NSUserInterfaceItemIdentifier("serial-" + name)
        }
        guard runDialog(L("Serial Numbers"), info: L("Range %@", clampedSelection.a1), ok: L("Fill"), rows: [
            [label(L("Prefix:")), prefix], [label(L("Start:")), start], [label(L("Increment:")), step],
            [label(L("Digits:")), digits], [label(L("Suffix:")), suffix],
        ]) == .alertFirstButtonReturn else { return }
        guard let first = UInt64(start.stringValue), let increment = UInt64(step.stringValue), let width = Int(digits.stringValue) else {
            report(L("Enter whole numbers for the start, increment and digit count.")); return
        }
        applySerialNumbers(range: clampedSelection, start: first, step: increment, digits: width,
                           prefix: prefix.stringValue, suffix: suffix.stringValue, reverse: reverse)
    }

    /// Serial identifiers are literal text, including leading zeroes and a leading = or +.
    @discardableResult
    func applySerialNumbers(range: CellRange, start: UInt64, step: UInt64, digits: Int, prefix: String, suffix: String, reverse: Bool = false) -> Bool {
        guard grid.commitEdit(move: nil), let doc, guardCellOperation(range), guardEditable(range), guardArrays(range) else { return false }
        let maximum: UInt64 = 9_007_199_254_740_991
        guard range.count <= 5_000, range.rows.count == 1 || range.cols.count == 1,
              start <= maximum, step > 0, step <= maximum, (0...32).contains(digits), prefix.count + suffix.count <= 1_024 else {
            report(L("Select one row or column with at most 5,000 cells. Use a nonnegative start, a positive increment and 0–32 digits.")); return false
        }
        let extra = UInt64(range.count - 1).multipliedReportingOverflow(by: step)
        let end = start.addingReportingOverflow(extra.partialValue)
        guard !extra.overflow, !end.overflow, end.partialValue <= maximum else {
            report(L("The serial number range is too large.")); return false
        }
        let refs = reverse ? Array(range.cells.reversed()) : range.cells
        let values = refs.indices.map { i -> String in
            let number = String(start + UInt64(i) * step)
            return prefix + String(repeating: "0", count: max(0, digits - number.count)) + number + suffix
        }
        for (ref, value) in zip(refs, values) where !validateInput("'" + value, at: ref) { return false }
        let si = sheetIndex
        doc.change(L("Fill Serial Numbers")) { wb in
            dropArrays(replacedBy: Set(refs), of: &wb, sheet: si)
            for (ref, value) in zip(refs, values) {
                var cell = wb.sheets[si][ref]
                cell.input = "'" + value; cell.value = .string(value); cell.array = false; cell.style.numberFormat = "@"
                wb.sheets[si][ref] = cell
            }
        }
        return true
    }
}
