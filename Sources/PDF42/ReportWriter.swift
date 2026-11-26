import AppKit
import CoreImage

/// Flowing text pages drawn with Core Text into a PDF context (real, searchable text with embedded fonts).
/// Used for the signature verification report and the certificate of completion.
final class ReportWriter {
    let pageSize: CGSize
    let margin: CGFloat = 50
    private let data = NSMutableData()
    private let ctx: CGContext
    private(set) var pageCount = 0
    /// Distance from the top of the page (flipped coordinates).
    private(set) var y: CGFloat = 0
    var footer: String?

    static let ink = NSColor(srgbRed: 0.12, green: 0.14, blue: 0.18, alpha: 1)
    static let muted = NSColor(srgbRed: 0.42, green: 0.45, blue: 0.5, alpha: 1)
    static let accent = NSColor(srgbRed: 0.1, green: 0.3, blue: 0.62, alpha: 1)
    static let good = NSColor(srgbRed: 0.1, green: 0.5, blue: 0.24, alpha: 1)
    static let warn = NSColor(srgbRed: 0.75, green: 0.45, blue: 0.0, alpha: 1)
    static let bad = NSColor(srgbRed: 0.75, green: 0.12, blue: 0.12, alpha: 1)

    init(pageSize: CGSize = CGSize(width: 595, height: 842), title: String) {
        self.pageSize = pageSize
        var box = CGRect(origin: .zero, size: pageSize)
        let info: [CFString: Any] = [kCGPDFContextTitle: title, kCGPDFContextCreator: "PDF42"]
        ctx = CGContext(consumer: CGDataConsumer(data: data as CFMutableData)!, mediaBox: &box, info as CFDictionary)!
    }

    var width: CGFloat { pageSize.width - 2 * margin }
    private var bottom: CGFloat { pageSize.height - margin - 18 }

    private func beginPage() {
        ctx.beginPDFPage(nil)
        pageCount += 1
        y = margin
        NSGraphicsContext.saveGraphicsState()
        // Flipped: y grows downwards, as text is laid out.
        ctx.translateBy(x: 0, y: pageSize.height)
        ctx.scaleBy(x: 1, y: -1)
        NSGraphicsContext.current = NSGraphicsContext(cgContext: ctx, flipped: true)
        if let footer {
            let text = NSAttributedString(string: "\(footer) · \(pageCount)", attributes: [.font: NSFont.systemFont(ofSize: 7.5), .foregroundColor: Self.muted])
            text.draw(with: CGRect(x: margin, y: pageSize.height - margin + 6, width: width, height: 12), options: [.usesLineFragmentOrigin])
        }
    }

    private func endPage() {
        NSGraphicsContext.restoreGraphicsState()
        ctx.endPDFPage()
    }

    /// Starts the first page or a new one.
    func newPage() {
        if pageCount > 0 { endPage() }
        beginPage()
    }

    func ensureSpace(_ height: CGFloat) {
        if pageCount == 0 || y + height > bottom { newPage() }
    }

    static func attributes(size: CGFloat = 10, weight: NSFont.Weight = .regular, color: NSColor = ink, mono: Bool = false) -> [NSAttributedString.Key: Any] {
        let font = mono ? NSFont.monospacedSystemFont(ofSize: size, weight: weight) : NSFont.systemFont(ofSize: size, weight: weight)
        let p = NSMutableParagraphStyle()
        p.lineBreakMode = .byWordWrapping
        p.paragraphSpacing = 1
        return [.font: font, .foregroundColor: color, .paragraphStyle: p]
    }

    /// Height of text laid out in `width`.
    func height(_ text: NSAttributedString, width: CGFloat) -> CGFloat {
        ceil(text.boundingRect(with: CGSize(width: width, height: 10_000), options: [.usesLineFragmentOrigin]).height)
    }

    /// Draws a paragraph, breaking to a new page when it does not fit.
    func text(_ string: String, size: CGFloat = 10, weight: NSFont.Weight = .regular, color: NSColor = ink, mono: Bool = false,
              indent: CGFloat = 0, after: CGFloat = 4) {
        attributed(NSAttributedString(string: string, attributes: Self.attributes(size: size, weight: weight, color: color, mono: mono)),
                   indent: indent, after: after)
    }

