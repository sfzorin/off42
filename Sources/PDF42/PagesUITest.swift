import AppKit
import CoreText
import Kit42
import PDFKit

extension EditorWindowController {
    /// Native page/sidebar commands and modal controls, including complete Undo and emitted PDF state.
    func auditPages() -> Bool {
        guard let doc = pdfDoc, let window else { print("FAIL PDF pages: no document window"); return false }
        prepareForSave()
        let original = doc.pdf, extras = doc.extras, originalUndo = doc.undoManager
        let originalMode = sidebar.mode, originalZoom = zoomMode
        let manager = UndoManager(); manager.groupsByEvent = false; doc.undoManager = manager
        var checks = 0, failures = 0
        func check(_ ok: Bool, _ name: String) { checks += 1; if !ok { failures += 1 }; print(ok ? "PASS" : "FAIL", "PDF pages:", name) }
        func event(_ block: () -> Void) { manager.beginUndoGrouping(); block(); manager.endUndoGrouping() }
        func send(_ selector: Selector, tag: Int = 0) {
            let item = NSMenuItem(title: "", action: selector, keyEquivalent: ""); item.tag = tag
            event { _ = NSApp.sendAction(selector, to: self, from: item) }
        }
        func views(_ view: NSView) -> [NSView] { [view] + view.subviews.flatMap(views) }
        func fields(_ view: NSView) -> [NSTextField] { views(view).compactMap { $0 as? NSTextField }.filter(\.isEditable) }
        func modal(_ button: String, edit: @escaping (NSView) -> Void, run: () -> Void) {
            let started = Date(); var handled = false
            let timer = Timer(timeInterval: 0.04, repeats: true) { timer in
                guard let dialog = NSApp.modalWindow, let content = dialog.contentView else {
                    if Date().timeIntervalSince(started) > 2 { timer.invalidate(); NSApp.abortModal() }
                    return
                }
                guard !handled else { return }; handled = true; edit(content)
                if let accept = views(content).compactMap({ $0 as? NSButton }).first(where: { $0.title == button }) {
                    accept.performClick(nil)
                } else { print("FAIL PDF pages: missing modal button", button); NSApp.abortModal() }
                timer.invalidate()
            }
            RunLoop.current.add(timer, forMode: .modalPanel)
            event(run)
            timer.invalidate()
            check(handled, "native dialog opened: " + button)
        }
        func fixture(_ count: Int = 4) -> PDFDocument {
            let data = NSMutableData(); var box = CGRect(x: 0, y: 0, width: 320, height: 240)
            let consumer = CGDataConsumer(data: data)!, context = CGContext(consumer: consumer, mediaBox: &box, nil)!
            for i in 0..<count {
                context.beginPDFPage(nil); context.textPosition = CGPoint(x: 30, y: 170)
                let line = CTLineCreateWithAttributedString(NSAttributedString(string: "Page-\(i + 1) audit text", attributes: [.font: NSFont.systemFont(ofSize: 18)]))
                CTLineDraw(line, context); context.endPDFPage()
            }
            context.closePDF(); return PDFDocument(data: data as Data)!
        }
        func reset(_ count: Int = 4) {
            window.makeFirstResponder(pdfView)
            doc.pdf = fixture(count); doc.extras = DocumentExtras(); replaceDocument(doc.pdf)
            pdfView.displayMode = .singlePage; sidebar.mode = .thumbnails
            if let page = pdf?.page(at: 0) { pdfView.go(to: page); sidebar.thumbnails.selectPages([page]) }
            manager.removeAllActions(); doc.updateChangeCount(.changeCleared)
        }
        func pages() -> [PDFPage] { guard let pdf else { return [] }; return (0..<pdf.pageCount).compactMap { pdf.page(at: $0) } }
        func order() -> [String] { pages().map { $0.string?.trimmingCharacters(in: .whitespacesAndNewlines) ?? "" } }
        defer {
            window.makeFirstResponder(pdfView); doc.pdf = original; doc.extras = extras; doc.undoManager = originalUndo
            replaceDocument(original); sidebar.mode = originalMode; zoomMode = originalZoom
        }
        reset(); window.makeKeyAndOrderFront(nil)
        let originalPages = pages(), originalOrder = order()
        sidebar.thumbnails.selectPages([originalPages[1], originalPages[2]]); pdfView.go(to: originalPages[1])
        send(#selector(movePageDownAction(_:)))
        check(order() == [originalOrder[0], originalOrder[3], originalOrder[1], originalOrder[2]], "Move Down moves every selected page together")
        check(sidebar.thumbnails.selectedPages.count == 2 && pdfView.currentPage === originalPages[1], "reorder retains multi-selection and current page identity")
        manager.undo(); check(order() == originalOrder && targetIndices == [1, 2], "page reorder Undo restores order and complete selection")
        manager.redo(); check(targetIndices == [2, 3], "page reorder Redo restores moved selection")
        send(#selector(movePageUpAction(_:))); check(order() == originalOrder, "Move Up moves the selected block back")
        event { movePages([-1, 2, 2, Int.max], to: Int.max) }
        check(order() == [originalOrder[0], originalOrder[1], originalOrder[3], originalOrder[2]] && pdf?.pageCount == 4, "drag reorder sanitizes negative, duplicate and stale indices")
        manager.undo(); check(order() == originalOrder, "sanitized drag move is undoable")
        reset(); let same = order(); movePages([-1, Int.max], to: Int.min)
        check(order() == same && !manager.canUndo, "invalid drag creates no mutation or Undo")
        let selected = [pages()[1], pages()[3]]; sidebar.thumbnails.selectPages(selected)
        send(#selector(rotateRightAction(_:)))
        check(pages().map(\.rotation) == [0, 90, 0, 90] && targetIndices == [1, 3], "Rotate Right applies to selected pages and retains selection")
        manager.undo(); check(pages().allSatisfy { $0.rotation == 0 }, "multi-page rotation Undo restores every page")
        manager.redo(); check(pages()[1].rotation == 90 && pages()[3].rotation == 90, "multi-page rotation Redo")
        reset(); let deleted = [pages()[1], pages()[2]]; sidebar.thumbnails.selectPages(deleted)
        send(#selector(deletePagesAction(_:))); check(pdf?.pageCount == 2, "Delete Pages removes the whole selection")
        manager.undo(); check(pdf?.pageCount == 4 && targetIndices == [1, 2], "Delete Pages Undo restores pages and selection")
        manager.redo(); check(pdf?.pageCount == 2, "Delete Pages Redo removes the same pages")
        reset(); send(#selector(insertBlankPageAction(_:)))
        check(pdf?.pageCount == 5 && currentIndex == 1 && targetIndices == [1], "Blank Page inserts after the current page and selects it")
        manager.undo(); check(pdf?.pageCount == 4 && currentIndex == 0, "Blank Page Undo restores the current page")
        manager.redo(); check(pdf?.pageCount == 5 && currentIndex == 1, "Blank Page Redo restores inserted selection")

        reset(); let source = pages()[0], destination = pages()[2]
        let destinationLabel = "Destination"
        let bookmark = PDFOutline(); bookmark.label = destinationLabel; bookmark.destination = PDFDestination(page: destination, at: CGPoint(x: 30, y: 170))
        let root = PDFOutline(); root.insertChild(bookmark, at: 0); pdf?.outlineRoot = root
        let link = Self.makeLink(bounds: CGRect(x: 30, y: 50, width: 80, height: 20)); link.action = PDFActionGoTo(destination: PDFDestination(page: destination, at: CGPoint(x: 30, y: 170)))
        source.addAnnotation(link)
        event { movePages([2], to: 0) }
        check(bookmark.destination?.page === destination && (link.action as? PDFActionGoTo)?.destination.page === destination, "bookmarks and internal links follow reordered pages")
        if let data = try? doc.data(ofType: "com.adobe.pdf"), let reopened = PDFDocument(data: data) {
            let target = reopened.outlineRoot?.child(at: 0)?.destination?.page
            check(target.map { reopened.index(for: $0) } == 0, "saved bookmark points to the reordered target page")
            let savedLink = reopened.page(at: 1)?.annotations.first { $0.isKind(.link) }
            let targetPage = savedLink?.destination?.page ?? (savedLink?.action as? PDFActionGoTo)?.destination.page
            check(targetPage.map { reopened.index(for: $0) } == 0, "saved internal link points to the reordered target page")
        } else { check(false, "reordered destination fixture saves and opens") }
        manager.undo(); check(bookmark.destination?.page === destination && (link.action as? PDFActionGoTo)?.destination.page === destination, "Undo keeps page destinations attached")
        event { replacePages([(2, PDFPage.blank(size: CGSize(width: 320, height: 240)))], name: "Replace") }
        check(bookmark.destination?.page === pdf?.page(at: 2) && (link.action as? PDFActionGoTo)?.destination.page === pdf?.page(at: 2), "page replacement remaps bookmarks and links")
        manager.undo(); check(bookmark.destination?.page === destination, "page replacement Undo restores destination page")
        sidebar.mode = .outline; sidebar.reloadOutline(select: bookmark)
        send(#selector(bookmarkStyleAction(_:)), tag: 1)
        check(doc.extras.outlineStyles[ObjectIdentifier(bookmark)]?.bold == true, "native bookmark Bold action updates its style")
        manager.undo(); check(doc.extras.outlineStyles.values.allSatisfy { !$0.bold }, "bookmark style Undo")
        manager.redo(); check(doc.extras.outlineStyles.values.contains { $0.bold }, "bookmark style Redo")
        if let parent = pdf?.outlineRoot?.child(at: 0) { moveBookmark(parent, to: parent, at: 0); check(pdf?.outlineRoot?.numberOfChildren == 1, "bookmark cannot be nested inside itself") }

        reset(2); let page = pages()[0]
        let a = PDFAnnotation(bounds: CGRect(x: 10, y: 10, width: 20, height: 20), forType: PDFAnnotationSubtype(rawValue: "FileAttachment"), withProperties: nil)
        let b = PDFAnnotation(bounds: CGRect(x: 50, y: 10, width: 20, height: 20), forType: PDFAnnotationSubtype(rawValue: "FileAttachment"), withProperties: nil)
        page.addAnnotation(a); page.addAnnotation(b)
        doc.extras.annotationFiles = [AnnotationFile(page: 0, rect: a.bounds, name: "one.txt", data: Data("one".utf8), annotation: a), AnnotationFile(page: 0, rect: b.bounds, name: "two.txt", data: Data("two".utf8), annotation: b)]
        event { movePages([0], to: 2) }
        check(attachmentInfo(.annotation(b, page: 1))?.data == Data("two".utf8), "attachment lookup follows its page after reorder")
        b.bounds.origin.x += 40
        check(attachmentInfo(.annotation(b, page: 1))?.name == "two.txt", "moved attachment keeps its own file rather than the first file on the page")
        manager.undo(); check(attachmentInfo(.annotation(a, page: 0))?.name == "one.txt", "attachment metadata follows page reorder Undo")
        let copyPage = PDFPage.blank(size: CGSize(width: 320, height: 240))
        let copyA = a.duplicateForEditing(on: copyPage)!, copyB = b.duplicateForEditing(on: copyPage)!
        copyPage.addAnnotation(copyA); copyPage.addAnnotation(copyB)
        let fitted = Self.makeLink(bounds: CGRect(x: 100, y: 10, width: 20, height: 20))
        fitted.action = PDFActionGoTo(destination: PDFDestination(page: pdf!.page(at: 1)!, at: .zero)); page.addAnnotation(fitted)
        doc.extras.linkFit[ObjectIdentifier(fitted)] = "FitH"
        let copiedLink = fitted.duplicateForEditing(on: copyPage)!; copyPage.addAnnotation(copiedLink)
        event { replacePages([(0, copyPage)], name: "Copy Page") }
        check(doc.extras.linkFit[ObjectIdentifier(copiedLink)] == "FitH", "page replacement retains link magnification metadata")
        check(attachmentInfo(.annotation(copyB, page: 0))?.name == "two.txt", "page replacement rebinds attachment metadata to the copied annotation")
        manager.undo(); check(doc.extras.linkFit[ObjectIdentifier(fitted)] == "FitH" && attachmentInfo(.annotation(b, page: 0))?.name == "two.txt", "page replacement Undo restores original metadata identities")

        reset(1); let full = pages()[0].bounds(for: .mediaBox)
        modal(L("Crop"), edit: { view in fields(view).first?.stringValue = "10" }) { cropAction(nil) }
        check(abs(pages()[0].bounds(for: .cropBox).minX - 10 * PageGeometry.mm) < 0.01, "native Crop applies a typed margin")
        manager.undo(); check(pages()[0].bounds(for: .cropBox) == full, "Crop Undo restores the full page")
        manager.redo(); check(pages()[0].bounds(for: .cropBox).minX > 0, "Crop Redo")
        let beforeInvalid = pages()[0].bounds(for: .cropBox)
        modal(L("Crop"), edit: { view in fields(view).first?.stringValue = "nan" }) { cropAction(nil) }
        check(pages()[0].bounds(for: .cropBox) == beforeInvalid, "NaN crop margin preserves the existing crop")
        modal(L("Remove Crop"), edit: { _ in }) { cropAction(nil) }
        check(pages()[0].bounds(for: .cropBox) == full, "Remove Crop restores the media box")
        manager.undo(); check(pages()[0].bounds(for: .cropBox) == beforeInvalid, "Remove Crop is undoable")

        reset(1)
        let text = PDFAnnotation(bounds: CGRect(x: 30, y: 40, width: 80, height: 30), forType: .freeText, withProperties: nil)
        text.contents = "Scaled annotation"; text.font = NSFont.systemFont(ofSize: 12); pages()[0].addAnnotation(text)
        let family = text.font?.familyName
        let cloned = text.duplicateForEditing(on: pages()[0])
        check(cloned?.font?.familyName == family, "annotation copy preserves the original font family")
        event { resizePages([0], to: CGSize(width: 640, height: 480), orientation: 2, scale: true) }
        let scaled = pages()[0].annotations.first { $0.contents == "Scaled annotation" }
        check(pages()[0].bounds(for: .mediaBox).size == CGSize(width: 640, height: 480), "Scale Page changes the paper size")
        check(scaled?.bounds == CGRect(x: 60, y: 80, width: 160, height: 60) && scaled?.font?.pointSize == 24, "Scale Page scales annotation geometry and font together")
        if scaled?.font?.familyName != family {
            print("font diagnostic", "original", text.font?.fontName as Any, family as Any,
                  "scaled", scaled?.font?.fontName as Any, scaled?.font?.familyName as Any)
        }
        check(scaled?.font?.familyName == family, "Scale Page preserves the annotation font family")
        check(text.font?.pointSize == 12 && text.font?.familyName == family && text.bounds.origin.x == 30, "Scale Page leaves the original annotation intact for Undo")
        manager.undo(); check(pages()[0].annotations.contains { $0 === text }, "Scale Page Undo restores the editable original annotation")
        manager.redo(); check(pages()[0].annotations.first?.font?.pointSize == 24, "Scale Page Redo restores the scaled annotation")
        let priorSize = pages()[0].bounds(for: .mediaBox)
        resizePages([0], to: CGSize(width: CGFloat.nan, height: 200), orientation: 0, scale: true)
        check(pages()[0].bounds(for: .mediaBox) == priorSize, "nonfinite page dimensions are rejected")
        reset(1)
        modal(L("Apply"), edit: { view in
            let popups = views(view).compactMap { $0 as? NSPopUpButton }
            check(popups.first?.indexOfSelectedItem == PageGeometry.paperSizes.count, "Page Size identifies existing custom dimensions")
        }) { pageSizeAction(nil) }
        check(abs(pages()[0].bounds(for: .mediaBox).width - 320) < 2, "accepting a custom page size does not change it to A4")

        reset(2)
        modal(L("Add"), edit: { view in
            let editable = fields(view)
            editable.first { $0.stringValue == "CONFIDENTIAL" }?.stringValue = "AUDIT-WATERMARK"
            editable.first { $0.stringValue == "45" }?.stringValue = "30"
        }) { watermarkAction(nil) }
        check(pages().allSatisfy { page in page.annotations.contains { ($0 as? WatermarkOverlay)?.spec.text == "AUDIT-WATERMARK" } }, "Watermark dialog adds the chosen text to every target page")
        manager.undo(); check(pages().allSatisfy { $0.annotations.isEmpty }, "multi-page watermark is one Undo event")
        manager.redo(); check(pages().allSatisfy { $0.annotations.count == 1 }, "watermark Redo restores every target")
        send(#selector(removeWatermarksAction(_:))); check(pages().allSatisfy { $0.annotations.isEmpty }, "Remove Watermarks removes live overlays")
        reset(2)
        modal(L("Add"), edit: { view in fields(view).first { $0.stringValue == "1" }?.stringValue = "4" }) { pageNumbersAction(nil) }
        check(pages().enumerated().allSatisfy { i, page in page.annotations.contains { ($0 as? PageNumberOverlay)?.text == "\(i + 4)" } }, "Page Numbers dialog applies the chosen starting number")
        manager.undo(); check(pages().allSatisfy { $0.annotations.isEmpty }, "Page Numbers Undo removes the entire numbering action")
        reset(2)
        modal(L("Add"), edit: { view in fields(view).first { $0.stringValue == "<<page>> / <<pages>>" }?.stringValue = "AUDIT-HF <<page>>" }) { headerFooterAction(nil) }
        check(pages().enumerated().allSatisfy { i, page in page.annotations.contains { ($0 as? HeaderFooterOverlay)?.lines[4] == "AUDIT-HF \(i + 1)" } }, "Header and Footer dialog expands page tokens on each page")
        if let data = try? doc.data(ofType: "com.adobe.pdf"), let reopened = PDFDocument(data: data) {
            check(reopened.string?.contains("AUDIT-HF 1") == true && reopened.string?.contains("AUDIT-HF 2") == true, "saved header/footer contains complete searchable page text")
        } else { check(false, "header/footer fixture saves and opens") }
        send(#selector(removeHeaderFooterAction(_:))); check(pages().allSatisfy { $0.annotations.allSatisfy { !($0 is HeaderFooterOverlay) } }, "Remove Headers and Footers removes live overlays")

        reset(3)
        doc.extras.view.openPage = 2; doc.extras.view.openZoom = "150"
        if let raw = pdf?.dataRepresentation() {
            let saved = DocumentWriter.apply(raw, extras: doc.extras, pdf: doc.pdf)
            if let restored = PDFDocument(data: saved) {
                let metadata = DocumentExtras.read(saved, pdf: restored)
                check(metadata.view.openPage == 2 && metadata.view.openZoom == "150", "Save retains initial page and zoom without layout or dirty flags")
            } else { check(false, "initial view saved PDF opens") }
        } else { check(false, "initial view fixture serializes") }
        doc.extras.view.pageLayout = "OneColumn"; doc.extras.view.openPage = 2; doc.extras.view.openZoom = "150"
        applyInitialView()
        check(pdfView.displayMode == .singlePageContinuous && currentIndex == 2, "Initial View applies continuous layout and selected opening page")
        check(abs(pdfView.scaleFactor - 1.5) < 0.01, "Initial View applies an explicit percentage")
        doc.extras.view.openZoom = "FitV"; applyInitialView()
        check(zoomMode == .custom && !pdfView.autoScales, "Initial View page-height magnification has its own scale")
        check(PageLabels.label(1, [PageLabelRange(start: 0, first: Int.max)]) == nil, "overflowing page labels cannot crash the sidebar")
        check(PageLabels.letters(Int.max).count < 100, "extreme letter labels cannot allocate unbounded captions")
        var number = PageNumberSpec(); number.start = Int.max
        check(number.labels(for: [0, 1]).isEmpty, "overflowing page-number sequence is rejected")
        let footer = HeaderFooterSpec()
        check(HeaderFooterOverlay.make(footer, pdf: doc.pdf, indices: [0, 1], fileName: "", labels: [], bates: Int.max).overlays.isEmpty, "overflowing Bates sequence is rejected before mutation")
        print("PDF pages UI regressions: \(checks - failures)/\(checks) passed")
        return failures == 0
    }
}
