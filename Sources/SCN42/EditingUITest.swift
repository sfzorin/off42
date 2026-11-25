import AppKit
import CoreText
import Kit42
import UniformTypeIdentifiers

extension ScanWindowController {
    @MainActor func auditEditing() async -> Bool {
        guard let window else { print("FAIL SCN editing: no window"); return false }
        var count = 0, failures = 0
        func check(_ value: Bool, _ name: String) {
            count += 1; if !value { failures += 1 }
            print(value ? "PASS" : "FAIL", "SCN editing:", name); fflush(stdout)
        }
        func command(_ command: Command) {
            check(NSApp.sendAction(command.action, to: self, from: command.menuItem()), "native action: " + command.title)
        }
        func image(_ n: Int) -> CGImage {
            var bitmap = Bitmap(width: 180 + n * 20, height: 260, channels: 4, fill: 255)
            for y in 20..<100 { for x in 20..<95 {
                let i = (y * bitmap.width + x) * 4
                bitmap.data[i] = UInt8(n * 35); bitmap.data[i + 1] = 30; bitmap.data[i + 2] = 180
            } }
            return bitmap.cgImage()!
        }
        var options = ProcessOptions(); options.crop = false; options.deskew = false; options.autoRotate = false; options.enhancement = .none
        func page(_ n: Int) -> ScanPage {
            let source = image(n)
            var report = ProcessReport(); report.outputSize = CGSize(width: source.width, height: source.height); report.ink = 0.1
            return ScanPage(base: source, image: source, dpi: 144, options: options, report: report)
        }
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("SCN42-editing-" + UUID().uuidString)
        let initialPages = pages, initialAreas = areas, savedAreas = Store.lastAreas
        let pasteboard = NSPasteboard.withUniqueName()
        var child: ScanWindowController?
        defer {
            child?.doc.isDirty = false; child?.window?.close()
            pageWorkGeneration = UUID(); busyText = nil
            preview.canvas.cancelInteraction()
            doc.setOrder(initialPages, name: "Fixture"); doc.undo.removeAllActions(); doc.markSaved(revision: doc.revision)
            areas = initialAreas; Store.lastAreas = savedAreas; showingGlass = false; refreshPreview()
            pasteboard.releaseGlobally(); try? FileManager.default.removeItem(at: folder)
        }
        func reset(_ list: [ScanPage]) {
            showingGlass = false; busyText = nil
            doc.setOrder(list, name: "Fixture"); doc.undo.removeAllActions(); doc.markSaved(revision: doc.revision)
            pageList.select(list.isEmpty ? [] : [0], scroll: false)
            NSApp.activate(ignoringOtherApps: true); window.makeKeyAndOrderFront(nil); window.makeFirstResponder(pageList.table)
            window.layoutIfNeeded(); refreshPreview()
        }
        do {
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            let a = page(0), b = page(1), c = page(2), d = page(3)
            reset([a, b, c, d])
            pageList.table.selectRowIndexes([1, 2], byExtendingSelection: false)
            check(targetPages.map(\.id) == [b.id, c.id] && shownPage === c, "native thumbnail selection targets the selected page identities")
            let beforeB = b.snapshot, beforeC = c.snapshot
            command(Cmd.rotateRight)
            check(b.image.width == beforeB.image.height && c.image.width == beforeC.image.height && a.image.width == 180,
                  "Rotate changes every selected page and leaves the unselected page intact")
            doc.undo.undo()
            check(b.snapshot.hasSameContent(as: beforeB) && c.snapshot.hasSameContent(as: beforeC) && !doc.isDirty,
                  "Rotate Undo restores complete pixel state and the saved baseline")
            doc.undo.redo()
            check(b.image.height == beforeB.image.width && pageList.selection == [1, 2], "Rotate Redo preserves the complete selection")
            doc.undo.undo(); doc.undo.removeAllActions()
            command(Cmd.deletePage)
            check(pages.map(\.id) == [a.id, d.id], "Delete removes exactly the selected pages")
            doc.undo.undo()
            check(pages.map(\.id) == [a.id, b.id, c.id, d.id] && pageList.selection == [1, 2], "Delete Undo restores page order and selected identities")
            doc.undo.redo(); check(pages.map(\.id) == [a.id, d.id], "Delete Redo removes the same identities")
            doc.undo.undo(); doc.undo.removeAllActions()
            pageList.select([1, 2]); command(Cmd.moveDown)
            check(pages.map(\.id) == [a.id, d.id, b.id, c.id] && pageList.selection == [2, 3], "Move Down keeps a multi-page selection together")
            doc.undo.undo(); check(pages.map(\.id) == [a.id, b.id, c.id, d.id] && pageList.selection == [1, 2], "Move Undo restores the original selected pages")
            command(Cmd.moveUp)
            check(pages.map(\.id) == [b.id, c.id, a.id, d.id] && pageList.selection == [0, 1], "Move Up places the selection before its preceding page")
            doc.undo.undo(); doc.undo.removeAllActions()
            pageList.select([b.id].compactMap { id in pages.firstIndex { $0.id == id } })
            command(Cmd.insertHere)
            check(insertionIndex == 1, "Insert Next Scan marks the selected page")
            doc.move(IndexSet(integer: 1), to: 4)
            check(insertionIndex == 3 && pageList.insertionIndex == 3, "insertion marker follows its page through reordering")
            doc.remove(ids: [b.id]); check(insertionIndex == nil, "deleting the insertion anchor clears the marker")
            reset([a, b, c, d])

            pageList.select([1])
            if let item = pageList.tableView(pageList.table, pasteboardWriterForRow: 1) as? NSPasteboardItem {
                check(item.string(forType: PageListController.dragType) == b.id.uuidString, "native row drag captures a page identity rather than a stale index")
                pasteboard.clearContents(); pasteboard.writeObjects([item])
                let drop = ScanAuditDraggingInfo(pasteboard: pasteboard, source: pageList.table, window: window)
                let extra = page(4); doc.insert([extra], at: 0)
                let beforeDrop = pages.map(\.id)
                check(pageList.tableView(pageList.table, validateDrop: drop, proposedRow: pages.count, proposedDropOperation: .above) == .move,
                      "native thumbnail drop delegate accepts local reordering")
                check(pageList.tableView(pageList.table, acceptDrop: drop, row: pages.count, dropOperation: .above), "native drop delegate performs the move")
                check(pages.last === b && pageList.selection == [pages.count - 1], "drop still moves the captured page after rows changed during the drag")
                if pages.last !== b || pageList.selection != [pages.count - 1] {
                    print("SCN drop diagnostic: captured", b.id, "order", pages.map(\.id), "selected rows", pageList.selection,
                          "pasteboard", pasteboard.pasteboardItems?.first?.string(forType: PageListController.dragType) ?? "missing")
                }
                doc.undo.undo(); check(pages.map(\.id) == beforeDrop, "drag reorder Undo restores the complete order")
            } else { check(false, "native table provides a page drag item") }
            reset([a, b, c, d])
            let importURL = folder.appendingPathComponent("insert.png")
            var imageOptions = SaveOptions(); imageOptions.format = .png
            try Exporter.data([page(5).exportPage], imageOptions).write(to: importURL)
            pasteboard.clearContents(); pasteboard.writeObjects([importURL as NSURL])
            let external = ScanAuditDraggingInfo(pasteboard: pasteboard, source: nil, window: window)
            var importSettings = settings
            importSettings.autoCrop = false; importSettings.deskew = false; importSettings.autoRotate = false
            importSettings.enhancement = .none; importSettings.colorMode = .color; importSettings.removeBlank = false
            settingsPanel.apply(importSettings)
            check(pageList.tableView(pageList.table, validateDrop: external, proposedRow: 0, proposedDropOperation: .above) == .copy,
                  "native drop delegate accepts a file at the first row")
            check(pageList.tableView(pageList.table, acceptDrop: external, row: 0, dropOperation: .above), "file drop delegates to image import")
            check(await UITest.wait({ self.pendingProcessing == 0 && self.pages.count == 5 }, seconds: 20), "dropped file finishes importing")
            check(pages.first?.image.width == 280 && Array(pages.dropFirst()).map(\.id) == [a.id, b.id, c.id, d.id], "drop before the first page preserves destination and existing order")
            doc.undo.undo(); check(pages.map(\.id) == [a.id, b.id, c.id, d.id], "import Undo removes the inserted page")

            reset([a, b, c, d]); pageList.select([2])
            let oldWindows = Set(AppDelegate.shared?.windows.map(ObjectIdentifier.init) ?? [])
            command(Cmd.split)
            child = AppDelegate.shared?.windows.first { !oldWindows.contains(ObjectIdentifier($0)) }
            check(pages.map(\.id) == [a.id, b.id] && child?.pages.count == 2, "Split creates a document with every page from the selected one onward")
            if let child, let copy = child.pages.first {
                check(copy !== c && copy.image === c.image, "split content uses independent mutable page objects")
                child.doc.modify([copy], name: "Fixture rotate") { $0.rotate(clockwise: 90) }
                doc.undo.undo()
                check(pages.map(\.id) == [a.id, b.id, c.id, d.id] && c.snapshot.hasSameContent(as: beforeC), "Undo Split restores originals even after the other window edits its copy")
                child.doc.isDirty = false; child.window?.close()
            } else { check(false, "Split new document exists") }
            child = nil

            reset([page(0), page(1)])
            pageList.select([0, 1]); let originals = pages.map(\.snapshot)
            command(Cmd.colorModes.first { $0.tag == ColorMode.gray.rawValue + 1 }!)
            check(await UITest.wait({ self.busyText == nil }, seconds: 20), "background colour change finishes")
            check(pages.allSatisfy { $0.options.colorMode == .gray && $0.image.colorSpace?.model == .monochrome }, "Grey applies to every selected page's rendered pixels")
            doc.undo.undo()
            check(zip(pages, originals).allSatisfy { $0.snapshot.hasSameContent(as: $1) } && !doc.isDirty, "one Undo restores every page before colour processing")
            doc.undo.redo(); check(pages.allSatisfy { $0.options.colorMode == .gray }, "colour Redo restores all processed pages")
            doc.undo.removeAllActions(); doc.markSaved(revision: doc.revision)
            command(Cmd.colorModes.first { $0.tag == ColorMode.gray.rawValue + 1 }!)
            check(busyText == nil && !doc.undo.canUndo && !doc.isDirty, "choosing the same colour mode does not start work or create Undo")

            reset([page(0)])
            let target = pages[0], gate = DispatchSemaphore(value: 0)
            processQueue.async { gate.wait() }
            let source = target.snapshot
            command(Cmd.enhance)
            check(target.snapshot.hasSameContent(as: source) && busyText != nil, "background enhancement does not mutate the live page before delivery")
            doc.modify([target], name: "Rotate during queued work") { $0.rotate(clockwise: 90) }
            let rotated = target.snapshot
            gate.signal()
            check(await UITest.wait({ self.busyText == nil }, seconds: 20), "queued enhancement finishes after the newer edit")
            check(target.snapshot.hasSameContent(as: rotated), "obsolete enhancement cannot overwrite a newer page edit")
            let staleRevision = target.contentRevision, staleSnapshot = target.snapshot
            doc.undo.undo()
            check(applyPageSnapshots([(target, staleRevision, staleSnapshot)], name: "Stale") == 0, "Undo invalidates captured asynchronous page work")
            let removedRevision = target.contentRevision
            doc.remove(ids: [target.id])
            check(applyPageSnapshots([(target, removedRevision, source)], name: "Stale") == 0 && pages.isEmpty, "removed pages cannot be resurrected by a background result")

            var blankReport = ProcessReport(); blankReport.blank = true
            let blankImage = Bitmap(width: 180, height: 260, channels: 1, fill: 255).cgImage()!
            let blank = ScanPage(base: blankImage, image: blankImage, dpi: 144, options: options, report: blankReport)
            reset([a, blank, b]); command(Cmd.removeBlank)
            check(pages.map(\.id) == [a.id, b.id], "Remove Blank Pages removes only pages marked blank")
            doc.undo.undo(); check(pages.map(\.id) == [a.id, blank.id, b.id], "Remove Blank Undo restores the exact position")

            let context = CGContext(data: nil, width: 800, height: 300, bitsPerComponent: 8, bytesPerRow: 0,
                                    space: Bitmap.sRGB, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)!
            context.setFillColor(CGColor(gray: 1, alpha: 1)); context.fill(CGRect(x: 0, y: 0, width: 800, height: 300))
            let line = CTLineCreateWithAttributedString(NSAttributedString(string: "SCAN ALPHA BETA", attributes: [.font: CTFontCreateWithName("Helvetica" as CFString, 46, nil), .foregroundColor: NSColor.black]))
            context.textPosition = CGPoint(x: 45, y: 170); CTLineDraw(line, context)
            let textImage = context.makeImage()!
            let textPage = ScanPage(base: textImage, image: textImage, dpi: 144, options: options, report: ProcessReport())
            reset([textPage]); command(Cmd.recognize)
            check(await UITest.wait({ self.busyText == nil }, seconds: 30), "real Vision OCR completes through the native command")
            check(textPage.text.map(OCR.text)?.contains("SCAN ALPHA BETA") == true && doc.isDirty, "recognized text belongs to the current pixels and participates in saving")
            var copied: Bool?
            copyText([textPage], to: pasteboard) { copied = $0 }
            check(copied == true && pasteboard.string(forType: .string)?.contains("SCAN ALPHA BETA") == true, "Copy Text writes recognized content to an isolated native pasteboard")
            check(copyPageImage(to: pasteboard) && NSImage(pasteboard: pasteboard) != nil, "Copy Image prepares the actual page image")
            let panel = NSSavePanel(); panel.nameFieldStringValue = "Recognized.txt"
            let textOptions = TextExportOptions(panel: panel)
            textOptions.popup.selectItem(at: 1)
            _ = NSApp.sendAction(textOptions.popup.action!, to: textOptions.popup.target, from: textOptions.popup)
            check(textOptions.rtf && panel.allowedContentTypes == [.rtf] && panel.nameFieldStringValue == "Recognized.rtf", "native text format control switches destination type and filename to RTF")
            let rtf = folder.appendingPathComponent("text.rtf"), txt = folder.appendingPathComponent("text.txt")
            try writeText(to: rtf, rtf: true); try writeText(to: txt, rtf: false)
            let styled = try NSAttributedString(data: Data(contentsOf: rtf), options: [.documentType: NSAttributedString.DocumentType.rtf], documentAttributes: nil)
            let plain = try String(contentsOf: txt, encoding: .utf8)
            check(styled.string.contains("SCAN ALPHA BETA") && plain.contains("SCAN ALPHA BETA"), "TXT and RTF reopen with the recognized content")
            textOptions.popup.selectItem(at: 0); _ = NSApp.sendAction(textOptions.popup.action!, to: textOptions.popup.target, from: textOptions.popup)
            check(!textOptions.rtf && panel.allowedContentTypes == [.plainText] && panel.nameFieldStringValue == "Recognized.txt", "text format can switch back to TXT")
            let staleOCR = textPage.contentRevision
            doc.modify([textPage], name: "Rotate") { $0.rotate(clockwise: 90) }
            check(!doc.setText(textPage, [TextLine(text: "Obsolete", box: .zero)], expectedContentRevision: staleOCR) && textPage.text == nil,
                  "rotating a page rejects recognition from its previous pixels")

            reset([a]); showingGlass = true; overview = blankImage; areas = []; refreshPreview(); window.layoutIfNeeded()
            let canvas = preview.canvas
            func mouse(_ type: NSEvent.EventType, _ x: CGFloat, _ y: CGFloat) {
                let point = canvas.convert(CGPoint(x: x * canvas.bounds.width, y: y * canvas.bounds.height), to: nil)
                let event = NSEvent.mouseEvent(with: type, location: point, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
                    windowNumber: window.windowNumber, context: nil, eventNumber: 1, clickCount: 1, pressure: 1)!
                switch type { case .leftMouseDown: canvas.mouseDown(with: event); case .leftMouseDragged: canvas.mouseDragged(with: event); default: canvas.mouseUp(with: event) }
            }
            func key(_ code: UInt16) {
                canvas.keyDown(with: NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
                    windowNumber: window.windowNumber, context: nil, characters: code == 53 ? "\u{1b}" : "\u{7f}", charactersIgnoringModifiers: "", isARepeat: false, keyCode: code)!)
            }
            mouse(.leftMouseDown, 0.1, 0.1); mouse(.leftMouseDragged, 0.6, 0.5); mouse(.leftMouseUp, 0.6, 0.5)
            check(areas.count == 1 && abs(areas[0].width - 0.5) < 0.001 && abs(areas[0].height - 0.4) < 0.001, "native glass drag creates a normalized persisted scan area")
            mouse(.leftMouseDown, 0.6, 0.5); mouse(.leftMouseDragged, 0.8, 0.7); mouse(.leftMouseUp, 0.8, 0.7)
            check(abs(areas[0].width - 0.7) < 0.001 && abs(areas[0].height - 0.6) < 0.001, "native corner drag resizes the selected scan area")
            mouse(.leftMouseDown, 0.4, 0.3); mouse(.leftMouseDragged, 1, 1); mouse(.leftMouseUp, 1, 1)
            check(abs(areas[0].maxX - 1) < 0.001 && abs(areas[0].maxY - 1) < 0.001, "moving an area clamps it to the scanner glass")
            let committed = areas
            mouse(.leftMouseDown, 0.65, 0.65); mouse(.leftMouseDragged, 0.3, 0.3); key(53); mouse(.leftMouseUp, 0.3, 0.3)
            check(canvas.areas == committed && areas == committed && Store.lastAreas == committed, "Escape cancels an unfinished area move without persisting it")
            mouse(.leftMouseDown, 0.65, 0.65); mouse(.leftMouseDragged, 0.3, 0.3)
            preview.show(image(0)); mouse(.leftMouseUp, 0.3, 0.3)
            check(areas == committed && !canvas.editsAreas, "changing the preview cancels the pending glass gesture")
            showingGlass = true; refreshPreview(); canvas.selectedArea = 0; key(51)
            check(areas.isEmpty && canvas.areas.isEmpty && !doc.isDirty, "Delete removes only the selected scan area and does not edit page pixels")
            areas = [CGRect(x: 0.2, y: 0.2, width: 0.4, height: 0.4), CGRect(x: 0.4, y: 0.4, width: 0.4, height: 0.4)]
            refreshPreview(); canvas.selectedArea = nil; canvas.displayIfNeeded()
            if let bitmap = canvas.bitmapImageRepForCachingDisplay(in: canvas.bounds) {
                canvas.cacheDisplay(in: canvas.bounds, to: bitmap)
                func light(_ x: Double, _ y: Double) -> CGFloat { bitmap.colorAt(x: Int(x * Double(bitmap.pixelsWide)), y: Int(y * Double(bitmap.pixelsHigh)))?.usingColorSpace(.deviceRGB)?.redComponent ?? 0 }
                check(light(0.5, 0.5) > 0.9 && light(0.05, 0.05) < 0.8, "overlapping scan areas remain undimmed while the excluded glass is shaded")
            } else { check(false, "glass preview renders for pixel inspection") }
        } catch { check(false, "fixture: \(error)") }
        print("SCN42 editing regressions: \(count - failures)/\(count) passed")
        return failures == 0
    }
}

