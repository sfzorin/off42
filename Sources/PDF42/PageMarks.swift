import AppKit
import CoreText
import Kit42
import PDFKit
import Security

/// Page marks for paper copies: the electronic-signature mark (GOST R 7.0.97 style), a seal split across the
/// right edges of the pages (cross-page seal), the "numbered, bound and sealed" note, and initials on every page.
/// Everything is an ordinary annotation (multiply blending, like ink), so it can be moved, undone or removed.
enum PageMarks {
    enum Corner: Int, CaseIterable {
        case bottomRight, bottomLeft, topRight, topLeft

        var title: String {
            switch self {
            case .bottomRight: return L("Bottom right")
            case .bottomLeft: return L("Bottom left")
            case .topRight: return L("Top right")
            case .topLeft: return L("Top left")
            }
        }

        /// Rectangle of `size` in a corner of a displayed page of `page` size, `margin` from both edges.
        func rect(_ size: CGSize, in page: CGSize, margin: CGFloat) -> CGRect {
            let x = self == .bottomLeft || self == .topLeft ? margin : page.width - margin - size.width
            let y = self == .bottomLeft || self == .bottomRight ? margin : page.height - margin - size.height
            return CGRect(origin: CGPoint(x: x, y: y), size: size)
        }
    }

    /// Language of the text printed on the page (independent of the UI language).
    enum MarkLanguage: Int, CaseIterable {
        case ru, en, tr

        var title: String {
            switch self {
            case .ru: return "Русский (ГОСТ Р 7.0.97)"
            case .en: return "English"
            case .tr: return "Türkçe"
            }
        }

        var locale: Locale { Locale(identifier: ["ru_RU", "en_GB", "tr_TR"][rawValue]) }

        static var current: MarkLanguage {
            switch Language.current {
            case .ru: return .ru
            case .tr: return .tr
            default: return .en
            }
        }
    }

    // MARK: Geometry of displayed pages

    /// Size of the page as displayed (crop box, rotation applied).
    static func displaySize(_ page: PDFPage) -> CGSize {
        PageOverlay.displayTransform(box: page.bounds(for: .cropBox), rotation: page.rotation).1
    }

    /// A rectangle in displayed coordinates (origin bottom left of the visible page) → page space.
    static func pageRect(_ display: CGRect, on page: PDFPage) -> CGRect {
        display.applying(PageOverlay.displayTransform(box: page.bounds(for: .cropBox), rotation: page.rotation).0).standardized
    }

    /// The inverse of `pageRect`: where a page-space rectangle appears on the displayed page.
    static func displayRect(_ rect: CGRect, on page: PDFPage) -> CGRect {
        rect.applying(PageOverlay.displayTransform(box: page.bounds(for: .cropBox), rotation: page.rotation).0.inverted()).standardized
    }

    /// An ink image placed upright at `display` on `page`.
    static func ink(_ image: CGImage, at display: CGRect, on page: PDFPage) -> ImageStampAnnotation {
        let a = ImageStampAnnotation(image: image, bounds: pageRect(display, on: page))
        a.rotation = page.rotation
        return a
    }

    // MARK: Cross-page seal

    /// Pixel columns of `width` split into `count` slices, left to right (widths differ by at most one pixel).
    static func sliceColumns(width: Int, count: Int) -> [Range<Int>] {
        guard count > 0 else { return [] }
        return (0..<count).map { i in (i * width / count)..<((i + 1) * width / count) }
    }

    /// One seal impression cut into vertical slices along the right edges of `pages` (first page: leftmost slice).
    /// Fanned out with every next page shifted right by one slice, the pages show the whole seal. `size` is the
    /// seal width in points; `level` is the height of the seal centre as a fraction of the page height.
    static func crossPageSeal(_ seal: CGImage, pages: [PDFPage], size: CGFloat, level: CGFloat) -> [(PDFPage, ImageStampAnnotation)] {
        let columns = sliceColumns(width: seal.width, count: pages.count)
        let height = size * CGFloat(seal.height) / CGFloat(max(1, seal.width))
        var out: [(PDFPage, ImageStampAnnotation)] = []
        for (page, cols) in zip(pages, columns) where !cols.isEmpty {
            guard let slice = seal.cropping(to: CGRect(x: cols.lowerBound, y: 0, width: cols.count, height: seal.height)) else { continue }
            let page_ = displaySize(page)
            let w = size * CGFloat(cols.count) / CGFloat(seal.width)
            let y = min(max(0, page_.height * level - height / 2), max(0, page_.height - height))
            out.append((page, ink(slice, at: CGRect(x: page_.width - w, y: y, width: w, height: height), on: page)))
        }
        return out
    }

    // MARK: Initials

