import AppKit
import PDFKit

/// Output assertions use PDFKit text/geometry and native save-to-file printing.
/// No test submits a physical print job or changes a user's saved profile.
public enum LabelSheetChecks {
    public static func run() -> [String] {
        var failures: [String] = []
        func check(_ condition: Bool, _ name: String) { if !condition { failures.append(name) } }
        func rejects(_ name: String, _ body: () throws -> Void) { do { try body(); failures.append(name) } catch { } }
        do {
            let stock = try LabelStock.custom(width: 100, height: 140, gapX: 2, gapY: 2)
            check(stock.capacity == 4, "Custom A4 layout capacity")
            var options = LabelSheetOptions(stock: stock)
            options.textColumns = [0, 1]; options.firstRowIsHeader = true; options.copiesColumn = 2
            let source = LabelSource(title: fixtureTitle("Rows"), headers: ["A", "B", "C"],
                                     rows: [["Serial", "Description", "Copies"], ["000012", "Visible first", "2"],
                                            ["000013", "Hidden record", "1"], ["000014", "Visible last", "3"]],
                                     rowNumbers: [40, 41, 42, 43], visible: [true, true, false, true])
            let result = try LabelSheetRenderer.render(source: source, options: options)
            check(result.labelCount == 5 && result.pageCount == 2, "Per-row copies and complete pagination")
            check(result.sourceRows == [41, 41, 43, 43, 43], "Filtered source row identities retained through copies")
            guard let pdf = PDFDocument(data: result.data) else { return failures + ["Generated label PDF missing"] }
            let text = pdf.string ?? ""
            check(pdf.pageCount == 2 && occurrences("000012", in: text) == 2 && occurrences("000014", in: text) == 3,
                  "Every copied label has exact leading-zero source value")
            check(!text.contains("000013") && !text.contains("Hidden record"), "Hidden rows excluded")
            check(text.contains("Serial: 000012") && !text.contains("Copies: Copies"), "Header values used only as field names")
            options.visibleOnly = false
            let all = try LabelSheetRenderer.render(source: source, options: options)
            check(all.labelCount == 6 && all.sourceRows.contains(42), "All-row mode includes filtered row")
            options.visibleOnly = true; options.includeFieldNames = false; options.startSlot = 3
            let offset = try LabelSheetRenderer.render(source: source, options: options)
            check(offset.pageCount == 2 && offset.labelCount == 5, "Used label positions included in pagination")
            let offsetText = PDFDocument(data: offset.data)?.string ?? ""
            check(!offsetText.contains("Serial:"), "Field names option preserves literal values")

            var wrong = options; wrong.startSlot = Int.min
            rejects("Negative first slot accepted") { _ = try LabelSheetRenderer.render(source: source, options: wrong) }
            wrong = options; wrong.startSlot = stock.capacity
            rejects("First slot outside sheet accepted") { _ = try LabelSheetRenderer.render(source: source, options: wrong) }
            wrong = options; wrong.copies = Int.max
            rejects("Unbounded copy count accepted") { _ = try LabelSheetRenderer.render(source: source, options: wrong) }
            wrong = options; wrong.textColumns = [0, 0]
            rejects("Duplicate text columns accepted") { _ = try LabelSheetRenderer.render(source: source, options: wrong) }
            wrong = options; wrong.codeColumn = 200
            rejects("Missing code column accepted") { _ = try LabelSheetRenderer.render(source: source, options: wrong) }
            wrong = options; wrong.textColumns = []; wrong.codeColumn = nil
            rejects("Empty field selection accepted") { _ = try LabelSheetRenderer.render(source: source, options: wrong) }

            var simple = LabelSheetOptions(stock: stock); simple.textColumns = [0]
            rejects("Blank source text accepted") { _ = try LabelSheetRenderer.render(source: .init(title: fixtureTitle("Blank"), text: " \n"), options: simple) }
            rejects("Blank source row with field names accepted") {
                _ = try LabelSheetRenderer.render(source: .init(title: fixtureTitle("Blank"), headers: ["Serial"], rows: [[" "]]), options: simple)
            }
            rejects("Missing last-row cell accepted") {
                _ = try LabelSheetRenderer.render(source: .init(title: fixtureTitle("Missing"), headers: ["Serial"], rows: [["first"], []]), options: simple)
            }
            rejects("Inconsistent row visibility accepted") {
                _ = try LabelSheetRenderer.render(source: .init(title: fixtureTitle("Visibility"), headers: ["Serial"], rows: [["first"], ["last"]], visible: [true]), options: simple)
            }
            rejects("Inconsistent source row numbers accepted") {
                _ = try LabelSheetRenderer.render(source: .init(title: fixtureTitle("Numbers"), headers: ["Serial"], rows: [["first"], ["last"]], rowNumbers: [9]), options: simple)
            }
            rejects("More than 5000 source rows accepted") {
                _ = try LabelSheetRenderer.render(source: .init(title: fixtureTitle("Rows"), headers: ["Serial"], rows: Array(repeating: ["x"], count: 5001)), options: simple)
            }
            var copyOptions = simple; copyOptions.copiesColumn = 1
            for count in ["0", "-1", "5001", "1.5", "oops", "9223372036854775808"] {
                rejects("Invalid copies column \(count) accepted") {
                    _ = try LabelSheetRenderer.render(source: .init(title: fixtureTitle("Copies"), headers: ["Serial", "Count"], rows: [["A", count]]), options: copyOptions)
                }
            }
            rejects("Aggregate label count above 5000 accepted") {
                _ = try LabelSheetRenderer.render(source: .init(title: fixtureTitle("Copies"), headers: ["Serial", "Count"], rows: [["A", "3000"], ["B", "3000"]]), options: copyOptions)
            }
            var codeOptions = simple; codeOptions.codeColumn = 0
            rejects("Empty code in last selected row accepted") {
                _ = try LabelSheetRenderer.render(source: .init(title: fixtureTitle("Codes"), headers: ["Serial"], rows: [["000001"], [""]]), options: codeOptions)
            }
            var tiny = LabelSheetOptions(stock: try .custom(width: 12, height: 8)); tiny.textColumns = [0]
            rejects("Long final row silently clipped") {
                _ = try LabelSheetRenderer.render(source: .init(title: fixtureTitle("Fit"), headers: ["Text"], rows: [["A"], [String(repeating: "Unclipped content ", count: 200)]]), options: tiny)
            }
            rejects("Malformed artwork accepted") {
                _ = try LabelSheetRenderer.render(source: .init(title: fixtureTitle("PDF"), pdfData: Data("bad PDF".utf8)), options: simple)
            }
            rejects("Multiple artwork pages accepted") {
                _ = try LabelSheetRenderer.render(source: .init(title: fixtureTitle("PDF"), pdfData: artwork(pages: 2)), options: simple)
            }
            var repeated = simple; repeated.copies = 5
            let art = try LabelSheetRenderer.render(source: .init(title: fixtureTitle("Artwork"), pdfData: artwork()), options: repeated)
            let artText = PDFDocument(data: art.data)?.string ?? ""
            check(art.pageCount == 2 && occurrences("VECTOR-ART-1", in: artText) == 5, "Repeated PDF artwork retains selectable vector text on every page")
            check(!LabelStock.catalog.isEmpty && LabelStock.catalog.allSatisfy { (try? $0.validate()) != nil }, "Catalog contains usable A4 layouts")
            check(!LabelStock.catalog.contains(where: { ["apli-13882", "herma-4106"].contains($0.id) }), "Known circular catalog stock excluded")
        } catch { failures.append("Label output: \(error)") }
        return failures
    }

