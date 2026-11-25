import AppKit
import Kit42

extension DocWindowController {
    /// The PNG owns the recipe, so it survives standard DOCX image round trips.
    var selectedTechnicalElement: (index: Int, element: TechnicalElement)? {
        let view = activeTextView
        guard let storage = view.textStorage, storage.length > 0 else { return nil }
        let selected = view.selectedRange()
        var indices = [selected.location]
        if selected.length == 0, selected.location > 0 { indices.append(selected.location - 1) }
        if headerEditor == nil, let index = selectedImageIndex { indices.insert(index, at: 0) }
        for index in indices where index >= 0 && index < storage.length {
            guard let attachment = storage.attribute(.attachment, at: index, effectiveRange: nil) as? NSTextAttachment,
                  let data = attachment.fileWrapper?.regularFileContents,
                  var element = TechnicalElement.fromPNG(data) else { continue }
            if attachment.bounds.width > 0 { element.widthMM = attachment.bounds.width * 25.4 / 72 }
            return (index, element)
        }
        return nil
    }

    @objc func insertTechnicalElementAction(_ sender: Any?) {
        guard let window else { return }
        let view = activeTextView
        let range = view.selectedRange()
        let value = view.textStorage.flatMap { storage -> String? in
            guard range.location != NSNotFound, NSMaxRange(range) <= storage.length else { return nil }
            return (storage.string as NSString).substring(with: range)
        } ?? ""
        TechnicalElementDialog.present(in: window, initialValue: value) { [weak self, weak view] element, render in
            guard let self, let view else { return }
            self.applyTechnicalElement(element, render: render, in: view, range: range, replacing: false)
        }
    }

    @objc func editTechnicalElementAction(_ sender: Any?) {
        guard let window, let selected = selectedTechnicalElement else { return }
        let view = activeTextView
        let range = NSRange(location: selected.index, length: 1)
        let original = view.textStorage?.attribute(.attachment, at: selected.index, effectiveRange: nil) as? NSTextAttachment
        TechnicalElementDialog.present(in: window, initial: selected.element) { [weak self, weak view] element, render in
            guard let self, let view, let storage = view.textStorage, NSMaxRange(range) <= storage.length,
                  storage.attribute(.attachment, at: range.location, effectiveRange: nil) as? NSTextAttachment === original else { return }
            self.applyTechnicalElement(element, render: render, in: view, range: range, replacing: true)
        }
    }

    /// Shared native command path for the dialog and behavioral regression scenarios.
    @discardableResult
    func applyTechnicalElement(_ element: TechnicalElement, render: TechnicalRender, in view: NSTextView,
                               range: NSRange, replacing: Bool) -> Bool {
        guard let storage = view.textStorage, range.location != NSNotFound, range.location >= 0,
              range.length >= 0, range.location <= storage.length, range.length <= storage.length - range.location,
              let image = NSImage(data: render.pngData) else { return false }
        let attachment = NSTextAttachment()
        attachment.fileWrapper = FileWrapper(regularFileWithContents: render.pngData)
        attachment.fileWrapper?.preferredFilename = "technical.png"
        attachment.attachmentCell = ImageCell(imageCell: image)
        attachment.bounds = NSRect(origin: .zero, size: render.sizePoints)
        let result = NSMutableAttributedString(attachment: attachment)
        var attrs = view.typingAttributes.filter { $0.key == .paragraphStyle }
        var props = DocImage()
        if replacing, range.length == 1, range.location < storage.length {
            attrs = storage.attributes(at: range.location, effectiveRange: nil)
            attrs.removeValue(forKey: .attachment)
            attrs.removeValue(forKey: .docField)
            if let old = attrs[.docImage] as? DocImage { props = old.duplicate() }
        }
        props.ink = false
        attrs[.docImage] = props
        result.addAttributes(attrs, range: NSRange(location: 0, length: result.length))
        guard view.shouldChangeText(in: range, replacementString: result.string) else { return false }
        view.breakUndoCoalescing()
        storage.replaceCharacters(in: range, with: result)
        view.didChangeText()
        view.setSelectedRange(NSRange(location: range.location, length: 1))
        view.undoManager?.setActionName(replacing ? L("Edit Symbol or Code") : L("Insert Symbol or Code"))
        if headerEditor == nil {
            paged.layoutAll()
            selectImage(range.location)
        }
        window?.makeFirstResponder(view)
        return true
    }

