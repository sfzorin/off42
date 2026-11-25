import AppKit
import CoreText
import Kit42
import PDFKit

extension EditorWindowController {
    /// Disposable native controls/gestures, exact Undo, persistence and print-to-file (no device).
    func auditTechnical() -> Bool {
        guard let doc = pdfDoc, let window else { print("FAIL PDF technical: missing document window"); return false }
        prepareForSave()
        let original = doc.pdf, extras = doc.extras, originalUndo = doc.undoManager, originalZoom = zoomMode
        let manager = UndoManager(); manager.groupsByEvent = false; doc.undoManager = manager
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("PDF42-technical-" + UUID().uuidString)
        var checks = 0, failures = 0
        func check(_ value: Bool, _ name: String) { checks += 1; if !value { failures += 1 }; print(value ? "PASS" : "FAIL", "PDF technical:", name) }
        func grouped<T>(_ body: () throws -> T) rethrows -> T { manager.beginUndoGrouping(); defer { manager.endUndoGrouping() }; return try body() }
        func pump(until condition: () -> Bool) {
            let deadline = Date().addingTimeInterval(4)
            while !condition() && Date() < deadline { RunLoop.current.run(until: Date().addingTimeInterval(0.02)) }
        }
        func views(_ view: NSView) -> [NSView] { [view] + view.subviews.flatMap(views) }
        func closeSheet() {
            guard let sheet = window.attachedSheet else { return }
            if let button = sheet.contentView.map(views)?.compactMap({ $0 as? NSButton }).first(where: { $0.title == L("Close") }) { button.performClick(nil) }
            else { window.endSheet(sheet, returnCode: .alertSecondButtonReturn); sheet.orderOut(nil) }
            pump { window.attachedSheet == nil }
        }
        defer {
            closeSheet(); pdfView.cancelTextEdit(); pdfView.cancelInteraction(); pdfView.selectAnnotation(nil)
            doc.pdf = original; doc.extras = extras; doc.undoManager = originalUndo; replaceDocument(original); zoomMode = originalZoom
            try? FileManager.default.removeItem(at: directory)
        }
        func fixture(text: Bool = false) -> PDFDocument {
            let data = NSMutableData(); var box = CGRect(x: 0, y: 0, width: 400, height: 500)
            let context = CGContext(consumer: CGDataConsumer(data: data)!, mediaBox: &box, nil)!
            context.beginPDFPage(nil); context.setFillColor(NSColor.white.cgColor); context.fill(box)
            if text {
                for (value, y) in [("SECRET REMOVE", 350.0), ("ORIGINAL TEXT", 290.0)] {
                    context.textPosition = CGPoint(x: 80, y: y)
                    CTLineDraw(PageOverlay.line(value, font: Self.textFont(18), color: .black), context)
                }
            }
            context.endPDFPage(); context.closePDF(); return PDFDocument(data: data as Data)!
        }
        @discardableResult func reset(_ incoming: PDFDocument) -> PDFPage {
            closeSheet(); pdfView.cancelTextEdit(); pdfView.cancelInteraction(); pdfView.selectAnnotation(nil)
            doc.pdf = incoming; doc.extras = DocumentExtras(); replaceDocument(incoming)
            zoomMode = .page; pdfView.displayMode = .singlePage; pdfView.layoutDocumentView()
            let page = incoming.page(at: 0)!; pdfView.go(to: page); setTool(.select)
            manager.removeAllActions(); doc.updateChangeCount(.changeCleared)
            window.makeKeyAndOrderFront(nil); window.makeFirstResponder(pdfView)
            return page
        }
        func send(_ command: Command) { check(NSApp.sendAction(command.action, to: self, from: command.menuItem()), "native action: " + command.title) }
        func editSheet(_ element: TechnicalElement, button: String) -> Bool {
            pump { window.attachedSheet != nil }
            guard let content = window.attachedSheet?.contentView else { return false }
            let controls = views(content)
            guard let mode = controls.compactMap({ $0 as? NSSegmentedControl }).first,
                  let value = controls.compactMap({ $0 as? NSTextField }).first(where: { $0.accessibilityLabel() == L("Code value") }),
                  let width = controls.compactMap({ $0 as? NSTextField }).first(where: { $0.accessibilityLabel() == L("Width in millimetres") }),
                  let types = controls.compactMap({ $0 as? NSPopUpButton }).first(where: { $0.accessibilityLabel() == L("Code type") }),
                  let accept = controls.compactMap({ $0 as? NSButton }).first(where: { $0.title == L(button) }) else { closeSheet(); return false }
            mode.selectedSegment = 1
            value.stringValue = element.value; width.stringValue = String(element.widthMM)
            types.selectItem(at: TechnicalCodeType.allCases.firstIndex(of: element.codeType)!)
            controls.compactMap { $0 as? NSButton }.first { $0.title == L("Show readable value below code") }?.state = element.caption ? .on : .off
            if let action = mode.action { _ = NSApp.sendAction(action, to: mode.target, from: mode) }
            width.delegate?.controlTextDidChange?(Notification(name: NSControl.textDidChangeNotification, object: width))
            guard accept.isEnabled else { closeSheet(); return false }
            accept.performClick(nil); pump { window.attachedSheet == nil }
            return window.attachedSheet == nil
        }
        func mouse(_ kind: NSEvent.EventType, _ point: CGPoint, on page: PDFPage, clicks: Int = 1) {
            let location = pdfView.convert(pdfView.convert(point, from: page), to: nil)
            let event = NSEvent.mouseEvent(with: kind, location: location, modifierFlags: [], timestamp: 0, windowNumber: window.windowNumber,
                                          context: nil, eventNumber: 0, clickCount: clicks, pressure: 1)!
            if kind == .leftMouseDown { pdfView.mouseDown(with: event) }
            else if kind == .leftMouseDragged { pdfView.mouseDragged(with: event) }
            else { pdfView.mouseUp(with: event) }
        }
        func bitmap(_ data: Data) -> NSBitmapImageRep? {
            guard let document = PDFDocument(data: data), let page = document.page(at: 0) else { return nil }
            return withExtendedLifetime(document) {
                guard let image = page.thumbnail(of: NSSize(width: 600, height: 800), for: .mediaBox).tiffRepresentation else { return nil }
                return NSBitmapImageRep(data: image)
            }
        }
        func pixelCounts(_ data: Data) -> (black: Int, red: Int) {
            guard let image = bitmap(data) else { return (0, 0) }
            var black = 0, red = 0
            for y in stride(from: 0, to: image.pixelsHigh, by: 2) { for x in stride(from: 0, to: image.pixelsWide, by: 2) {
                guard let c = image.colorAt(x: x, y: y)?.usingColorSpace(.deviceRGB) else { continue }
                if c.redComponent + c.greenComponent + c.blueComponent < 1 { black += 1 }
                if c.redComponent > 0.7 && c.greenComponent < 0.35 && c.blueComponent < 0.35 { red += 1 }
            } }
            return (black, red)
        }
        func vectorOnly(_ data: Data) -> Bool { String(decoding: data, as: UTF8.self).range(of: #"/Subtype\s*/Image"#, options: .regularExpression) == nil }
        func nativeLabelsPreview() -> PDFDocument? {
            pump {
                guard let content = window.attachedSheet?.contentView else { return false }
                return views(content).compactMap { $0 as? PDFView }.first?.document != nil
            }
            let output = window.attachedSheet?.contentView.map(views)?.compactMap { $0 as? PDFView }.first?.document
            if output == nil {
                print("PDF technical label diagnostic:", "sheet", window.attachedSheet?.title ?? "none", "tool", pdfView.tool, "region", pdfView.labelRegionSelection,
                      "fields", window.attachedSheet?.contentView.map(views)?.compactMap { ($0 as? NSTextField)?.stringValue } ?? [])
            }
            return output
        }
        do {
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
            var page = reset(fixture())
            let before = PDFAnnotation(bounds: CGRect(x: 40, y: 60, width: 30, height: 25), forType: .square, withProperties: nil)
            before.color = .red; before.interiorColor = .red; page.addAnnotation(before)
            let first = TechnicalElement(value: "PDF-0007", widthMM: 28, caption: false), firstRender = try first.render()
            grouped { send(Cmd.technicalElement); check(editSheet(first, button: "Insert"), "native code dialog accepts value and physical width") }
            guard let inserted = page.annotations.first(where: { $0.technicalElement != nil }) else { throw TechnicalElementError("Audit code was not inserted") }
            let firstBounds = inserted.bounds
            check(abs(firstBounds.width - firstRender.sizePoints.width) < 0.001 && abs(firstBounds.midX - 200) < 0.001 && abs(firstBounds.midY - 250) < 0.001, "code is centered at the requested physical size")
            check(inserted.technicalElement?.value == first.value && inserted is TechnicalElementAnnotation, "insert retains recipe and vector subclass")
            manager.undo(); check(page.annotations.filter(\.shouldPrint).count == 1 && page.annotations.contains { $0 === before }, "insert Undo retains previous annotation")
            manager.redo(); check(page.annotations.contains { $0 === inserted }, "insert Redo restores same annotation")
            let after = PDFAnnotation(bounds: CGRect(x: 300, y: 420, width: 20, height: 20), forType: .circle, withProperties: nil)
            page.addAnnotation(after); pdfView.selectAnnotation(inserted, on: page)
            var edited = first; edited.value = "PDF-0008"; edited.codeType = .dataMatrix; edited.widthMM = 24
            grouped { send(Cmd.editTechnicalElement); check(editSheet(edited, button: "Apply"), "native edit controls replace selected code") }
            guard let changed = page.annotations.first(where: { $0.technicalElement != nil }) else { throw TechnicalElementError("Audit edit is missing") }
            check(changed.technicalElement?.value == edited.value && changed.bounds.origin == firstBounds.origin, "edit retains position and changes payload")
            check(page.annotations.filter(\.shouldPrint).elementsEqual([before, changed, after], by: { $0 === $1 }), "edit retains annotation drawing order")
            manager.undo(); check(page.annotations.filter(\.shouldPrint).elementsEqual([before, inserted, after], by: { $0 === $1 }) && inserted.bounds == firstBounds, "edit Undo restores exact object, bounds and order")
            manager.redo(); check(page.annotations.filter(\.shouldPrint).elementsEqual([before, changed, after], by: { $0 === $1 }), "edit Redo restores changed object and order")
            check(changed.duplicateForEditing(on: page) is TechnicalElementAnnotation, "page editing copies preserve the vector subclass")
            let saved = try doc.data(ofType: "com.adobe.pdf"), url = directory.appendingPathComponent("codes.pdf")
            try saved.write(to: url, options: .atomic)
            check(vectorOnly(saved) && pixelCounts(saved).black > 100, "saved code has vector appearance and visible black modules")
            let reopened = PDFDocument(url: url)!
            page = reset(reopened)
            guard let generic = page.annotations.first(where: { $0.technicalElement != nil }) else { throw TechnicalElementError("Saved recipe is missing") }
            check(generic.technicalElement?.value == edited.value && generic.technicalElement?.codeType == .dataMatrix, "save/reopen retains recipe on ordinary PDFAnnotation")
            grouped {
                mouse(.leftMouseDown, CGPoint(x: generic.bounds.midX, y: generic.bounds.midY), on: page, clicks: 2)
                check(editSheet(first, button: "Apply"), "native double-click edits the reopened code")
            }
            check(page.annotations.contains { $0.technicalElement?.value == first.value }, "reopened edit applies new payload")
            manager.undo(); check(page.annotations.contains { $0 === generic } && generic.technicalElement?.value == edited.value, "reopened edit Undo retains original appearance")
            manager.redo()
            let countBeforeOversized = page.annotations.count
            do {
                let large = TechnicalElement(value: "TOO-LARGE", widthMM: 200, caption: false)
                try insertTechnicalElement(large, render: large.render(), on: page)
                check(false, "oversized insertion is rejected")
            } catch { check(true, "oversized insertion is rejected without silent scaling") }
            check(page.annotations.count == countBeforeOversized, "oversized insertion is atomic")

            // The full-page native print route must retain both saved stamp appearances and markup.
            let operation = try doc.printOperation(withSettings: [:]), printURL = directory.appendingPathComponent("print.pdf")
            check(operation.printPanel.accessoryControllers.contains { $0.title == L("Calibration") }, "native print includes calibration accessory")
            operation.printInfo.jobDisposition = .save; operation.printInfo.dictionary()[NSPrintInfo.AttributeKey.jobSavingURL] = printURL
            ProductionPrint.setCorrection(nil, in: operation.printInfo)
            operation.showsPrintPanel = false; operation.showsProgressPanel = false
            check(operation.run(), "native PDF print operation saves to file")
            let printed = try Data(contentsOf: printURL), printPixels = pixelCounts(printed)
            check(printPixels.black > 100 && printPixels.red > 30, "print-to-file retains code appearance and red annotation")

            // Export commits visible text and removes redacted content before building either source.
            page = reset(fixture(text: true))
            let secret = doc.pdf.findString("SECRET REMOVE", withOptions: []).first!, originalText = doc.pdf.findString("ORIGINAL TEXT", withOptions: []).first!
            grouped {
                add(RedactOverlay(bounds: secret.bounds(for: page).insetBy(dx: -2, dy: -2), color: .black), to: page, name: "Audit Redaction")
                editPageText(originalText, on: page)
            }
            if let editor = pdfView.inlineEditor?.field.currentEditor() as? NSTextView { editor.insertText("CURRENT CONTENT", replacementRange: editor.selectedRange()) }
            check(pdfView.hasPendingTextEdit, "label fixture includes a pending native text edit")
            let full = try grouped { try makePageLabelSource(page) }
            let fullText = full.pdfData.flatMap(PDFDocument.init(data:))?.string ?? ""
            check(fullText.contains("CURRENT CONTENT") && !fullText.contains("SECRET REMOVE") && !fullText.contains("ORIGINAL TEXT"), "page labels use committed edits and destructive redaction snapshot")
            page.rotation = 90; let crop = CGRect(x: 20, y: 30, width: 320, height: 400); page.setBounds(crop, for: .cropBox)
            let region = CGRect(x: 40, y: 100, width: 270, height: 320)
            let regionSource = try makePageLabelSource(page, region: region)
            let regionPDF = regionSource.pdfData.flatMap(PDFDocument.init(data:))
            check(regionPDF?.page(at: 0)?.bounds(for: .mediaBox).size == CGSize(width: 320, height: 270), "region snapshot applies rotation at exact physical size")
            check(regionPDF?.string?.contains("CURRENT CONTENT") == true && regionPDF?.string?.contains("SECRET REMOVE") == false, "region snapshot retains current rendered content without redacted text")
            check(page.rotation == 90 && page.bounds(for: .cropBox) == crop, "source geometry remains unchanged after both snapshots")

            manager.removeAllActions(); doc.updateChangeCount(.changeCleared); pdfView.layoutDocumentView(); pdfView.go(to: page)
            send(Cmd.createLabels)
            pump { window.attachedSheet != nil }
            let chooser = window.attachedSheet?.contentView.map(views)?.compactMap { $0 as? NSButton }.first { $0.title == L("Select Region") }
            check(chooser != nil, "Create Labels exposes compact page/region chooser")
            chooser?.performClick(nil); pump { pdfView.labelRegionSelection && window.attachedSheet == nil }
            check(pdfView.labelRegionSelection && pdfView.tool == .crop, "chooser enters transient region gesture")
            mouse(.leftMouseDown, region.origin, on: page)
            mouse(.leftMouseDragged, CGPoint(x: region.maxX, y: region.maxY), on: page)
            mouse(.leftMouseUp, CGPoint(x: region.maxX, y: region.maxY), on: page)
            let preview = nativeLabelsPreview()
            check(preview?.pageCount ?? 0 > 0 && !pdfView.labelRegionSelection && pdfView.tool == .select, "native region drag opens generated labels and returns to selection")
            check(page.bounds(for: .cropBox) == crop && page.rotation == 90 && !doc.isDocumentEdited && !manager.canUndo, "region gesture does not crop or dirty the source")
            if let data = preview?.dataRepresentation() {
                let labelsURL = directory.appendingPathComponent("labels.pdf")
                try data.write(to: labelsURL, options: .atomic)
                let labels = PDFDocument(url: labelsURL)
                check(labels?.pageCount ?? 0 > 0 && labels?.string?.contains("CURRENT CONTENT") == true && labels?.string?.contains("SECRET REMOVE") == false, "generated label preview saves and reopens with current safe content")
                check(vectorOnly(data), "label output retains vector content")
            } else { check(false, "label output is available for saving") }
            closeSheet()
            send(Cmd.createLabels); pump { window.attachedSheet != nil }
            window.attachedSheet?.contentView.map(views)?.compactMap { $0 as? NSButton }.first { $0.title == L("Current Page") }?.performClick(nil)
            check(nativeLabelsPreview() != nil, "Current Page chooser generates labels through native dialog")
            closeSheet()
        } catch { check(false, "scenario completed: " + error.localizedDescription) }
        print("PDF technical: \(checks) checks, \(failures) failures")
        return failures == 0
    }
}