    func attributed(_ text: NSAttributedString, indent: CGFloat = 0, after: CGFloat = 4) {
        let w = width - indent
        let h = height(text, width: w)
        ensureSpace(min(h, bottom - margin))
        text.draw(with: CGRect(x: margin + indent, y: y, width: w, height: h), options: [.usesLineFragmentOrigin])
        y += h + after
    }

    /// "Label   value" rows with a fixed label column.
    func field(_ label: String, _ value: String, color: NSColor = ink, mono: Bool = false, labelWidth: CGFloat = 130, indent: CGFloat = 0) {
        let l = NSAttributedString(string: label, attributes: Self.attributes(size: 9, weight: .medium, color: Self.muted))
        let v = NSAttributedString(string: value, attributes: Self.attributes(size: mono ? 8.5 : 9.5, color: color, mono: mono))
        let w = width - indent - labelWidth
        let h = max(height(l, width: labelWidth - 6), height(v, width: w))
        ensureSpace(h)
        l.draw(with: CGRect(x: margin + indent, y: y + (mono ? 0.5 : 0), width: labelWidth - 6, height: h), options: [.usesLineFragmentOrigin])
        v.draw(with: CGRect(x: margin + indent + labelWidth, y: y, width: w, height: h), options: [.usesLineFragmentOrigin])
        y += h + 3
    }

    func rule(color: NSColor = NSColor(white: 0.82, alpha: 1), after: CGFloat = 8) {
        ensureSpace(2)
        color.setFill()
        NSRect(x: margin, y: y, width: width, height: 0.6).fill()
        y += after
    }

    func space(_ h: CGFloat) { y += h }

    /// Colored status badge followed by text, on one line.
    func badge(_ label: String, color: NSColor, text: String) {
        let b = NSAttributedString(string: " \(label) ", attributes: Self.attributes(size: 9, weight: .bold, color: .white))
        let size = b.size()
        let t = NSAttributedString(string: text, attributes: Self.attributes(size: 10, weight: .medium, color: color))
        let h = max(size.height + 2, height(t, width: width - size.width - 10))
        ensureSpace(h)
        color.setFill()
        NSBezierPath(roundedRect: NSRect(x: margin, y: y, width: size.width + 2, height: size.height + 2), xRadius: 3, yRadius: 3).fill()
        b.draw(at: NSPoint(x: margin + 1, y: y + 1))
        t.draw(with: CGRect(x: margin + size.width + 10, y: y, width: width - size.width - 10, height: h), options: [.usesLineFragmentOrigin])
        y += h + 6
    }

    /// Draws an image at a point (top-left, flipped page coordinates).
    func image(_ image: CGImage, in rect: CGRect, interpolate: Bool = true) {
        ensureSpace(0)
        ctx.saveGState()
        ctx.interpolationQuality = interpolate ? .high : .none
        // Undo the flip locally so the image is upright.
        ctx.translateBy(x: rect.minX, y: rect.maxY)
        ctx.scaleBy(x: 1, y: -1)
        ctx.draw(image, in: CGRect(origin: .zero, size: rect.size))
        ctx.restoreGState()
    }

    func finish() -> Data {
        if pageCount == 0 { beginPage() }
        endPage()
        ctx.closePDF()
        return data as Data
    }

    /// QR code (error correction M) as a bitmap with one pixel per module plus the quiet zone.
    static func qrCode(_ payload: String, correction: String = "M") -> CGImage? {
        guard let filter = CIFilter(name: "CIQRCodeGenerator") else { return nil }
        filter.setValue(Data(payload.utf8), forKey: "inputMessage")
        filter.setValue(correction, forKey: "inputCorrectionLevel")
        guard let output = filter.outputImage else { return nil }
        let scaled = output.transformed(by: CGAffineTransform(scaleX: 8, y: 8))
        return CIContext(options: [.useSoftwareRenderer: true]).createCGImage(scaled, from: scaled.extent)
    }

    /// Reads QR codes in an image (used to verify a certificate of completion offline).
    static func readQRCodes(_ image: CGImage) -> [String] {
        let detector = CIDetector(ofType: CIDetectorTypeQRCode, context: CIContext(options: [.useSoftwareRenderer: true]),
                                  options: [CIDetectorAccuracy: CIDetectorAccuracyHigh])
        return (detector?.features(in: CIImage(cgImage: image)) ?? []).compactMap { ($0 as? CIQRCodeFeature)?.messageString }
    }
}
