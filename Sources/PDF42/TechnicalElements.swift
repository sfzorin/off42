import AppKit
import Kit42
import PDFKit

/// A vector appearance and a portable recipe. PDFKit reopens stamps as PDFAnnotation;
/// the custom PDF key lets that ordinary annotation return to the same editor.
final class TechnicalElementAnnotation: PDFAnnotation {
    static let recipeKey = PDFAnnotationKey(rawValue: "/Suite42TechnicalElement")
    let element: TechnicalElement
    let vectorData: Data
    private let vectorDocument: CGPDFDocument

    init(element: TechnicalElement, pdfData: Data, bounds: CGRect) throws {
        guard let provider = CGDataProvider(data: pdfData as CFData), let vector = CGPDFDocument(provider), vector.page(at: 1) != nil else {
            throw TechnicalElementError(L("Could not render the symbol or code."))
        }
        self.element = element; vectorData = pdfData; vectorDocument = vector
        super.init(bounds: bounds, forType: .stamp, withProperties: nil)
        setValue(element.encodedDescription, forAnnotationKey: Self.recipeKey)
        contents = element.symbolID.map { id in TechnicalSymbols.all.first { $0.id == id }?.name ?? id } ?? element.value
        userName = NSFullUserName(); modificationDate = Date()
    }

    required init?(coder: NSCoder) { nil }

    override func draw(with box: PDFDisplayBox, in context: CGContext) {
        guard let page = vectorDocument.page(at: 1) else { return }
        context.saveGState(); defer { context.restoreGState() }
        context.setBlendMode(.normal)
        context.concatenate(page.getDrawingTransform(.mediaBox, rect: bounds, rotate: 0, preserveAspectRatio: false))
        context.drawPDFPage(page)
    }
}

extension PDFAnnotation {
    var technicalElement: TechnicalElement? {
        guard let description = value(forAnnotationKey: TechnicalElementAnnotation.recipeKey) as? String,
              var element = TechnicalElement.fromDescription(description) else { return nil }
        // An ordinary resize in the PDF editor remains the starting physical width on re-edit.
        element.widthMM = bounds.width * 25.4 / 72
        return element
    }
}

extension EditorWindowController {
    @objc func insertTechnicalElementAction(_ sender: Any?) {
        guard let window, let page = pdfView.currentPage, pdf?.isLocked == false else { return }
        TechnicalElementDialog.present(in: window, initialValue: pdfView.currentSelection?.string ?? "") { [weak self, weak page] element, render in
            guard let self, let page, page.document === self.pdf else { return }
            do { try self.insertTechnicalElement(element, render: render, on: page) }
            catch { self.presentError(error) }
        }
    }

    func insertTechnicalElement(_ element: TechnicalElement, render: TechnicalRender, on page: PDFPage) throws {
        let box = page.bounds(for: .cropBox)
        let bounds = CGRect(x: box.midX - render.sizePoints.width / 2, y: box.midY - render.sizePoints.height / 2,
                            width: render.sizePoints.width, height: render.sizePoints.height)
        guard box.contains(bounds) else { throw TechnicalElementError(L("The symbol or code is larger than this page. Choose a smaller width.")) }
        let annotation = try TechnicalElementAnnotation(element: element, pdfData: render.pdfData, bounds: bounds)
        add(annotation, to: page, name: L("Insert Symbol or Code"))
        setTool(.select); pdfView.selectAnnotation(annotation, on: page)
    }

    @objc func editTechnicalElementAction(_ sender: Any?) {
        guard let annotation = pdfView.selectedAnnotation, let page = pdfView.selectedPage else { return }
        editTechnicalElement(annotation, on: page)
    }

    func editTechnicalElement(_ annotation: PDFAnnotation, on page: PDFPage) {
        guard let window, let initial = annotation.technicalElement else { return }
        TechnicalElementDialog.present(in: window, initial: initial) { [weak self, weak page, weak annotation] element, render in
            guard let self, let page, let annotation, page.document === self.pdf,
                  page.annotations.contains(where: { $0 === annotation }) else { return }
            do {
                let bounds = CGRect(origin: annotation.bounds.origin, size: render.sizePoints)
                guard page.bounds(for: .cropBox).contains(bounds) else {
                    throw TechnicalElementError(L("The symbol or code does not fit at this position. Choose a smaller width or move it first."))
                }
                let replacement = try TechnicalElementAnnotation(element: element, pdfData: render.pdfData, bounds: bounds)
                replacement.userName = annotation.userName
                replacement.shouldDisplay = annotation.shouldDisplay; replacement.shouldPrint = annotation.shouldPrint
                self.replaceTechnicalElement(annotation, with: replacement, on: page)
            } catch { self.presentError(error) }
        }
    }

