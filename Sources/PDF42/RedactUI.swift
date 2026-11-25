import AppKit
import Kit42
import PDFKit

/// Last resort for pages whose content cannot be rewritten safely (unsupported fonts or images):
/// the page is rendered at 300 dpi with the areas filled, and becomes that picture plus an
/// invisible OCR text layer. Crop box, rotation and annotations are kept.
enum RedactionRaster {
    static let dpi: CGFloat = 300

    static func page(_ page: PDFPage, areas: [RedactionArea], overlay: (CGContext) -> Void) -> PDFPage? {
        let media = page.bounds(for: .mediaBox), crop = page.bounds(for: .cropBox)
        let rotation = page.rotation
        let annotations = page.annotations
        let scale = dpi / 72
        guard let size = bitmapSize(media.size) else { return nil }
        let w = size.width, h = size.height
        annotations.forEach { page.removeAnnotation($0) }
        page.rotation = 0
        defer {
            page.rotation = rotation
            annotations.forEach { page.addAnnotation($0) }
        }
        guard
              let bitmap = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0, space: CGColorSpaceCreateDeviceRGB(),
                                     bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue) else {
            return nil
        }
        bitmap.setFillColor(CGColor(gray: 1, alpha: 1))
        bitmap.fill(CGRect(x: 0, y: 0, width: w, height: h))
        bitmap.interpolationQuality = .high
        bitmap.scaleBy(x: scale, y: scale)
        bitmap.translateBy(x: -media.minX, y: -media.minY)
        bitmap.saveGState()
        page.draw(with: .mediaBox, to: bitmap)
        bitmap.restoreGState()
        for a in areas {
            bitmap.setFillColor(red: a.fill.r, green: a.fill.g, blue: a.fill.b, alpha: 1)
            bitmap.fill(a.rect)
        }
        overlay(bitmap)
        guard let image = bitmap.makeImage() else {
            return nil
        }
        // JPEG keeps a 300 dpi page at a sensible size; Quartz embeds it as is.
        let jpeg = NSMutableData()
        var picture = image
        if let dest = CGImageDestinationCreateWithData(jpeg, "public.jpeg" as CFString, 1, nil) {
            CGImageDestinationAddImage(dest, image, [kCGImageDestinationLossyCompressionQuality: 0.85] as CFDictionary)
            if CGImageDestinationFinalize(dest), let provider = CGDataProvider(data: jpeg),
               let j = CGImage(jpegDataProviderSource: provider, decode: nil, shouldInterpolate: true, intent: .defaultIntent) { picture = j }
        }
        let observations = EditorWindowController.recognizeText(in: image)
        let data = NSMutableData()
        var box = media
        guard let consumer = CGDataConsumer(data: data), let ctx = CGContext(consumer: consumer, mediaBox: &box, nil) else {
            return nil
        }
        ctx.beginPDFPage(nil)
        ctx.draw(picture, in: media)
        EditorWindowController.drawInvisibleText(observations, box: media, in: ctx)
        ctx.endPDFPage()
        ctx.closePDF()
        guard let newPage = PDFDocument(data: data as Data)?.page(at: 0) else {
            return nil
        }
        newPage.setBounds(crop, for: .cropBox)
        newPage.rotation = rotation
        for annotation in annotations {
            guard let copy = annotation.copy() as? PDFAnnotation else { return nil }
            newPage.addAnnotation(copy)
        }
        return newPage
    }

    static func bitmapSize(_ size: CGSize) -> (width: Int, height: Int)? {
        let width = (size.width * dpi / 72).rounded(), height = (size.height * dpi / 72).rounded()
        guard width.isFinite, height.isFinite, width >= 1, height >= 1,
              width <= 32_768, height <= 32_768, width * height <= 64_000_000 else { return nil }
        return (Int(width), Int(height))
    }

}

extension EditorWindowController {
    /// Redaction marks waiting to be applied.
    var pendingRedactions: [(RedactOverlay, PDFPage)] {
        guard let pdf else { return [] }
        return (0..<pdf.pageCount).flatMap { i -> [(RedactOverlay, PDFPage)] in
            guard let page = pdf.page(at: i) else { return [] }
            return page.annotations.compactMap { $0 as? RedactOverlay }.map { ($0, page) }
        }
    }

