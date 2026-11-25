import AppKit

/// The same small editor is used by PDF42, DOC42 and XLS42. Hosts decide where
/// an element belongs and own the resulting document mutation and Undo entry.
public enum TechnicalElementDialog {
    public static func present(in window: NSWindow, initial: TechnicalElement? = nil, initialValue: String = "",
                               completion: @escaping (TechnicalElement, TechnicalRender) -> Void) {
        let editor = TechnicalElementEditor(initial: initial, initialValue: initialValue, completion: completion)
        editor.show(in: window)
    }
}

final class TechnicalElementEditor: NSObject, NSSearchFieldDelegate {
    let alert = NSAlert()
    let mode = NSSegmentedControl(labels: [L("Symbols"), L("Codes")], trackingMode: .selectOne, target: nil, action: nil)
    let search = NSSearchField()
    let symbols = NSPopUpButton()
    let types = NSPopUpButton()
    let value = NSTextField()
    let width = NSTextField()
    let caption = NSButton(checkboxWithTitle: L("Show readable value below code"), target: nil, action: nil)
    let preview = NSImageView()
    let errorLabel = NSTextField(wrappingLabelWithString: "")
    /// What the selected mark is for, plus its use conditions (conformity marks, trademarks, unverified artwork).
    let notice = NSTextField(wrappingLabelWithString: "")
    let dimensions = NSTextField(labelWithString: "")
    let symbolRows = NSStackView()
    let codeRows = NSStackView()
    var visibleSymbols = TechnicalSymbols.all
    var rendered: (TechnicalElement, TechnicalRender)?
    private let completion: (TechnicalElement, TechnicalRender) -> Void

    init(initial: TechnicalElement?, initialValue: String, completion: @escaping (TechnicalElement, TechnicalRender) -> Void) {
        self.completion = completion
        super.init()
        for (view, id) in [(mode, "technical-mode"), (types, "technical-type"), (value, "technical-value"),
                           (width, "technical-width"), (caption, "technical-caption"), (symbols, "technical-symbol")] as [(NSView, String)] {
            view.identifier = NSUserInterfaceItemIdentifier(id)
            view.setAccessibilityIdentifier(id)
        }
        alert.messageText = initial == nil ? L("Insert Symbol or Code") : L("Edit Symbol or Code")
        alert.addButton(withTitle: initial == nil ? L("Insert") : L("Apply"))
        alert.addButton(withTitle: L("Cancel"))
        mode.selectedSegment = initial.map { $0.symbolID == nil ? 1 : 0 } ?? (initialValue.isEmpty ? 0 : 1)
        mode.target = self; mode.action = #selector(changed)
        search.placeholderString = L("Find a technical symbol")
        search.delegate = self
        search.setAccessibilityLabel(L("Find a technical symbol"))
        reloadSymbols(selecting: initial?.symbolID)
        symbols.target = self; symbols.action = #selector(changed)
        symbols.setAccessibilityLabel(L("Technical symbol"))
        TechnicalCodeType.allCases.forEach { types.addItem(withTitle: $0.title) }
        types.selectItem(at: TechnicalCodeType.allCases.firstIndex(of: initial?.codeType ?? .qr) ?? 0)
        types.target = self; types.action = #selector(typeChanged)
        types.setAccessibilityLabel(L("Code type"))
        value.stringValue = initial?.value ?? initialValue
        value.placeholderString = L("Text, identifier or URL")
        value.delegate = self; value.setAccessibilityLabel(L("Code value"))
        let initialWidth: Double = initial?.widthMM ?? 20.0
        width.stringValue = String(format: "%.1f", initialWidth)
        width.delegate = self; width.setAccessibilityLabel(L("Width in millimetres"))
        caption.state = (initial?.caption ?? true) ? .on : .off
        caption.target = self; caption.action = #selector(changed)
        symbolRows.orientation = .vertical; symbolRows.alignment = .leading; symbolRows.spacing = 8
        symbolRows.addArrangedSubview(search); symbolRows.addArrangedSubview(symbols)
        codeRows.orientation = .vertical; codeRows.alignment = .leading; codeRows.spacing = 8
        codeRows.addArrangedSubview(row(L("Type"), types)); codeRows.addArrangedSubview(row(L("Value"), value))
        codeRows.addArrangedSubview(caption)
        preview.imageScaling = .scaleProportionallyUpOrDown
        preview.wantsLayer = true; preview.layer?.backgroundColor = NSColor.white.cgColor
        preview.layer?.cornerRadius = 6
        preview.setAccessibilityLabel(L("Symbol or code preview"))
        dimensions.font = .systemFont(ofSize: 11)
        dimensions.textColor = .secondaryLabelColor
        errorLabel.textColor = .systemRed; errorLabel.font = .systemFont(ofSize: 11)
        notice.font = .systemFont(ofSize: 11); notice.maximumNumberOfLines = 6; notice.preferredMaxLayoutWidth = 430
        notice.setAccessibilityIdentifier("technical-notice")
        let stack = NSStackView(views: [mode, symbolRows, codeRows, row(L("Width (mm)"), width), preview, dimensions, notice, errorLabel])
        stack.orientation = .vertical; stack.alignment = .leading; stack.spacing = 10
        stack.translatesAutoresizingMaskIntoConstraints = false
        // Fixed room for up to six notice lines, so conditions of use are never clipped and fields keep their height.
        let root = NSView(frame: NSRect(x: 0, y: 0, width: 430, height: 480))
        root.addSubview(stack)
        NSLayoutConstraint.activate([
            root.widthAnchor.constraint(equalToConstant: 430),
            stack.leadingAnchor.constraint(equalTo: root.leadingAnchor), stack.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            stack.topAnchor.constraint(equalTo: root.topAnchor), stack.bottomAnchor.constraint(equalTo: root.bottomAnchor),
            symbolRows.widthAnchor.constraint(equalTo: stack.widthAnchor), codeRows.widthAnchor.constraint(equalTo: stack.widthAnchor),
            search.widthAnchor.constraint(equalTo: stack.widthAnchor), symbols.widthAnchor.constraint(equalTo: stack.widthAnchor),
            preview.widthAnchor.constraint(equalTo: stack.widthAnchor), preview.heightAnchor.constraint(equalToConstant: 150),
            errorLabel.widthAnchor.constraint(equalTo: stack.widthAnchor), notice.widthAnchor.constraint(equalTo: stack.widthAnchor), notice.heightAnchor.constraint(equalToConstant: 86), errorLabel.heightAnchor.constraint(greaterThanOrEqualToConstant: 30),
        ])
        alert.accessoryView = root
        update()
    }

