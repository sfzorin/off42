import AppKit
import CoreText
import Kit42
import PDFKit
import Vision

extension EditorWindowController {
    // MARK: - OCR

    struct OCRBatch {
        let document: PDFDoc
        let pdf: PDFDocument
        let revision: UInt64
        let generation: UUID
        let items: [(index: Int, original: PDFPage, snapshot: PDFPage)]
    }

    /// Capture independent pages before leaving the main thread.
    func captureOCR(indices: [Int]) -> OCRBatch? {
        guard let document = pdfDoc, let pdf, !pdf.isLocked else { return nil }
        let chosen = Array(Set(indices)).sorted()
        var items: [(Int, PDFPage, PDFPage)] = []
        for i in chosen {
            guard i >= 0, i < pdf.pageCount, let page = pdf.page(at: i), let snapshot = Self.ocrInput(page) else { return nil }
            items.append((i, page, snapshot))
        }
        guard !items.isEmpty else { return nil }
        ocrGeneration = UUID()
        return OCRBatch(document: document, pdf: pdf, revision: document.revision, generation: ocrGeneration, items: items)
    }

    func isCurrentOCR(_ batch: OCRBatch) -> Bool {
        pdfDoc === batch.document && pdf === batch.pdf && batch.document.revision == batch.revision &&
            ocrGeneration == batch.generation && !pdfView.hasPendingTextEdit &&
            batch.items.allSatisfy { batch.pdf.page(at: $0.index) === $0.original }
    }

    @discardableResult func applyOCR(_ results: [(Int, PDFPage)], batch: OCRBatch) -> Bool {
        guard isCurrentOCR(batch) else { return false }
        replacePages(results, name: L("Recognize Text"))
        return true
    }

    /// Recognises selected pages (or all) without touching the live pages in the background.
    @objc func recognizeTextAction(_ sender: Any?) {
        guard let pdf, window != nil else { return }
        prepareForSave()
        let selected = sidebar.thumbnails.selectedPages
        let indices = selected.count > 1 ? selected.map { pdf.index(for: $0) } : Array(0..<pdf.pageCount)
        let alert = NSAlert()
        alert.messageText = L("Recognize Text")
        alert.informativeText = L("Scanned pages get an invisible text layer: the text can then be searched, selected and copied. Pages: %d.", indices.count)
        alert.addButton(withTitle: L("Recognize")); alert.addButton(withTitle: L("Cancel"))
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        DispatchQueue.main.async { [weak self] in
            guard let self, let batch = self.captureOCR(indices: indices) else { return }
            self.banner.show(.info, L("Recognizing text…"))
            DispatchQueue.global(qos: .userInitiated).async {
                var results: [(Int, PDFPage)] = []
                for (n, item) in batch.items.enumerated() {
                    if let page = Self.ocrPage(item.snapshot) { results.append((item.index, page)) }
                    DispatchQueue.main.async {
                        if self.isCurrentOCR(batch) { self.banner.show(.info, L("Recognizing text… %d of %d", n + 1, batch.items.count)) }
                    }
                }
                DispatchQueue.main.async {
                    guard self.applyOCR(results, batch: batch) else {
                        if self.ocrGeneration == batch.generation {
                            self.banner.show(.warning, L("The document changed during text recognition. Run OCR again."))
                        }
                        return
                    }
                    self.banner.show(.success, L("Text recognized on %d pages", results.count))
                    DispatchQueue.main.asyncAfter(deadline: .now() + 4) { [weak self] in
                        if self?.ocrGeneration == batch.generation { self?.updateSignatureBanner() }
                    }
                }
            }
        }
    }

    /// Independent content and typed annotation copies; no changes to the source page or its annotations.
    static func ocrInput(_ page: PDFPage) -> PDFPage? {
        guard PDFOutput.imageSize(page, dpi: 180, box: .mediaBox, rotated: false) != nil,
              let source = page.pageRef else { return nil }
        let data = NSMutableData(); var media = page.bounds(for: .mediaBox)
        guard let consumer = CGDataConsumer(data: data), let ctx = CGContext(consumer: consumer, mediaBox: &media, nil) else { return nil }
        ctx.beginPDFPage(nil); ctx.drawPDFPage(source); ctx.endPDFPage(); ctx.closePDF()
        guard let copy = PDFDocument(data: data as Data)?.page(at: 0) else { return nil }
        copy.setBounds(page.bounds(for: .cropBox), for: .cropBox); copy.rotation = page.rotation
        for annotation in page.annotations {
            if let a = annotation.duplicateForEditing(on: copy) { copy.addAnnotation(a) }
        }
        return copy
    }

