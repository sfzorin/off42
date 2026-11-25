import AppKit
import PDFKit

extension PDFAnnotation {
    /// PDFKit's generic NSCopying path does not retain custom drawing subclasses. Page transformations and OCR
    /// need independent annotations so Undo can keep the original page, including pending edits and redactions.
    func duplicateForEditing(on page: PDFPage) -> PDFAnnotation? {
        let duplicate: PDFAnnotation
        switch self {
        case let original as TechnicalElementAnnotation:
            guard let copy = try? TechnicalElementAnnotation(element: original.element, pdfData: original.vectorData, bounds: original.bounds) else { return nil }
            duplicate = copy
        case let original as ImageStampAnnotation:
            let stamp = ImageStampAnnotation(image: original.image, bounds: original.bounds)
            stamp.blendMode = original.blendMode
            duplicate = stamp
        case let original as EditedTextOverlay:
            duplicate = EditedTextOverlay(cover: original.originalCover, text: original.text, originalText: original.originalText,
                                          font: original.textFont, color: original.color, baseline: original.baseline)
        case let original as WatermarkOverlay: duplicate = WatermarkOverlay(spec: original.spec, page: page)
        case let original as PageNumberOverlay: duplicate = PageNumberOverlay(text: original.text, spec: original.spec, page: page)
        case let original as HeaderFooterOverlay: duplicate = HeaderFooterOverlay(spec: original.spec, lines: original.lines, page: page)
        case let original as RedactOverlay: duplicate = RedactOverlay(bounds: original.bounds, color: original.color)
        case let original as CropRemovalOverlay: duplicate = CropRemovalOverlay(bounds: original.bounds)
        case is TemporaryHighlight:
            let highlight = TemporaryHighlight(bounds: bounds, forType: .highlight, withProperties: nil)
            highlight.quadrilateralPoints = quadrilateralPoints
            duplicate = highlight
        case is LinkBorderMarker: return nil
        case is PageOverlay: return nil
        default:
            guard let copy = copy() as? PDFAnnotation else { return nil }
            duplicate = copy
        }
        duplicate.bounds = bounds
        duplicate.color = color
        duplicate.contents = contents
        duplicate.userName = userName
        duplicate.modificationDate = modificationDate
        duplicate.shouldDisplay = shouldDisplay
        duplicate.shouldPrint = shouldPrint
        if let border { duplicate.border = border.copy() as? PDFBorder }
        // PDFKit's NSCopying path recreates fonts by PostScript name. Private system UI font names are not
        // loadable that way and silently become Times; retain the original font object and text colour.
        if let font { duplicate.font = font }
        if let fontColor { duplicate.fontColor = fontColor }
        return duplicate
    }
}
