import AppKit
import Kit42
import PDFKit
import UniformTypeIdentifiers

/// Text field placed over a run of page text. Return applies, Esc cancels, clicking elsewhere applies.
final class InlineTextEditor: NSObject, NSTextFieldDelegate {
    let field = NSTextField()
    private var finish: ((String?) -> Void)?
    private let originalText: String
    var currentText: String { field.currentEditor()?.string ?? field.stringValue }
    var hasPendingChanges: Bool { finish != nil && currentText != originalText }

    init(frame: NSRect, text: String, font: NSFont, color: NSColor, in view: NSView, finish: @escaping (String?) -> Void) {
        self.finish = finish
        self.originalText = text
        super.init()
        field.frame = frame
        field.stringValue = text
        field.font = font
        field.textColor = color
        field.isBordered = false
        field.drawsBackground = true
        field.backgroundColor = .white
        field.focusRingType = .exterior
        field.usesSingleLineMode = true
        field.cell?.isScrollable = true
        field.delegate = self
        view.addSubview(field)
        view.window?.makeFirstResponder(field)
        field.currentEditor()?.selectAll(nil)
    }

    func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        if selector == #selector(NSResponder.insertNewline(_:)) { end(commit: true); return true }
        if selector == #selector(NSResponder.cancelOperation(_:)) { end(commit: false); return true }
        return false
    }

    func controlTextDidChange(_ obj: Notification) {
        // Grow with the text so the whole line stays visible.
        let width = field.attributedStringValue.size().width + 16
        if width > field.frame.width { field.frame.size.width = width }
    }

    func controlTextDidEndEditing(_ obj: Notification) { end(commit: true) }

    func end(commit: Bool) {
        guard let finish else { return }
        self.finish = nil
        let text = currentText
        field.removeFromSuperview()
        finish(commit ? text : nil)
    }
}

extension EditorWindowController {
    // MARK: - Edit page text