@MainActor private final class ScanAuditDraggingInfo: NSObject, @preconcurrency NSDraggingInfo {
    let draggingPasteboard: NSPasteboard
    let draggingSource: Any?
    let draggingDestinationWindow: NSWindow?
    init(pasteboard: NSPasteboard, source: Any?, window: NSWindow) { draggingPasteboard = pasteboard; draggingSource = source; draggingDestinationWindow = window }
    var draggingSourceOperationMask: NSDragOperation { [.copy, .move] }
    var draggingLocation: NSPoint { .zero }
    var draggedImageLocation: NSPoint { .zero }
    var draggedImage: NSImage? { nil }
    var draggingSequenceNumber: Int { 1 }
    var draggingFormation: NSDraggingFormation = .none
    var animatesToDestination = false
    var numberOfValidItemsForDrop = 1
    var springLoadingHighlight: NSSpringLoadingHighlight { .none }
    func slideDraggedImage(to screenPoint: NSPoint) {}
    override func namesOfPromisedFilesDropped(atDestination dropDestination: URL) -> [String]? { nil }
    func resetSpringLoading() {}
    func enumerateDraggingItems(options: NSDraggingItemEnumerationOptions, for view: NSView?, classes: [AnyClass], searchOptions: [NSPasteboard.ReadingOptionKey: Any], using block: (NSDraggingItem, Int, UnsafeMutablePointer<ObjCBool>) -> Void) {
        for (index, item) in (draggingPasteboard.pasteboardItems ?? []).enumerated() {
            var stop = ObjCBool(false)
            block(NSDraggingItem(pasteboardWriter: item), index, &stop)
            if stop.boolValue { break }
        }
    }
}