    /// A new impression of the mark on every page, in one corner. `width` in points.
    static func initials(pages: [PDFPage], corner: Corner, width: CGFloat, margin: CGFloat,
                         impression: () -> CGImage?) -> [(PDFPage, ImageStampAnnotation)] {
        pages.compactMap { page in
            guard let image = impression() else { return nil }
            let size = CGSize(width: width, height: width * CGFloat(image.height) / CGFloat(max(1, image.width)))
            return (page, ink(image, at: corner.rect(size, in: displaySize(page), margin: margin), on: page))
        }
    }

    // MARK: Electronic signature mark

    /// What the mark shows about a signing certificate.
    struct CertificateDetails: Equatable {
        var owner: String
        var serial: String
        var validFrom: Date?
        var validTo: Date?
        /// Signing time, when the mark is made from a signature in the document.
        var signedAt: Date?

        init(owner: String, serial: String, validFrom: Date? = nil, validTo: Date? = nil, signedAt: Date? = nil) {
            self.owner = owner
            self.serial = serial
            self.validFrom = validFrom
            self.validTo = validTo
            self.signedAt = signedAt
        }

        init(certificate: SecCertificate, signedAt: Date? = nil) {
            owner = SecCertificateCopySubjectSummary(certificate) as String? ?? ""
            serial = Self.serial(certificate)
            let keys = [kSecOIDX509V1ValidityNotBefore, kSecOIDX509V1ValidityNotAfter] as CFArray
            let values = SecCertificateCopyValues(certificate, keys, nil) as? [String: Any] ?? [:]
            func date(_ key: CFString) -> Date? {
                guard let entry = values[key as String] as? [String: Any], let n = entry[kSecPropertyKeyValue as String] as? NSNumber else { return nil }
                return Date(timeIntervalSinceReferenceDate: n.doubleValue)
            }
            validFrom = date(kSecOIDX509V1ValidityNotBefore)
            validTo = date(kSecOIDX509V1ValidityNotAfter)
            self.signedAt = signedAt
        }

        init?(identity: SigningIdentity) {
            var cert: SecCertificate?
            guard SecIdentityCopyCertificate(identity.identity, &cert) == errSecSuccess, let cert else { return nil }
            self.init(certificate: cert)
        }

        /// Serial number in hex, without the sign byte DER adds to positive numbers.
        static func serial(_ certificate: SecCertificate) -> String {
            guard var bytes = (SecCertificateCopySerialNumberData(certificate, nil) as Data?).map([UInt8].init) else { return "" }
            while bytes.count > 1, bytes[0] == 0 { bytes.removeFirst() }
            return bytes.map { String(format: "%02X", $0) }.joined()
        }
    }

    /// Text of the mark: heading lines, then (label, value) rows.
    static func eSignatureText(_ d: CertificateDetails, language: MarkLanguage) -> (heading: [String], rows: [(String, String)]) {
        let f = DateFormatter()
        f.locale = language.locale
        f.dateFormat = "dd.MM.yyyy"
        let time = DateFormatter()
        time.locale = language.locale
        time.dateFormat = "dd.MM.yyyy HH:mm"
        let from = d.validFrom.map(f.string) ?? "—", to = d.validTo.map(f.string) ?? "—"
        var rows: [(String, String)]
        let heading: [String]
        switch language {
        case .ru:
            heading = ["ДОКУМЕНТ ПОДПИСАН", "ЭЛЕКТРОННОЙ ПОДПИСЬЮ"]
            rows = [("Сертификат:", d.serial), ("Владелец:", d.owner), ("Действителен:", "с \(from) по \(to)")]
            if let s = d.signedAt { rows.append(("Подписан:", time.string(from: s))) }
        case .en:
            heading = ["DOCUMENT SIGNED", "WITH AN ELECTRONIC SIGNATURE"]
            rows = [("Certificate:", d.serial), ("Owner:", d.owner), ("Valid:", "from \(from) to \(to)")]
            if let s = d.signedAt { rows.append(("Signed:", time.string(from: s))) }
        case .tr:
            heading = ["BELGE ELEKTRONİK İMZA", "İLE İMZALANMIŞTIR"]
            rows = [("Sertifika:", d.serial), ("Sahibi:", d.owner), ("Geçerlilik:", "\(from) – \(to)")]
            if let s = d.signedAt { rows.append(("İmza tarihi:", time.string(from: s))) }
        }
        return (heading, rows)
    }

    /// Ink colour of the mark (the usual blue of e-signature marks).
    static let markBlue = NSColor(srgbRed: 0.12, green: 0.24, blue: 0.58, alpha: 1)

    /// Design size of the mark in points (85 mm wide) for a number of rows.
    static func eSignatureSize(rows: Int) -> CGSize {
        CGSize(width: 85 * PageGeometry.mm, height: 12 + 2 * 9.5 + 5 + CGFloat(rows) * 9.2)
    }

