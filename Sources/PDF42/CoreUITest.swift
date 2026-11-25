import AppKit
import CoreText
import Kit42
import PDFKit

extension EditorWindowController {
    /// Navigation, view commands, command routing, committed export snapshots and stale OCR guards.
    func auditCore() -> Bool {
        guard let doc = pdfDoc, let window else { print("FAIL PDF core: missing document window"); return false }
        pdfView.cancelTextEdit(); pdfView.cancelInteraction(); pdfView.selectAnnotation(nil)
        let savedPDF = doc.pdf, savedExtras = doc.extras, savedUndo = doc.undoManager
        let savedMode = pdfView.displayMode, savedZoom = zoomMode
        let manager = UndoManager(); manager.groupsByEvent = false; doc.undoManager = manager
        var checks = 0, failures = 0
        func check(_ value: Bool, _ name: String) { checks += 1; if !value { failures += 1 }; print(value ? "PASS" : "FAIL", "PDF core:", name) }
        func action(_ body: () -> Void) { manager.beginUndoGrouping(); body(); manager.endUndoGrouping() }
        func fixture(_ lines: [[String]] = [["needle alpha needle", "SECRET REMOVE", "ORIGINAL TEXT"], ["needle beta", "PAGE TWO"], ["PAGE THREE"]]) -> PDFDocument {
            let bytes = NSMutableData(); var box = CGRect(x: 0, y: 0, width: 400, height: 600)
            let ctx = CGContext(consumer: CGDataConsumer(data: bytes)!, mediaBox: &box, nil)!
            for texts in lines {
                ctx.beginPDFPage(nil); ctx.setFillColor(NSColor.white.cgColor); ctx.fill(box)
                for (i, text) in texts.enumerated() {
                    ctx.textPosition = CGPoint(x: 40, y: 500 - i * 50)
                    CTLineDraw(PageOverlay.line(text, font: Self.textFont(22), color: .black), ctx)
                }
                ctx.endPDFPage()
            }
            ctx.closePDF(); return PDFDocument(data: bytes as Data)!
        }
        @discardableResult func reset(_ fresh: PDFDocument? = nil) -> PDFPage {
            pdfView.cancelTextEdit(); pdfView.cancelInteraction(); pdfView.selectAnnotation(nil)
            doc.pdf = fresh ?? fixture(); doc.extras = DocumentExtras(); replaceDocument(doc.pdf)
            pdfView.displayMode = .singlePage; zoomMode = .page
            window.makeKeyAndOrderFront(nil); window.makeFirstResponder(pdfView)
            pdfView.layoutDocumentView(); pdfView.go(to: doc.pdf.page(at: 0)!)
            manager.removeAllActions(); doc.updateChangeCount(.changeCleared)
            return doc.pdf.page(at: 0)!
        }
        defer {
            if Panels.allHidden(in: window) { Panels.toggleAll(in: window) }
            pdfView.cancelTextEdit(); pdfView.cancelInteraction(); pdfView.selectAnnotation(nil)
            doc.pdf = savedPDF; doc.extras = savedExtras; doc.undoManager = savedUndo; replaceDocument(savedPDF)
            pdfView.displayMode = savedMode; zoomMode = savedZoom
        }
        func send(_ command: Command) {
            let sender = command.menuItem()
            check(NSApp.sendAction(command.action, to: self, from: sender), "native command dispatch: " + command.title)
        }
        func commands(_ elements: [MenuElement]) -> [Command] {
            elements.flatMap { item -> [Command] in
                switch item { case .command(let command): return [command]; case .submenu(_, let children): return commands(children); default: return [] }
            }
        }
        func menuItems(_ menu: NSMenu) -> [NSMenuItem] { menu.items.flatMap { [$0] + ($0.submenu.map(menuItems) ?? []) } }
        func pump() { RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.06)) }
        var page = reset()
        // Activation is asynchronous when the audit app was launched directly from the command line.
        NSApp.activate(ignoringOtherApps: true); window.makeKeyAndOrderFront(nil); window.makeMain()
        let focusDeadline = Date(timeIntervalSinceNow: 0.8)
        while NSApp.keyWindow !== window && Date() < focusDeadline {
            if let event = NSApp.nextEvent(matching: .any, until: Date(timeIntervalSinceNow: 0.03), inMode: .default, dequeue: true) { NSApp.sendEvent(event) }
            window.makeKeyAndOrderFront(nil); window.makeMain()
        }
        print("PDF routing focus:", "active", NSApp.isActive, "key", NSApp.keyWindow === window, "main", NSApp.mainWindow === window)
        let spec = Cmd.menuBar
        let inventory = commands(spec.fileExtras + spec.editExtras + spec.find + spec.view + spec.insert + spec.format + spec.help + spec.custom.flatMap(\.items))
        let menus = NSApp.mainMenu.map(menuItems) ?? []
        let missing = inventory.filter { command in !menus.contains { $0.action == command.action && $0.tag == command.tag && $0.title == command.title } }
        check(missing.isEmpty, "all declared PDF menu commands are installed (routing inventory only)")
        let unwired = inventory.filter { NSApp.target(forAction: $0.action, to: nil, from: $0.menuItem()) == nil }
        if !unwired.isEmpty { print("Unwired commands:", unwired.map(\.title)) }
        check(unwired.isEmpty, "all declared commands resolve a responder (not a behavior assertion)")
        let toolbarItems = (window.toolbar?.items ?? []).flatMap { item in (item as? NSToolbarItemGroup)?.subitems ?? [item] }
        let brokenToolbar = toolbarItems.filter { item in item.action.map { NSApp.target(forAction: $0, to: item.target, from: item) == nil } ?? false }
        check(brokenToolbar.isEmpty, "toolbar actions resolve a responder")
        print("PDF command inventory:", inventory.count, "menu command declarations,", toolbarItems.count, "toolbar items")

        for (index, command) in Cmd.displayModes.enumerated() {
            send(command); check(pdfView.displayMode.rawValue == index, "display mode changes to " + command.title)
        }
        send(Cmd.displayModes[0]); pdfView.go(to: page)
        send(Cmd.actualSize); check(abs(pdfView.scaleFactor - 1) < 0.001 && !pdfView.autoScales, "Actual Size uses 100 percent")
        send(Cmd.zoomIn); let enlarged = pdfView.scaleFactor; check(enlarged > 1, "Zoom In enlarges the page")
        send(Cmd.zoomOut); check(pdfView.scaleFactor < enlarged, "Zoom Out reduces the page")
        send(Cmd.zoomToFit)
        let size = Self.displayedSize(page)
        let whole = min((pdfView.bounds.width - 32) / size.width, (pdfView.bounds.height - 32) / size.height)
        check(abs(pdfView.scaleFactor - whole) < 0.02, "Whole Page fits both page dimensions")
        send(Cmd.fitWidth)
        check(abs(pdfView.scaleFactor - (pdfView.bounds.width - 32) / size.width) < 0.02, "Fit Width uses the available width rather than whole-page fitting")
        send(Cmd.nextPage); check(currentIndex == 1, "Next Page navigates to page two")
        send(Cmd.previousPage); check(currentIndex == 0, "Previous Page returns to page one")
        let search = (window.toolbar?.items ?? []).compactMap { $0 as? NSSearchToolbarItem }.first?.searchField
        check(search != nil, "native toolbar Search field is present")
        if let search {
            search.stringValue = "needle"; _ = NSApp.sendAction(search.action ?? #selector(searchChanged(_:)), to: self, from: search); pump()
            let first = pdfView.currentSelection?.bounds(for: page)
            check(pdfView.currentSelection?.string?.lowercased() == "needle" && currentIndex == 0, "Search selects the first occurrence")
            send(Cmd.findNext); pump(); let second = pdfView.currentSelection?.bounds(for: page)
            check(second != nil && second != first && currentIndex == 0, "Find Next visits the second occurrence on the same page")
            send(Cmd.findNext); pump(); check(pdfView.currentSelection?.pages.first === doc.pdf.page(at: 1), "Find Next crosses to the next matching page")
            send(Cmd.findNext); pump(); check(pdfView.currentSelection?.bounds(for: page) == first, "Find Next wraps to the first occurrence")
            send(Cmd.findPrevious); pump(); check(pdfView.currentSelection?.pages.first === doc.pdf.page(at: 1), "Find Previous wraps to the last occurrence")
            search.stringValue = "no such phrase"; searchChanged(search)
            check(pdfView.highlightedSelections == nil && window.subtitle.contains(L(" · not found")), "a missing query removes highlights and reports not found")
            search.stringValue = ""; searchChanged(search)
            check(pdfView.highlightedSelections == nil && !window.subtitle.contains(L(" · not found")), "clearing Search removes its result state")
        }
        send(Cmd.thumbnails); check(sidebar.mode == .thumbnails, "Thumbnails chooses the thumbnail sidebar")
        send(Cmd.outline); check(sidebar.mode == .outline, "Contents chooses the outline sidebar")
        send(Cmd.attachments); check(sidebar.mode == .attachments, "Attachments chooses the attachment sidebar")
        let split = window.contentViewController as? NSSplitViewController
        for initiallyHidden in [false, true] {
            window.toolbar?.isVisible = true
            window.titlebarAccessoryViewControllers.forEach { $0.isHidden = initiallyHidden }
            split?.splitViewItems.first?.isCollapsed = initiallyHidden
            NSAnimationContext.runAnimationGroup { context in context.duration = 0; toggleAllPanelsAction(nil) }
            check(Panels.allHidden(in: window) && window.toolbar?.isVisible == false && window.titlebarAccessoryViewControllers.allSatisfy(\.isHidden), "Hide All Panels hides visible controls")
            NSAnimationContext.runAnimationGroup { context in context.duration = 0; toggleAllPanelsAction(nil) }
            check(!Panels.allHidden(in: window) && window.toolbar?.isVisible == true && window.titlebarAccessoryViewControllers.allSatisfy { $0.isHidden == initiallyHidden } && split?.splitViewItems.first?.isCollapsed == initiallyHidden, "Show All Panels restores the prior markup/sidebar visibility")
        }

        page = reset()
        let rootOutline = PDFOutline(), destination = PDFOutline()
        let destinationLabel = "Audit Destination"
        destination.label = destinationLabel; destination.destination = PDFDestination(page: page, at: CGPoint(x: 40, y: 500))
        rootOutline.insertChild(destination, at: 0); doc.pdf.outlineRoot = rootOutline
        doc.extras.labels = [PageLabelRange(start: 0, style: .decimal, prefix: "AUD-", first: 7)]
        doc.extras.dirty.formUnion([.outline, .labels])
        let secret = doc.pdf.findString("SECRET REMOVE", withOptions: []).first!
        let original = doc.pdf.findString("ORIGINAL TEXT", withOptions: []).first!
        action { add(RedactOverlay(bounds: secret.bounds(for: page).insetBy(dx: -2, dy: -2), color: .black), to: page, name: "Audit Redaction"); editPageText(original, on: page) }
        if let fieldEditor = pdfView.inlineEditor?.field.currentEditor() as? NSTextView { fieldEditor.insertText("NEW CONTENT", replacementRange: fieldEditor.selectedRange()) }
        check(pdfView.hasPendingTextEdit, "export fixture has an uncommitted native text draft")
        var snapshot: PDFDocument?
        do {
            manager.beginUndoGrouping(); defer { manager.endUndoGrouping() }
            snapshot = try exportSnapshot()
            check(snapshot?.string?.contains("NEW CONTENT") == true && snapshot?.string?.contains("ORIGINAL TEXT") == false && snapshot?.string?.contains("SECRET REMOVE") == false, "export snapshot commits visible text and removes redacted source text")
            check(pdfView.inlineEditor == nil, "export snapshot finishes the visible text editor")
            for format in ["txt", "rtf", "docx", "html"] {
                let data = try exportTextData(format: format)
                let output: String
                if format == "docx" { output = try ZipArchive(data: data).string("word/document.xml") ?? "" }
                else if format == "rtf" { output = try NSAttributedString(data: data, options: [.documentType: NSAttributedString.DocumentType.rtf], documentAttributes: nil).string }
                else { output = String(data: data, encoding: .utf8) ?? "" }
                check(output.contains("NEW CONTENT") && !output.contains("SECRET REMOVE") && !output.contains("ORIGINAL TEXT"), format.uppercased() + " exports current text without redacted/replaced content")
            }
            if let snapshot {
                let encrypted = try PDFOutput.protectedData(snapshot, password: "audit-password")
                let reopened = PDFDocument(data: encrypted)
                check(reopened?.isLocked == true, "protected export requires a password")
                check(reopened?.unlock(withPassword: "wrong") == false, "protected export rejects the wrong password")
                check(reopened?.unlock(withPassword: "audit-password") == true && reopened?.string?.contains("NEW CONTENT") == true && reopened?.string?.contains("SECRET REMOVE") == false, "correct password opens the redacted current snapshot")
                check(reopened?.outlineRoot?.child(at: 0)?.label == destinationLabel, "protected export preserves the saved outline")
                check(reopened?.page(at: 0)?.label == "AUD-7", "protected export preserves logical page labels")
            }
            if let overlay = page.annotations.compactMap({ $0 as? EditedTextOverlay }).first {
                editPageText(overlay, on: page)
                if let fieldEditor = pdfView.inlineEditor?.field.currentEditor() as? NSTextView { fieldEditor.insertText("PRINTED DRAFT", replacementRange: fieldEditor.selectedRange()) }
            }
            check(pdfView.hasPendingTextEdit, "Print fixture contains a visible uncommitted draft")
            let previousPrintInfo = doc.printInfo
            let printInfo = previousPrintInfo.copy() as! NSPrintInfo
            let printURL = FileManager.default.temporaryDirectory.appendingPathComponent("PDF42-print-audit-" + UUID().uuidString + ".pdf")
            defer { doc.printInfo = previousPrintInfo; try? FileManager.default.removeItem(at: printURL) }
            // Save disposition writes a temporary PDF. This audit never sends a job to a physical printer.
            printInfo.jobDisposition = .save
            printInfo.dictionary()[NSPrintInfo.AttributeKey.jobSavingURL] = printURL
            doc.printInfo = printInfo
            let operation = try doc.printOperation(withSettings: [:])
            operation.printInfo.jobDisposition = .save
            operation.printInfo.dictionary()[NSPrintInfo.AttributeKey.jobSavingURL] = printURL
            operation.showsPrintPanel = false; operation.showsProgressPanel = false
            let printed = operation.run()
            check(printed && FileManager.default.fileExists(atPath: printURL.path), "standard Print writes its PDF through the native save disposition")
            let printedPDF = PDFDocument(url: printURL)
            check(printedPDF?.string?.contains("PRINTED DRAFT") == true && printedPDF?.string?.contains("SECRET REMOVE") == false && printedPDF?.string?.contains("ORIGINAL TEXT") == false,
                  "printed PDF contains the visible draft and excludes redacted/replaced source text")
        } catch { check(false, "snapshot/text/protected export: " + error.localizedDescription) }
        page = reset()
        page.setBounds(CGRect(x: 20, y: 30, width: 300, height: 200), for: .cropBox); page.rotation = 90
        do {
            for jpeg in [false, true] {
                let data = try PDFOutput.imageData(page, dpi: 108, jpeg: jpeg)
                let bitmap = NSBitmapImageRep(data: data)
                check(bitmap?.pixelsWide == 300 && bitmap?.pixelsHigh == 450, "rotated cropped " + (jpeg ? "JPEG" : "PNG") + " export has exact pixel dimensions")
            }
        } catch { check(false, "image output: " + error.localizedDescription) }
        for dpi: CGFloat in [0, -1, .nan, .infinity, 1_000_000] {
            check(PDFOutput.imageSize(page, dpi: dpi) == nil, "invalid image resolution is rejected before allocation")
        }
        check(PageNumberSpec(start: Int.max).labels(for: [0, 1]).isEmpty, "overflowing page numbering is rejected instead of trapping")

        for size in [CGSize.zero, CGSize(width: -1, height: 72), CGSize(width: CGFloat.nan, height: 72), CGSize(width: 1e30, height: 72)] {
            check(Self.signatureAppearance(size: size, image: nil, name: "Audit", reason: "", date: Date()) == nil,
                  "visible-signature rendering rejects invalid or oversized geometry")
        }
        let signatureImage = Self.signatureAppearance(size: CGSize(width: 220, height: 72), image: nil, name: "Audit", reason: "", date: Date())
        check(signatureImage?.width == 660 && signatureImage?.height == 216, "visible-signature appearance renders at the intended resolution")
        page = reset()
        page.rotation = 90; page.setBounds(CGRect(x: 20, y: 30, width: 320, height: 520), for: .cropBox)
        let redaction = RedactOverlay(bounds: CGRect(x: 40, y: 40, width: 50, height: 40), color: .black)
        page.addAnnotation(redaction)
        let oldAnnotations = page.annotations.map(ObjectIdentifier.init), oldCrop = page.bounds(for: .cropBox)
        if let input = Self.ocrInput(page) {
            check(input !== page && input.rotation == 90 && input.bounds(for: .cropBox) == oldCrop, "OCR capture retains crop/rotation in an independent page")
            check(input.annotations.first is RedactOverlay && input.annotations.first !== redaction, "OCR capture retains an independent typed redaction overlay")
            let result = Self.ocrPage(input)
            check(result != nil && result?.rotation == 90 && result?.bounds(for: .cropBox) == oldCrop, "OCR output retains crop/rotation")
            check(page.rotation == 90 && page.bounds(for: .cropBox) == oldCrop && page.annotations.map(ObjectIdentifier.init) == oldAnnotations, "OCR rendering leaves live page geometry and annotation identities unchanged")
        } else { check(false, "OCR snapshot") }
        page = reset()
        check(captureOCR(indices: [-1]) == nil && captureOCR(indices: [doc.pdf.pageCount]) == nil, "OCR capture refuses invalid page indices")
        if let batch = captureOCR(indices: [0]) {
            check(isCurrentOCR(batch), "fresh OCR batch applies to the current document")
            doc.updateChangeCount(.changeDone)
            check(!isCurrentOCR(batch) && !applyOCR([(0, PDFPage.blank())], batch: batch), "edits invalidate an outstanding OCR result")
            check(doc.pdf.page(at: 0) === page, "stale OCR cannot replace the edited page")
        } else { check(false, "OCR revision fixture") }
        page = reset()
        if let batch = captureOCR(indices: [0]) {
            doc.pdf.exchangePage(at: 0, withPageAt: 1)
            check(!isCurrentOCR(batch) && !applyOCR([(0, PDFPage.blank())], batch: batch), "page reordering invalidates OCR indices even before a change-count update")
        } else { check(false, "OCR reorder fixture") }
        page = reset()
        if let batch = captureOCR(indices: [0]) {
            _ = reset()
            check(!isCurrentOCR(batch), "opening another document invalidates the previous OCR batch")
        } else { check(false, "OCR document fixture") }
        page = reset()
        if let old = captureOCR(indices: [0]), let latest = captureOCR(indices: [0]) {
            check(!isCurrentOCR(old) && isCurrentOCR(latest), "a newer OCR request supersedes its previous generation")
            let replacement = PDFPage.blank(size: CGSize(width: 400, height: 600))
            action { check(applyOCR([(0, replacement)], batch: latest), "unchanged OCR batch replaces its page") }
            check(doc.pdf.page(at: 0) === replacement, "OCR result is visible in the current document")
            manager.undo(); check(doc.pdf.page(at: 0) === page, "OCR Undo restores the original page")
            manager.redo(); check(doc.pdf.page(at: 0) === replacement, "OCR Redo restores the recognized page")
        } else { check(false, "OCR apply fixture") }
        print("PDF core UI regressions: \(checks - failures)/\(checks) passed")
        return failures == 0
    }
}
