import AppKit
import PDFKit
import Kit42

extension UITest {
    static func technicalLabels(_ w: SheetWindowController, _ doc: SheetDocument) -> Int {
        var failures = 0
        func check(_ name: String, _ condition: Bool) {
            print(condition ? "ok  " : "FAIL", "technical labels: " + name)
            if !condition { failures += 1 }
        }
        func darkBounds(_ data: Data) -> CGRect? {
            guard let document = PDFDocument(data: data), let page = document.page(at: 0),
                  let tiff = page.thumbnail(of: NSSize(width: 600, height: 800), for: .mediaBox).tiffRepresentation,
                  let bitmap = NSBitmapImageRep(data: tiff) else { return nil }
            defer { withExtendedLifetime(document) {} }
            var minX = bitmap.pixelsWide, minY = bitmap.pixelsHigh, maxX = -1, maxY = -1
            for y in stride(from: 0, to: bitmap.pixelsHigh, by: 2) {
                for x in stride(from: 0, to: bitmap.pixelsWide, by: 2) {
                    if let color = bitmap.colorAt(x: x, y: y)?.usingColorSpace(.deviceRGB),
                       color.redComponent + color.greenComponent + color.blueComponent < 1 {
                        minX = min(minX, x); minY = min(minY, y); maxX = max(maxX, x); maxY = max(maxY, y)
                    }
                }
            }
            guard maxX >= minX, maxY >= minY else { return nil }
            return CGRect(x: minX, y: minY, width: maxX - minX + 1, height: maxY - minY + 1)
        }
        var wb = Workbook(); wb.sheets = [Sheet(name: "Labels")]
        doc.workbook = wb; w.sheetIndex = 0; w.workbookDidChange()
        w.grid.select(CellRef("B2")!); w.suppressAlerts = true
        doc.undoManager?.removeAllActions()
        let element = TechnicalElement(codeType: .qr, value: "0000123", widthMM: 25, caption: true)
        let insert = NSMenuItem(title: "", action: #selector(w.insertTechnicalElementAction(_:)), keyEquivalent: "")
        insert.representedObject = element
        check("native insertion action", NSApp.sendAction(insert.action!, to: w, from: insert))
        guard let image = w.sheet.images.first else { check("image inserted", false); return failures }
        check("exact size and anchor", image.anchor == CellRef("B2")! && abs(image.size.width - 25 * 72 / 25.4) < 0.001)
        check("recipe embedded", TechnicalElement.fromPNG(image.data) == element)
        check("rendered image pixels", NSBitmapImageRep(data: image.data).map { $0.pixelsWide > 500 && $0.pixelsHigh > 500 } ?? false)
        doc.undoManager?.undo(); check("insertion Undo", w.sheet.images.isEmpty)
        doc.undoManager?.redo(); check("insertion Redo bytes and geometry", w.sheet.images.first?.data == image.data && w.sheet.images.first?.size == image.size)

        let replacement = TechnicalElement(codeType: .dataMatrix, value: "LOT-123", widthMM: 30, caption: true)
        w.grid.selectedObject = .image(0)
        let edit = NSMenuItem(title: "", action: #selector(w.editTechnicalElementAction(_:)), keyEquivalent: "")
        edit.representedObject = replacement
        check("native edit action", NSApp.sendAction(edit.action!, to: w, from: edit))
        check("edit value and anchor", TechnicalElement.fromPNG(w.sheet.images[0].data) == replacement && w.sheet.images[0].anchor == image.anchor)
        let changed = w.sheet.images[0]
        doc.undoManager?.undo(); check("edit Undo", w.sheet.images[0].data == image.data && w.sheet.images[0].size == image.size)
        doc.undoManager?.redo(); check("edit Redo", w.sheet.images[0].data == changed.data && w.sheet.images[0].size == changed.size)

        for type in ["org.openxmlformats.spreadsheetml.sheet", "org.oasis-open.opendocument.spreadsheet"] {
            do {
                let data = try doc.data(ofType: type)
                let reopened = SheetDocument(); try reopened.read(from: data, ofType: type)
                let restored = reopened.workbook.sheets[0].images.first
                let geometry = restored.map { abs($0.size.width - changed.size.width) <= 1.0 / 12700 && abs($0.size.height - changed.size.height) <= 1.0 / 12700 } ?? false
                check("\(type) image round trip to one EMU", restored?.data == changed.data && restored?.anchor == changed.anchor && restored?.offset == changed.offset && geometry)
                check("\(type) editable recipe", restored.flatMap { TechnicalElement.fromPNG($0.data) } == replacement)
                let output = SheetExport.Options(); output.gridlines = false; output.pageNumbers = false
                let pdf = SheetExport.pdf(reopened.workbook, activeSheet: 0, selection: nil, options: output, base: NSPrintInfo.shared)
                let thumbnail = PDFDocument(data: pdf)?.page(at: 0)?.thumbnail(of: NSSize(width: 600, height: 800), for: .mediaBox)
                let bitmap = thumbnail?.tiffRepresentation.flatMap { NSBitmapImageRep(data: $0) }
                var darkPixels = 0
                if let bitmap {
                    for y in stride(from: 0, to: bitmap.pixelsHigh, by: 2) {
                        for x in stride(from: 0, to: bitmap.pixelsWide, by: 2) {
                            if let color = bitmap.colorAt(x: x, y: y)?.usingColorSpace(.deviceRGB),
                               color.redComponent + color.greenComponent + color.blueComponent < 1 { darkPixels += 1 }
                        }
                    }
                }
                check("\(type) image-only sheet renders code pixels", darkPixels > 100)
                reopened.close()
            } catch { check("\(type) opens", false) }
        }

        // Exercise the real print operation with Save to File, never a physical printer.
        do {
            func printFile(_ correction: PrintCorrection?) throws -> Data {
                let operation = try doc.printOperation(withSettings: [:])
                let settings = operation.printPanel.accessoryControllers.compactMap { $0 as? SheetPrintAccessory }.first?.options
                settings?.scope = .sheet; settings?.gridlines = false; settings?.pageNumbers = false; settings?.fitWidth = false
                let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString + ".pdf")
                defer { try? FileManager.default.removeItem(at: url) }
                operation.printInfo.jobDisposition = .save
                operation.printInfo.dictionary()[NSPrintInfo.AttributeKey.jobSavingURL] = url
                ProductionPrint.setCorrection(correction, in: operation.printInfo)
                operation.showsPrintPanel = false; operation.showsProgressPanel = false
                guard operation.run() else { throw NSError(domain: "XLS42Audit", code: 1) }
                return try Data(contentsOf: url)
            }
            let uncorrected = try printFile(nil), corrected = try printFile(PrintCorrection(offsetX: 5, offsetY: 3))
            let originalBounds = darkBounds(uncorrected), correctedBounds = darkBounds(corrected)
            check("native print-to-file renders image", originalBounds != nil && correctedBounds != nil)
            if let a = originalBounds, let b = correctedBounds {
                check("print calibration moves rendered pixels", b.minX - a.minX > 10 && b.minX - a.minX < 18 && abs(b.minY - a.minY) > 4 && abs(b.minY - a.minY) < 12)
            }
            check("print calibration preserves workbook image", w.sheet.images[0].data == changed.data && w.sheet.images[0].size == changed.size)
        } catch { check("native print-to-file", false) }

        func set(_ ref: String, _ value: String) {
            var cell = Cell(); cell.input = "'" + value; cell.value = .string(value)
            doc.workbook.sheets[0][CellRef(ref)!] = cell
        }
        set("A1", "Serial"); set("B1", "Name"); set("A2", "0000123"); set("B2", "Controller")
        set("A3", "0000124"); set("B3", "Hidden"); set("A4", "0000125"); set("B4", "Sensor")
        doc.workbook.sheets[0].filterHidden = [2]
        w.workbookDidChange(); w.grid.select(CellRef("A1")!); w.grid.select(CellRef("B4")!, extend: true)
        check("native label action", NSApp.sendAction(#selector(w.createLabelsAction(_:)), to: w, from: nil))
        if let source = w.lastCreatedLabelSource {
            check("source preserves complete displayed rows", source.rows == [["Serial", "Name"], ["0000123", "Controller"], ["0000124", "Hidden"], ["0000125", "Sensor"]])
            check("visibility and row identity", source.visible == [true, true, false, true] && source.rowNumbers == [1, 2, 3, 4] && source.firstRowIsHeader)
            do {
                var options = LabelSheetOptions(stock: try LabelStock.custom(width: 70, height: 37))
                options.firstRowIsHeader = true; options.textColumns = [0, 1]; options.copies = 2
                let visible = try LabelSheetRenderer.render(source: source, options: options)
                check("visible rows and copies", visible.labelCount == 4 && Set(visible.sourceRows) == Set([2, 4]))
                options.visibleOnly = false
                let all = try LabelSheetRenderer.render(source: source, options: options)
                check("all source rows", all.labelCount == 6 && Set(all.sourceRows) == Set([2, 3, 4]))
                check("label PDF contains identifiers", PDFDocument(data: all.data)?.string?.contains("0000124") == true)
            } catch { check("label output", false) }
        } else { check("source captured", false) }
        w.grid.select(CellRef("A1")!); w.grid.select(CellRef(row: 5_000, col: 0), extend: true)
        check("oversized source rejected before materialization", w.makeLabelSource() == nil)

        // Drive both real Fill Series dialogs through native controls, including the serial mode selection.
        w.grid.select(CellRef("D2")!); w.grid.select(CellRef("D4")!, extend: true)
        func descendants(_ view: NSView) -> [NSView] { [view] + view.subviews.flatMap(descendants) }
        var phase = 0, ticks = 0
        let timer = Timer(timeInterval: 0.04, repeats: true) { timer in
            ticks += 1
            guard ticks < 100 else { NSApp.abortModal(); timer.invalidate(); return }
            guard let window = NSApp.modalWindow, let content = window.contentView else { return }
            let controls = descendants(content)
            if phase == 0, let type = controls.first(where: { $0.identifier?.rawValue == "fill-series-type" }) as? NSPopUpButton {
                type.selectItem(at: 4); phase = 1
                controls.compactMap { $0 as? NSButton }.first { $0.title == L("OK") }?.performClick(nil)
            } else if phase == 1, controls.contains(where: { $0.identifier?.rawValue == "serial-prefix" }) {
                for (key, value) in [("prefix", "PCB-"), ("start", "7"), ("step", "2"), ("digits", "4"), ("suffix", "-A")] {
                    (controls.first { $0.identifier?.rawValue == "serial-" + key } as? NSTextField)?.stringValue = value
                }
                phase = 2; timer.invalidate()
                controls.compactMap { $0 as? NSButton }.first { $0.title == L("Fill") }?.performClick(nil)
            }
        }
        RunLoop.main.add(timer, forMode: .common)
        _ = NSApp.sendAction(#selector(w.fillSeriesAction(_:)), to: w, from: nil)
        timer.invalidate()
        check("serial dialog controls dispatched", phase == 2)
        let serials = ["D2", "D3", "D4"].map { w.sheet[CellRef($0)!].value }
        check("serial complete destination", serials == [.string("PCB-0007-A"), .string("PCB-0009-A"), .string("PCB-0011-A")] && w.sheet[CellRef("C2")!].value.isEmpty && w.sheet[CellRef("E4")!].value.isEmpty)
        doc.undoManager?.undo(); check("serial Undo entire range", ["D2", "D3", "D4"].allSatisfy { w.sheet[CellRef($0)!].value.isEmpty })
        doc.undoManager?.redo(); check("serial Redo entire range", ["D2", "D3", "D4"].map { w.sheet[CellRef($0)!].value } == serials)
        let before = w.sheet.cells
        check("serial overflow rejected", !w.applySerialNumbers(range: CellRange("D2:D4")!, start: 9_007_199_254_740_991, step: 1, digits: 0, prefix: "", suffix: "") && w.sheet.cells == before)
        doc.workbook.sheets[0].protection = SheetProtection()
        check("protected serial destination rejected", !w.applySerialNumbers(range: CellRange("D2:D4")!, start: 1, step: 1, digits: 4, prefix: "", suffix: "") && w.sheet.cells == before)
        doc.workbook.sheets[0].protection = nil
        do {
            let reopened = try ODS.read(ODS.write(doc.workbook))
            check("saved serials retain literal text", ["D2", "D3", "D4"].map { reopened.sheets[0][CellRef($0)!].value } == serials)
        } catch { check("serial file opens", false) }
        w.suppressAlerts = false
        return failures
    }
}