    /// Opens the in-place editor over a run of page text (the first line of `run`).
    func editPageText(_ run: PDFSelection, on page: PDFPage) {
        guard let line = run.selectionsByLine().first(where: { $0.pages.first === page }) ?? run.selectionsByLine().first else { return }
        let original = (line.string ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        let probe = EditedTextOverlay.make(replacing: line, on: page, with: original)
        beginInlineEdit(over: line.bounds(for: page), text: original, font: probe.textFont, color: probe.color, on: page) { [weak self] text in
            guard let self, let text, text != original else { return }
            let overlay = EditedTextOverlay.make(replacing: line, on: page, with: text)
            self.add(overlay, to: page, name: L("Edit Text"))
        }
    }

    /// Re-edits text replaced earlier (still an overlay until the document is saved).
    func editPageText(_ overlay: EditedTextOverlay, on page: PDFPage) {
        pdfView.selectAnnotation(nil)
        beginInlineEdit(over: overlay.bounds, text: overlay.text, font: overlay.textFont, color: overlay.color, on: page) { [weak self] text in
            guard let self, let text, text != overlay.text else { return }
            self.setEditedText(overlay, text, on: page)
        }
    }

    private func setEditedText(_ overlay: EditedTextOverlay, _ text: String, on page: PDFPage) {
        let old = overlay.text
        overlay.setText(text)
        pdfView.annotationsChanged(on: page)
        pdfView.refreshMarker()
        sidebar.thumbnails.refresh(page)
        undo?.registerUndo(withTarget: self) { $0.setEditedText(overlay, old, on: page) }
        undo?.setActionName(L("Edit Text"))
    }

    private func beginInlineEdit(over rect: CGRect, text: String, font: NSFont, color: NSColor, on page: PDFPage,
                                 finish: @escaping (String?) -> Void) {
        pdfView.inlineEditor?.end(commit: true)
        guard let documentView = pdfView.documentView else { return }
        let viewRect = pdfView.convert(rect, from: page)
        var frame = documentView.convert(viewRect, from: pdfView).insetBy(dx: -3, dy: -3)
        let scaled = NSFont(descriptor: font.fontDescriptor, size: max(6, font.pointSize * pdfView.scaleFactor)) ?? font
        let height = ceil(scaled.ascender - scaled.descender + scaled.leading) + 6
        frame.origin.y -= (height - frame.height) / 2
        frame.size.height = height
        frame.size.width = max(frame.width + 24, 140)
        pdfView.inlineEditor = InlineTextEditor(frame: frame, text: text, font: scaled, color: color, in: documentView) { [weak self] result in
            self?.pdfView.inlineEditor = nil
            finish(result)
            DispatchQueue.main.async { [weak self] in
                if let self, !(self.window?.firstResponder is NSTextView) { self.window?.makeFirstResponder(self.pdfView) }
            }
        }
    }

    // MARK: - Form fields

    /// Adds a form field drawn with a form tool (a click places a default-sized one).
    func createFormField(_ kind: PDFTools.FieldKind, in rect: CGRect, on page: PDFPage) {
        guard let pdf else { return }
        var r = rect
        if r.width < 6 || r.height < 6 {
            let size = kind == .checkBox ? CGSize(width: 14, height: 14) : CGSize(width: kind == .text ? 180 : 150, height: 22)
            r = CGRect(x: rect.minX, y: rect.minY - size.height, width: size.width, height: size.height)
        }
        if kind == .checkBox { let side = min(r.width, r.height); r.size = CGSize(width: side, height: side) }
        let base = kind == .text ? "Text" : kind == .checkBox ? "Checkbox" : "Dropdown"
        let nameField = NSTextField(string: PDFTools.uniqueFieldName(base, in: pdf))
        let optionsField = NSTextField(string: [L("Option 1"), L("Option 2"), L("Option 3")].joined(separator: ", "))
        var rows: [(String, NSView)] = [(L("Field name:"), nameField)]
        if kind == .choice { rows.append((L("Options:"), optionsField)) }
        let alert = NSAlert.form(L("New Form Field"), info: kind == .choice ? L("Separate the options with commas.") : nil,
                                 ok: L("Add"), rows: rows)
        alert.window.initialFirstResponder = nameField
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        var name = nameField.stringValue.trimmingCharacters(in: .whitespaces)
        if name.isEmpty || PDFTools.fields(pdf).contains(where: { $0.1.fieldName == name }) {
            name = PDFTools.uniqueFieldName(name.isEmpty ? base : name, in: pdf)
        }
        let options = optionsField.stringValue.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
        let field = PDFTools.makeField(kind, name: name, bounds: r, options: options.isEmpty ? [L("Option 1")] : options)
        add(field, to: page, name: L("Add Form Field"))
        pdfView.selectAnnotation(field, on: page)
    }

    // MARK: - Watermark and page numbers

    private static var lastWatermark = WatermarkSpec(text: L("CONFIDENTIAL"))
    private static var lastNumbers = PageNumberSpec()
    static let overlayColors: [(String, NSColor)] = [
        (L("Red"), .systemRed), (L("Grey"), .gray), (L("Blue"), .systemBlue), (L("Green"), .systemGreen), (L("Black"), .black),
    ]

    private func rangeField() -> NSTextField {
        let field = NSTextField()
        field.placeholderString = L("All pages, or e.g. 1-3, 5")
        return field
    }

    /// Page indices from a range field, or nil after telling the user the range is wrong.
    private func indices(from field: NSTextField) -> [Int]? {
        guard let pdf else { return nil }
        if let indices = PDFTools.pageIndices(field.stringValue, pageCount: pdf.pageCount), !indices.isEmpty { return indices }
        let alert = NSAlert()
        alert.messageText = L("Invalid page range")
        alert.informativeText = L("Use page numbers from 1 to %d, e.g. “1-3, 5, 8-”.", pdf.pageCount)
        alert.runModal()
        return nil
    }

    @objc func watermarkAction(_ sender: Any?) {
        guard let pdf else { return }
        var spec = Self.lastWatermark
        let text = NSTextField(string: spec.text)
        let size = NSTextField(string: "\(Int(spec.fontSize))")
        let color = NSPopUpButton(frame: .zero, pullsDown: false)
        color.addItems(withTitles: Self.overlayColors.map(\.0))
        color.selectItem(at: Self.overlayColors.firstIndex { $0.1 == spec.color } ?? 0)
        let opacity = NSSlider(value: Double(spec.opacity * 100), minValue: 5, maxValue: 100, target: nil, action: nil)
        opacity.translatesAutoresizingMaskIntoConstraints = false
        opacity.widthAnchor.constraint(equalToConstant: 240).isActive = true
        let rotation = NSTextField(string: "\(Int(spec.rotation))")
        let pages = rangeField()
        var image: CGImage?
        let imageLabel = NSTextField(labelWithString: L("None — text watermark"))
        imageLabel.textColor = .secondaryLabelColor
        let chooser = ClosureTarget {
            image = StampLibrary.pickImage(window: nil, askCleanup: false)
            imageLabel.stringValue = image == nil ? L("None — text watermark") : L("Image chosen (%d × %d)", image!.width, image!.height)
        }
        let chooseButton = NSButton(title: L("Choose…"), target: chooser, action: #selector(ClosureTarget.fire))
        let imageRow = NSStackView(views: [chooseButton, imageLabel])
        let alert = NSAlert.form(L("Add Watermark"), info: L("The watermark is drawn on the pages; it becomes part of the page content when the document is saved."),
                                 ok: L("Add"), rows: [
                                     (L("Text:"), text), (L("Image:"), imageRow), (L("Font size:"), size), (L("Colour:"), color),
                                     (L("Opacity:"), opacity), (L("Rotation (°):"), rotation), (L("Pages:"), pages),
                                 ])
        alert.window.initialFirstResponder = text
        guard alert.runModal() == .alertFirstButtonReturn, let targets = indices(from: pages) else { _ = chooser; return }
        spec.text = text.stringValue.isEmpty ? L("CONFIDENTIAL") : text.stringValue
        guard let fontSize = Double(size.stringValue.replacingOccurrences(of: ",", with: ".")), fontSize.isFinite, fontSize >= 6, fontSize <= 400,
              let angle = Double(rotation.stringValue.replacingOccurrences(of: ",", with: ".")), angle.isFinite else { NSSound.beep(); return }
        spec.fontSize = CGFloat(fontSize)
        spec.color = Self.overlayColors[max(0, color.indexOfSelectedItem)].1
        spec.opacity = CGFloat(opacity.doubleValue / 100)
        spec.rotation = CGFloat(angle.truncatingRemainder(dividingBy: 360))
        spec.image = image
        Self.lastWatermark = spec
        Self.lastWatermark.image = nil
        for i in targets {
            guard let page = pdf.page(at: i) else { continue }
            add(WatermarkOverlay(spec: spec, page: page), to: page, name: L("Watermark"))
        }
    }

    @objc func pageNumbersAction(_ sender: Any?) {
        guard let pdf else { return }
        var spec = Self.lastNumbers
        let position = NSPopUpButton(frame: .zero, pullsDown: false)
        position.addItems(withTitles: [L("Bottom centre"), L("Bottom right"), L("Bottom left"), L("Top centre"), L("Top right"), L("Top left")])
        position.selectItem(at: spec.position.rawValue)
        let format = NSPopUpButton(frame: .zero, pullsDown: false)
        format.addItems(withTitles: ["1", L("Page 1 of N"), "- 1 -"])
        format.selectItem(at: spec.format.rawValue)
        let start = NSTextField(string: "\(spec.start)")
        let size = NSTextField(string: "\(Int(spec.fontSize))")
        let skip = NSButton(checkboxWithTitle: L("No number on the first page"), target: nil, action: nil)
        skip.state = spec.skipFirst ? .on : .off
        let pages = rangeField()
        let alert = NSAlert.form(L("Add Page Numbers"), ok: L("Add"), rows: [
            (L("Position:"), position), (L("Format:"), format), (L("Start at:"), start), (L("Font size:"), size),
            ("", skip), (L("Pages:"), pages),
        ])
        guard alert.runModal() == .alertFirstButtonReturn, let targets = indices(from: pages) else { return }
        spec.position = PageNumberSpec.Position(rawValue: position.indexOfSelectedItem) ?? .bottomCenter
        spec.format = PageNumberSpec.Format(rawValue: format.indexOfSelectedItem) ?? .plain
        guard let first = Int(start.stringValue), let fontSize = Double(size.stringValue.replacingOccurrences(of: ",", with: ".")),
              fontSize.isFinite, fontSize >= 4, fontSize <= 72 else { NSSound.beep(); return }
        spec.start = first
        spec.fontSize = CGFloat(fontSize)
        spec.skipFirst = skip.state == .on
        let labels = spec.labels(for: targets)
        guard !labels.isEmpty else { NSSound.beep(); return }
        Self.lastNumbers = spec
        for (i, text) in labels {
            guard let page = pdf.page(at: i) else { continue }
            add(PageNumberOverlay(text: text, spec: spec, page: page), to: page, name: L("Page Numbers"))
        }
    }

    /// Watermarks and page numbers added since the last save (saved ones are page content).
    private var removableOverlays: [(PDFAnnotation, PDFPage)] {
        guard let pdf else { return [] }
        return (0..<pdf.pageCount).flatMap { i -> [(PDFAnnotation, PDFPage)] in
            guard let page = pdf.page(at: i) else { return [] }
            return page.annotations.filter { $0 is WatermarkOverlay || $0 is PageNumberOverlay }.map { ($0, page) }
        }
    }

    var hasRemovableOverlays: Bool { !removableOverlays.isEmpty }

    @objc func removeWatermarksAction(_ sender: Any?) {
        for (a, page) in removableOverlays { remove(a, from: page, name: L("Remove Watermarks and Page Numbers")) }
    }

    // MARK: - Split

    @objc func splitAction(_ sender: Any?) {
        guard let pdf, let window else { return }
        let mode = NSPopUpButton(frame: .zero, pullsDown: false)
        mode.addItems(withTitles: [L("Every N pages"), L("By page ranges"), L("One file per page")])
        let every = NSTextField(string: "1")
        let ranges = NSTextField()
        ranges.placeholderString = "1-3, 4-10"
        let alert = NSAlert.form(L("Split Document"), info: L("Each part is saved as a separate PDF in the folder you choose. Pages: %d.", pdf.pageCount),
                                 ok: L("Choose Folder…"), rows: [(L("Split:"), mode), (L("Pages per file:"), every), (L("Ranges:"), ranges)])
        let sync = ClosureTarget {
            every.isEnabled = mode.indexOfSelectedItem == 0
            ranges.isEnabled = mode.indexOfSelectedItem == 1
        }
        mode.target = sync
        mode.action = #selector(ClosureTarget.fire)
        sync.fire()
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        let groups: [[Int]]?
        switch mode.indexOfSelectedItem {
        case 0: groups = Int(every.stringValue).flatMap { $0 > 0 ? PDFTools.everyN($0, pageCount: pdf.pageCount) : nil }
        case 1: groups = PDFTools.parseRanges(ranges.stringValue, pageCount: pdf.pageCount)
        default: groups = PDFTools.everyN(1, pageCount: pdf.pageCount)
        }
        guard let groups, !groups.isEmpty else {
            let bad = NSAlert()
            bad.messageText = L("Invalid page range")
            bad.informativeText = L("Use page numbers from 1 to %d, e.g. “1-3, 5, 8-”.", pdf.pageCount)
            bad.beginSheetModal(for: window)
            return
        }
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = true
        panel.prompt = L("Save Here")
        panel.message = L("Folder for the %d parts", groups.count)
        panel.directoryURL = pdfDoc?.fileURL?.deletingLastPathComponent()
        panel.beginSheetModal(for: window) { [self] response in
            guard response == .OK, let dir = panel.url else { return }
            prepareForSave()
            let base = pdfDoc?.fileURL?.deletingPathExtension().lastPathComponent ?? pdfDoc?.displayName ?? L("Document")
            let source: PDFDocument
            do { source = try exportSnapshot() } catch { presentError(error); return }
            var written: [URL] = []
            for (group, part) in zip(groups, PDFTools.split(source, groups: groups)) {
                let url = dir.appendingPathComponent(PDFTools.partName(base, group))
                guard let data = part.dataRepresentation().map(AcroFormFixer.ensureFields) else { continue }
                do { try data.write(to: url, options: .atomic); written.append(url) } catch { presentError(error); return }
            }
            banner.show(.success, L("Saved %d files", written.count))
            DispatchQueue.main.asyncAfter(deadline: .now() + 4) { [weak self] in self?.updateSignatureBanner() }
            NSWorkspace.shared.activateFileViewerSelecting(written)
        }
    }

    // MARK: - Compare

    @objc func compareAction(_ sender: Any?) {
        guard let window else { return }
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.pdf]
        panel.message = L("Choose the PDF to compare this document with")
        panel.beginSheetModal(for: window) { [weak self] response in
            guard response == .OK, let url = panel.url, let self else { return }
            guard let other = PDFDocument(url: url), !other.isLocked else {
                self.presentError(NSError(domain: NSCocoaErrorDomain, code: NSFileReadCorruptFileError, userInfo: [NSURLErrorKey: url]))
                return
            }
            self.showComparison(with: other, name: url.lastPathComponent)
        }
    }

    func showComparison(with other: PDFDocument, name: String) {
        guard let pdf else { return }
        clearCompareHighlights()
        let changes = PDFTools.compare(pdf, other)
        compareWindow?.close()
        let controller = CompareWindowController(editor: self, changes: changes, otherName: name)
        compareWindow = controller
        addCompareHighlights(changes)
        controller.showWindow(nil)
    }

    /// Marks words that differ: red where removed, orange where replaced. Not undoable, not saved by default.
    func addCompareHighlights(_ changes: [PDFTools.Change]) {
        guard let pdf else { return }
        for page in Self.addCompareHighlights(changes, to: pdf) { pdfView.annotationsChanged(on: page) }
    }

    /// Adds `TemporaryHighlight`s over the removed/replaced words; returns the pages touched.
    @discardableResult
    static func addCompareHighlights(_ changes: [PDFTools.Change], to pdf: PDFDocument) -> [PDFPage] {
        var pages: [PDFPage] = []
        for change in changes where !change.removed.isEmpty {
            guard let page = pdf.page(at: change.page), let first = change.removed.first, let last = change.removed.last else { continue }
            let range = NSRange(location: first.range.location, length: NSMaxRange(last.range) - first.range.location)
            guard let selection = page.selection(for: range) else { continue }
            let color: NSColor = change.added.isEmpty ? .systemRed : .systemOrange
            for line in selection.selectionsByLine() {
                let b = line.bounds(for: page)
                guard b.width > 0.5, b.height > 0.5 else { continue }
                let h = TemporaryHighlight(bounds: b, forType: .highlight, withProperties: nil)
                h.color = color.withAlphaComponent(0.4)
                h.quadrilateralPoints = [
                    NSValue(point: NSPoint(x: 0, y: b.height)), NSValue(point: NSPoint(x: b.width, y: b.height)),
                    NSValue(point: NSPoint(x: 0, y: 0)), NSValue(point: NSPoint(x: b.width, y: 0)),
                ]
                h.userName = L("Comparison")
                page.addAnnotation(h)
            }
            pages.append(page)
        }
        return pages
    }

    var hasCompareHighlights: Bool {
        guard let pdf else { return false }
        return (0..<pdf.pageCount).contains { pdf.page(at: $0)?.annotations.contains { $0 is TemporaryHighlight } == true }
    }

    func clearCompareHighlights() {
        guard let pdf else { return }
        for i in 0..<pdf.pageCount {
            guard let page = pdf.page(at: i) else { continue }
            let temp = page.annotations.filter { $0 is TemporaryHighlight }
            guard !temp.isEmpty else { continue }
            temp.forEach { page.removeAnnotation($0) }
            pdfView.annotationsChanged(on: page)
        }
        pdfDoc?.keepCompareHighlights = false
    }

    @objc func clearCompareHighlightsAction(_ sender: Any?) {
        clearCompareHighlights()
        compareWindow?.highlightsRemoved()
    }

    /// Shows a change from the comparison: its page, with the removed words selected.
    func showChange(_ change: PDFTools.Change) {
        guard let pdf, let page = pdf.page(at: min(change.page, pdf.pageCount - 1)) else { return }
        if change.page < pdf.pageCount, let first = change.removed.first, let last = change.removed.last,
           let selection = page.selection(for: NSRange(location: first.range.location, length: NSMaxRange(last.range) - first.range.location)) {
            pdfView.setCurrentSelection(selection, animate: true)
            pdfView.go(to: selection)
        } else {
            pdfView.go(to: page)
        }
    }
}

extension AppDelegate {
    /// Combines PDFs and images, in the order the user chooses, into a new document window.
    @objc func mergeFilesAction(_ sender: Any?) {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.pdf, .image]
        panel.allowsMultipleSelection = true
        panel.message = L("Choose the PDFs and images to merge (you can change the order next)")
        guard panel.runModal() == .OK, !panel.urls.isEmpty else { return }
        let sorted = panel.urls.sorted { $0.lastPathComponent.localizedStandardCompare($1.lastPathComponent) == .orderedAscending }
        guard let urls = FileOrderList.run(sorted) else { return }
        let merged = PDFTools.merge(urls)
        guard merged.pageCount > 0 else { NSSound.beep(); return }
        let doc = PDFDoc(pdf: merged)
        NSDocumentController.shared.addDocument(doc)
        doc.makeWindowControllers()
        doc.showWindows()
        doc.updateChangeCount(.changeDone)
    }
}