    static func eSignatureMark(_ d: CertificateDetails, language: MarkLanguage, at origin: CGPoint? = nil, corner: Corner = .bottomRight,
                               on page: PDFPage) -> VectorMarkAnnotation {
        let text = eSignatureText(d, language: language)
        let size = eSignatureSize(rows: text.rows.count)
        let display = origin.map { CGRect(origin: $0, size: size) } ?? corner.rect(size, in: displaySize(page), margin: 15 * PageGeometry.mm)
        let mark = VectorMarkAnnotation(bounds: pageRect(display, on: page), size: size, rotation: page.rotation) { ctx in
            drawESignature(text, size: size, in: ctx)
        }
        mark.contents = (text.heading + text.rows.map { "\($0.0) \($0.1)" }).joined(separator: "\n")
        return mark
    }

    private static func drawESignature(_ text: (heading: [String], rows: [(String, String)]), size: CGSize, in ctx: CGContext) {
        let blue = markBlue.cgColor
        let frame = CGRect(origin: .zero, size: size).insetBy(dx: 1, dy: 1)
        ctx.setStrokeColor(blue)
        ctx.setLineWidth(1.2)
        ctx.addPath(CGPath(roundedRect: frame, cornerWidth: 6, cornerHeight: 6, transform: nil))
        ctx.strokePath()
        let bold = NSFont(name: "HelveticaNeue-Bold", size: 7.6) ?? .boldSystemFont(ofSize: 7.6)
        let regular = NSFont(name: "HelveticaNeue", size: 7) ?? .systemFont(ofSize: 7)
        let label = NSFont(name: "HelveticaNeue-Medium", size: 7) ?? .systemFont(ofSize: 7, weight: .medium)
        let color = NSColor(cgColor: blue) ?? markBlue
        var y = size.height - 7 - bold.ascender
        for line in text.heading {
            let l = PageOverlay.line(line, font: bold, color: color)
            drawLine(l, x: (size.width - PageOverlay.width(of: l)) / 2, y: y, maxWidth: size.width - 16, in: ctx)
            y -= 9.5
        }
        // Rule under the heading.
        let ruleY = y + 9.5 + bold.descender - 2
        ctx.setLineWidth(0.6)
        ctx.move(to: CGPoint(x: 8, y: ruleY))
        ctx.addLine(to: CGPoint(x: size.width - 8, y: ruleY))
        ctx.strokePath()
        y = ruleY - 3 - regular.ascender
        let labels = text.rows.map { PageOverlay.line($0.0, font: label, color: color) }
        let column = 10 + (labels.map(PageOverlay.width).max() ?? 0) + 4
        for (row, l) in zip(text.rows, labels) {
            drawLine(l, x: 10, y: y, maxWidth: column - 14, in: ctx)
            drawLine(PageOverlay.line(row.1, font: regular, color: color), x: column, y: y, maxWidth: size.width - column - 8, in: ctx)
            y -= 9.2
        }
    }

    /// Draws a line, condensed horizontally when it is wider than `maxWidth` (long serial numbers and names).
    private static func drawLine(_ line: CTLine, x: CGFloat, y: CGFloat, maxWidth: CGFloat, in ctx: CGContext) {
        let w = PageOverlay.width(of: line)
        ctx.saveGState()
        ctx.translateBy(x: x, y: y)
        if w > maxWidth, w > 0 { ctx.scaleBy(x: maxWidth / w, y: 1) }
        ctx.textPosition = .zero
        CTLineDraw(line, ctx)
        ctx.restoreGState()
    }

    // MARK: Certification note ("numbered, bound and sealed")

    /// Russian noun form after a number: 1 лист, 2 листа, 5 листов (11–14 листов).
    static func russianPlural(_ n: Int, _ one: String, _ few: String, _ many: String) -> String {
        let n100 = abs(n) % 100, n10 = abs(n) % 10
        if (11...14).contains(n100) { return many }
        if n10 == 1 { return one }
        if (2...4).contains(n10) { return few }
        return many
    }

    static func spelled(_ n: Int, _ language: MarkLanguage) -> String {
        let f = NumberFormatter()
        f.numberStyle = .spellOut
        f.locale = language.locale
        return f.string(from: NSNumber(value: n)) ?? "\(n)"
    }

