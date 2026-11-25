import AppKit
import CoreText
import Kit42
import PDFKit

extension EditorWindowController {
    /// Actual tool actions, pointer gestures, modal controls, Undo/Redo and saved/rendered annotations.
    func auditMarkup() -> Bool {
        guard let doc = pdfDoc, let window else { print("FAIL PDF markup: document window missing"); return false }
        pdfView.cancelTextEdit(); pdfView.cancelInteraction(); pdfView.selectAnnotation(nil)
        let originalPDF = doc.pdf, originalUndo = doc.undoManager, originalZoom = zoomMode
        let manager = UndoManager(); manager.groupsByEvent = false; doc.undoManager = manager
        var checks = 0, failures = 0
        func check(_ value: Bool, _ name: String) {
            checks += 1; if !value { failures += 1 }
            print(value ? "PASS" : "FAIL", "PDF markup:", name)
        }
        func action(_ body: () -> Void) { manager.beginUndoGrouping(); body(); manager.endUndoGrouping() }
        // PDFKit's view/page coordinate conversion introduces sub-picometre roundoff at fractional zoom.
        func samePoint(_ a: CGPoint, _ b: CGPoint) -> Bool { abs(a.x - b.x) < 0.0000001 && abs(a.y - b.y) < 0.0000001 }
        func sameSize(_ a: CGSize, _ b: CGSize) -> Bool { abs(a.width - b.width) < 0.0000001 && abs(a.height - b.height) < 0.0000001 }
        func sameRect(_ a: CGRect, _ b: CGRect) -> Bool { samePoint(a.origin, b.origin) && sameSize(a.size, b.size) }
        func sameRGB(_ a: NSColor?, _ b: NSColor) -> Bool {
            guard let a = a?.usingColorSpace(.sRGB), let b = b.usingColorSpace(.sRGB) else { return false }
            return abs(a.redComponent - b.redComponent) < 0.0000001 && abs(a.greenComponent - b.greenComponent) < 0.0000001 && abs(a.blueComponent - b.blueComponent) < 0.0000001
        }
        func fixture() -> PDFDocument {
            let data = NSMutableData(); var box = CGRect(x: 0, y: 0, width: 600, height: 600)
            let ctx = CGContext(consumer: CGDataConsumer(data: data)!, mediaBox: &box, nil)!
            ctx.beginPDFPage(nil); ctx.setFillColor(NSColor.white.cgColor); ctx.fill(box)
            for (text, y) in [("SOURCE TEXT ONE", 470.0), ("SECOND LINE", 420.0)] {
                ctx.textPosition = CGPoint(x: 80, y: y)
                CTLineDraw(PageOverlay.line(text, font: Self.textFont(24), color: .black), ctx)
            }
            ctx.endPDFPage(); ctx.closePDF()
            return PDFDocument(data: data as Data)!
        }
        @discardableResult func reset(_ incoming: PDFDocument? = nil) -> PDFPage {
            pdfView.cancelTextEdit(); pdfView.cancelInteraction(); pdfView.selectAnnotation(nil)
            let fresh = incoming ?? fixture(); doc.pdf = fresh; replaceDocument(fresh)
            zoomMode = .custom; pdfView.autoScales = false; pdfView.scaleFactor = 0.8
            window.makeKeyAndOrderFront(nil); window.makeFirstResponder(pdfView)
            pdfView.layoutDocumentView(); pdfView.go(to: fresh.page(at: 0)!)
            setTool(.select); manager.removeAllActions(); doc.updateChangeCount(.changeCleared)
            return fresh.page(at: 0)!
        }
        defer {
            pdfView.cancelTextEdit(); pdfView.cancelInteraction(); pdfView.selectAnnotation(nil)
            doc.pdf = originalPDF; doc.undoManager = originalUndo; replaceDocument(originalPDF); zoomMode = originalZoom
        }
        func views(_ view: NSView) -> [NSView] { [view] + view.subviews.flatMap(views) }
        func allViews() -> [NSView] {
            (window.contentView.map(views) ?? []) + window.titlebarAccessoryViewControllers.flatMap { views($0.view) }
        }
        func send(_ selector: String, sender: Any? = nil) {
            action { _ = NSApp.sendAction(NSSelectorFromString(selector), to: self, from: sender) }
        }
        func choose(_ tool: Tool) {
            let item = NSMenuItem(); item.tag = tool.rawValue
            _ = NSApp.sendAction(#selector(selectToolAction(_:)), to: self, from: item)
        }
        func mouse(_ kind: NSEvent.EventType, _ point: CGPoint, on page: PDFPage, clicks: Int = 1) {
            let location = pdfView.convert(pdfView.convert(point, from: page), to: nil)
            guard let event = NSEvent.mouseEvent(with: kind, location: location, modifierFlags: [], timestamp: 0,
                windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: clicks, pressure: 1) else { return }
            if kind == .leftMouseDown { pdfView.mouseDown(with: event) }
            else if kind == .leftMouseDragged { pdfView.mouseDragged(with: event) }
            else { pdfView.mouseUp(with: event) }
        }
        func drag(_ from: CGPoint, _ to: CGPoint, on page: PDFPage) {
            mouse(.leftMouseDown, from, on: page); mouse(.leftMouseDragged, to, on: page); mouse(.leftMouseUp, to, on: page)
        }
        func key(_ value: String) {
            let event = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [], timestamp: 0,
                windowNumber: window.windowNumber, context: nil, characters: value, charactersIgnoringModifiers: value,
                isARepeat: false, keyCode: 0)!
            pdfView.keyDown(with: event)
        }
        func modal(_ button: String, fields: [String] = [], text: String? = nil, invoke: () -> Void) {
            var operated = false
            let timer = Timer(timeInterval: 0.06, repeats: false) { _ in
                guard let modal = NSApp.modalWindow, let content = modal.contentView else { NSApp.abortModal(); return }
                modal.makeFirstResponder(nil)
                let descendants = views(content)
                let inputs = descendants.compactMap { $0 as? NSTextField }.filter { $0.isEditable }
                for (field, value) in zip(inputs, fields) { field.stringValue = value }
                if let text, let editor = descendants.compactMap({ $0 as? NSTextView }).first(where: { !$0.isFieldEditor && $0.isEditable }) { editor.string = text }
                guard let done = descendants.compactMap({ $0 as? NSButton }).first(where: { $0.title == L(button) }) else { NSApp.abortModal(); return }
                operated = true; done.performClick(nil)
            }
            RunLoop.main.add(timer, forMode: .modalPanel)
            action(invoke); timer.invalidate(); check(operated, "native modal control: " + button)
        }
        func bitmap(_ page: PDFPage) -> CGImage? {
            guard let ctx = CGContext(data: nil, width: 600, height: 600, bitsPerComponent: 8, bytesPerRow: 0,
                space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return nil }
            ctx.setFillColor(NSColor.white.cgColor); ctx.fill(CGRect(x: 0, y: 0, width: 600, height: 600))
            page.draw(with: .mediaBox, to: ctx); return ctx.makeImage()
        }
        func darkPixels(_ page: PDFPage, in rect: CGRect) -> Int {
            guard let image = bitmap(page) else { return -1 }
            let pixels = NSBitmapImageRep(cgImage: image)
            var count = 0
            for y in Int(rect.minY)..<Int(rect.maxY) { for x in Int(rect.minX)..<Int(rect.maxX) {
                var c = [UInt](repeating: 0, count: 4); pixels.getPixel(&c, atX: x, y: 599 - y)
                let offset = pixels.bitmapFormat.contains(.alphaFirst) ? 1 : 0
                if c[offset] < 100 && c[offset + 1] < 100 && c[offset + 2] < 100 { count += 1 }
            } }
            return count
        }

        var page = reset()
        for tool in Tool.allCases {
            choose(tool); check(pdfView.tool == tool, "tool command selects " + tool.title)
        }
        for tool in [Tool.rectangle, .oval, .whiteout, .redact] {
            page = reset(); choose(tool)
            action { drag(CGPoint(x: 120, y: 140), CGPoint(x: 220, y: 200), on: page) }
            guard let annotation = page.annotations.first else { check(false, "draw " + tool.title); continue }
            print("PDF gesture geometry", tool.title, annotation.bounds)
            check(sameRect(annotation.bounds, CGRect(x: 120, y: 140, width: 100, height: 60)), "native drag creates " + tool.title)
            check(tool != .whiteout || annotation.isWhiteout, "whiteout retains its destructive-cover semantics")
            manager.undo(); check(page.annotations.isEmpty, tool.title + " creation Undo")
            manager.redo(); check(page.annotations.count == 1 && page.annotations[0] === annotation, tool.title + " creation Redo")
        }
        page = reset(); choose(.ink)
        action { mouse(.leftMouseDown, CGPoint(x: 100, y: 150), on: page); mouse(.leftMouseDragged, CGPoint(x: 140, y: 185), on: page); mouse(.leftMouseDragged, CGPoint(x: 200, y: 155), on: page); mouse(.leftMouseUp, CGPoint(x: 200, y: 155), on: page) }
        check(page.annotations.first?.isKind(.ink) == true && page.annotations.first?.paths?.first?.elementCount == 3, "Pencil creates the complete drawn path")
        manager.undo(); check(page.annotations.isEmpty, "Pencil Undo removes the stroke")
        manager.redo(); check(page.annotations.count == 1, "Pencil Redo restores the stroke")

        page = reset(); choose(.rectangle)
        mouse(.leftMouseDown, CGPoint(x: 120, y: 140), on: page); mouse(.leftMouseDragged, CGPoint(x: 220, y: 200), on: page)
        key("\u{1B}"); mouse(.leftMouseUp, CGPoint(x: 220, y: 200), on: page)
        check(page.annotations.isEmpty && !manager.canUndo, "Escape cancels a drawing preview without a phantom annotation or Undo")
        choose(.rectangle); mouse(.leftMouseDown, CGPoint(x: 120, y: 140), on: page); mouse(.leftMouseDragged, CGPoint(x: 220, y: 200), on: page)
        choose(.oval); mouse(.leftMouseUp, CGPoint(x: 220, y: 200), on: page)
        check(page.annotations.isEmpty && !manager.canUndo, "switching tools cancels the previous drawing preview")

        page = reset(); choose(.rectangle)
        action { drag(CGPoint(x: 120, y: 140), CGPoint(x: 220, y: 200), on: page) }
        let shape = page.annotations[0]; choose(.select); manager.removeAllActions()
        action { drag(CGPoint(x: 170, y: 170), CGPoint(x: 205, y: 195), on: page) }
        print("PDF move geometry", shape.bounds)
        check(sameRect(shape.bounds, CGRect(x: 155, y: 165, width: 100, height: 60)), "Select drag moves the annotation")
        manager.undo(); check(samePoint(shape.bounds.origin, CGPoint(x: 120, y: 140)), "move Undo restores original bounds")
        manager.redo(); check(samePoint(shape.bounds.origin, CGPoint(x: 155, y: 165)), "move Redo restores moved bounds")
        manager.removeAllActions()
        mouse(.leftMouseDown, CGPoint(x: 205, y: 195), on: page); mouse(.leftMouseDragged, CGPoint(x: 260, y: 250), on: page)
        key("\u{1B}"); mouse(.leftMouseUp, CGPoint(x: 260, y: 250), on: page)
        check(samePoint(shape.bounds.origin, CGPoint(x: 155, y: 165)) && !manager.canUndo, "Escape rolls a live move back without adding Undo")
        pdfView.selectAnnotation(shape, on: page)
        action { drag(CGPoint(x: shape.bounds.maxX + 3, y: shape.bounds.minY - 3), CGPoint(x: shape.bounds.maxX + 40, y: shape.bounds.minY - 30), on: page) }
        print("PDF resized geometry", shape.bounds)
        check(sameSize(shape.bounds.size, CGSize(width: 140, height: 90)), "corner handle resizes the annotation")
        manager.undo(); check(sameSize(shape.bounds.size, CGSize(width: 100, height: 60)), "resize Undo restores size")
        manager.redo(); check(sameSize(shape.bounds.size, CGSize(width: 140, height: 90)), "resize Redo restores size")
        let widthItem = NSMenuItem(); widthItem.tag = 4; send("widthChosen:", sender: widthItem)
        check(shape.border?.lineWidth == 8, "line-width menu changes the selected shape")
        manager.undo(); check(shape.border?.lineWidth == 2, "line width Undo restores the imported border")
        manager.redo(); check(shape.border?.lineWidth == 8, "line width Redo restores the selected thickness")
        let color = allViews().compactMap { $0 as? ColorButton }.first(where: { $0.action == NSSelectorFromString("colorChanged:") })
        color?.color = .blue; send("colorChanged:", sender: color)
        check(shape.color == .blue && color?.color == .blue, "native colour control changes the shape and reflects its value")
        manager.undo(); check(shape.color != .blue, "colour Undo restores the previous colour")
        manager.redo(); check(shape.color == .blue, "colour Redo restores the chosen colour")
        pdfView.selectAnnotation(nil)
        if let data = PageOverlay.bakedData(doc.pdf), let saved = PDFDocument(data: AcroFormFixer.ensureFields(data)) {
            check(saved.page(at: 0)?.annotations.first?.border?.lineWidth == 8, "saved and reopened PDF retains the changed border width")
        } else { check(false, "save styled annotation") }
        let lower = PDFAnnotation(bounds: CGRect(x: 50, y: 50, width: 70, height: 70), forType: .square, withProperties: nil)
        lower.color = .black; page.addAnnotation(lower)
        let beforeOrder = page.annotations.map(ObjectIdentifier.init)
        pdfView.selectAnnotation(shape, on: page); action { key("\u{7F}") }
        check(!page.annotations.contains(where: { $0 === shape }), "Delete key removes the selected annotation")
        manager.undo(); check(page.annotations.map(ObjectIdentifier.init) == beforeOrder, "Delete Undo restores the original annotation stacking order")
        manager.redo(); check(!page.annotations.contains(where: { $0 === shape }), "Delete Redo removes the annotation again")

        page = reset(); choose(.redact)
        action { drag(CGPoint(x: 100, y: 150), CGPoint(x: 200, y: 210), on: page) }
        let redaction = page.annotations[0]; choose(.select); pdfView.selectAnnotation(redaction, on: page)
        action { drag(CGPoint(x: 203, y: 147), CGPoint(x: 240, y: 145), on: page) }
        check(sameSize(redaction.bounds.size, CGSize(width: 140, height: 65)), "redaction handle changes width and height independently")

        for tool in [Tool.highlight, .underline, .strikeOut] {
            page = reset(); choose(tool)
            let selection = doc.pdf.findString("SOURCE TEXT ONE", withOptions: []).first!
            action { addMarkup(for: selection, tool: tool) }
            let annotation = page.annotations.first!
            check(annotation.isMarkup && annotation.quadrilateralPoints?.count == 4, tool.title + " creates a text-aligned markup quad")
            manager.undo(); check(page.annotations.isEmpty, tool.title + " Undo")
            manager.redo(); check(page.annotations.count == 1, tool.title + " Redo")
        }
        page = reset(); choose(.whiteout)
        pdfView.setCurrentSelection(doc.pdf.findString("SOURCE TEXT ONE", withOptions: []).first, animate: false)
        send("highlightSelectionAction:")
        print("PDF highlight colours", page.annotations.first?.color.usingColorSpace(.sRGB) as Any, Tool.highlight.defaultColor.usingColorSpace(.sRGB) as Any)
        check(sameRGB(page.annotations.first?.color, Tool.highlight.defaultColor) && abs((page.annotations.first?.color.alphaComponent ?? 0) - 0.45) < 0.0000001, "Highlight Selection uses the highlight colour even when Whiteout is selected")
        choose(.select)
        let highlight = page.annotations[0]; highlight.color = .yellow.withAlphaComponent(0.2)
        pdfView.selectAnnotation(highlight, on: page); color?.color = .green; send("colorChanged:", sender: color)
        check(abs(highlight.color.alphaComponent - 0.45) < 0.001, "chosen highlight colour uses the normal translucency")
        manager.undo(); check(abs(highlight.color.alphaComponent - 0.2) < 0.001, "recolour Undo preserves the original imported highlight alpha")
        manager.redo(); check(abs(highlight.color.alphaComponent - 0.45) < 0.001, "recolour Redo restores the new alpha")

        page = reset(); choose(.text)
        modal("Done", text: "Typed text box") { mouse(.leftMouseDown, CGPoint(x: 150, y: 250), on: page); mouse(.leftMouseUp, CGPoint(x: 150, y: 250), on: page) }
        let textBox = pdfView.selectedAnnotation
        check(textBox?.contents == "Typed text box" && textBox?.isKind(.freeText) == true, "Text Box modal creates editable text")
        let lineWidthControl = allViews().compactMap { $0 as? StripButton }.first(where: { $0.toolTip == L("Line width") })
        check(lineWidthControl?.isEnabled == false, "text boxes disable the unsupported border-width control")
        manager.undo(); check(page.annotations.isEmpty, "Text Box Undo")
        manager.redo(); check(page.annotations.contains(where: { $0 === textBox }), "Text Box Redo")
        if let textBox {
            textBox.font = Self.textFont(15); manager.removeAllActions()
            print("PDF unlisted font before dialog", textBox.font?.pointSize as Any)
            modal("Done", text: "Typed text box") { editAnnotationText(textBox, on: page) }
            print("PDF unlisted font after dialog", textBox.font?.pointSize as Any)
            check(textBox.font?.pointSize == 15, "unchanged text dialog preserves a font size absent from its presets")
            // Modal event groups themselves can leave an empty group; inspect the model and the change count separately.
            manager.removeAllActions(); doc.updateChangeCount(.changeCleared)
        }
        choose(.note)
        modal("Done", text: "A visible note") { mouse(.leftMouseDown, CGPoint(x: 280, y: 250), on: page); mouse(.leftMouseUp, CGPoint(x: 280, y: 250), on: page) }
        check(page.annotations.contains { $0.isKind(.text) && $0.contents == "A visible note" }, "Note modal creates the typed comment")
        manager.undo(); check(!page.annotations.contains { $0.isKind(.text) }, "Note Undo removes its comment")
        manager.redo(); check(page.annotations.contains { $0.isKind(.text) }, "Note Redo restores its comment")

        for (tool, name) in [(Tool.textField, "AuditText"), (.checkBox, "AuditCheck"), (.choiceField, "AuditChoice")] {
            page = reset(); choose(tool)
            modal("Add", fields: [name, "One, Two, Three"]) { drag(CGPoint(x: 100, y: 150), CGPoint(x: 240, y: 180), on: page) }
            guard let field = PDFTools.fields(doc.pdf).first?.1 else { check(false, "form field created"); continue }
            check(field.fieldName == name && field.isKind(.widget), "native form tool creates " + name)
            let original = field.bounds
            action { drag(CGPoint(x: field.bounds.midX, y: field.bounds.midY), CGPoint(x: field.bounds.midX + 30, y: field.bounds.midY + 40), on: page) }
            check(samePoint(field.bounds.origin, CGPoint(x: original.minX + 30, y: original.minY + 40)), "form tool moves its widget " + name)
            manager.undo(); check(field.bounds == original, "widget move Undo " + name)
            manager.redo(); check(field.bounds.minX == original.minX + 30, "widget move Redo " + name)
            action { key("\u{7F}") }; check(PDFTools.fields(doc.pdf).isEmpty, "Delete removes widget " + name)
            manager.undo(); check(PDFTools.fields(doc.pdf).first?.1.fieldName == name, "widget Delete Undo " + name)
        }

        // Open serialized AcroForm widgets, then fill them through PDFKit's native hit-testing/editors.
        let formFixture = fixture(), formPage = formFixture.page(at: 0)!
        formPage.addAnnotation(PDFTools.makeField(.checkBox, name: "ExistingCheck", bounds: CGRect(x: 70, y: 300, width: 24, height: 24)))
        formPage.addAnnotation(PDFTools.makeField(.text, name: "ExistingText", bounds: CGRect(x: 130, y: 295, width: 220, height: 30)))
        if let bytes = formFixture.dataRepresentation(), let opened = PDFDocument(data: AcroFormFixer.ensureFields(bytes)) {
            page = reset(opened); choose(.select)
            // The synchronous audit runs inside one outer AppKit event. Supply the same group boundary
            // that returning from each real input event creates; the nested event pump cannot end that outer event.
            func settleNativeEvents() {
                let deadline = Date(timeIntervalSinceNow: 0.08)
                while Date() < deadline {
                    if let event = NSApp.nextEvent(matching: .any, until: deadline, inMode: .default, dequeue: true) { NSApp.sendEvent(event) }
                }
                window.layoutIfNeeded(); pdfView.layoutDocumentView()
            }
            func nativeEvent(_ body: () -> Void) {
                // Complete deferred PDFKit page/widget layout before calculating window coordinates.
                settleNativeEvents()
                manager.beginUndoGrouping()
                body()
                manager.endUndoGrouping()
                settleNativeEvents()
                print("PDF native form event", "grouping", manager.groupingLevel, "undo", manager.canUndo, "revision", doc.revision, "dirty", doc.isDocumentEdited)
            }
            func clickWidget(_ annotation: PDFAnnotation) -> Bool {
                // Another application can take focus while PDFKit commits or serializes a field.
                // Re-establish the native input precondition before measuring coordinates or dispatching the click.
                let focusDeadline = Date(timeIntervalSinceNow: 0.8)
                repeat {
                    NSApp.activate(ignoringOtherApps: true)
                    window.makeKeyAndOrderFront(nil); window.makeMain()
                    settleNativeEvents()
                } while (!NSApp.isActive || !window.isKeyWindow) && Date() < focusDeadline
                guard NSApp.isActive, window.isKeyWindow else {
                    print("FAIL PDF markup: native form click requires an active key window", annotation.fieldName ?? "",
                          "active", NSApp.isActive, "key", window.isKeyWindow)
                    return false
                }
                let point = CGPoint(x: annotation.bounds.midX, y: annotation.bounds.midY)
                let location = pdfView.convert(pdfView.convert(point, from: page), to: nil)
                let timestamp = ProcessInfo.processInfo.systemUptime
                print("PDF native form click", annotation.fieldName ?? "", "page point", point, "window point", location,
                      "view bounds", pdfView.bounds, "key", window.isKeyWindow, "mapped page", pdfView.page(for: pdfView.convert(location, from: nil), nearest: false) === page)
                let down = NSEvent.mouseEvent(with: .leftMouseDown, location: location, modifierFlags: [], timestamp: timestamp,
                    windowNumber: window.windowNumber, context: nil, eventNumber: 1, clickCount: 1, pressure: 1)!
                let up = NSEvent.mouseEvent(with: .leftMouseUp, location: location, modifierFlags: [], timestamp: timestamp + 0.01,
                    windowNumber: window.windowNumber, context: nil, eventNumber: 2, clickCount: 1, pressure: 0)!
                // NSCell tracking may consume the mouse-up synchronously inside mouseDown.
                NSApp.postEvent(up, atStart: true)
                NSApp.sendEvent(down)
                if let pending = NSApp.nextEvent(matching: .leftMouseUp, until: Date(), inMode: .default, dequeue: true) { NSApp.sendEvent(pending) }
                return true
            }
            let fields = PDFTools.fields(doc.pdf).map(\.1)
            if let checkbox = fields.first(where: { $0.fieldName == "ExistingCheck" }), let textField = fields.first(where: { $0.fieldName == "ExistingText" }) {
                check(checkbox.buttonWidgetState == .offState && !doc.isDocumentEdited, "opened form starts with an unchecked field and clean document")
                let beforeCheckbox = captureOCR(indices: [0])
                var clickDelivered = false
                nativeEvent { clickDelivered = clickWidget(checkbox) }
                check(clickDelivered && checkbox.buttonWidgetState == .onState, "Select click fills an existing checkbox through PDFKit")
                check(doc.isDocumentEdited, "checkbox fill marks the document dirty")
                check(beforeCheckbox.map { !isCurrentOCR($0) } == true, "native checkbox input invalidates an older OCR batch")
                manager.undo(); check(checkbox.buttonWidgetState == .offState, "native checkbox Undo restores its value")
                manager.redo(); check(checkbox.buttonWidgetState == .onState, "native checkbox Redo restores its value")
                manager.removeAllActions(); doc.updateChangeCount(.changeCleared)
                nativeEvent {
                    clickDelivered = clickWidget(textField)
                    guard clickDelivered else { return }
                    if let editor = window.firstResponder as? NSTextView {
                        editor.selectAll(nil); editor.insertText("FILLED BY NATIVE EDITOR", replacementRange: editor.selectedRange())
                    } else { print("PDF form first responder:", String(describing: window.firstResponder)) }
                }
                check(clickDelivered && doc.isDocumentEdited, "typing in an existing form text field marks the document dirty")
                prepareForSave()
                settleNativeEvents()
                check(textField.widgetStringValue == "FILLED BY NATIVE EDITOR", "leaving the native form editor commits its visible text")
                if manager.canUndo { manager.undo() }
                check((textField.widgetStringValue ?? "").isEmpty, "native form text Undo restores the old field value")
                if manager.canRedo { manager.redo() }
                check(textField.widgetStringValue == "FILLED BY NATIVE EDITOR", "native form text Redo restores its value")
                do {
                    let bytes = try doc.data(ofType: "com.adobe.pdf")
                    let saved = PDFDocument(data: bytes)!
                    let reopenedFields = PDFTools.fields(saved).map(\.1)
                    check(reopenedFields.first(where: { $0.fieldName == "ExistingCheck" })?.buttonWidgetState == .onState, "Save/reopen preserves a natively filled checkbox")
                    check(reopenedFields.first(where: { $0.fieldName == "ExistingText" })?.widgetStringValue == "FILLED BY NATIVE EDITOR", "Save/reopen preserves natively typed form text")
                    nativeEvent { clickDelivered = clickWidget(checkbox) }
                    check(clickDelivered && checkbox.buttonWidgetState == .offState, "a second native click clears the checkbox")
                    let cleared = PDFDocument(data: try doc.data(ofType: "com.adobe.pdf"))!
                    check(PDFTools.fields(cleared).first(where: { $0.1.fieldName == "ExistingCheck" })?.1.buttonWidgetState == .offState, "Save/reopen preserves a natively cleared checkbox")
                } catch { check(false, "native form Save/reopen: " + error.localizedDescription) }
            } else { check(false, "opened native AcroForm fixtures exist") }
        } else { check(false, "serialized native AcroForm fixture opens") }

        page = reset()
        let run = doc.pdf.findString("SOURCE TEXT ONE", withOptions: []).first!
        action { editPageText(run, on: page) }
        check(!pdfView.hasPendingTextEdit, "opening Edit Text without changes stays clean")
        if let fieldEditor = pdfView.inlineEditor?.field.currentEditor() as? NSTextView { fieldEditor.insertText("REPLACEMENT TEXT", replacementRange: fieldEditor.selectedRange()) }
        check(pdfView.hasPendingTextEdit, "native inline typing participates in pending-edit state")
        action { pdfView.commitTextEdit() }
        let edited = page.annotations.compactMap { $0 as? EditedTextOverlay }.first
        check(edited?.text == "REPLACEMENT TEXT" && pdfView.inlineEditor == nil, "commit reads the active field editor's text")
        manager.undo(); check(page.annotations.isEmpty, "inline edit Undo restores the original page text")
        manager.redo(); check(page.annotations.compactMap { $0 as? EditedTextOverlay }.first?.text == "REPLACEMENT TEXT", "inline edit Redo restores the replacement")
        if let edited {
            choose(.select)
            let start = CGPoint(x: edited.bounds.midX, y: edited.bounds.midY)
            action { drag(start, CGPoint(x: start.x + 100, y: start.y - 100), on: page) }
            pdfView.selectAnnotation(nil)
            check(edited.bounds.origin != edited.originalCover.origin, "edited text can be moved independently of its source cover")
            check(darkPixels(page, in: CGRect(x: 78, y: 467, width: 230, height: 30)) < 10, "moving edited text keeps the original glyphs visually covered")
            if let data = PageOverlay.bakedData(doc.pdf), let saved = PDFDocument(data: data) {
                check(saved.string?.contains("REPLACEMENT TEXT") == true && saved.string?.contains("SOURCE TEXT ONE") == false, "saving a moved edit removes the original text and retains the replacement")
            } else { check(false, "save moved inline text") }
            action { editPageText(edited, on: page) }
            if let editor = pdfView.inlineEditor?.field.currentEditor() as? NSTextView { editor.insertText("CANCELLED", replacementRange: editor.selectedRange()) }
            pdfView.cancelTextEdit()
            check(edited.text == "REPLACEMENT TEXT" && pdfView.inlineEditor == nil, "cancelling inline re-edit preserves the previous text")
        }
        page = reset()
        let imageContext = CGContext(data: nil, width: 10, height: 1000, bitsPerComponent: 8, bytesPerRow: 0,
            space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
        imageContext.setFillColor(NSColor.black.cgColor); imageContext.fill(CGRect(x: 0, y: 0, width: 10, height: 1000))
        action { insertImage(imageContext.makeImage()!) }
        check(pdfView.selectedAnnotation.map { page.bounds(for: .cropBox).contains($0.bounds) } == true, "a tall inserted stamp fits inside the visible page")
        check(color?.isEnabled == false, "a raster stamp does not expose an ineffective recolour button")
        print("PDF markup UI regressions: \(checks - failures)/\(checks) passed")
        return failures == 0
    }
}
