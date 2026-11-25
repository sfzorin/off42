import AppKit
import PDFKit
import UniformTypeIdentifiers

/// One transient sheet shared by PDF42, DOC42 and XLS42. Source adapters stay in their apps.
public enum LabelSheetDialog {
    private static var active: [ObjectIdentifier: LabelSheetController] = [:]
    public static func present(in window: NSWindow, sources: [LabelSource]) {
        guard !sources.isEmpty, window.attachedSheet == nil else { return }
        let controller = LabelSheetController(sources: sources)
        let key = ObjectIdentifier(controller)
        active[key] = controller
        window.beginSheet(controller.panel) { _ in active.removeValue(forKey: key) }
        controller.generate(nil)
    }
}

private struct SavedLabelTemplate: Codable {
    var name: String
    var options: LabelSheetOptions
    var columns: [String]
}

final class LabelSheetController: NSObject, NSComboBoxDelegate {
    let panel = NSPanel(contentRect: CGRect(x: 0, y: 0, width: 960, height: 720), styleMask: [.titled, .closable], backing: .buffered, defer: false)
    private let sources: [LabelSource]
    let sourcePicker = NSPopUpButton()
    let stockPicker = NSComboBox()
    let width = NSTextField(string: "63.5"), height = NSTextField(string: "38.1")
    let gapX = NSTextField(string: "2.5"), gapY = NSTextField(string: "0")
    let copies = NSTextField(string: "1"), startSlot = NSTextField(string: "1")
    let header = NSButton(checkboxWithTitle: L("First row contains column names"), target: nil, action: nil)
    let visibleOnly = NSButton(checkboxWithTitle: L("Visible rows only"), target: nil, action: nil)
    let fieldNames = NSButton(checkboxWithTitle: L("Include field names"), target: nil, action: nil)
    let codeColumn = NSPopUpButton(), codeType = NSPopUpButton(), copiesColumn = NSPopUpButton()
    let fields = NSStackView()
    let fieldsScroll = NSScrollView()
    let controlsScroll = NSScrollView()
    let preview = PDFView()
    let status = NSTextField(wrappingLabelWithString: "")
    private let stockNote = NSTextField(wrappingLabelWithString: "")
    private let templates = NSPopUpButton()
    let generateButton = NSButton(title: L("Update Preview"), target: nil, action: nil)
    let saveButton = NSButton(title: L("Save PDF…"), target: nil, action: nil)
    let printButton = NSButton(title: L("Print…"), target: nil, action: nil)
    var customRows: NSStackView!
    private var tableRows: NSStackView!
    var result: LabelSheetResult?
    var generation = 0
    private let defaults = UserDefaults(suiteName: "com.sfzorin.suite42")!
    private var savedTemplates: [SavedLabelTemplate] {
        guard let data = defaults.data(forKey: "LabelTemplates.v1"), data.count <= 1_000_000 else { return [] }
        return (try? JSONDecoder().decode([SavedLabelTemplate].self, from: data)) ?? []
    }
    private var source: LabelSource { sources[max(0, sourcePicker.indexOfSelectedItem)] }
    private var columns: [String] { source.columnTitles(firstRowIsHeader: header.state == .on) }
    init(sources: [LabelSource]) {
        self.sources = sources
        super.init()
        panel.title = L("Create Labels")
        panel.isReleasedWhenClosed = false
        panel.standardWindowButton(.closeButton)?.target = self; panel.standardWindowButton(.closeButton)?.action = #selector(close)
        sourcePicker.addItems(withTitles: sources.map(\.title)); sourcePicker.target = self; sourcePicker.action = #selector(sourceChanged)
        stockPicker.addItems(withObjectValues: [L("Custom")] + LabelStock.catalog.map(\.name)); stockPicker.selectItem(at: 0)
        stockPicker.completes = true; stockPicker.delegate = self; stockPicker.target = self; stockPicker.action = #selector(stockChanged)
        codeType.addItems(withTitles: TechnicalCodeType.allCases.map(\.title)); codeType.selectItem(at: 0)
        codeType.target = self; codeType.action = #selector(changed)
        for field in [width, height, gapX, gapY, copies, startSlot] { field.target = self; field.action = #selector(changed); field.delegate = self; field.widthAnchor.constraint(equalToConstant: 75).isActive = true }
        for button in [header, visibleOnly, fieldNames] { button.target = self; button.action = #selector(changed) }
        header.action = #selector(headerChanged); visibleOnly.state = .on; fieldNames.state = .on
        for popup in [codeColumn, copiesColumn] { popup.target = self; popup.action = #selector(changed) }
        fields.orientation = .vertical; fields.alignment = .leading; fields.spacing = 4
        fieldsScroll.hasVerticalScroller = true; fieldsScroll.borderType = .bezelBorder
        let fieldsDocument = LabelControlsDocument()
        fieldsDocument.translatesAutoresizingMaskIntoConstraints = false; fields.translatesAutoresizingMaskIntoConstraints = false
        fieldsScroll.documentView = fieldsDocument; fieldsDocument.addSubview(fields)
        let compactFields = fieldsDocument.heightAnchor.constraint(equalTo: fields.heightAnchor, constant: 8)
        compactFields.priority = .defaultLow
        NSLayoutConstraint.activate([
            fieldsDocument.widthAnchor.constraint(equalTo: fieldsScroll.contentView.widthAnchor),
            fieldsDocument.heightAnchor.constraint(greaterThanOrEqualTo: fieldsScroll.contentView.heightAnchor),
            fields.leadingAnchor.constraint(equalTo: fieldsDocument.leadingAnchor, constant: 4),
            fields.trailingAnchor.constraint(equalTo: fieldsDocument.trailingAnchor, constant: -4),
            fields.topAnchor.constraint(equalTo: fieldsDocument.topAnchor, constant: 4),
            fields.bottomAnchor.constraint(lessThanOrEqualTo: fieldsDocument.bottomAnchor, constant: -4),
            compactFields, fieldsScroll.heightAnchor.constraint(equalToConstant: 100)])
        func row(_ title: String, _ controls: [NSView]) -> NSStackView {
            let label = NSTextField(labelWithString: title); label.setContentCompressionResistancePriority(.required, for: .horizontal)
            let value = NSStackView(views: [label] + controls); value.spacing = 8; return value
        }
        customRows = NSStackView(views: [row(L("Size (mm)"), [width, NSTextField(labelWithString: "×"), height]), row(L("Gaps X / Y (mm)"), [gapX, gapY])]); customRows.orientation = .vertical; customRows.alignment = .leading; customRows.spacing = 7
        tableRows = NSStackView(views: [header, visibleOnly, NSTextField(labelWithString: L("Text columns (up to six)")), fieldsScroll, fieldNames,
                                      row(L("Code column"), [codeColumn]), row(L("Code type"), [codeType]), row(L("Copies column"), [copiesColumn])])
        tableRows.orientation = .vertical; tableRows.alignment = .leading; tableRows.spacing = 6
        templates.target = self; templates.action = #selector(loadTemplate)
        let saveTemplate = NSButton(title: L("Save Template…"), target: self, action: #selector(saveTemplateAction)); saveTemplate.bezelStyle = .rounded
        let controls = NSStackView(views: [row(L("Source"), [sourcePicker]), row(L("Template"), [templates]), saveTemplate,
                                         row(L("Label stock"), [stockPicker]), customRows, stockNote, tableRows,
                                         row(L("Copies"), [copies, NSTextField(labelWithString: L("First position")), startSlot])])
        controls.orientation = .vertical; controls.alignment = .leading; controls.spacing = 10
        tableRows.widthAnchor.constraint(equalTo: controls.widthAnchor).isActive = true
        fieldsScroll.widthAnchor.constraint(equalTo: tableRows.widthAnchor).isActive = true
        sourcePicker.widthAnchor.constraint(equalToConstant: 260).isActive = true
        stockPicker.widthAnchor.constraint(equalToConstant: 255).isActive = true
        templates.widthAnchor.constraint(equalToConstant: 255).isActive = true
        for popup in [codeColumn, copiesColumn] { popup.widthAnchor.constraint(lessThanOrEqualToConstant: 240).isActive = true }
        stockNote.font = .systemFont(ofSize: 10); stockNote.textColor = .secondaryLabelColor; stockNote.maximumNumberOfLines = 3
        preview.autoScales = true; preview.displayMode = .singlePageContinuous; preview.backgroundColor = .windowBackgroundColor
        let closeButton = NSButton(title: L("Close"), target: self, action: #selector(close)); closeButton.keyEquivalent = "\u{1b}"
        for button in [generateButton, saveButton, printButton, closeButton] { button.bezelStyle = .rounded }
        generateButton.target = self; generateButton.action = #selector(generate(_:)); generateButton.keyEquivalent = "\r"
        saveButton.target = self; saveButton.action = #selector(savePDF); printButton.target = self; printButton.action = #selector(printPDF)
        let actions = NSStackView(views: [closeButton, generateButton, saveButton, printButton]); actions.spacing = 8
        let root = NSView(); panel.contentView = root
        controlsScroll.hasVerticalScroller = true; controlsScroll.autohidesScrollers = true
        controlsScroll.drawsBackground = false
        let controlsDocument = LabelControlsDocument()
        controlsDocument.translatesAutoresizingMaskIntoConstraints = false
        controls.translatesAutoresizingMaskIntoConstraints = false
        controlsScroll.documentView = controlsDocument; controlsDocument.addSubview(controls)
        NSLayoutConstraint.activate([
            controlsDocument.widthAnchor.constraint(equalTo: controlsScroll.contentView.widthAnchor),
            controls.leadingAnchor.constraint(equalTo: controlsDocument.leadingAnchor),
            controls.trailingAnchor.constraint(equalTo: controlsDocument.trailingAnchor, constant: -8),
            controls.topAnchor.constraint(equalTo: controlsDocument.topAnchor),
            controls.bottomAnchor.constraint(equalTo: controlsDocument.bottomAnchor, constant: -8)])
        for view in [controlsScroll, preview, status, actions] { view.translatesAutoresizingMaskIntoConstraints = false; root.addSubview(view) }
        NSLayoutConstraint.activate([
            controlsScroll.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 20), controlsScroll.topAnchor.constraint(equalTo: root.topAnchor, constant: 20), controlsScroll.widthAnchor.constraint(equalToConstant: 390),
            controlsScroll.bottomAnchor.constraint(equalTo: status.topAnchor, constant: -12),
            preview.leadingAnchor.constraint(equalTo: controlsScroll.trailingAnchor, constant: 20), preview.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -20), preview.topAnchor.constraint(equalTo: root.topAnchor, constant: 20), preview.bottomAnchor.constraint(equalTo: status.topAnchor, constant: -12),
            status.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 20), status.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -20), status.bottomAnchor.constraint(equalTo: actions.topAnchor, constant: -12), status.heightAnchor.constraint(greaterThanOrEqualToConstant: 30),
            actions.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -20), actions.bottomAnchor.constraint(equalTo: root.bottomAnchor, constant: -16)])
        reloadTemplates(); sourceChanged()
    }
    @objc private func sourceChanged() { header.state = source.firstRowIsHeader ? .on : .off; rebuildFields(); invalidate() }
    @objc private func headerChanged() { rebuildFields(); invalidate() }
    private func rebuildFields() {
        fields.arrangedSubviews.forEach { fields.removeArrangedSubview($0); $0.removeFromSuperview() }
        for (index, name) in columns.enumerated() {
            let button = NSButton(checkboxWithTitle: name, target: self, action: #selector(changed)); button.tag = index; button.state = index < 3 ? .on : .off
            button.lineBreakMode = .byTruncatingTail; fields.addArrangedSubview(button)
        }
        codeColumn.removeAllItems(); codeColumn.addItems(withTitles: [L("None")] + columns)
        copiesColumn.removeAllItems(); copiesColumn.addItems(withTitles: [L("Use copy count")] + columns)
        tableRows.isHidden = !source.isTable
    }
    @objc private func changed() { invalidate() }
    private func invalidate() {
        generation += 1; result = nil; preview.document = nil; saveButton.isEnabled = false; printButton.isEnabled = false
        status.stringValue = L("Update the preview before saving or printing."); status.textColor = .secondaryLabelColor
        generateButton.isEnabled = true
    }
    func comboBoxSelectionDidChange(_ notification: Notification) { stockChanged() }
    func controlTextDidChange(_ obj: Notification) {
        if obj.object as? NSComboBox === stockPicker { stockChanged() }
        else { invalidate() }
    }
    @objc private func stockChanged() {
        let match = LabelStock.catalog.first(where: { $0.name == stockPicker.stringValue })
        customRows.isHidden = stockPicker.stringValue != L("Custom")
        stockNote.stringValue = match?.note ?? ""
        invalidate()
    }
    func options() throws -> LabelSheetOptions {
        let stock: LabelStock
        if stockPicker.stringValue == L("Custom") {
            func number(_ field: NSTextField) -> Double { Double(field.stringValue.replacingOccurrences(of: ",", with: ".")) ?? .nan }
            stock = try .custom(width: number(width), height: number(height), gapX: number(gapX), gapY: number(gapY))
        } else if let match = LabelStock.catalog.first(where: { $0.name == stockPicker.stringValue }) { stock = match }
        else { throw LabelSheetError.message(L("Choose a label stock or Custom.")) }
        try stock.validate()
        var value = LabelSheetOptions(stock: stock)
        guard let count = Int(copies.stringValue), let first = Int(startSlot.stringValue),
              (1...5000).contains(count), (1...stock.capacity).contains(first) else {
            throw LabelSheetError.message(L("Enter 1–5,000 copies and a first position within the label sheet."))
        }
        value.copies = count; value.startSlot = first - 1
        value.textColumns = fields.arrangedSubviews.compactMap { ($0 as? NSButton).flatMap { $0.state == .on ? $0.tag : nil } }
        value.codeColumn = codeColumn.indexOfSelectedItem > 0 ? codeColumn.indexOfSelectedItem - 1 : nil
        value.codeType = TechnicalCodeType.allCases[max(0, codeType.indexOfSelectedItem)]
        value.copiesColumn = copiesColumn.indexOfSelectedItem > 0 ? copiesColumn.indexOfSelectedItem - 1 : nil
        value.firstRowIsHeader = header.state == .on; value.visibleOnly = visibleOnly.state == .on; value.includeFieldNames = fieldNames.state == .on
        return value
    }
    @objc func generate(_ sender: Any?) {
        generation += 1; let token = generation
        result = nil; saveButton.isEnabled = false; printButton.isEnabled = false
        do {
            let config = try options(), source = self.source
            status.stringValue = L("Preparing labels…"); status.textColor = .secondaryLabelColor; generateButton.isEnabled = false
            DispatchQueue.global(qos: .userInitiated).async { [weak self] in
                let output = Result { try LabelSheetRenderer.render(source: source, options: config) }
                DispatchQueue.main.async {
                    guard let self, self.generation == token else { return }
                    self.generateButton.isEnabled = true
                    switch output {
                    case let .success(result):
                        self.result = result; self.preview.document = PDFDocument(data: result.data)
                        self.status.stringValue = L("%d labels on %d pages", result.labelCount, result.pageCount); self.status.textColor = .labelColor
                        self.saveButton.isEnabled = true; self.printButton.isEnabled = true
                    case let .failure(error): self.status.stringValue = error.localizedDescription; self.status.textColor = .systemRed; self.preview.document = nil
                    }
                }
            }
        } catch { status.stringValue = error.localizedDescription; status.textColor = .systemRed; generateButton.isEnabled = true; preview.document = nil }
    }
    @objc private func close() { generation += 1; panel.sheetParent?.endSheet(panel); panel.orderOut(nil) }
    @objc private func savePDF() {
        guard let result else { return }
        let save = NSSavePanel(); save.allowedContentTypes = [.pdf]; save.nameFieldStringValue = "labels.pdf"
        guard save.runModal() == .OK, let url = save.url else { return }
        do { try result.data.write(to: url, options: .atomic); status.stringValue = L("Saved %@", url.lastPathComponent) }
        catch { status.stringValue = error.localizedDescription; status.textColor = .systemRed }
    }
    @objc private func printPDF() {
        guard let result else { return }
        do {
            let info = NSPrintInfo.shared.copy() as! NSPrintInfo; info.paperSize = CGSize(width: 210 * 72 / 25.4, height: 297 * 72 / 25.4); info.scalingFactor = 1
            let operation = try ProductionPrint.operation(pdfData: result.data, printInfo: info)
            operation.jobTitle = L("Labels"); operation.runModal(for: panel, delegate: nil, didRun: nil, contextInfo: nil)
        } catch { status.stringValue = error.localizedDescription; status.textColor = .systemRed }
    }
    private func reloadTemplates() { templates.removeAllItems(); templates.addItems(withTitles: [L("Choose Template…")] + savedTemplates.map(\.name)) }
    @objc private func saveTemplateAction() {
        do {
            let config = try options(); let name = NSTextField(string: "")
            let alert = NSAlert.form(L("Save Label Template"), ok: L("Save"), rows: [(L("Name:"), name)])
            guard alert.runModal() == .alertFirstButtonReturn else { return }
            let title = name.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !title.isEmpty, title.count <= 100 else { throw LabelSheetError.message(L("Enter a template name of 1–100 characters.")) }
            var list = savedTemplates.filter { $0.name != title }
            guard list.count < 50 else { throw LabelSheetError.message(L("The template library is limited to 50 entries.")) }
            list.append(SavedLabelTemplate(name: title, options: config, columns: columns))
            defaults.set(try JSONEncoder().encode(list), forKey: "LabelTemplates.v1"); reloadTemplates(); status.stringValue = L("Template saved.")
        } catch { status.stringValue = error.localizedDescription; status.textColor = .systemRed }
    }
    @objc private func loadTemplate() {
        let index = templates.indexOfSelectedItem - 1
        guard savedTemplates.indices.contains(index) else { return }
        let saved = savedTemplates[index], value = saved.options
        guard (try? value.stock.validate()) != nil, (1...5000).contains(value.copies),
              (0..<value.stock.capacity).contains(value.startSlot) else {
            invalidate(); status.stringValue = L("This label template has invalid dimensions or counts."); return
        }
        header.state = value.firstRowIsHeader ? .on : .off; rebuildFields()
        func mapped(_ old: Int) -> Int? {
            guard saved.columns.indices.contains(old) else { return nil }
            let matches = columns.indices.filter { columns[$0] == saved.columns[old] }
            return matches.count == 1 ? matches[0] : nil
        }
        let textColumns = value.textColumns.compactMap(mapped)
        guard !source.isTable || (textColumns.count == value.textColumns.count && (value.codeColumn == nil || mapped(value.codeColumn!) != nil) && (value.copiesColumn == nil || mapped(value.copiesColumn!) != nil)) else {
            invalidate(); status.stringValue = L("This template uses different columns. Select the fields for this table."); return
        }
        if let stock = LabelStock.catalog.firstIndex(where: { $0.id == value.stock.id }) { stockPicker.selectItem(at: stock + 1) }
        else { stockPicker.selectItem(at: 0); width.stringValue = String(value.stock.width); height.stringValue = String(value.stock.height); gapX.stringValue = String(value.stock.gapX); gapY.stringValue = String(value.stock.gapY) }
        for button in fields.arrangedSubviews.compactMap({ $0 as? NSButton }) { button.state = textColumns.contains(button.tag) ? .on : .off }
        codeColumn.selectItem(at: value.codeColumn.flatMap(mapped).map { $0 + 1 } ?? 0)
        copiesColumn.selectItem(at: value.copiesColumn.flatMap(mapped).map { $0 + 1 } ?? 0)
        codeType.selectItem(at: TechnicalCodeType.allCases.firstIndex(of: value.codeType) ?? 0)
        copies.stringValue = String(value.copies); startSlot.stringValue = String(value.startSlot + 1)
        visibleOnly.state = value.visibleOnly ? .on : .off; fieldNames.state = value.includeFieldNames ? .on : .off
        stockChanged()
    }
}

private final class LabelControlsDocument: NSView { override var isFlipped: Bool { true } }