    private func row(_ label: String, _ field: NSView) -> NSView {
        let title = NSTextField(labelWithString: label)
        title.font = .systemFont(ofSize: 12)
        title.widthAnchor.constraint(equalToConstant: 90).isActive = true
        let row = NSStackView(views: [title, field]); row.spacing = 8
        row.widthAnchor.constraint(equalToConstant: 430).isActive = true
        field.setContentHuggingPriority(.defaultLow, for: .horizontal)
        return row
    }

    func show(in window: NSWindow) {
        alert.beginSheetModal(for: window) { [self] response in
            if response == .alertFirstButtonReturn, let (element, result) = rendered { completion(element, result) }
        }
    }

    @objc private func changed(_ sender: Any?) { update() }
    @objc private func typeChanged(_ sender: Any?) {
        if types.indexOfSelectedItem == TechnicalCodeType.allCases.firstIndex(of: .code128),
           (Double(width.stringValue.replacingOccurrences(of: ",", with: ".")) ?? 0) < 50 { width.stringValue = "50" }
        update()
    }
    func controlTextDidChange(_ obj: Notification) {
        if obj.object as? NSSearchField === search {
            visibleSymbols = TechnicalSymbols.search(search.stringValue)
            let query = search.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
            // A new query selects its best match; clearing the field keeps the current choice.
            reloadSymbols(selecting: query.isEmpty ? selectedSymbolID : visibleSymbols.first?.id)
        }
        update()
    }

    var selectedSymbolID: String? { symbols.selectedItem?.representedObject as? String }

    /// Category headings (disabled) followed by their marks; each item carries its symbol id.
    func reloadSymbols(selecting id: String?) {
        let menu = NSMenu()
        menu.autoenablesItems = false
        for category in TechnicalSymbols.categories {
            let group = visibleSymbols.filter { $0.category == category }
            guard let first = group.first else { continue }
            let heading = NSMenuItem(title: first.categoryName, action: nil, keyEquivalent: "")
            heading.isEnabled = false
            menu.addItem(heading)
            for symbol in group {
                let title = symbol.region.isEmpty || symbol.name.contains(symbol.region) ? symbol.name : "\(symbol.name) — \(symbol.region)"
                let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
                item.representedObject = symbol.id; item.indentationLevel = 1
                menu.addItem(item)
            }
        }
        symbols.menu = menu
        symbols.autoenablesItems = false
        let marks = menu.items.filter { $0.representedObject is String }
        let target = marks.first { id != nil && ($0.representedObject as? String) == id } ?? marks.first
        if let target { symbols.selectItem(at: menu.index(of: target)) }
    }

    func update() {
        symbolRows.isHidden = mode.selectedSegment != 0
        codeRows.isHidden = mode.selectedSegment == 0
        rendered = nil; errorLabel.stringValue = ""; dimensions.stringValue = ""
        let symbol = mode.selectedSegment == 0 ? TechnicalSymbols.all.first { $0.id == selectedSymbolID } : nil
        // Purpose in secondary text, conditions of use in orange.
        let text = NSMutableAttributedString()
        let font = NSFont.systemFont(ofSize: 11)
        if let symbol, !symbol.summary.isEmpty {
            text.append(NSAttributedString(string: symbol.summary, attributes: [.font: font, .foregroundColor: NSColor.secondaryLabelColor]))
        }
        if let symbol, !symbol.notice.isEmpty {
            if text.length > 0 { text.append(NSAttributedString(string: "\n")) }
            text.append(NSAttributedString(string: symbol.notice, attributes: [.font: font, .foregroundColor: NSColor.systemOrange]))
        }
        notice.attributedStringValue = text
        do {
            guard let size = Double(width.stringValue.replacingOccurrences(of: ",", with: ".")) else {
                throw TechnicalElementError(L("Enter a valid width in millimetres."))
            }
            let symbolID: String?
            if mode.selectedSegment == 0 {
                guard let id = selectedSymbolID, visibleSymbols.contains(where: { $0.id == id }) else {
                    throw TechnicalElementError(L("No matching technical symbols."))
                }
                symbolID = id
            } else { symbolID = nil }
            let type = TechnicalCodeType.allCases[max(0, types.indexOfSelectedItem)]
            let element = TechnicalElement(symbolID: symbolID, codeType: type, value: value.stringValue, widthMM: size, caption: caption.state == .on)
            let result = try element.render()
            preview.image = NSImage(data: result.pdfData)
            dimensions.stringValue = String(format: "%.1f × %.1f mm", result.sizePoints.width * 25.4 / 72, result.sizePoints.height * 25.4 / 72)
            rendered = (element, result)
        } catch {
            preview.image = nil; errorLabel.stringValue = error.localizedDescription
        }
        alert.buttons.first?.isEnabled = rendered != nil
    }
}