    /// Replacing in place retains the drawing order; one inverse replacement is one Undo entry.
    private func replaceTechnicalElement(_ old: PDFAnnotation, with new: PDFAnnotation, on page: PDFPage) {
        pdfView.selectAnnotation(nil)
        var annotations = page.annotations
        guard let index = annotations.firstIndex(where: { $0 === old }) else { return }
        annotations[index] = new
        page.annotations.forEach { page.removeAnnotation($0) }
        annotations.forEach { page.addAnnotation($0) }
        pdfView.annotationsChanged(on: page); sidebar.thumbnails.refresh(page)
        setTool(.select); pdfView.selectAnnotation(new, on: page)
        undo?.registerUndo(withTarget: self) { $0.replaceTechnicalElement(new, with: old, on: page) }
        undo?.setActionName(L("Edit Symbol or Code"))
    }

    @objc func createLabelsAction(_ sender: Any?) {
        guard let window, pdfView.currentPage != nil else { return }
        let alert = NSAlert()
        alert.messageText = L("Create Labels")
        alert.informativeText = L("Use the current page or select a region to repeat on a sheet of labels.")
        alert.addButton(withTitle: L("Current Page")); alert.addButton(withTitle: L("Select Region")); alert.addButton(withTitle: L("Cancel"))
        alert.beginSheetModal(for: window) { [weak self] response in
            guard let self else { return }
            if response == .alertFirstButtonReturn, let page = self.pdfView.currentPage {
                // NSAlert calls its completion before the sheet has fully detached. Let it close
                // before presenting the label editor, which deliberately refuses nested sheets.
                DispatchQueue.main.async { [weak self, weak page] in
                    guard let self, let page else { return }
                    self.presentPageLabels(from: page)
                }
            }
            else if response == .alertSecondButtonReturn { self.selectLabelRegionAction(nil) }
        }
    }

    @objc func selectLabelRegionAction(_ sender: Any?) {
        guard pdfView.currentPage != nil else { return }
        setTool(.crop)
        pdfView.labelRegionSelection = true
        banner.show(.info, L("Drag a region for labels. Press Escape to cancel."))
    }

    func finishLabelRegion(_ rect: CGRect, on page: PDFPage) {
        setTool(.select); updateSignatureBanner()
        presentPageLabels(from: page, region: rect)
    }

    private func presentPageLabels(from page: PDFPage, region: CGRect? = nil) {
        guard let window else { return }
        do { LabelSheetDialog.present(in: window, sources: [try makePageLabelSource(page, region: region)]) }
        catch { presentError(error) }
    }

    /// Flatten the saved, redacted snapshot into an independent vector page. Its coordinate system
    /// is the visible rotated region; neither the source crop box nor its annotations are changed.
    func makePageLabelSource(_ source: PDFPage, region: CGRect? = nil) throws -> LabelSource {
        guard let pdf, source.document === pdf else { throw TechnicalElementError(L("The source page is no longer available.")) }
        let index = pdf.index(for: source)
        let snapshot = try exportSnapshot()
        guard let page = snapshot.page(at: index) else { throw TechnicalElementError(L("The source page is no longer available.")) }
        let crop = page.bounds(for: .cropBox), area = region.map { $0.standardized.intersection(crop) } ?? crop
        guard !area.isNull, area.width.isFinite, area.height.isFinite, area.width > 1, area.height > 1 else {
            throw TechnicalElementError(L("Select a region inside the page."))
        }
        let (transform, size) = PageOverlay.displayTransform(box: area, rotation: page.rotation)
        page.rotation = 0
        let data = NSMutableData(); var output = CGRect(origin: .zero, size: size)
        guard let consumer = CGDataConsumer(data: data), let context = CGContext(consumer: consumer, mediaBox: &output, nil) else {
            throw TechnicalElementError(L("Could not create the label source."))
        }
        context.beginPDFPage(nil); context.clip(to: output)
        context.concatenate(transform.inverted())
        page.draw(with: .mediaBox, to: context)
        context.endPDFPage(); context.closePDF()
        return LabelSource(title: region == nil ? L("Page %d", index + 1) : L("Page %d — Region", index + 1), pdfData: data as Data)
    }
}
