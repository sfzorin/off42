import AppKit
import Kit42
import PDFKit

/// All export paths use the same committed, redacted snapshot as Save.
extension EditorWindowController {
    func exportSnapshot() throws -> PDFDocument {
        guard let doc = pdfDoc, let snapshot = PDFDocument(data: try doc.data(ofType: "com.adobe.pdf")), !snapshot.isLocked else {
            throw NSError(domain: NSCocoaErrorDomain, code: NSFileWriteUnknownError)
        }
        return snapshot
    }

    func exportTextData(format: String) throws -> Data {
        try PDFOutput.textData(exportSnapshot(), format: format)
    }
}

enum PDFOutput {
    static func textData(_ pdf: PDFDocument, format: String) throws -> Data {
        let text = PDFDoc.attributedText(pdf)
        let range = NSRange(location: 0, length: text.length)
        switch format.lowercased() {
        case "docx": return DocxWriter.data(text)
        case "rtf": return try text.data(from: range, documentAttributes: [.documentType: NSAttributedString.DocumentType.rtf])
        case "html": return try text.data(from: range, documentAttributes: [.documentType: NSAttributedString.DocumentType.html,
                                                                           .characterEncoding: String.Encoding.utf8.rawValue])
        case "txt": return Data(text.string.utf8)
        default: throw NSError(domain: NSCocoaErrorDomain, code: NSFeatureUnsupportedError)
        }
    }

    static func imageSize(_ page: PDFPage, dpi: CGFloat, box: PDFDisplayBox = .cropBox, rotated: Bool = true) -> CGSize? {
        let bounds = page.bounds(for: box)
        var size = bounds.size
        if rotated && page.rotation % 180 != 0 { size = CGSize(width: size.height, height: size.width) }
        let width = (size.width * dpi / 72).rounded(), height = (size.height * dpi / 72).rounded()
        guard dpi.isFinite, dpi > 0, bounds.origin.x.isFinite, bounds.origin.y.isFinite,
              width.isFinite, height.isFinite, width >= 1, height >= 1,
              width <= 16384, height <= 16384, width * height <= 64_000_000 else { return nil }
        return CGSize(width: width, height: height)
    }

    static func imageData(_ page: PDFPage, dpi: CGFloat, jpeg: Bool) throws -> Data {
        guard let size = imageSize(page, dpi: dpi) else {
            throw NSError(domain: NSCocoaErrorDomain, code: NSFileWriteUnknownError,
                          userInfo: [NSLocalizedDescriptionKey: L("The image size is invalid")])
        }
        let image = page.thumbnail(of: size, for: .cropBox)
        var rect = CGRect(origin: .zero, size: image.size)
        guard let source = image.cgImage(forProposedRect: &rect, context: nil, hints: nil),
              let bitmap = CGContext(data: nil, width: Int(size.width), height: Int(size.height), bitsPerComponent: 8,
                                     bytesPerRow: 0, space: CGColorSpace(name: CGColorSpace.sRGB)!,
                                     bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else {
            throw NSError(domain: NSCocoaErrorDomain, code: NSFileWriteUnknownError)
        }
        bitmap.setFillColor(CGColor(gray: 1, alpha: 1)); bitmap.fill(CGRect(origin: .zero, size: size))
        bitmap.draw(source, in: CGRect(origin: .zero, size: size))
        guard let cg = bitmap.makeImage(),
              let data = NSBitmapImageRep(cgImage: cg).representation(using: jpeg ? .jpeg : .png,
                  properties: jpeg ? [.compressionFactor: 0.9] : [:]) else {
            throw NSError(domain: NSCocoaErrorDomain, code: NSFileWriteUnknownError)
        }
        return data
    }

    static func protectedData(_ pdf: PDFDocument, password: String) throws -> Data {
        guard !password.isEmpty,
              let data = pdf.dataRepresentation(options: [PDFDocumentWriteOption.userPasswordOption: password, PDFDocumentWriteOption.ownerPasswordOption: password]) else {
            throw NSError(domain: NSCocoaErrorDomain, code: NSFileWriteUnknownError)
        }
        return data
    }
}
