import AppKit
import PDFKit
import Kit42

extension ScanWindowController {
    /// Part of `SCN42 --uitest`: a signature/stamp placed by native events and printed into the page pixels.
    @MainActor func auditMarks() async -> Bool {
        guard let window else { print("FAIL SCN marks: no window"); return false }
        var count = 0, failures = 0
        func check(_ value: Bool, _ name: String) {
            count += 1; if !value { failures += 1 }
            print(value ? "PASS" : "FAIL", "SCN marks:", name); fflush(stdout)
        }
        var options = ProcessOptions(); options.crop = false; options.deskew = false; options.autoRotate = false; options.enhancement = .none
        // A white page with a black "text line" across the middle.
        func page(channels: Int) -> ScanPage {
            var bitmap = Bitmap(width: 400, height: 300, channels: channels, fill: 255)
            for y in 140..<160 { for x in 20..<380 { for c in 0..<min(3, channels) { bitmap.data[(y * 400 + x) * channels + c] = 0 } } }
            let image = bitmap.cgImage()!
            var report = ProcessReport(); report.outputSize = CGSize(width: image.width, height: image.height); report.ink = 0.05
            return ScanPage(base: bitmap.cgImage()!, image: image, dpi: 100, options: options, report: report)
        }
        func bwPage() -> ScanPage {
            var bitmap = Bitmap(width: 400, height: 300, channels: 1, fill: 255)
            for y in 140..<160 { for x in 20..<380 { bitmap.data[y * 400 + x] = 0 } }
            let image = bitmap.oneBitImage(threshold: 128)!
            var report = ProcessReport(); report.outputSize = CGSize(width: 400, height: 300); report.ink = 0.05
            var o = options; o.colorMode = .bw
            return ScanPage(base: bitmap.cgImage()!, image: image, dpi: 100, options: o, report: report)
        }
        // A blue stamp-like mark on a transparent background.
        let mark: CGImage = {
            let ctx = CGContext(data: nil, width: 120, height: 60, bitsPerComponent: 8, bytesPerRow: 0, space: Bitmap.sRGB,
                                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
            ctx.setFillColor(CGColor(srgbRed: 0.1, green: 0.25, blue: 0.85, alpha: 1))
            ctx.fill(CGRect(x: 10, y: 10, width: 100, height: 40))
            return ctx.makeImage()!
        }()
        /// RGB at a point given as fractions of the image, origin top left.
        func rgb(_ image: CGImage, _ fx: CGFloat, _ fy: CGFloat) -> (Int, Int, Int) {
            let b = Bitmap(image, gray: false)
            let x = min(b.width - 1, Int(fx * CGFloat(b.width))), y = min(b.height - 1, Int(fy * CGFloat(b.height)))
            let i = (y * b.width + x) * 4
            return (Int(b.data[i]), Int(b.data[i + 1]), Int(b.data[i + 2]))
        }
        func isBlue(_ p: (Int, Int, Int)) -> Bool { p.2 > 150 && p.0 < 90 }
        func isWhite(_ p: (Int, Int, Int)) -> Bool { p.0 > 240 && p.1 > 240 && p.2 > 240 }
        func mouse(_ type: NSEvent.EventType, _ p: CGPoint, clicks: Int = 1) {
            let event = NSEvent.mouseEvent(with: type, location: preview.canvas.convert(p, to: nil), modifierFlags: [],
                timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber, context: nil, eventNumber: 1,
                clickCount: clicks, pressure: 1)!
            window.sendEvent(event)
        }
        func key(_ code: UInt16, _ characters: String) {
            window.sendEvent(NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
                windowNumber: window.windowNumber, context: nil, characters: characters, charactersIgnoringModifiers: characters,
                isARepeat: false, keyCode: code)!)
        }
        let initialPages = pages
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("SCN42-marks-" + UUID().uuidString)
        defer {
            cancelMarkPlacement(); ScanWindowController.stampPopover?.close()
            doc.setOrder(initialPages, name: "Fixture"); doc.undo.removeAllActions(); doc.markSaved(revision: doc.revision)
            refreshPreview(); try? FileManager.default.removeItem(at: folder)
        }
        func reset(_ list: [ScanPage], select index: Int = 0) {
            showingGlass = false; busyText = nil
            doc.setOrder(list, name: "Fixture"); doc.undo.removeAllActions(); doc.markSaved(revision: doc.revision)
            pageList.select([index], scroll: false)
            NSApp.activate(ignoringOtherApps: true); window.makeKeyAndOrderFront(nil)
            refreshPreview(); window.layoutIfNeeded()
        }

        let color = page(channels: 4), gray = page(channels: 1), bw = bwPage()
        reset([color, gray, bw])
        check(shownPage === color && validate(#selector(showStampsAction(_:))), "Signature or Stamp is enabled for the shown page")
        check(NSApp.sendAction(Cmd.stamps.action, to: self, from: Cmd.stamps.menuItem()) && ScanWindowController.stampPopover?.isShown == true,
              "native command opens the signature and stamp library")
        ScanWindowController.stampPopover?.close()
        showingGlass = true; refreshPreview()
        check(!validate(#selector(showStampsAction(_:))), "the scanner glass cannot be stamped")
        showingGlass = false; refreshPreview(); window.layoutIfNeeded()

        let original = color.snapshot
        beginMarkPlacement(mark)
        guard let view = markPlacement else { check(false, "placement starts"); print("SCN42 mark regressions: \(count - failures)/\(count) passed"); return false }
        check(window.firstResponder === view && preview.view.subviews.contains { $0.accessibilityIdentifier() == "SCN42.markPlacementBar" },
              "placement takes the keyboard and shows the Print into Page bar")
        check(!doc.isDirty && color.image === original.image, "placing a mark does not change the page yet")
        let aspectBefore = view.rect.height / view.rect.width
        let start = view.rect
        let canvasSize = preview.canvas.bounds.size
        let center = CGPoint(x: view.viewRect.midX, y: view.viewRect.midY)
        mouse(.leftMouseDown, center); mouse(.leftMouseDragged, CGPoint(x: center.x - 0.2 * canvasSize.width, y: center.y - 0.1 * canvasSize.height))
        mouse(.leftMouseUp, CGPoint(x: center.x - 0.2 * canvasSize.width, y: center.y - 0.1 * canvasSize.height))
        check(abs(view.rect.minX - (start.minX - 0.2)) < 0.01 && abs(view.rect.minY - (start.minY - 0.1)) < 0.01 && view.rect.size == start.size,
              "native drag moves the mark")
        let corner = CGPoint(x: view.viewRect.maxX, y: view.viewRect.maxY)
        let widthBefore = view.rect.width
        mouse(.leftMouseDown, corner); mouse(.leftMouseDragged, CGPoint(x: corner.x + 0.05 * canvasSize.width, y: corner.y))
        mouse(.leftMouseUp, CGPoint(x: corner.x + 0.05 * canvasSize.width, y: corner.y))
        check(abs(view.rect.width - (widthBefore + 0.05)) < 0.01 && abs(view.rect.height / view.rect.width - aspectBefore) < 0.01,
              "native corner drag resizes the mark and keeps its proportions")
        let placed = view.rect
        mouse(.leftMouseDown, CGPoint(x: 3, y: 3)); mouse(.leftMouseUp, CGPoint(x: 3, y: 3))
        check(markPlacement === view && view.rect == placed, "a click outside the mark leaves the placement alone")
        window.makeFirstResponder(view)
        key(53, "\u{1b}")
        check(markPlacement == nil && view.superview == nil && !doc.isDirty && color.image === original.image, "Esc cancels without touching the page")

        // Over the text line: the ink must multiply (text stays black under the stamp, paper turns blue).
        beginMarkPlacement(mark)
        markPlacement?.rect = CGRect(x: 0.3, y: 0.4, width: 0.3, height: 0.15)
        let rect = CGRect(x: 0.3, y: 0.4, width: 0.3, height: 0.15)
        key(36, "\r")
        let paperUnder = CGPoint(x: rect.midX, y: rect.minY + rect.height * 0.25), textUnder = CGPoint(x: rect.midX, y: 0.5)
        check(markPlacement == nil && doc.isDirty && doc.undo.canUndo, "Return prints the mark into the page as one undoable step")
        check(isBlue(rgb(color.image, paperUnder.x, paperUnder.y)) && rgb(color.image, textUnder.x, textUnder.y).0 < 30
              && isWhite(rgb(color.image, 0.05, 0.05)) && isWhite(rgb(color.image, rect.maxX + 0.05, paperUnder.y)),
              "ink is multiplied into the shown pixels: paper turns blue, text stays black, the rest is untouched")
        check(isBlue(rgb(color.base, paperUnder.x, paperUnder.y)), "the original behind clean-up also carries the mark")
        let printed = color.snapshot
        doc.modify([color], name: "Colour") { page in var o = page.options; o.colorMode = .gray; try? page.refinish(o) }
        check(rgb(color.image, paperUnder.x, paperUnder.y).0 < 200, "re-running clean-up keeps the printed mark")
        doc.undo.undo()
        check(color.image === printed.image, "Undo of the clean-up returns to the stamped page")
        doc.undo.undo()
        check(color.image === original.image && color.base === original.base, "Undo restores the exact unstamped pixels")
        doc.undo.redo()
        check(color.image === printed.image && isBlue(rgb(color.image, paperUnder.x, paperUnder.y)), "Redo prints it again")

        // Saved PDF: the mark is part of the page image, with no annotation layer.
        do {
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            var save = SaveOptions(); save.format = .pdf; save.ocr = false; save.compression = .balanced
            let outputs = try Exporter.write([color.exportPage], folder: folder, baseName: "stamped", options: save, replaceExisting: true)
            if let url = outputs.first, let pdf = PDFDocument(url: url), let pdfPage = pdf.page(at: 0) {
                let box = pdfPage.bounds(for: .mediaBox)
                let rendered = pdfPage.thumbnail(of: NSSize(width: 400, height: 400 * box.height / box.width), for: .mediaBox)
                let cg = rendered.cgImage(forProposedRect: nil, context: nil, hints: nil)
                check(pdfPage.annotations.isEmpty && cg.map { isBlue(rgb($0, paperUnder.x, paperUnder.y)) && isWhite(rgb($0, 0.05, 0.05)) } == true,
                      "the saved PDF has the mark inside the page image and no annotations")
            } else { check(false, "the stamped page saves as a PDF") }
        } catch { check(false, "saving the stamped page: \(error)") }

        // Gray and black-and-white pages keep their kind.
        reset([color, gray, bw], select: 1)
        beginMarkPlacement(mark); markPlacement?.rect = rect; applyMarkPlacementAction(nil)
        check(gray.image.colorSpace?.model == .monochrome && rgb(gray.image, paperUnder.x, paperUnder.y).0 < 180, "a gray page stays gray with the mark printed")
        reset([color, gray, bw], select: 2)
        beginMarkPlacement(mark); markPlacement?.rect = rect; applyMarkPlacementAction(nil)
        check(bw.image.bitsPerPixel == 1 && rgb(bw.image, paperUnder.x, paperUnder.y).0 < 50 && isWhite(rgb(bw.image, 0.05, 0.05)),
              "a black-and-white page stays 1-bit with the mark printed")

        // A placement belongs to its page.
        reset([color, gray, bw])
        beginMarkPlacement(mark)
        pageList.select([1], scroll: false); refreshPreview()
        check(markPlacement == nil && !doc.isDirty, "showing another page drops an unfinished placement")
        print("SCN42 mark regressions: \(count - failures)/\(count) passed")
        return failures == 0
    }
}
