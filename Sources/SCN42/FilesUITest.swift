import AppKit
import Kit42
import PDFKit

extension ScanWindowController {
    @MainActor func auditFiles() async -> Bool {
        guard let window, Store.testDirectory != nil else { print("FAIL SCN files UI: isolated history/window required"); return false }
        var count = 0, failures = 0
        func check(_ condition: Bool, _ name: String) { count += 1; if !condition { failures += 1 }; print(condition ? "PASS" : "FAIL", "SCN files UI:", name) }
        func views(_ view: NSView) -> [NSView] { [view] + view.subviews.flatMap(views) }
        func click(_ control: NSControl) -> Bool { guard let action = control.action else { return false }; return control.sendAction(action, to: control.target) }
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("SCN42-file-ui-" + UUID().uuidString)
        let oldSave = Store.saveOptions
        defer {
            Store.saveOptions = oldSave
            printOperationRunner = nil
            if let sheet = window.attachedSheet { window.endSheet(sheet, returnCode: .cancel) }
            try? FileManager.default.removeItem(at: folder)
        }
        func page(_ image: CGImage, text: String) -> ScanPage {
            let page = ScanPage(base: image, image: image, dpi: 144, options: ProcessOptions(), report: ProcessReport())
            page.text = [TextLine(text: text, box: CGRect(x: 0.1, y: 0.5, width: 0.8, height: 0.2))]
            return page
        }
        do {
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            let first = page(ScannerFileAudit.fixture(), text: "ALPHA SAVED TEXT")
            let second = page(ScannerFileAudit.fixture(width: 60, height: 90), text: "BRAVO SAVED TEXT")
            doc.setOrder([first], name: "fixture"); doc.undo.removeAllActions()
            var options = SaveOptions(); options.folder = folder.path; options.openAfter = .none; options.ocr = true
            options.compression = .original; options.nameTemplate = "saved {n}"; options.counter = 7
            var saved: URL?, completed = false
            save(options) { saved = $0; completed = true }
            doc.append([second])
            check(await UITest.wait({ completed && self.busyText == nil }), "asynchronous Save completes")
            check(saved?.lastPathComponent == "saved 007.pdf" && PDFDocument(url: saved!)?.pageCount == 1, "Save writes the captured page snapshot and expanded name")
            check(doc.isDirty && pages.count == 2, "a page added during Save remains unsaved")
            check(History.shared.entries.first(where: { $0.url == saved })?.pages == 1, "history uses the saved snapshot count rather than later pages")
            check(Store.saveOptions.counter == 8, "successful Save advances the name counter once")
            completed = false
            save(options) { saved = $0; completed = true }
            check(await UITest.wait({ completed && self.busyText == nil }) && !doc.isDirty, "saving the current complete document marks it clean")
            check(saved?.lastPathComponent == "saved 007 2.pdf" && PDFDocument(url: saved!)?.pageCount == 2, "ordinary Save creates a unique name and keeps both pages")
            let output = folder.appendingPathComponent("explicit.pdf")
            let urls = try writeExport(to: output, options: options)
            check(urls == [output] && savedURL == output && !doc.isDirty, "Export adopts the chosen exact file and clean revision")
            check(PDFDocument(url: output)?.string?.contains("ALPHA SAVED TEXT") == true && PDFDocument(url: output)?.string?.contains("BRAVO SAVED TEXT") == true,
                  "exported PDF contains both searchable text layers")
            let savedA = first.snapshot
            doc.modify([first], name: "rotate for async save") { $0.rotate(clockwise: 90) }
            let savedB = first.snapshot
            var baselineOptions = options; baselineOptions.ocr = false; baselineOptions.nameTemplate = "saved-baseline"
            var baselineURL: URL?, baselineCompleted = false
            save(baselineOptions) { baselineURL = $0; baselineCompleted = true }
            doc.undo.undo()
            check(first.snapshot.hasSameContent(as: savedA) && !doc.isDirty, "Undo during Save initially returns to the previous saved baseline")
            check(await UITest.wait({ baselineCompleted && self.busyText == nil }) && baselineURL != nil,
                  "the captured edited snapshot finishes saving after Undo")
            let savedPage = baselineURL.flatMap { PDFDocument(url: $0)?.page(at: 0) }
            check(doc.isDirty && first.snapshot.hasSameContent(as: savedA)
                  && savedPage?.bounds(for: .mediaBox).size == CGSize(width: 32, height: 48),
                  "Save completion compares current Undo state to the actual edited snapshot on disk")
            doc.undo.redo()
            check(first.snapshot.hasSameContent(as: savedB) && !doc.isDirty, "Redo to the snapshot saved in the background becomes clean")
            doc.modify([first], name: "rotate") { $0.rotate(clockwise: 90) }
            let dirtyRevision = doc.revision, bytes = try Data(contentsOf: output)
            var invalid = options; invalid.quality = .nan
            do { _ = try writeExport(to: output, options: invalid); check(false, "invalid export must fail") }
            catch { check(doc.revision == dirtyRevision && doc.isDirty && (try? Data(contentsOf: output)) == bytes, "failed Export preserves the document revision and existing file") }
            doc.undo.undo()

            let exampleTitle = "Invoice"
            let saveSheet = SaveSheetController(pages: pages.map(\.exportPage), title: exampleTitle, options: options)
            guard let sheetWindow = saveSheet.window else { throw CocoaError(.coderInvalidValue) }
            window.beginSheet(sheetWindow, completionHandler: { _ in })
            check(await UITest.wait({ window.attachedSheet === sheetWindow }), "native Save options sheet opens")
            saveSheet.format.selectItem(at: SaveFormat.jpeg.rawValue)
            check(click(saveSheet.format) && saveSheet.options.format == .jpeg && !saveSheet.quality.isHiddenOrHasHiddenAncestor && saveSheet.ocr.isHiddenOrHasHiddenAncestor,
                  "JPEG format enables quality and hides PDF-only OCR")
            saveSheet.quality.doubleValue = 0.63
            check(click(saveSheet.quality) && abs(saveSheet.options.quality - 0.63) < 0.001, "native quality slider changes the saved option")
            saveSheet.format.selectItem(at: SaveFormat.pdfPerPage.rawValue); _ = click(saveSheet.format)
            check(saveSheet.preview.stringValue.contains("001.pdf") && !saveSheet.ocr.isHiddenOrHasHiddenAncestor && saveSheet.quality.isHiddenOrHasHiddenAncestor,
                  "PDF per page updates filename preview and relevant controls")
            saveSheet.size.selectItem(at: Compression.smallest.rawValue); _ = click(saveSheet.size)
            check(saveSheet.options.compression == .smallest, "file-size popup changes compression")
            saveSheet.ocr.performClick(nil)
            check(saveSheet.options.ocr == false, "native searchable-text checkbox updates the option")
            saveSheet.openAfter.selectItem(at: OpenAfter.none.rawValue); _ = click(saveSheet.openAfter)
            check(saveSheet.options.openAfter == .none, "Then Open choice stores the requested behavior")
            sheetWindow.makeFirstResponder(saveSheet.name)
            if let editor = saveSheet.name.currentEditor() as? NSTextView {
                editor.selectAll(nil); editor.insertText("typed {n}", replacementRange: editor.selectedRange())
                var received: SaveOptions?
                saveSheet.onSave = { received = $0 }
                if let button = views(sheetWindow.contentView!).compactMap({ $0 as? NSButton }).first(where: { $0.title == L("Save") }) {
                    button.performClick(nil)
                    check(await UITest.wait({ received != nil }) && received?.nameTemplate == "typed {n}", "Save commits the active native name editor")
                } else { check(false, "Save sheet has a working Save button") }
            } else { check(false, "Save name field has a native field editor") }
            _ = await UITest.wait { window.attachedSheet == nil }
            let cancelSheet = SaveSheetController(pages: pages.map(\.exportPage), title: nil, options: options)
            var cancelledCallback = false; cancelSheet.onSave = { _ in cancelledCallback = true }
            window.beginSheet(cancelSheet.window!, completionHandler: { _ in })
            if let button = views(cancelSheet.window!.contentView!).compactMap({ $0 as? NSButton }).first(where: { $0.title == L("Cancel") }) {
                button.performClick(nil)
                check(await UITest.wait({ window.attachedSheet == nil }) && !cancelledCallback, "native Cancel closes Save options without saving")
            } else { check(false, "Save sheet has a working Cancel button") }
            let panel = NSSavePanel(), popup = NSPopUpButton()
            popup.addItems(withTitles: SaveFormat.allCases.map(\.title)); panel.nameFieldStringValue = "Chosen.pdf"
            let handler = FormatPopupHandler(panel: panel); popup.target = handler; popup.action = #selector(FormatPopupHandler.changed(_:))
            for format in SaveFormat.allCases {
                popup.selectItem(at: format.rawValue); _ = click(popup)
                check(panel.allowedContentTypes == [format.type] && panel.nameFieldStringValue == "Chosen." + format.ext,
                      "Export format updates native type and filename: \(format)")
            }

            let beforeShare = doc.revision, beforeDirty = doc.isDirty
            let items = shareItems()
            if let shared = items.first as? URL {
                check(PDFDocument(url: shared)?.pageCount == pages.count, "Share prepares a readable PDF containing every page")
                check(doc.revision == beforeShare && doc.isDirty == beforeDirty, "preparing Share does not mark the editing session saved")
                try? FileManager.default.removeItem(at: shared.deletingLastPathComponent())
            } else { check(false, "Share prepares PDF data") }
            if let operation = printOperation(pages, copies: 1) {
                let destination = folder.appendingPathComponent("printed.pdf")
                operation.printInfo.jobDisposition = .save
                operation.printInfo.dictionary()[NSPrintInfo.AttributeKey.jobSavingURL] = destination
                operation.showsPrintPanel = false; operation.showsProgressPanel = false
                let success = operation.run(), document = PDFDocument(url: destination)
                check(success && document?.pageCount == pages.count, "native Print saves every page to PDF without sending a physical job")
                if let page = document?.page(at: 0) {
                    let thumbnail = page.thumbnail(of: CGSize(width: 120, height: 120), for: .mediaBox)
                    var rect = CGRect(origin: .zero, size: thumbnail.size)
                    if let image = thumbnail.cgImage(forProposedRect: &rect, context: nil, hints: nil) {
                        let pixels = Bitmap(image, gray: true).data
                        check((pixels.min() ?? 255) < 200 && (pixels.max() ?? 0) > 200, "printed PDF contains the scanned image pixels")
                    } else { check(false, "printed page renders") }
                }
            } else { check(false, "native Print operation exists") }
            check(printOperation([], copies: 1) == nil && printOperation(pages, copies: 0) == nil, "invalid print jobs do not silently run")
            let invalidPage = page(ScannerFileAudit.fixture(), text: "Invalid DPI"); invalidPage.dpi = .nan
            do { _ = try temporaryPDFChecked(targets: [invalidPage]); check(false, "invalid Share data must throw") }
            catch { check(true, "checked Share rejects an invalid page without producing a successful empty PDF") }
            do { _ = try printOperationChecked([invalidPage], copies: 1); check(false, "invalid Print data must throw") }
            catch { check(true, "checked Print rejects invalid geometry before creating a print job") }
            check(temporaryPDF(targets: [invalidPage]) == nil && window.attachedSheet != nil, "Share output failure presents a visible error")
            if let alert = window.attachedSheet { window.endSheet(alert, returnCode: .OK); alert.orderOut(nil) }
            _ = await UITest.wait { window.attachedSheet == nil }
            printPages([invalidPage], copies: 1, direct: false)
            check(window.attachedSheet != nil, "Print output failure presents a visible error")
            if let alert = window.attachedSheet { window.endSheet(alert, returnCode: .OK); alert.orderOut(nil) }
            _ = await UITest.wait { window.attachedSheet == nil }

            // The native Copy dialog and real processing pipeline feed a print-to-PDF runner, never a physical printer.
            let device = AuditScanner(); choose(device)
            var plain = ScanSettings(); plain.autoCrop = false; plain.deskew = false; plain.autoRotate = false
            plain.enhancement = .none; plain.removeBlank = false; plain.detectPhotos = false; plain.resolution = 75
            settingsPanel.apply(plain); areas = []; areaQuads = []
            check(canScan, "Copy fixture uses a ready isolated scanner")
            _ = NSApp.sendAction(#selector(scanAndPrintAction(_:)), to: self, from: nil)
            if let cancel = window.attachedSheet?.contentView.flatMap({ views($0).compactMap { $0 as? NSButton }.first { $0.title == L("Cancel") } }) {
                cancel.performClick(nil)
                check(await UITest.wait({ window.attachedSheet == nil }) && device.requests.isEmpty && copierCopies == nil,
                      "native Copy Cancel starts neither a scan nor a print job")
            } else { check(false, "Copy dialog exposes Cancel") }
            let copiedPDF = folder.appendingPathComponent("copied.pdf")
            var printedCopies: Int?, wasDirect = false, printSucceeded = false
            printOperationRunner = { operation, direct in
                printedCopies = operation.printInfo.dictionary()[NSPrintInfo.AttributeKey.copies] as? Int
                wasDirect = direct
                operation.printInfo.jobDisposition = .save
                operation.printInfo.dictionary()[NSPrintInfo.AttributeKey.jobSavingURL] = copiedPDF
                operation.showsPrintPanel = false; operation.showsProgressPanel = false
                printSucceeded = operation.run()
            }
            check(NSApp.sendAction(#selector(scanAndPrintAction(_:)), to: self, from: nil), "native Copy command opens its options")
            if let content = window.attachedSheet?.contentView,
               let stepper = views(content).compactMap({ $0 as? NSStepper }).first,
               let scan = views(content).compactMap({ $0 as? NSButton }).first(where: { $0.title == L("Scan and Print") }) {
                stepper.integerValue = 3; check(click(stepper), "Copy stepper updates the copy count")
                scan.performClick(nil)
                check(await UITest.wait({ device.requests.count == 1 && self.isScanning }), "Scan and Print starts the configured scan after confirmation")
                device.page?(RawScan(image: first.image, dpi: 144)); device.done?(nil)
                check(await UITest.wait({ printedCopies != nil && self.pendingProcessing == 0 }, seconds: 15), "Copy waits for real image processing before creating its print job")
                check(printedCopies == 3 && wasDirect && printSucceeded && PDFDocument(url: copiedPDF)?.pageCount == 1,
                      "Copy sends only the fresh scan and the requested three copies to the controlled print runner")
                check(copierCopies == nil && !isScanning, "Copy clears its pending job after printing")
            } else { check(false, "Copy options expose working stepper and Scan and Print button") }
            printOperationRunner = nil

            let historyA = HistoryPanelController(), historyB = HistoryPanelController()
            _ = historyA.view; _ = historyB.view
            let historyFile = folder.appendingPathComponent("history.png")
            var png = options; png.format = .png
            try Exporter.data([first.exportPage], png).write(to: historyFile)
            History.shared.add(historyFile, pages: 1, thumbnail: first.image)
            check(historyA.table.numberOfRows == History.shared.entries.count && historyB.table.numberOfRows == History.shared.entries.count,
                  "history changes update every open inspector")
            let oldThumb = History.shared.entries.first?.thumb
            History.shared.add(historyFile, pages: 1, thumbnail: first.image)
            let historyFolder = Store.testDirectory!.appendingPathComponent(".history")
            check(History.shared.entries.filter { $0.url == historyFile }.count == 1 && oldThumb.map { !FileManager.default.fileExists(atPath: historyFolder.appendingPathComponent($0).path) } == true,
                  "re-saving history replaces its entry and removes the obsolete thumbnail")
            historyA.table.selectRowIndexes(IndexSet(integer: 0), byExtendingSelection: false)
            let oldCount = History.shared.entries.count
            check(NSApp.sendAction(#selector(HistoryPanelController.removeSelected), to: historyA, from: nil), "native Remove from History action dispatches")
            check(History.shared.entries.count == oldCount - 1 && FileManager.default.fileExists(atPath: historyFile.path), "removing History leaves the saved scan file intact")
            check(historyB.table.numberOfRows == History.shared.entries.count, "removing History refreshes another open inspector")
        } catch { check(false, "file UI fixture: \(error)") }
        print("SCN42 file UI regressions: \(count - failures)/\(count) passed")
        return failures == 0
    }
}