/// Modal list for ordering files: move up/down, add and remove.
final class FileOrderList: NSObject, NSTableViewDataSource, NSTableViewDelegate {
    private var urls: [URL]
    private let table = NSTableView()

    private init(_ urls: [URL]) { self.urls = urls }

    static func run(_ urls: [URL]) -> [URL]? {
        let list = FileOrderList(urls)
        let alert = NSAlert()
        alert.messageText = L("Merge Files")
        alert.informativeText = L("The files are joined in this order.")
        alert.addButton(withTitle: L("Merge"))
        alert.addButton(withTitle: L("Cancel"))
        alert.accessoryView = list.makeView()
        alert.layout()
        guard alert.runModal() == .alertFirstButtonReturn, !list.urls.isEmpty else { return nil }
        return list.urls
    }

    private func makeView() -> NSView {
        let column = NSTableColumn(identifier: .init("file"))
        column.width = 380
        table.addTableColumn(column)
        table.headerView = nil
        table.dataSource = self
        table.delegate = self
        table.allowsMultipleSelection = false
        table.usesAlternatingRowBackgroundColors = true
        let scroll = NSScrollView(frame: NSRect(x: 0, y: 30, width: 400, height: 200))
        scroll.documentView = table
        scroll.hasVerticalScroller = true
        scroll.borderType = .bezelBorder
        let buttons = [(L("Move Up"), #selector(up)), (L("Move Down"), #selector(down)), (L("Add…"), #selector(addFiles)), (L("Remove"), #selector(removeFile))]
            .map { NSButton(title: $0.0, target: self, action: $0.1) }
        buttons.forEach { $0.controlSize = .small }
        let row = NSStackView(views: buttons)
        row.frame = NSRect(x: 0, y: 0, width: 400, height: 24)
        let view = NSView(frame: NSRect(x: 0, y: 0, width: 400, height: 232))
        view.addSubview(scroll)
        view.addSubview(row)
        if !urls.isEmpty { table.selectRowIndexes([0], byExtendingSelection: false) }
        return view
    }

    func numberOfRows(in tableView: NSTableView) -> Int { urls.count }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        let url = urls[row]
        let count = PDFDocument(url: url)?.pageCount
        let text = count.map { "\(row + 1). \(url.lastPathComponent) — " + L("%d pages", $0) } ?? "\(row + 1). \(url.lastPathComponent)"
        let label = NSTextField(labelWithString: text)
        label.lineBreakMode = .byTruncatingMiddle
        return label
    }

    private func move(by delta: Int) {
        let i = table.selectedRow
        guard i >= 0, i + delta >= 0, i + delta < urls.count else { return }
        urls.swapAt(i, i + delta)
        table.reloadData()
        table.selectRowIndexes([i + delta], byExtendingSelection: false)
    }

    @objc private func up() { move(by: -1) }
    @objc private func down() { move(by: 1) }

    @objc private func removeFile() {
        let i = table.selectedRow
        guard i >= 0 else { return }
        urls.remove(at: i)
        table.reloadData()
        if !urls.isEmpty { table.selectRowIndexes([min(i, urls.count - 1)], byExtendingSelection: false) }
    }

    @objc private func addFiles() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.pdf, .image]
        panel.allowsMultipleSelection = true
        guard panel.runModal() == .OK else { return }
        urls += panel.urls
        table.reloadData()
    }
}

private extension CGFloat {
    func clamped(_ lo: CGFloat, _ hi: CGFloat) -> CGFloat { Swift.min(hi, Swift.max(lo, self)) }
}
