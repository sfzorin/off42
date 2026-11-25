import AppKit
import Kit42
import PDFKit

/// File/lifecycle regressions use small independent PDF fixtures and the real document writer.
enum PDFFilesAudit {
    static func fixture(_ texts: [String] = ["SECRET-ONE", "VISIBLE-TWO", "VISIBLE-THREE"], media: String = "0 0 400 500", content: String? = nil) -> Data {
        var objects = ["<< /Type /Catalog /Pages 2 0 R >>", "", "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"]
        var children: [String] = []
        for text in texts {
            let page = objects.count + 1, stream = page + 1
            children.append("\(page) 0 R")
            objects.append("<< /Type /Page /Parent 2 0 R /MediaBox [\(media)] /Resources << /Font << /F1 3 0 R >> >> /Contents \(stream) 0 R >>")
            let safe = text.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "(", with: "\\(").replacingOccurrences(of: ")", with: "\\)")
            let commands = content ?? "BT /F1 20 Tf 40 420 Td (\(safe)) Tj ET"
            objects.append("<< /Length \(commands.utf8.count) >>\nstream\n\(commands)\nendstream")
        }
        objects[1] = "<< /Type /Pages /Count \(texts.count) /Kids [\(children.joined(separator: " "))] >>"
        var output = "%PDF-1.4\n", offsets = [0]
        for (index, object) in objects.enumerated() {
            offsets.append(output.utf8.count)
            output += "\(index + 1) 0 obj\n\(object)\nendobj\n"
        }
        let xref = output.utf8.count
        output += "xref\n0 \(objects.count + 1)\n0000000000 65535 f \n"
        for offset in offsets.dropFirst() { output += String(format: "%010d 00000 n \n", offset) }
        output += "trailer\n<< /Size \(objects.count + 1) /Root 1 0 R >>\nstartxref\n\(xref)\n%%EOF\n"
        return Data(output.utf8)
    }

    /// One page holding a page-sized grey image, as a scanner writes it (`dpi` relative to A4).
    static func scanFixture(dpi: CGFloat, landscape: Bool = false) -> Data? {
        var box = landscape ? CGRect(x: 0, y: 0, width: 842, height: 595) : CGRect(x: 0, y: 0, width: 595, height: 842)
        let w = Int(box.width / 72 * dpi), h = Int(box.height / 72 * dpi)
        guard let bitmap = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0, space: CGColorSpaceCreateDeviceGray(),
                                     bitmapInfo: CGImageAlphaInfo.none.rawValue) else { return nil }
        bitmap.setFillColor(gray: 1, alpha: 1); bitmap.fill(CGRect(x: 0, y: 0, width: w, height: h))
        bitmap.setFillColor(gray: 0, alpha: 1); bitmap.fill(CGRect(x: w / 10, y: h / 2, width: w / 2, height: h / 80))
        guard let image = bitmap.makeImage() else { return nil }
        let data = NSMutableData()
        guard let consumer = CGDataConsumer(data: data), let ctx = CGContext(consumer: consumer, mediaBox: &box, nil) else { return nil }
        ctx.beginPDFPage(nil)
        ctx.draw(image, in: box)
        ctx.endPDFPage()
        ctx.closePDF()
        return data as Data
    }

    static func imageData() -> Data {
        let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 8, pixelsHigh: 6, bitsPerSample: 8,
                                      samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
        for y in 0..<6 { for x in 0..<8 { bitmap.setColor(.blue, atX: x, y: y) } }
        return bitmap.representation(using: .png, properties: [:])!
    }

    static func run() -> Int {
        var count = 0, failures = 0
        func check(_ name: String, _ condition: Bool) { count += 1; if !condition { failures += 1 }; print(condition ? "PASS" : "FAIL", "PDF files:", name) }
        func sameColor(_ lhs: NSColor?, _ rhs: NSColor?) -> Bool {
            guard let lhs = lhs?.usingColorSpace(.sRGB), let rhs = rhs?.usingColorSpace(.sRGB) else { return false }
            return abs(lhs.redComponent - rhs.redComponent) < 0.0000001 &&
                abs(lhs.greenComponent - rhs.greenComponent) < 0.0000001 &&
                abs(lhs.blueComponent - rhs.blueComponent) < 0.0000001 &&
                abs(lhs.alphaComponent - rhs.alphaComponent) < 0.0000001
        }
        check("range grouping and open end", PDFTools.parseRanges("1-2; 3-", pageCount: 4) == [[0, 1], [2, 3]])
        for invalid in ["0", "2-1", "1,", ",1", "1;;2", "1-5", "9223372036854775807"] {
            check("reject malformed range \(invalid)", PDFTools.parseRanges(invalid, pageCount: 4) == nil)
        }
        check("negative page count cannot form a Swift range", PDFTools.pageIndices("", pageCount: -1) == nil)
        check("large split interval does not overflow", PDFTools.everyN(Int.max, pageCount: 3) == [[0, 1, 2]])
        check("zero split interval is rejected", PDFTools.everyN(0, pageCount: 3).isEmpty)
        check("strict area accepts a valid explicit page", ToolsCLI.area("10,20,30,40@2", pageCount: 3)?.1 == 1)
        for invalid in ["", "1,2,3", "1,2,3,4,5", "nan,2,3,4", "1,inf,3,4", "1,2,-3,4", "1,2,0,4", "1,2,3,4@", "1,2,3,4@0", "1,2,3,4@-9223372036854775808", "1,2,3,4@4", "1,2,3,4@1@2"] {
            check("reject malformed area \(invalid)", ToolsCLI.area(invalid, pageCount: 3) == nil)
        }
        check("ordinary A4 raster fits bounded allocation", RedactionRaster.bitmapSize(CGSize(width: 595, height: 842)) != nil)
        for size in [CGSize(width: CGFloat.nan, height: 500), CGSize(width: CGFloat.infinity, height: 500), CGSize(width: -1, height: 500), CGSize(width: 1e100, height: 500), CGSize(width: 10000, height: 10000)] {
            check("reject unsafe redaction bitmap \(size)", RedactionRaster.bitmapSize(size) == nil)
        }
        do {
            let doc = PDFDoc()
            try doc.read(from: fixture(), ofType: "com.adobe.pdf")
            let metadataValue = "Original metadata"
            doc.extras.info.title = metadataValue
            doc.extras.dirty.insert(.info)
            let original = doc.pdf, originalBytes = doc.originalData
            do { try doc.read(from: Data("broken".utf8), ofType: "com.adobe.pdf"); check("invalid read throws", false) }
            catch { check("invalid read preserves model, source bytes and extras", doc.pdf === original && doc.originalData == originalBytes && doc.extras.info.title == "Original metadata") }
            try doc.read(from: imageData(), ofType: "public.png")
            check("image import replaces old PDF source and signature state", doc.pdf.pageCount == 1 && doc.originalData == nil && doc.signatures.isEmpty)
            check("image import cannot inherit previous PDF metadata", doc.extras.info.title.isEmpty && doc.extras.files.isEmpty && doc.extras.labels.isEmpty)

            try doc.read(from: fixture(), ofType: "com.adobe.pdf")
            let page = doc.pdf.page(at: 0)!
            let first = PDFAnnotation(bounds: CGRect(x: 20, y: 20, width: 40, height: 40), forType: .square, withProperties: nil)
            let watermark = WatermarkOverlay(spec: WatermarkSpec(text: "AUDIT"), page: page)
            let last = PDFAnnotation(bounds: CGRect(x: 80, y: 20, width: 40, height: 40), forType: .circle, withProperties: nil)
            page.addAnnotation(first); page.addAnnotation(watermark); page.addAnnotation(last)
            let before = page.annotations.map(ObjectIdentifier.init)
            let saved = try doc.data(ofType: "com.adobe.pdf")
            check("Save keeps original annotation stacking and identities", page.annotations.map(ObjectIdentifier.init) == before)
            check("saved document keeps every page", PDFDocument(data: saved)?.pageCount == 3)
            page.removeAnnotation(watermark)
            let styledNote = PDFAnnotation(bounds: CGRect(x: 45, y: 170, width: 220, height: 40), forType: .freeText, withProperties: nil)
            let originalFont = NSFont.boldSystemFont(ofSize: 18)
            styledNote.contents = "REDRAW-STYLED-TEXT"; styledNote.font = originalFont; styledNote.fontColor = .systemBlue
            page.addAnnotation(styledNote)
            let originalNoteBounds = styledNote.bounds
            let beforeRedraw = page.annotations.map(ObjectIdentifier.init)
            page.rotation = 90
            let redrawn = PageOverlay.redraw(page, overlay: { _ in })
            check("redraw leaves source rotation and annotations intact", page.rotation == 90 && page.annotations.map(ObjectIdentifier.init) == beforeRedraw)
            check("redraw output keeps annotation count and crop", redrawn?.annotations.count == beforeRedraw.count && redrawn?.bounds(for: .cropBox) == page.bounds(for: .cropBox))
            let redrawnNote = redrawn?.annotations.first { $0.contents == "REDRAW-STYLED-TEXT" }
            check("redraw preserves the editable system font family", redrawnNote?.font?.familyName == originalFont.familyName)
            let sameTextStyle = redrawnNote?.font?.pointSize == originalFont.pointSize && redrawnNote?.font?.fontDescriptor.symbolicTraits == originalFont.fontDescriptor.symbolicTraits && sameColor(redrawnNote?.fontColor, styledNote.fontColor)
            if !sameTextStyle {
                print("PDF redraw style diagnostic", "source font", originalFont, "copy font", redrawnNote?.font as Any,
                      "source colour", styledNote.fontColor?.usingColorSpace(.sRGB) as Any, "copy colour", redrawnNote?.fontColor?.usingColorSpace(.sRGB) as Any)
            }
            check("redraw preserves editable font size, style and colour", sameTextStyle)
            check("redraw duplicates the text annotation at its original coordinates", redrawnNote != nil && redrawnNote !== styledNote && redrawnNote?.bounds == originalNoteBounds)
            check("redraw leaves the source text annotation and font unchanged", styledNote.page === page && styledNote.bounds == originalNoteBounds && styledNote.contents == "REDRAW-STYLED-TEXT" && styledNote.font?.fontName == originalFont.fontName && styledNote.font?.pointSize == originalFont.pointSize)

            try doc.read(from: fixture(), ofType: "com.adobe.pdf")
            let fields = [PDFTools.makeField(.text, name: "Name", bounds: CGRect(x: 40, y: 320, width: 200, height: 24)),
                          PDFTools.makeField(.checkBox, name: "Agree", bounds: CGRect(x: 40, y: 280, width: 18, height: 18)),
                          PDFTools.makeField(.choice, name: "Colour", bounds: CGRect(x: 40, y: 240, width: 150, height: 24), options: ["Red", "Green", "Blue"])]
            for field in fields { doc.pdf.page(at: 0)?.addAnnotation(field) }
            PDFTools.setValue("A=B", of: fields[0]); PDFTools.setValue("yes", of: fields[1]); PDFTools.setValue("Green", of: fields[2])
            let formData = try doc.data(ofType: "com.adobe.pdf")
            let reopened = PDFDocument(data: formData)!
            let values = Dictionary(uniqueKeysWithValues: PDFTools.fields(reopened).map { ($0.1.fieldName ?? "", PDFTools.value(of: $0.1)) })
            check("text, checkbox and choice fields reopen with their values", values == ["Name": "A=B", "Agree": "on", "Colour": "Green"])
            let structure = try PDFFileStructure(formData)
            let catalog = try structure.resolve(structure.trailer["Root"])
            check("saved widgets have a catalog AcroForm", catalog?["AcroForm"] != nil)
            doc.pdf = reopened
            let checked = PDFTools.fields(reopened).first { $0.1.fieldName == "Agree" }!.1
            PDFTools.setValue("off", of: checked)
            let cleared = PDFDocument(data: try doc.data(ofType: "com.adobe.pdf"))!
            check("clearing a previously saved checkbox remains Off after another save", PDFTools.fields(cleared).first { $0.1.fieldName == "Agree" }.map { PDFTools.value(of: $0.1) } == "off")

            try doc.read(from: fixture(), ofType: "com.adobe.pdf")
            let redactedPage = doc.pdf.page(at: 0)!
            let hit = doc.pdf.findString("SECRET-ONE", withOptions: []).first!
            redactedPage.addAnnotation(RedactOverlay(bounds: hit.bounds(for: redactedPage).insetBy(dx: -2, dy: -2), color: .black))
            let data = try doc.data(ofType: "com.adobe.pdf")
            check("redaction removes original searchable text and preserves other pages", !(PDFDocument(data: data)?.string?.contains("SECRET-ONE") ?? true) && PDFDocument(data: data)?.string?.contains("VISIBLE-TWO") == true)
            check("redaction compaction removes original literal bytes", data.range(of: Data("SECRET-ONE".utf8)) == nil)

            let partial = PDFDoc(pdf: PDFDocument(data: fixture())!), partialPage = partial.pdf.page(at: 0)!
            let note = PDFAnnotation(bounds: CGRect(x: 40, y: 300, width: 200, height: 40), forType: .freeText, withProperties: nil)
            note.contents = "ANNOTATION-SECRET"; note.font = .systemFont(ofSize: 12)
            let widget = PDFTools.makeField(.text, name: "SecretField", bounds: CGRect(x: 40, y: 200, width: 200, height: 40))
            widget.widgetStringValue = "FIELD-SECRET"
            partialPage.addAnnotation(note); partialPage.addAnnotation(widget)
            for y in [200, 300] { partialPage.addAnnotation(RedactOverlay(bounds: CGRect(x: 40, y: y, width: 10, height: 5), color: .black)) }
            let partialData = try partial.data(ofType: "com.adobe.pdf"), partialSaved = PDFDocument(data: partialData)!
            check("small explicit overlap removes the entire free-text annotation and widget", !partialSaved.page(at: 0)!.annotations.contains { $0.isKind(.freeText) || $0.isKind(.widget) } && PDFTools.fields(partialSaved).isEmpty)
            let partialStructure = try PDFFileStructure(partialData)
            let partialCatalog = try partialStructure.resolve(partialStructure.trailer["Root"])
            let partialForm = try partialStructure.resolve(partialCatalog?["AcroForm"])
            let remainingFields = try partialStructure.resolve(partialForm?["Fields"])
            check("redacted widget leaves no recoverable AcroForm field or secret literal bytes", (remainingFields?.items ?? []).isEmpty && partialData.range(of: Data("FIELD-SECRET".utf8)) == nil && partialData.range(of: Data("ANNOTATION-SECRET".utf8)) == nil)
            check("partial redaction serializes a copy without destroying editable source fields", widget.widgetStringValue == "FIELD-SECRET" && note.contents == "ANNOTATION-SECRET")
            let whiteout = PDFDoc(pdf: PDFDocument(data: fixture())!), whiteoutPage = whiteout.pdf.page(at: 0)!
            let coveredNote = PDFAnnotation(bounds: CGRect(x: 40, y: 200, width: 200, height: 40), forType: .freeText, withProperties: nil)
            coveredNote.contents = "SAME-BOUNDS-SECRET"
            let cover = PDFAnnotation(bounds: coveredNote.bounds, forType: .square, withProperties: nil)
            cover.interiorColor = .white; let border = PDFBorder(); border.lineWidth = 0; cover.border = border
            whiteoutPage.addAnnotation(coveredNote); whiteoutPage.addAnnotation(cover)
            let whiteoutSaved = PDFDocument(data: try whiteout.data(ofType: "com.adobe.pdf"))!
            check("keeping a whiteout cannot also preserve a same-bounds text annotation", whiteoutSaved.page(at: 0)!.annotations.contains { $0.isWhiteout } && !whiteoutSaved.page(at: 0)!.annotations.contains { $0.isKind(.freeText) })

            let layered = PDFDocument(data: fixture())!, layeredPage = layered.page(at: 0)!
            let layeredHit = layered.findString("SECRET-ONE", withOptions: []).first!
            let edited = EditedTextOverlay.make(replacing: layeredHit, on: layeredPage, with: "OVERLAY-SECRET")
            layeredPage.addAnnotation(edited)
            layeredPage.addAnnotation(RedactOverlay(bounds: edited.bounds.insetBy(dx: -2, dy: -2), color: .black))
            let layeredData = PageOverlay.bakedData(layered)
            check("explicit redaction removes newly rendered edited text as well as original glyphs", layeredData != nil && PDFDocument(data: layeredData!)?.string?.contains("OVERLAY-SECRET") == false && PDFDocument(data: layeredData!)?.string?.contains("SECRET-ONE") == false)

            let linked = PDFDocument(data: fixture())!, incoming = PDFPage.blank()
            let destination = PDFDestination(page: linked.page(at: 0)!, at: CGPoint(x: 30, y: 400)); destination.zoom = 2
            let link = PDFAnnotation(bounds: CGRect(x: 10, y: 10, width: 60, height: 20), forType: .link, withProperties: nil)
            link.action = PDFActionGoTo(destination: destination); incoming.addAnnotation(link)
            let outline = PDFOutline(), child = PDFOutline()
            let replacementLabel = "Replaced page"
            child.label = replacementLabel; child.destination = destination; outline.insertChild(child, at: 0); linked.outlineRoot = outline
            PageOverlay.replace([(-1, PDFPage.blank()), (0, incoming), (0, PDFPage.blank())], in: linked)
            check("page replacement rejects negative and duplicate indices", linked.pageCount == 3 && linked.page(at: 0) === incoming)
            check("incoming page links and outline destinations retain target and zoom", (link.action as? PDFActionGoTo)?.destination.page === incoming && (link.action as? PDFActionGoTo)?.destination.zoom == 2 && child.destination?.page === incoming && child.destination?.zoom == 2)

            let oversized = PDFDocument(data: fixture([""], media: "0 0 100 20000", content: "q BI /W 1 /H 1 /BPC 8 /CS /RGB ID abc EI Q"))!
            let oversizedPage = oversized.page(at: 0)!
            let unsafeMark = RedactOverlay(bounds: CGRect(x: 0, y: 0, width: 4, height: 4), color: .black)
            oversizedPage.addAnnotation(unsafeMark)
            check("failed bounded raster fallback aborts redaction instead of returning source content", PageOverlay.bakedData(oversized) == nil)
            check("failed redaction leaves original editable mark in place", oversizedPage.annotations.contains { $0 === unsafeMark })

            let root = FileManager.default.temporaryDirectory.appendingPathComponent("PDF42-files-" + UUID().uuidString)
            try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
            defer { try? FileManager.default.removeItem(at: root) }
            let valid = root.appendingPathComponent("valid.pdf")
            try fixture().write(to: valid)
            do { _ = try PDFTools.mergeChecked([valid, root.appendingPathComponent("missing.pdf")]); check("missing merge input throws", false) }
            catch { check("missing merge input cannot produce a partial document", true) }
            check("valid merge keeps complete page order", try PDFTools.mergeChecked([valid, valid]).pageCount == 6)
        } catch { check("fixture error: " + error.localizedDescription, false) }

        // Signatures and stamps are drawn at the page's resolution: a scan's own dpi, or the maximum on a vector page.
        if let scan = scanFixture(dpi: 300), let page = PDFDocument(data: scan)?.page(at: 0), let dpi = ScanResolution.dpi(of: page) {
            check("A4 scan page reports its 300 dpi for signatures and stamps", abs(dpi - 300) < 3)
        } else { check("A4 scan page reports its 300 dpi for signatures and stamps", false) }
        if let scan = scanFixture(dpi: 200, landscape: true), let page = PDFDocument(data: scan)?.page(at: 0) {
            check("landscape 200-dpi scan page reports 200 dpi", ScanResolution.dpi(of: page).map { abs($0 - 200) < 3 } ?? false)
        }
        if let page = PDFDocument(data: fixture(["LINES"]))?.page(at: 0) {
            check("text page is vector (no scan resolution: maximum)", ScanResolution.dpi(of: page) == nil)
        }
        if let template = SealTemplate.all.first {
            let seal = GeneratedMark.seal(SealSpec(template: template, values: [:], ink: template.ink.stampPad, designSeed: 7))
            for dpi in [300, 1200] as [CGFloat] {
                let image = MarkInsertDialog.image(.mark(seal), seed: 99, widthPoints: 120, dpi: dpi)
                let target = 120 / 72 * dpi
                check("seal drawn at \(Int(dpi)) dpi is \(Int(target)) px wide (±5%)", image.map { abs(CGFloat($0.width) - target) / target < 0.05 } ?? false)
            }
        }
        let signature = GeneratedMark.signature("Test Name", ink: InkColor.blue.pen, design: 5, pen: .ballpoint)
        let low = MarkInsertDialog.image(.mark(signature), seed: 3, widthPoints: 170, dpi: 300)
        let high = MarkInsertDialog.image(.mark(signature), seed: 3, widthPoints: 170, dpi: 1200)
        check("signature at 1200 dpi is 2833 px wide (±5%)", high.map { abs(CGFloat($0.width) - 2833) / 2833 < 0.05 } ?? false)
        if let low, let high {
            let a = CGFloat(low.width) / CGFloat(low.height), b = CGFloat(high.width) / CGFloat(high.height)
            check("one impression keeps its shape at every resolution", abs(a - b) / a < 0.04)
        } else { check("one impression keeps its shape at every resolution", false) }
        print("PDF file/output regressions: \(count - failures)/\(count) passed")
        return failures
    }

    static func runUI(owner: EditorWindowController) -> Int {
        guard let doc = owner.pdfDoc, let window = owner.window else { print("FAIL PDF lifecycle: document window missing"); return 1 }
        owner.prepareForSave()
        let originalPDF = doc.pdf, originalExtras = doc.extras, originalUndo = doc.undoManager
        let originallyEdited = doc.isDocumentEdited
        let manager = UndoManager(); manager.groupsByEvent = false; doc.undoManager = manager
        var count = 0, failures = 0
        func check(_ name: String, _ condition: Bool) {
            count += 1; if !condition { failures += 1 }
            print(condition ? "PASS" : "FAIL", "PDF lifecycle:", name)
            fflush(stdout)
        }
        func event(_ body: () throws -> Void) rethrows { manager.beginUndoGrouping(); defer { manager.endUndoGrouping() }; try body() }
        defer {
            owner.pdfView.cancelTextEdit(); doc.pdf = originalPDF; doc.extras = originalExtras; doc.undoManager = originalUndo
            owner.replaceDocument(originalPDF)
            doc.updateChangeCount(.changeCleared)
            if originallyEdited { doc.updateChangeCount(.changeDone) }
        }
        do {
            try doc.read(from: fixture(), ofType: "com.adobe.pdf")
            window.makeKeyAndOrderFront(nil); window.layoutIfNeeded(); owner.pdfView.layoutDocumentView()
            func begin(_ replacement: String?) -> Bool {
                guard let page = doc.pdf.page(at: 0), let hit = doc.pdf.findString("SECRET-ONE", withOptions: []).first else { return false }
                owner.editPageText(hit, on: page)
                guard let inline = owner.pdfView.inlineEditor else { return false }
                if let replacement {
                    // InlineTextEditor has already installed its field editor. Focusing the NSTextField
                    // again would end that edit through AppKit before the native insertText event.
                    if let editor = inline.field.currentEditor() as? NSTextView {
                        editor.insertText(replacement, replacementRange: NSRange(location: 0, length: (editor.string as NSString).length))
                    } else {
                        print("PDF lifecycle inline responder:", String(describing: window.firstResponder), "field editor:", String(describing: inline.field.currentEditor()))
                        return false
                    }
                }
                return true
            }
            check("opening unchanged in-place editor stays clean", begin(nil) && !doc.isDocumentEdited)
            owner.pdfView.cancelTextEdit()
            check("cancel unchanged edit creates no Undo", !manager.canUndo)
            check("native in-place typing marks document dirty", begin("VISIBLE-DRAFT") && doc.isDocumentEdited)
            var saved = Data()
            try event { saved = try doc.data(ofType: "com.adobe.pdf") }
            check("Save serializes visible in-place draft", PDFDocument(data: saved)?.string?.contains("VISIBLE-DRAFT") == true && owner.pdfView.inlineEditor == nil)
            manager.undo()
            check("Undo after Save restores editable original content", doc.pdf.page(at: 0)?.annotations.contains { $0 is EditedTextOverlay } == false)
            manager.redo()
            check("Redo restores committed text overlay", doc.pdf.page(at: 0)?.annotations.compactMap { $0 as? EditedTextOverlay }.first?.text == "VISIBLE-DRAFT")
            try doc.read(from: fixture(), ofType: "com.adobe.pdf")
            check("native Revert draft can be edited", begin("DISCARD-ME"))
            let before = doc.pdf
            do { try doc.read(from: Data("broken".utf8), ofType: "com.adobe.pdf"); check("bad Revert throws", false) }
            catch { check("failed Revert preserves model and visible draft", doc.pdf === before && owner.pdfView.inlineEditor?.currentText == "DISCARD-ME" && doc.isDocumentEdited) }
            try doc.read(from: fixture(["RESTORED"]), ofType: "com.adobe.pdf")
            check("successful Revert replaces model and visible document together", owner.pdf === doc.pdf && doc.pdf.string?.contains("RESTORED") == true && owner.pdfView.inlineEditor == nil)
            check("successful Revert clears obsolete Undo and dirty state", !manager.canUndo && !doc.isDocumentEdited)

            let previous = doc.pdf
            let replacement = PDFDocument(data: fixture(["COMPRESSED-RESULT"]))!
            event { owner.applyCompressedDocument(replacement, extras: doc.extras) }
            check("compression changes the model used by Save", doc.pdf === replacement && owner.pdf === replacement)
            saved = try doc.data(ofType: "com.adobe.pdf")
            check("Save after compression writes replacement pages", PDFDocument(data: saved)?.string?.contains("COMPRESSED-RESULT") == true)
            manager.undo()
            check("compression Undo restores model and view", doc.pdf === previous && owner.pdf === previous)
            manager.redo()
            check("compression Redo restores replacement model and view", doc.pdf === replacement && owner.pdf === replacement)

            try doc.read(from: fixture(), ofType: "com.adobe.pdf")
            let page = doc.pdf.page(at: 0)!, hit = doc.pdf.findString("SECRET-ONE", withOptions: []).first!
            let covered = EditedTextOverlay.make(replacing: hit, on: page, with: "COVERED-EDIT")
            event {
                owner.add(covered, to: page, name: "Audit Edited Text")
                owner.add(RedactOverlay(bounds: covered.bounds.insetBy(dx: -2, dy: -2), color: .black), to: page, name: "Audit Redaction")
            }
            event { check("Apply Redactions succeeds", owner.applyPendingRedactions()) }
            check("applied redaction changes saved model as well as view", owner.pdf === doc.pdf && doc.pdf.string?.contains("SECRET-ONE") == false && owner.pendingRedactions.isEmpty)
            check("Apply Redactions cannot restore an overlapping edited-text overlay", doc.pdf.page(at: 0)?.annotations.contains { $0 is EditedTextOverlay } == false && doc.pdf.string?.contains("COVERED-EDIT") == false)
            manager.undo()
            check("redaction Undo restores source and editable mark", owner.pdf === doc.pdf && doc.pdf.string?.contains("SECRET-ONE") == true && owner.pendingRedactions.count == 1)
            manager.redo()
            check("redaction Redo removes source again", owner.pdf === doc.pdf && doc.pdf.string?.contains("SECRET-ONE") == false && owner.pendingRedactions.isEmpty)
            saved = try doc.data(ofType: "com.adobe.pdf")
            check("Save after Apply Redactions cannot bring the old or edited text back", PDFDocument(data: saved)?.string?.contains("SECRET-ONE") == false && PDFDocument(data: saved)?.string?.contains("COVERED-EDIT") == false)
        } catch { check("fixture error: " + error.localizedDescription, false) }
        print("PDF document lifecycle regressions: \(count - failures)/\(count) passed")
        fflush(stdout)
        return failures
    }
}