    /// Keep the command discoverable on selected inline objects without a permanent panel.
    func textView(_ view: NSTextView, menu: NSMenu, for event: NSEvent, at charIndex: Int) -> NSMenu? {
        if let storage = view.textStorage, charIndex >= 0, charIndex < storage.length,
           let attachment = storage.attribute(.attachment, at: charIndex, effectiveRange: nil) as? NSTextAttachment,
           let data = attachment.fileWrapper?.regularFileContents, TechnicalElement.fromPNG(data) != nil {
            view.setSelectedRange(NSRange(location: charIndex, length: 1))
            if headerEditor == nil { selectImage(charIndex) }
            let item = NSMenuItem(title: L("Edit Symbol or Code…"), action: #selector(editTechnicalElementAction(_:)), keyEquivalent: "")
            item.target = self
            menu.insertItem(item, at: 0)
            menu.insertItem(.separator(), at: 1)
        }
        if hasLabelSource {
            let item = NSMenuItem(title: L("Create Labels…"), action: #selector(createLabelsAction(_:)), keyEquivalent: "")
            item.target = self
            menu.addItem(.separator())
            menu.addItem(item)
        }
        return menu
    }
}

extension DocWindowController {
    var hasLabelSource: Bool {
        activeTextView.selectedRange().length > 0 ||
            (headerEditor == nil && (selectedImageIndex != nil || tableGrid != nil))
    }

    @objc func createLabelsAction(_ sender: Any?) {
        guard let window else { return }
        do {
            let sources = try labelSources()
            guard !sources.isEmpty else { NSSound.beep(); return }
            LabelSheetDialog.present(in: window, sources: sources)
        } catch { NSApp.presentError(error) }
    }

    /// Freeze contents before opening the output dialog. It never references the live document.
    func labelSources() throws -> [LabelSource] {
        let view = activeTextView
        guard let storage = view.textStorage else { return [] }
        var sources: [LabelSource] = []
        let range = view.selectedRange()
        let pictureIndex = headerEditor == nil ? selectedImageIndex : nil
        if let index = pictureIndex, index >= 0, index < storage.length,
           let attachment = storage.attribute(.attachment, at: index, effectiveRange: nil) as? NSTextAttachment {
            let content = storage.attributedSubstring(from: NSRange(location: index, length: 1))
            let width = max(36, attachment.bounds.width > 0 ? attachment.bounds.width : attachment.picture?.size.width ?? 400)
            sources.append(LabelSource(title: L("Selected Picture"), pdfData: try Self.labelContentPDF(content, width: width, info: doc?.fieldInfo ?? DocInfo())))
        }
        if range.location != NSNotFound, range.length > 0, range.location <= storage.length,
           range.length <= storage.length - range.location {
            let content = storage.attributedSubstring(from: range)
            if !(range.length == 1 && pictureIndex == range.location) {
                sources.append(LabelSource(title: L("Selected Content"), pdfData: try Self.labelContentPDF(content, width: paged.setup(at: range.location).columnWidth, info: doc?.fieldInfo ?? DocInfo())))
            }
            let text = Fields.flatten(content, info: doc?.fieldInfo ?? DocInfo()).string.replacingOccurrences(of: "\u{FFFC}", with: "")
            if !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                sources.append(LabelSource(title: L("Selected Text"), text: text))
            }
        }
        if headerEditor == nil, let table = try labelTableSnapshot() {
            sources.append(LabelSource(title: L("Current Table"), headers: table.headers, rows: table.rows,
                                       rowNumbers: table.rowNumbers, firstRowIsHeader: table.rows.count > 1))
        }
        return sources
    }

    func labelTableSnapshot() throws -> (headers: [String], rows: [[String]], rowNumbers: [Int])? {
        guard let storage = textView.textStorage, let grid = tableGrid, grid.rows > 0, grid.columns > 0 else { return nil }
        guard grid.rows <= 5_000, grid.columns <= 200, grid.rows * grid.columns <= 500_000 else {
            throw LabelSheetError.message(L("Select at most 5,000 rows, 200 columns and 500,000 cells for labels."))
        }
        let headers = (0..<grid.columns).map { String(format: L("Column %d"), $0 + 1) }
        let rows = (0..<grid.rows).map { row in
            (0..<grid.columns).map { column -> String in
                guard let cell = grid.cell(row: row, col: column), cell.row == row, cell.col == column else { return "" }
                return cell.text(in: storage)
            }
        }
        return (headers, rows, Array(1...grid.rows))
    }

    static func labelContentPDF(_ content: NSAttributedString, width: CGFloat, info: DocInfo = DocInfo()) throws -> Data {
        let text = NSMutableAttributedString(attributedString: Fields.flatten(content, info: info))
        // A selected floating picture becomes a normal picture in this standalone snapshot.
        text.enumerateAttribute(.docImage, in: NSRange(location: 0, length: text.length)) { value, range, _ in
            if let image = value as? DocImage {
                let props = image.duplicate()
                props.wrap = .inline
                text.addAttribute(.docImage, value: props, range: range)
            }
        }
        let storage = NSTextStorage(attributedString: text)
        let layout = NSLayoutManager()
        let maximum: CGFloat = 14_400
        let container = NSTextContainer(containerSize: NSSize(width: max(36, min(maximum, width)), height: maximum))
        container.lineFragmentPadding = 0
        layout.addTextContainer(container)
        storage.addLayoutManager(layout)
        let snapshot = NSTextView(frame: NSRect(x: 0, y: 0, width: container.size.width, height: maximum), textContainer: container)
        snapshot.textContainerInset = .zero
        snapshot.drawsBackground = true
        snapshot.backgroundColor = .white
        layout.ensureLayout(for: container)
        let glyphRange = layout.glyphRange(for: container)
        guard NSMaxRange(glyphRange) == layout.numberOfGlyphs else {
            throw NSError(domain: "DOC42.Labels", code: 1, userInfo: [NSLocalizedDescriptionKey: L("The selected content is too large for a label.")])
        }
        let used = layout.usedRect(for: container)
        let bounds = NSRect(x: 0, y: 0, width: max(1, ceil(used.maxX)), height: max(1, ceil(used.maxY)))
        let result = snapshot.dataWithPDF(inside: bounds)
        storage.removeLayoutManager(layout)
        return result
    }
}