    /// Adds an invisible text layer while preserving rotation, crop and editable annotations.
    static func ocrPage(_ page: PDFPage) -> PDFPage? {
        let box = page.bounds(for: .mediaBox)
        guard let size = PDFOutput.imageSize(page, dpi: 180, box: .mediaBox, rotated: false),
              let source = page.pageRef,
              let bitmap = CGContext(data: nil, width: Int(size.width), height: Int(size.height), bitsPerComponent: 8, bytesPerRow: 0,
                                     space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue) else { return nil }
        bitmap.setFillColor(CGColor(gray: 1, alpha: 1)); bitmap.fill(CGRect(origin: .zero, size: size))
        bitmap.scaleBy(x: size.width / box.width, y: size.height / box.height)
        bitmap.translateBy(x: -box.minX, y: -box.minY); bitmap.drawPDFPage(source)
        guard let image = bitmap.makeImage() else { return nil }
        let observations = recognizeText(in: image)
        guard !observations.isEmpty else { return nil }
        let data = NSMutableData(); var media = box
        guard let consumer = CGDataConsumer(data: data), let ctx = CGContext(consumer: consumer, mediaBox: &media, nil) else { return nil }
        ctx.beginPDFPage(nil); ctx.drawPDFPage(source); drawInvisibleText(observations, box: box, in: ctx)
        ctx.endPDFPage(); ctx.closePDF()
        guard let newPage = PDFDocument(data: data as Data)?.page(at: 0) else { return nil }
        newPage.rotation = page.rotation; newPage.setBounds(page.bounds(for: .cropBox), for: .cropBox)
        for a in page.annotations { if let copy = a.duplicateForEditing(on: newPage) { newPage.addAnnotation(copy) } }
        return newPage
    }

    /// Text lines found by Vision in a page image.
    static func recognizeText(in image: CGImage) -> [VNRecognizedTextObservation] {
        let request = VNRecognizeTextRequest()
        request.recognitionLevel = .accurate
        request.usesLanguageCorrection = true
        request.recognitionLanguages = ["ru-RU", "en-US", "tr-TR"]
        do { try VNImageRequestHandler(cgImage: image).perform([request]) } catch { return [] }
        return request.results ?? []
    }

    /// Invisible (searchable, selectable) text over the recognised lines; `box` is the page area the image covers.
    static func drawInvisibleText(_ observations: [VNRecognizedTextObservation], box: CGRect, in ctx: CGContext) {
        ctx.saveGState()
        ctx.setTextDrawingMode(.invisible)
        for o in observations {
            guard let text = o.topCandidates(1).first?.string, !text.isEmpty else { continue }
            let b = o.boundingBox
            let r = CGRect(x: box.minX + b.minX * box.width, y: box.minY + b.minY * box.height, width: b.width * box.width, height: b.height * box.height)
            let font = CTFontCreateWithName("Helvetica" as CFString, max(1, r.height * 0.8), nil)
            let line = CTLineCreateWithAttributedString(NSAttributedString(string: text, attributes: [.font: font]))
            let width = CGFloat(CTLineGetTypographicBounds(line, nil, nil, nil))
            ctx.saveGState()
            ctx.translateBy(x: r.minX, y: r.minY + r.height * 0.18)
            if width > 0 { ctx.scaleBy(x: r.width / width, y: 1) }
            ctx.textPosition = .zero
            CTLineDraw(line, ctx)
            ctx.restoreGState()
        }
        ctx.restoreGState()
    }

    /// Swaps pages with destinations and selection following the replacements (undoable).
    func replacePages(_ replacements: [(Int, PDFPage)], name: String) {
        swapPages(replacements, name: name)
    }

    // MARK: - Print selected pages

    @objc func printSelectedPagesAction(_ sender: Any?) {
        guard pdf != nil, let window else { return }
        prepareForSave()
        let chosen = targetIndices
        let part = PDFDocument()
        let source: PDFDocument
        do { source = try exportSnapshot() } catch { presentError(error); return }
        for i in chosen { if let p = source.page(at: i)?.copy() as? PDFPage { part.insert(p, at: part.pageCount) } }
        guard part.pageCount > 0 else { return }
        let op: NSPrintOperation
        do {
            guard let bytes = part.dataRepresentation() else { throw CocoaError(.fileWriteUnknown) }
            op = try ProductionPrint.operation(pdfData: bytes, printInfo: pdfDoc?.printInfo ?? .shared, autoFit: true)
        } catch { presentError(error); return }
        op.jobTitle = pdfDoc?.displayName ?? "PDF42"
        op.printPanel.options.formUnion([.showsPaperSize, .showsOrientation, .showsScaling, .showsPreview, .showsCopies])
        op.runModal(for: window, delegate: nil, didRun: nil, contextInfo: nil)
    }
}