    /// Lines of the note for `sheets` sheets.
    static func certificationText(sheets: Int, signatory: String, date: Date, language: MarkLanguage) -> [String] {
        let f = DateFormatter()
        f.locale = language.locale
        f.dateFormat = "d MMMM yyyy"
        let words = spelled(sheets, language)
        let signer = signatory.trimmingCharacters(in: .whitespaces)
        switch language {
        case .ru:
            let noun = russianPlural(sheets, "лист", "листа", "листов")
            return ["Всего пронумеровано, прошнуровано", "и скреплено печатью \(sheets) (\(words)) \(noun)", "",
                    signer.isEmpty ? "____________________ / ____________________" : "\(signer)  ____________________",
                    "«\(Calendar.current.component(.day, from: date))» \(f.string(from: date).split(separator: " ").dropFirst().joined(separator: " ")) г."]
        case .en:
            return ["Numbered, bound and sealed:", "\(sheets) (\(words)) \(sheets == 1 ? "sheet" : "sheets") in total", "",
                    signer.isEmpty ? "____________________ / ____________________" : "\(signer)  ____________________",
                    f.string(from: date)]
        case .tr:
            return ["Toplam \(sheets) (\(words)) yaprak numaralandırılmış,", "ciltlenmiş ve mühürlenmiştir.", "",
                    signer.isEmpty ? "____________________ / ____________________" : "\(signer)  ____________________",
                    f.string(from: date)]
        }
    }

    /// The note in the bottom-left of the last page, with a seal over the signature line and an optional signature.
    static func certificationNote(on page: PDFPage, sheets: Int, signatory: String, date: Date, language: MarkLanguage,
                                  seal: CGImage?, sealSize: CGFloat, signature: CGImage?) -> [PDFAnnotation] {
        let lines = certificationText(sheets: sheets, signatory: signatory, date: date, language: language)
        let font = NSFont(name: "Times New Roman", size: 11) ?? NSFont(name: "Times-Roman", size: 11) ?? .systemFont(ofSize: 11)
        let leading: CGFloat = 15
        let widths = lines.map { PageOverlay.width(of: PageOverlay.line($0, font: font, color: .black)) }
        let size = CGSize(width: ceil((widths.max() ?? 100) + 4), height: CGFloat(lines.count) * leading + 4)
        let margin = 20 * PageGeometry.mm
        let display = CGRect(origin: CGPoint(x: margin, y: margin), size: size)
        let note = VectorMarkAnnotation(bounds: pageRect(display, on: page), size: size, rotation: page.rotation) { ctx in
            var y = size.height - 2 - font.ascender
            for text in lines {
                ctx.textPosition = CGPoint(x: 2, y: y)
                CTLineDraw(PageOverlay.line(text, font: font, color: .black), ctx)
                y -= leading
            }
        }
        note.contents = lines.joined(separator: "\n")
        var out: [PDFAnnotation] = [note]
        // The signature line is the fourth line: the signature goes over its underscores, the seal over their right end.
        let lineY = display.maxY - 2 - 3 * leading - font.ascender / 3
        let rule = PageOverlay.width(of: PageOverlay.line("____________________", font: font, color: .black))
        let ruleStart = display.minX + 2 + (signatory.trimmingCharacters(in: .whitespaces).isEmpty ? 0 : widths[3] - rule)
        if let signature {
            let w = min(rule, 34 * PageGeometry.mm)
            let h = w * CGFloat(signature.height) / CGFloat(max(1, signature.width))
            out.append(ink(signature, at: CGRect(x: ruleStart + (rule - w) / 2, y: lineY - h * 0.3, width: w, height: h), on: page))
        }
        if let seal {
            let h = sealSize * CGFloat(seal.height) / CGFloat(max(1, seal.width))
            let cx = ruleStart + rule
            out.insert(ink(seal, at: CGRect(x: cx - sealSize / 2, y: lineY - h / 2, width: sealSize, height: h), on: page), at: 1)
        }
        return out
    }
}

/// Vector mark drawn by a closure in its own upright design space (`size`), scaled to the bounds; ink blending.
/// PDFKit writes the drawing into the annotation's appearance stream on save, with the text as real text.
final class VectorMarkAnnotation: PDFAnnotation {
    let size: CGSize
    let render: (CGContext) -> Void
    var rotation: Int
    var blendMode: CGBlendMode = .multiply

    init(bounds: CGRect, size: CGSize, rotation: Int, render: @escaping (CGContext) -> Void) {
        self.size = size
        self.rotation = rotation
        self.render = render
        super.init(bounds: bounds, forType: .stamp, withProperties: nil)
    }

    required init?(coder: NSCoder) { nil }

    override func draw(with box: PDFDisplayBox, in context: CGContext) {
        context.saveGState()
        context.setBlendMode(blendMode)
        compensatePageRotation(context)
        let turn = ((rotation % 360) + 360) % 360
        let upright = turn == 90 || turn == 270 ? CGSize(width: bounds.height, height: bounds.width) : bounds.size
        context.translateBy(x: bounds.midX, y: bounds.midY)
        context.rotate(by: CGFloat(turn) * .pi / 180)
        context.translateBy(x: -upright.width / 2, y: -upright.height / 2)
        context.scaleBy(x: upright.width / max(1, size.width), y: upright.height / max(1, size.height))
        render(context)
        context.restoreGState()
    }
}