    /// Markup › Apply Redactions: removes what is under every redaction mark now (after a warning).
    @objc func applyRedactionsAction(_ sender: Any?) {
        guard let pdf, let doc = pdfDoc, let window else { return }
        prepareForSave()
        let revision = doc.revision
        let marks = pendingRedactions
        guard !marks.isEmpty else {
            let alert = NSAlert()
            alert.messageText = L("Nothing to redact")
            alert.informativeText = L("Mark areas with the Redact tool first.")
            alert.beginSheetModal(for: window)
            return
        }
        let alert = NSAlert()
        alert.alertStyle = .warning
        alert.messageText = L("Apply %d redactions?", marks.count)
        alert.informativeText = L("Text, images and annotations under the marked areas will be removed from the document. Once the document is saved this cannot be undone: the removed content is not in the file anymore.")
        alert.addButton(withTitle: L("Apply Redactions"))
        alert.addButton(withTitle: L("Cancel"))
        alert.beginSheetModal(for: window) { [self] response in
            guard response == .alertFirstButtonReturn else { return }
            guard pdfDoc === doc, self.pdf === pdf, doc.revision == revision, !pdfView.hasPendingTextEdit else { return }
            _ = applyPendingRedactions()

        }
    }

    /// Apply the confirmed operation to the model used by Save, with a complete document Undo step.
    @discardableResult
    func applyPendingRedactions() -> Bool {
        guard let pdf, let doc = pdfDoc else { return false }
        let marks = pendingRedactions
        guard !marks.isEmpty else { return false }
        func intersectsMark(_ annotation: PDFAnnotation) -> Bool {
            marks.contains { mark, page in annotation.page === page && annotation.bounds.intersects(mark.bounds) }
        }
        let others: [(PDFAnnotation, Int)] = (0..<pdf.pageCount).flatMap { index in
            (pdf.page(at: index)?.annotations ?? [])
                .filter { (($0 is PageOverlay && !($0 is RedactOverlay)) || $0 is TemporaryHighlight) && !intersectsMark($0) }
                .map { ($0, index) }
        }
        // Overlapping editable text/headers must be rendered before redaction, otherwise restoring the
        // overlay afterwards would bring the hidden text back. Non-overlapping overlays stay editable.
        guard let result = PageOverlay.bake(pdf, only: { $0 is RedactOverlay || intersectsMark($0) }) else {
            banner.show(.error, L("The redactions could not be applied."))
            return false
        }
        let persisted = DocumentWriter.apply(AcroFormFixer.ensureFields(result.data), extras: doc.extras, pdf: pdf)
        guard let redacted = PDFDocument(data: persisted) else {
            banner.show(.error, L("The redactions could not be applied."))
            return false
        }
        // These mutable overlays stay with their source pages for Undo. PDFKit copies custom overlays poorly;
        // move them when swapping documents and move them back with the inverse operation.
        swapRedactedDocument(redacted, overlays: others, extras: DocumentExtras.read(persisted, pdf: redacted))
        showRedactionResult(result.report, applied: marks.count)
        return true
    }

    private func swapRedactedDocument(_ replacement: PDFDocument, overlays: [(PDFAnnotation, Int)], extras: DocumentExtras) {
        guard let doc = pdfDoc else { return }
        let old = doc.pdf, oldExtras = doc.extras
        for (overlay, index) in overlays {
            overlay.page?.removeAnnotation(overlay)
            replacement.page(at: index)?.addAnnotation(overlay)
        }
        doc.pdf = replacement
        doc.extras = extras
        replaceDocument(replacement)
        undo?.registerUndo(withTarget: self) { $0.swapRedactedDocument(old, overlays: overlays, extras: oldExtras) }
        undo?.setActionName(L("Apply Redactions"))
    }

    func showRedactionResult(_ report: RedactionReport, applied: Int? = nil) {
        let glyphs = report.pages.reduce(0) { $0 + $1.glyphs }
        let images = report.pages.reduce(0) { $0 + $1.images }
        let raster = report.rasterizedPages
        var text = applied.map { L("Redactions applied: %d.", $0) } ?? L("Redacted content removed from the file.")
        text += " " + L("Characters removed: %d, images cleared: %d.", glyphs, images)
        if !raster.isEmpty {
            text += " " + L("Pages %@ could not be edited safely and were turned into images with recognized text.",
                            raster.map { "\($0 + 1)" }.joined(separator: ", "))
        }
        banner.show(raster.isEmpty ? .success : .warning, text)
    }
}