    public static func runUI(outputDirectory: URL? = nil) -> [String] {
        var failures: [String] = []
        func check(_ condition: Bool, _ name: String) { if !condition { failures.append(name) } }
        let controller = LabelSheetController(sources: [.init(title: fixtureTitle("Fixture"), text: "PCB-000001")])
        controller.generate(nil)
        check(wait { controller.result != nil }, "Label preview completes")
        check(controller.saveButton.isEnabled && controller.printButton.isEnabled, "Fresh preview enables output")
        controller.panel.contentView?.layoutSubtreeIfNeeded()
        check(controller.preview.bounds.width > 100 && controller.preview.bounds.height > 100 && controller.preview.scaleFactor.isFinite && controller.preview.scaleFactor > 0 && controller.preview.visiblePages.count == 1,
              "Initial label preview has a laid-out visible page at a valid scale")
        let previewPDF = controller.preview.dataWithPDF(inside: controller.preview.bounds)
        check((PDFDocument(data: previewPDF)?.string ?? "").contains("PCB-000001"), "Initial native PDF preview draws source content")
        check(controller.width.delegate === controller, "Width edits have a native delegate")
        check(controller.copies.delegate === controller && controller.startSlot.delegate === controller, "Count edits have native delegates")
        controller.width.stringValue = "75"
        controller.width.delegate?.controlTextDidChange?(Notification(name: NSControl.textDidChangeNotification, object: controller.width))
        check(controller.result == nil && !controller.saveButton.isEnabled && !controller.printButton.isEnabled,
              "Typing a width immediately invalidates saved/printed preview")
        controller.startSlot.stringValue = String(Int.min)
        do { _ = try controller.options(); failures.append("Int.min first position accepted") } catch { }
        controller.startSlot.stringValue = "1"
        controller.generate(nil)
        controller.copies.stringValue = "2"
        controller.copies.delegate?.controlTextDidChange?(Notification(name: NSControl.textDidChangeNotification, object: controller.copies))
        // Let the old completion run: it must not restore stale output after an edit.
        RunLoop.main.run(until: Date(timeIntervalSinceNow: 0.15))
        check(controller.result == nil && !controller.saveButton.isEnabled, "Outdated asynchronous preview is ignored")
        controller.generate(nil)
        check(wait { controller.result?.labelCount == 2 }, "Regenerated preview uses edited copy count")
        if let stock = LabelStock.catalog.first {
            controller.stockPicker.stringValue = stock.name
            controller.controlTextDidChange(Notification(name: NSControl.textDidChangeNotification, object: controller.stockPicker))
            check(controller.result == nil && !controller.saveButton.isEnabled && controller.customRows.isHidden,
                  "Stock typeahead invalidates preview and updates fields")
            check((try? controller.options().stock.id) == stock.id, "Typed stock uses the exact matching geometry")
        }
        controller.stockPicker.stringValue = L("Custom")
        controller.controlTextDidChange(Notification(name: NSControl.textDidChangeNotification, object: controller.stockPicker))
        check(!controller.customRows.isHidden && (try? controller.options().stock.id) == "custom", "Typed Custom restores custom geometry")
        controller.stockPicker.stringValue = "unknown stock"
        controller.controlTextDidChange(Notification(name: NSControl.textDidChangeNotification, object: controller.stockPicker))
        do { _ = try controller.options(); failures.append("Unresolved stock typeahead accepted") } catch { }
        controller.generation += 1
        let table = LabelSheetController(sources: [.init(title: fixtureTitle("Table"), headers: ["Serial", "Description", "Count"], rows: [["000001", "Controller board", "2"]])])
        table.panel.contentView?.layoutSubtreeIfNeeded()
        check(table.controlsScroll.frame.minY > table.status.frame.maxY, "Scrollable options stay above status and actions")
        check(table.controlsScroll.hasVerticalScroller, "Long options remain reachable")
        if let document = table.fieldsScroll.documentView, let first = table.fields.arrangedSubviews.first {
            let fieldRect = document.convert(first.bounds, from: first)
            check(table.fieldsScroll.frame.width > 300 && fieldRect.minY >= 0 && fieldRect.minY < 12,
                  "Text-column picker uses the pane width and starts at the top")
        } else { failures.append("Text-column picker is missing its controls") }
        if let outputDirectory {
            do {
                try FileManager.default.createDirectory(at: outputDirectory, withIntermediateDirectories: true)
                table.generate(nil); _ = wait { table.result != nil }
                table.panel.contentView?.layoutSubtreeIfNeeded()
                if let content = table.panel.contentView { try capture(content, to: outputDirectory.appendingPathComponent("labels-dialog.png")) }
                let editor = TechnicalElementEditor(initial: .init(codeType: .dataMatrix, value: "PCB-000001", widthMM: 25), initialValue: "") { _, _ in }
                editor.alert.layout()
                if let content = editor.alert.window.contentView { try capture(content, to: outputDirectory.appendingPathComponent("technical-dialog.png")) }
            } catch { failures.append("Native dialog snapshots: \(error)") }
        }
        table.generation += 1
        return failures
    }

