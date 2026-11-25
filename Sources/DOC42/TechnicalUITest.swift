import AppKit
import Kit42
import PDFKit

extension DocWindowController {
    /// K42_SCRIPT=audittechnical: native text editing, complete Undo/Redo, saved DOCX and output snapshots.
    /// Uses disposable documents and save-to-file output only; never prints to a device.
    func auditTechnicalElements() -> Bool {
        var checks = 0, failures = 0
        func check(_ condition: Bool, _ name: String) {
            checks += 1
            if !condition { failures += 1 }
            print("\(condition ? "PASS" : "FAIL") DOC technical: \(name)")
        }
        let document = TextDocument()
        document.storage.setAttributedString(NSAttributedString(string: "Before\nAfter\n", attributes: Styles.normal.attributes))
        document.makeWindowControllers()
        guard let editor = document.editor, let undo = document.undoManager else { return false }
        let storage = document.storage, view = editor.textView
        undo.groupsByEvent = false
        let temporary = FileManager.default.temporaryDirectory.appendingPathComponent("DOC42-technical-\(UUID().uuidString).docx")
        defer {
            editor.headerEditor?.onCommit = nil
            editor.headerEditor?.commit()
            undo.removeAllActions()
            document.updateChangeCount(.changeCleared)
            document.close()
            editor.paged.detach()
            try? FileManager.default.removeItem(at: temporary)
        }
        func grouped(_ body: () -> Void) {
            view.breakUndoCoalescing()
            undo.beginUndoGrouping()
            body()
            undo.endUndoGrouping()
        }
        func pump(until condition: () -> Bool) {
            let deadline = Date().addingTimeInterval(3)
            while !condition() && Date() < deadline {
                RunLoop.current.run(until: Date().addingTimeInterval(0.01))
            }
        }
        func descendants(_ root: NSView) -> [NSView] { [root] + root.subviews.flatMap(descendants) }
        func insertThroughControls(_ element: TechnicalElement) -> Bool {
            guard let window = editor.window else { return false }
            editor.showWindow(nil)
            window.makeKeyAndOrderFront(nil)
            view.setSelectedRange(NSRange(location: 0, length: 0))
            guard NSApp.sendAction(#selector(insertTechnicalElementAction(_:)), to: editor, from: nil) else { return false }
            pump { window.attachedSheet != nil }
            guard let sheet = window.attachedSheet, let content = sheet.contentView else { return false }
            let controls = descendants(content)
            guard let mode = controls.compactMap({ $0 as? NSSegmentedControl }).first,
                  let value = controls.compactMap({ $0 as? NSTextField }).first(where: { $0.accessibilityLabel() == L("Code value") }),
                  let width = controls.compactMap({ $0 as? NSTextField }).first(where: { $0.accessibilityLabel() == L("Width in millimetres") }),
                  let insertButton = controls.compactMap({ $0 as? NSButton }).first(where: { $0.title == L("Insert") }) else {
                window.endSheet(sheet, returnCode: .alertSecondButtonReturn)
                return false
            }
            mode.selectedSegment = 1
            if let action = mode.action { _ = NSApp.sendAction(action, to: mode.target, from: mode) }
            value.stringValue = element.value
            width.stringValue = String(element.widthMM)
            value.delegate?.controlTextDidChange?(Notification(name: NSControl.textDidChangeNotification, object: value))
            width.delegate?.controlTextDidChange?(Notification(name: NSControl.textDidChangeNotification, object: width))
            guard insertButton.isEnabled else {
                window.endSheet(sheet, returnCode: .alertSecondButtonReturn)
                return false
            }
            insertButton.performClick(nil)
            pump { window.attachedSheet == nil && recipe(storage) == element }
            return recipe(storage) == element
        }
        func recipe(_ text: NSAttributedString, at index: Int = 0) -> TechnicalElement? {
            guard index >= 0, index < text.length,
                  let attachment = text.attribute(.attachment, at: index, effectiveRange: nil) as? NSTextAttachment,
                  let data = attachment.fileWrapper?.regularFileContents else { return nil }
            return TechnicalElement.fromPNG(data)
        }
        func hasVisibleInk(_ data: Data) -> Bool {
            guard let pdf = PDFDocument(data: data), let page = pdf.page(at: 0) else { return false }
            defer { withExtendedLifetime(pdf) {} }
            guard
                  let image = page.thumbnail(of: NSSize(width: 180, height: 180), for: .mediaBox).cgImage(forProposedRect: nil, context: nil, hints: nil) else { return false }
            let bitmap = NSBitmapImageRep(cgImage: image)
            var dark = 0
            for y in stride(from: 0, to: bitmap.pixelsHigh, by: 3) {
                for x in stride(from: 0, to: bitmap.pixelsWide, by: 3) {
                    if let c = bitmap.colorAt(x: x, y: y)?.usingColorSpace(.deviceRGB), c.redComponent < 0.5, c.greenComponent < 0.5, c.blueComponent < 0.5 { dark += 1 }
                }
            }
            return dark > 20
        }
        do {
            let first = TechnicalElement(value: "SN-0001", widthMM: 24)
            let firstRender = try first.render()
            let before = NSAttributedString(attributedString: storage)
            grouped {
                check(insertThroughControls(first), "Insert menu dispatch and native dialog controls insert requested code")
            }
            let inserted = NSAttributedString(attributedString: storage)
            let props = storage.attribute(.docImage, at: 0, effectiveRange: nil) as? DocImage
            check(storage.string == "\u{FFFC}Before\nAfter\n" && recipe(storage) == first, "complete value and surrounding text retained")
            check(props?.wrap == .inline && props?.ink == false, "new code is inline with normal image rendering")
            check((storage.attribute(.attachment, at: 0, effectiveRange: nil) as? NSTextAttachment)?.bounds.size == firstRender.sizePoints, "physical dimensions retained")
            for round in 1...2 {
                undo.undo()
                check(storage.isEqual(to: before), "insert Undo restores full attributed content \(round)")
                undo.redo()
                check(storage.isEqual(to: inserted), "insert Redo restores full attributed content \(round)")
            }
            let editItem = NSMenuItem(title: "", action: #selector(editTechnicalElementAction(_:)), keyEquivalent: "")
            view.setSelectedRange(NSRange(location: 0, length: 1))
            check(editor.validateMenuItem(editItem), "Edit command discovers code from selected attachment")
            var second = first
            second.value = "SN-0002"
            let secondRender = try second.render()
            grouped {
                check(editor.applyTechnicalElement(second, render: secondRender, in: view, range: NSRange(location: 0, length: 1), replacing: true), "edit replaces selected code")
            }
            let edited = NSAttributedString(attributedString: storage)
            check(recipe(storage) == second && storage.string == inserted.string, "edit changes recipe without replacing surrounding content")
            for round in 1...2 {
                undo.undo()
                check(storage.isEqual(to: inserted), "edit Undo restores image bytes and all attributes \(round)")
                undo.redo()
                check(storage.isEqual(to: edited), "edit Redo restores image bytes and all attributes \(round)")
            }

            let encoded = try document.exportData(.officeOpenXML)
            try encoded.write(to: temporary, options: .atomic)
            let reopened = TextDocument()
            try reopened.read(from: Data(contentsOf: temporary), ofType: "docx")
            reopened.makeWindowControllers()
            defer { reopened.updateChangeCount(.changeCleared); reopened.close(); reopened.editor?.paged.detach() }
            check(recipe(reopened.storage) == second && reopened.storage.string.contains("Before\nAfter"), "saved and reopened DOCX retains recipe and surrounding content")
            let bytes = (reopened.storage.attribute(.attachment, at: 0, effectiveRange: nil) as? NSTextAttachment)?.fileWrapper?.regularFileContents
            check(bytes == secondRender.pngData, "DOCX preserves exact PNG metadata bytes")
            if let reopenedEditor = reopened.editor, let reopenedUndo = reopened.undoManager {
                reopenedEditor.textView.setSelectedRange(NSRange(location: 0, length: 1))
                check(reopenedEditor.selectedTechnicalElement?.element.value == second.value, "reopened code is discoverable for editing")
                reopenedUndo.groupsByEvent = false
                reopenedUndo.beginUndoGrouping()
                reopenedEditor.applyTechnicalElement(first, render: firstRender, in: reopenedEditor.textView, range: NSRange(location: 0, length: 1), replacing: true)
                reopenedUndo.endUndoGrouping()
                check(recipe(reopened.storage) == first, "reopened code can be edited")
                reopenedUndo.undo()
                check(recipe(reopened.storage) == second, "reopened edit Undo restores saved value")
            }

            editor.selectImage(0)
            view.setSelectedRange(NSRange(location: 0, length: 1))
            let pictureSources = try editor.labelSources()
            check(pictureSources.count == 1 && pictureSources.first?.pdfData.map(hasVisibleInk) == true, "selected code produces visible PDF label artwork")
            check(storage.isEqual(to: edited), "label snapshot preserves full live content")
            let savedPicture = pictureSources.first?.pdfData
            grouped { editor.applyTechnicalElement(first, render: firstRender, in: view, range: NSRange(location: 0, length: 1), replacing: true) }
            check(pictureSources.first?.pdfData == savedPicture, "label source remains a frozen snapshot after editing")

            editor.selectImage(nil)
            let textRange = (storage.string as NSString).range(of: "Before")
            view.setSelectedRange(textRange)
            let textSources = try editor.labelSources()
            check(textSources.contains(where: { $0.text == "Before" }), "selected text is available as literal label text")
            check(textSources.compactMap(\.pdfData).contains(where: { PDFDocument(data: $0)?.string?.contains("Before") == true }), "selected formatted content renders readable PDF text")

            let title = Fields.reference(DocField(.title), attributes: Styles.normal.attributes)
            let snapshotTitle = "Controller passport"
            let titlePDF = try Self.labelContentPDF(title, width: 200, info: DocInfo(title: snapshotTitle))
            check(PDFDocument(data: titlePDF)?.string?.contains(snapshotTitle) == true, "formatted label snapshot resolves fields with source document metadata")

            let rows = [["Part", "Serial"], ["Module A", "000001"], ["Module B", "000002"]]
            let model = TableModel(rows: rows.map { row in row.map { TableModel.Cell(content: NSAttributedString(string: $0 + "\n", attributes: Styles.normal.attributes), props: CellProps()) } }, widths: [50, 50])
            storage.setAttributedString(model.render())
            editor.paged.layoutAll()
            editor.selectImage(nil)
            view.setSelectedRange(NSRange(location: 0, length: 0))
            undo.removeAllActions()
            let tableBefore = NSAttributedString(attributedString: storage)
            let tableSources = try editor.labelSources()
            check(tableSources.count == 1 && tableSources.first?.rows == rows, "table labels include complete raw rows with leading-zero identifiers")
            check(tableSources.first?.rowNumbers == [1, 2, 3] && tableSources.first?.firstRowIsHeader == true, "table sources retain row numbers and explicit header choice")
            check(storage.isEqual(to: tableBefore) && !undo.canUndo, "table extraction does not mutate document or Undo")

            let oversizedTable = NSTextTable()
            oversizedTable.numberOfColumns = 201
            let oversizedParagraph = NSMutableParagraphStyle()
            oversizedParagraph.textBlocks = [NSTextTableBlock(table: oversizedTable, startingRow: 0, rowSpan: 1, startingColumn: 0, columnSpan: 1)]
            storage.setAttributedString(NSAttributedString(string: "Sparse\n", attributes: [.paragraphStyle: oversizedParagraph]))
            view.setSelectedRange(NSRange(location: 0, length: 0))
            var rejected = false
            do { _ = try editor.labelTableSnapshot() } catch { rejected = true }
            check(rejected && storage.string == "Sparse\n" && !undo.canUndo, "oversized table source rejected before allocation without mutation")
            storage.setAttributedString(tableBefore)
            editor.paged.layoutAll()

            let imageBefore = NSAttributedString(attributedString: storage)
            editor.beginHeaderEdit(editor.paged.cards[0], header: true)
            if let header = editor.headerEditor {
                header.undo.groupsByEvent = false
                header.undo.beginUndoGrouping()
                editor.applyTechnicalElement(first, render: firstRender, in: header.textView, range: NSRange(location: 0, length: 0), replacing: false)
                header.undo.endUndoGrouping()
                check(recipe(header.textView.attributedString()) == first && storage.isEqual(to: imageBefore), "insertion targets active header without changing body")
                header.undo.undo()
                check(recipe(header.textView.attributedString()) == nil, "header uses its own Undo")
                header.undo.redo()
                check(recipe(header.textView.attributedString()) == first, "header Redo restores payload")
                header.onCommit = nil
                header.commit()
                editor.headerEditor = nil
            } else { check(false, "header fixture") }

            let setupBeforeExport = document.setup
            let ordinaryPDF = PDFDocument(data: try document.pdfData(PDFOptions()))
            let oldPrintInfo = document.printInfo.copy() as! NSPrintInfo
            defer {
                // Restoring a disposable fixture is not an editing command.
                undo.disableUndoRegistration()
                document.printInfo = oldPrintInfo
                undo.enableUndoRegistration()
            }
            let correction = PrintCorrection(offsetX: 3, offsetY: -2, scaleX: 1.03, scaleY: 0.98)
            ProductionPrint.setCorrection(correction, in: document.printInfo)
            let exportedPDF = PDFDocument(data: try document.pdfData(PDFOptions()))
            let ordinarySelection = ordinaryPDF?.findString("Module B", withOptions: []).first
            let exportedSelection = exportedPDF?.findString("Module B", withOptions: []).first
            if let ordinaryPage = ordinarySelection?.pages.first, let exportedPage = exportedSelection?.pages.first,
               let ordinaryBounds = ordinarySelection?.bounds(for: ordinaryPage),
               let exportedBounds = exportedSelection?.bounds(for: exportedPage) {
                check(abs(ordinaryBounds.minX - exportedBounds.minX) < 0.01 && abs(ordinaryBounds.minY - exportedBounds.minY) < 0.01 &&
                      abs(ordinaryBounds.width - exportedBounds.width) < 0.01 && abs(ordinaryBounds.height - exportedBounds.height) < 0.01,
                      "ordinary PDF export ignores retained printer calibration")
            } else { check(false, "export calibration text geometry fixture") }
            check(ordinaryPDF?.pageCount == exportedPDF?.pageCount && ordinaryPDF?.string == exportedPDF?.string && document.setup == setupBeforeExport,
                  "export calibration isolation preserves complete text, pagination and page setup")
            check(ProductionPrint.correction(in: document.printInfo) == correction, "export isolation leaves source print settings unchanged")
        } catch {
            check(false, "unexpected error: \(error.localizedDescription)")
        }
        print("DOC technical: \(checks - failures)/\(checks) passed")
        return failures == 0
    }
}