    public static func runPrint(outputDirectory: URL) -> [String] {
        var failures: [String] = []
        func check(_ condition: Bool, _ name: String) { if !condition { failures.append(name) } }
        do {
            let mm = 72.0 / 25.4
            let correction = PrintCorrection(offsetX: 2, offsetY: 3, scaleX: 1.02, scaleY: 0.98)
            check(correction.isValid && !PrintCorrection(scaleX: .nan).isValid, "Calibration bounds")
            let measured = try PrintCorrection.measured(horizontal: 99, vertical: 101)
            check(abs(measured.scaleX - 100 / 99.0) < 0.000001 && abs(measured.scaleY - 100 / 101.0) < 0.000001, "Measured 100 mm correction")
            do { _ = try PrintCorrection.measured(horizontal: 1, vertical: 1); failures.append("Extreme measured correction accepted") } catch { }
            let info = NSPrintInfo.shared.copy() as! NSPrintInfo
            info.paperSize = CGSize(width: 210 * mm, height: 297 * mm); info.scalingFactor = 1
            ProductionPrint.setCorrection(correction, in: info)
            let accessory = PrintCalibrationAccessory(info: info)
            _ = accessory.view
            check(accessory.enabled && abs(accessory.offsetX - 2) < 0.000001 && abs(accessory.scaleX - 102) < 0.000001,
                  "Print accessory restores the active correction")
            check(ProductionPrint.correction(in: info) == correction, "Opening accessory preserves print correction")
            accessory.enabled = false
            check(ProductionPrint.correction(in: info) == nil, "Disabling correction takes effect")
            accessory.enabled = true
            check(ProductionPrint.correction(in: info) == correction, "Re-enabling restores correction")
            for flipped in [false, true] {
                let context = CGContext(data: nil, width: 20, height: 20, bitsPerComponent: 8, bytesPerRow: 80,
                                        space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
                let page = CGRect(x: 10, y: 20, width: 300, height: 400), point = CGPoint(x: 110, y: 220)
                ProductionPrint.apply(correction, in: context, pageRect: page, flipped: flipped)
                let actual = point.applying(context.ctm), originY = flipped ? page.minY : page.maxY
                let expected = CGPoint(x: page.minX + (point.x - page.minX) * correction.scaleX + correction.offsetX * mm,
                                       y: originY + (point.y - originY) * correction.scaleY + (flipped ? 1 : -1) * correction.offsetY * mm)
                check(abs(actual.x - expected.x) < 0.000001 && abs(actual.y - expected.y) < 0.000001, "Calibration top-origin transform, flipped=\(flipped)")
            }
            try FileManager.default.createDirectory(at: outputDirectory, withIntermediateDirectories: true)
            let source = artwork(pages: 2)
            try source.write(to: outputDirectory.appendingPathComponent("print-source.pdf"))
            let originalInfo = NSPrintInfo.shared.copy() as! NSPrintInfo
            originalInfo.paperSize = CGSize(width: 210 * mm, height: 297 * mm); originalInfo.scalingFactor = 1
            originalInfo.leftMargin = 19; originalInfo.topMargin = 23
            let unchangedMargins = (originalInfo.leftMargin, originalInfo.topMargin)
            var printed: [PDFDocument] = []
            for (name, adjustment) in [("identity", Optional<PrintCorrection>.none), ("translated", PrintCorrection(offsetX: 2, offsetY: 3)), ("scaled", correction)] {
                let local = originalInfo.copy() as! NSPrintInfo
                ProductionPrint.setCorrection(adjustment, in: local)
                let op = try ProductionPrint.operation(pdfData: source, printInfo: local)
                let url = outputDirectory.appendingPathComponent("print-\(name).pdf")
                op.printInfo.jobDisposition = .save
                op.printInfo.dictionary()[NSPrintInfo.AttributeKey.jobSavingURL] = url
                op.showsPrintPanel = false; op.showsProgressPanel = false
                check(op.run(), "Native save-to-file print \(name)")
                guard let pdf = PDFDocument(url: url) else { failures.append("Missing saved print \(name)"); continue }
                printed.append(pdf)
                check(pdf.pageCount == 2 && (pdf.string ?? "").contains("VECTOR-ART-1") && (pdf.string ?? "").contains("VECTOR-ART-2"),
                      "Printed \(name) retains both complete pages")
            }
            check(originalInfo.leftMargin == unchangedMargins.0 && originalInfo.topMargin == unchangedMargins.1 && ProductionPrint.correction(in: originalInfo) == nil,
                  "Print preparation preserves caller margins and correction state")
            if let identity = printed.first, let saved = selectionBounds(identity, "VECTOR-ART-1"), let original = selectionBounds(PDFDocument(data: source)!, "VECTOR-ART-1") {
                check(abs(saved.minX - original.minX) < 0.1 && abs(saved.minY - original.minY) < 0.1 && abs(saved.width - original.width) < 0.1,
                      "Disabled calibration preserves absolute paper coordinates")
            } else { failures.append("Identity print geometry missing") }
            if printed.count == 3, let a = selectionBounds(printed[0], "VECTOR-ART-1"),
               let b = selectionBounds(printed[1], "VECTOR-ART-1"), let c = selectionBounds(printed[2], "VECTOR-ART-1") {
                check(abs((b.minX - a.minX) - 2 * mm) < 0.8 && abs((b.minY - a.minY) + 3 * mm) < 0.8,
                      "Saved print contains actual translation")
                check(abs(c.width / a.width - 1.02) < 0.01 && abs(c.height / a.height - 0.98) < 0.03,
                      "Saved print contains actual independent scales")
            } else { failures.append("Saved print text geometry missing") }
            // Annotation appearances must survive printing, including hidden-but-printable items.
            let annotated = PDFDocument(data: artwork())!
            let annotatedPage = annotated.page(at: 0)!
            for (i, name) in ["PRINT-ANNOTATION", "DO-NOT-PRINT", "HIDDEN-PRINT"].enumerated() {
                let annotation = PDFAnnotation(bounds: CGRect(x: 100, y: 300 - i * 40, width: 250, height: 30), forType: .freeText, withProperties: nil)
                annotation.contents = name; annotation.font = .systemFont(ofSize: 14); annotation.fontColor = .black
                annotation.color = .clear; annotation.shouldDisplay = i != 2; annotation.shouldPrint = i != 1
                annotatedPage.addAnnotation(annotation)
            }
            let annotatedBytes = annotated.dataRepresentation()!
            let annotationURL = outputDirectory.appendingPathComponent("print-annotations.pdf")
            let annotationOperation = try ProductionPrint.operation(pdfData: annotatedBytes, printInfo: originalInfo)
            annotationOperation.printInfo.jobDisposition = .save
            annotationOperation.printInfo.dictionary()[NSPrintInfo.AttributeKey.jobSavingURL] = annotationURL
            annotationOperation.showsPrintPanel = false; annotationOperation.showsProgressPanel = false
            check(annotationOperation.run(), "Native annotation print saves")
            let annotationText = PDFDocument(url: annotationURL)?.string ?? ""
            check(annotationText.contains("PRINT-ANNOTATION") && annotationText.contains("HIDDEN-PRINT") && !annotationText.contains("DO-NOT-PRINT"),
                  "Saved print honors annotation appearance and print visibility")
            check(annotatedPage.annotations[2].shouldDisplay == false && annotatedPage.annotations[1].shouldPrint == false,
                  "Print visibility changes do not affect source annotations")

            let rotated = PDFDocument(data: artwork())!, rotatedPage = PDFDocument(data: artwork())!.page(at: 0)!
            rotatedPage.setBounds(CGRect(x: 50, y: 50, width: 400, height: 600), for: .cropBox); rotatedPage.rotation = 90
            rotated.removePage(at: 0); rotated.insert(rotatedPage, at: 0)
            let rotatedBytes = rotated.dataRepresentation()!, rotatedURL = outputDirectory.appendingPathComponent("print-rotated-crop.pdf")
            let rotationOperation = try ProductionPrint.operation(pdfData: rotatedBytes, printInfo: originalInfo)
            rotationOperation.printInfo.jobDisposition = .save; rotationOperation.printInfo.dictionary()[NSPrintInfo.AttributeKey.jobSavingURL] = rotatedURL
            rotationOperation.showsPrintPanel = false; rotationOperation.showsProgressPanel = false
            check(rotationOperation.run(), "Native rotated crop print saves")
            let rotationOutput = PDFDocument(url: rotatedURL)
            check(rotationOutput?.pageCount == 1 && (rotationOutput?.string ?? "").contains("VECTOR-ART-1"), "Rotated cropped page retains complete content")
            if let output = rotationOutput, let actual = selectionBounds(output, "VECTOR-ART-1"),
               let original = selectionBounds(PDFDocument(data: artwork())!, "VECTOR-ART-1"),
               let ref = CGPDFDocument(CGDataProvider(data: rotatedBytes as CFData)!)?.page(at: 1) {
                let pageSize = originalInfo.paperSize, target = CGRect(x: (pageSize.width - 600) / 2, y: (pageSize.height - 400) / 2, width: 600, height: 400)
                let expected = original.applying(ref.getDrawingTransform(.cropBox, rect: target, rotate: 0, preserveAspectRatio: true))
                check(abs(actual.midX - expected.midX) < 1 && abs(actual.midY - expected.midY) < 1 && abs(actual.width - expected.width) < 1,
                      "Saved print preserves rotation and nonzero crop origin")
            } else { failures.append("Rotated cropped print geometry missing") }
            let calibration = try ProductionPrint.calibrationPDF()
            try calibration.write(to: outputDirectory.appendingPathComponent("calibration.pdf"))
            check(PDFDocument(data: calibration)?.pageCount == 1 && (PDFDocument(data: calibration)?.string ?? "").contains("100 mm"), "Calibration test sheet is readable PDF")
        } catch { failures.append("Print calibration: \(error)") }
        return failures
    }

    private static func capture(_ view: NSView, to url: URL) throws {
        view.layoutSubtreeIfNeeded(); view.displayIfNeeded()
        guard let bitmap = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { throw TechnicalElementError("Cannot allocate native dialog snapshot") }
        view.cacheDisplay(in: view.bounds, to: bitmap)
        guard let data = bitmap.representation(using: .png, properties: [:]) else { throw TechnicalElementError("Cannot encode native dialog snapshot") }
        try data.write(to: url)
    }
    private static func fixtureTitle(_ value: String) -> String { value }
    private static func occurrences(_ needle: String, in text: String) -> Int { text.components(separatedBy: needle).count - 1 }
    private static func wait(_ condition: () -> Bool) -> Bool {
        let until = Date(timeIntervalSinceNow: 5)
        while !condition() && Date() < until { RunLoop.main.run(until: Date(timeIntervalSinceNow: 0.01)) }
        return condition()
    }
    private static func selectionBounds(_ document: PDFDocument, _ text: String) -> CGRect? {
        guard let selection = document.findString(text, withOptions: []).first, let page = selection.pages.first else { return nil }
        return selection.bounds(for: page)
    }
    private static func artwork(pages: Int = 1) -> Data {
        let data = NSMutableData(); var box = CGRect(x: 0, y: 0, width: 210 * 72 / 25.4, height: 297 * 72 / 25.4)
        let consumer = CGDataConsumer(data: data)!, context = CGContext(consumer: consumer, mediaBox: &box, nil)!
        for page in 1...pages {
            context.beginPDFPage(nil)
            context.setFillColor(NSColor.black.cgColor); context.fill(CGRect(x: 100, y: 500, width: 50, height: 30))
            NSGraphicsContext.saveGraphicsState(); NSGraphicsContext.current = NSGraphicsContext(cgContext: context, flipped: false)
            ("VECTOR-ART-\(page)" as NSString).draw(at: CGPoint(x: 100, y: 450), withAttributes: [.font: NSFont.systemFont(ofSize: 16), .foregroundColor: NSColor.black])
            NSGraphicsContext.restoreGraphicsState(); context.endPDFPage()
        }
        context.closePDF(); return data as Data
    }
}
