import AppKit
import Kit42

/// Local helper for website screenshots (not part of the product).
/// `PDF42 --site-samples COPY_DIR OUT_DIR` reads `<lang>.json` copy files and writes, per language, a signed services
/// agreement with an invoice (`agreement-<lang>.pdf`) and the signature/stamp pictures for the sample letter. Companies,
/// people and registration numbers are fictional; signatures and stamps come from the generator.
/// `PDF42 --site-candidates "Name" OUT.png` draws a contact sheet of signature designs to pick good-looking ones.
enum SiteSamples {
    private static let page = CGRect(x: 0, y: 0, width: 595, height: 842)
    private static let blue = InkColor.blue.pen
    private static let accent = NSColor(srgbRed: 0.18, green: 0.27, blue: 0.21, alpha: 1)
    private static let warm = NSColor(srgbRed: 0.74, green: 0.25, blue: 0.15, alpha: 1)
    /// Right-to-left documents mirror the layout horizontally.
    private static var rtl = false

    struct Stamp: Decodable { var id: String; var width: CGFloat; var values: [String: String] }
    struct Party: Decodable {
        var role, company, person, title, date: String
        /// Name written by the signature generator; empty for a seal-only party (East Asian practice).
        var signName: String
        var pen: String
        var design: UInt64
        var stamp: Stamp
        /// Personal or representative seal pressed where the signature would go (used when `signName` is empty).
        var seal2: Stamp?
    }
    struct Copy: Decodable {
        var rtl: Bool
        var brand, brandLine, number, title, intro: String
        var sections: [[String]]
        var feeTitle: String, fees: [[String]], total: [String]
        var signTitle: String
        var parties: [Party]
        var invoiceTitle, invoiceNumber, billTo: String, billLines: [String]
        var items: [[String]], invoiceTotal: [String], paid: Stamp, note: String
    }

    static func write(copies: URL, to folder: URL) throws {
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let files = try FileManager.default.contentsOfDirectory(at: copies, includingPropertiesForKeys: nil).filter { $0.pathExtension == "json" }
        for file in files.sorted(by: { $0.lastPathComponent < $1.lastPathComponent }) {
            let lang = file.deletingPathExtension().lastPathComponent
            let c = try JSONDecoder().decode(Copy.self, from: Data(contentsOf: file))
            rtl = c.rtl
            try document(c).write(to: folder.appendingPathComponent("agreement-\(lang).pdf"))
            // Signature (or personal seal) and company stamp of the first party for the sample letter in DOC42.
            let p = c.parties[0]
            try png(p.signName.isEmpty ? p.seal2.flatMap { seal($0, impression: 41) } : signature(p, copy: 1), folder, "signature-\(lang).png")
            try png(seal(p.stamp, impression: 31), folder, "stamp-\(lang).png")
            print("wrote \(lang)")
        }
    }

    private static func png(_ image: CGImage?, _ folder: URL, _ name: String) throws {
        guard let image, let data = NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:]) else { return }
        try data.write(to: folder.appendingPathComponent(name))
    }

    static func candidates(_ name: String, to url: URL) throws {
        let forms: [SignatureForm] = [.fullName, .initialSurname, .surname]
        let seeds: [UInt64] = [11, 23, 37]
        let cellW = 300, cellH = 120, cols = 8
        var cells: [(String, CGImage?)] = []
        for form in forms { for seed in seeds { for variant in 0..<cols {
            let design = SignatureGenerator.design(form: form, variant: variant, seed: seed)
            cells.append(("\(design)", SignatureGenerator.render(name, design: design, impression: 5, ink: blue, pen: .ballpoint)))
        } } }
        let rows = (cells.count + cols - 1) / cols
        let W = cols * cellW, H = rows * cellH
        guard let ctx = CGContext(data: nil, width: W, height: H, bitsPerComponent: 8, bytesPerRow: 0, space: CGColorSpace(name: CGColorSpace.sRGB)!,
                                  bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue) else { return }
        ctx.setFillColor(.white); ctx.fill(CGRect(x: 0, y: 0, width: W, height: H))
        rtl = false
        for (i, (label, image)) in cells.enumerated() {
            let x = CGFloat(i % cols * cellW), y = CGFloat(H - (i / cols + 1) * cellH)
            ctx.setStrokeColor(NSColor(white: 0.85, alpha: 1).cgColor); ctx.stroke(CGRect(x: x, y: y, width: CGFloat(cellW), height: CGFloat(cellH)))
            text(ctx, "#\(i)  " + label, x: x + 4, y: y + 4, size: 9, color: .gray)
            if let image {
                let k = min(CGFloat(cellW - 20) / CGFloat(image.width), CGFloat(cellH - 26) / CGFloat(image.height))
                let w = CGFloat(image.width) * k, h = CGFloat(image.height) * k
                ctx.draw(image, in: CGRect(x: x + (CGFloat(cellW) - w) / 2, y: y + 16 + (CGFloat(cellH - 16) - h) / 2, width: w, height: h))
            }
        }
        try NSBitmapImageRep(cgImage: ctx.makeImage()!).representation(using: .png, properties: [:])!.write(to: url)
    }

    // MARK: - Pages

    static func document(_ c: Copy) -> Data {
        let data = NSMutableData()
        var box = page
        guard let consumer = CGDataConsumer(data: data as CFMutableData), let ctx = CGContext(consumer: consumer, mediaBox: &box, nil) else { return Data() }
        agreement(ctx, c)
        invoice(ctx, c)
        ctx.closePDF()
        return data as Data
    }

    private static func letterhead(_ ctx: CGContext, _ c: Copy, right: String) {
        fill(ctx, CGRect(x: 0, y: 812, width: 595, height: 30), accent)
        ctx.setFillColor(warm.cgColor)
        ctx.fillEllipse(in: CGRect(x: mx(44, 26), y: 762, width: 26, height: 26))
        text(ctx, c.brand, x: 80, y: 777, size: 13, bold: true, color: accent)
        text(ctx, c.brandLine, x: 80, y: 764, size: 8.5, color: .darkGray)
        text(ctx, right, x: 551 - width(right, 9), y: 772, size: 9, color: .darkGray)
        rule(ctx, y: 748)
    }

    private static func agreement(_ ctx: CGContext, _ c: Copy) {
        ctx.beginPDFPage(nil)
        letterhead(ctx, c, right: c.number)
        text(ctx, c.title, x: 44, y: 712, size: 24, bold: true, color: accent)
        var y: CGFloat = 696
        y -= paragraph(ctx, c.intro, x: 44, top: y, width: 507, size: 10) + 16
        for s in c.sections {
            text(ctx, s[0], x: 44, y: y - 10, size: 10.5, bold: true)
            y -= 16
            y -= paragraph(ctx, s[1], x: 44, top: y, width: 507, size: 10) + 12
        }
        text(ctx, c.feeTitle, x: 44, y: y - 10, size: 10.5, bold: true)
        y -= 26
        for f in c.fees {
            text(ctx, f[0], x: 56, y: y, size: 10)
            text(ctx, f[1], x: 551 - width(f[1], 10), y: y, size: 10)
            fill(ctx, CGRect(x: 44, y: y - 7, width: 507, height: 0.5), NSColor(white: 0.88, alpha: 1))
            y -= 20
        }
        fill(ctx, CGRect(x: 44, y: y - 8, width: 507, height: 22), NSColor(srgbRed: 0.93, green: 0.94, blue: 0.9, alpha: 1))
        text(ctx, c.total[0], x: 56, y: y, size: 10.5, bold: true)
        text(ctx, c.total[1], x: 551 - width(c.total[1], 10.5, bold: true), y: y, size: 10.5, bold: true)
        let top: CGFloat = y - 70
        rule(ctx, y: top + 22)
        text(ctx, c.signTitle, x: 44, y: top + 6, size: 10.5, bold: true, color: accent)
        party(ctx, c.parties[0], x: 44, top: top - 18, impression: 1)
        party(ctx, c.parties[1], x: 318, top: top - 18, impression: 2)
        footer(ctx, "1 / 2")
        ctx.endPDFPage()
    }

    private static func invoice(_ ctx: CGContext, _ c: Copy) {
        ctx.beginPDFPage(nil)
        letterhead(ctx, c, right: c.invoiceNumber)
        text(ctx, c.invoiceTitle, x: 44, y: 712, size: 24, bold: true, color: accent)
        text(ctx, c.billTo.uppercased(), x: 44, y: 680, size: 8, bold: true, color: .darkGray)
        for (i, line) in c.billLines.enumerated() { text(ctx, line, x: 44, y: 664 - CGFloat(i) * 14, size: 10, bold: i == 0) }
        var y: CGFloat = 600
        fill(ctx, CGRect(x: 44, y: y - 8, width: 507, height: 24), accent)
        text(ctx, "#", x: 54, y: y, size: 9, bold: true, color: .white)
        y -= 28
        for (i, item) in c.items.enumerated() {
            text(ctx, "\(i + 1)", x: 54, y: y, size: 10)
            text(ctx, item[0], x: 80, y: y, size: 10)
            text(ctx, item[1], x: 400, y: y, size: 10)
            text(ctx, item[2], x: 541 - width(item[2], 10), y: y, size: 10)
            fill(ctx, CGRect(x: 44, y: y - 9, width: 507, height: 0.5), NSColor(white: 0.88, alpha: 1))
            y -= 26
        }
        text(ctx, c.invoiceTotal[0], x: 330, y: y - 6, size: 12, bold: true)
        text(ctx, c.invoiceTotal[1], x: 541 - width(c.invoiceTotal[1], 12, bold: true), y: y - 6, size: 12, bold: true)
        ink(ctx, seal(c.paid, impression: 77), center: CGPoint(x: 430, y: y - 70), width: c.paid.width, angle: -0.16)
        paragraph(ctx, c.note, x: 44, top: y - 120, width: 300, size: 10)
        let p = c.parties[0]
        text(ctx, p.person + ", " + p.title, x: 44, y: y - 210, size: 9.5)
        fill(ctx, CGRect(x: 44, y: y - 196, width: 190, height: 0.6), NSColor(white: 0.6, alpha: 1))
        if p.signName.isEmpty {
            ink(ctx, p.seal2.flatMap { seal($0, impression: 9) }, center: CGPoint(x: 120, y: y - 170), width: (p.seal2?.width ?? 60) * 0.9)
        } else {
            ink(ctx, signature(p, copy: 9), center: CGPoint(x: 120, y: y - 180), width: 120)
        }
        footer(ctx, "2 / 2")
        ctx.endPDFPage()
    }

    private static func party(_ ctx: CGContext, _ p: Party, x: CGFloat, top: CGFloat, impression: UInt64) {
        text(ctx, p.role.uppercased(), x: x, y: top, size: 8, bold: true, color: .darkGray)
        text(ctx, p.company, x: x, y: top - 16, size: 10.5, bold: true)
        text(ctx, p.person + ", " + p.title, x: x, y: top - 30, size: 9.5)
        let lineY = top - 92
        fill(ctx, CGRect(x: x, y: lineY, width: 200, height: 0.6), NSColor(white: 0.55, alpha: 1))
        text(ctx, p.date, x: x, y: lineY - 14, size: 9, color: .darkGray)
        let round = p.stamp.width < 120
        ink(ctx, seal(p.stamp, impression: impression &* 7_777_777),
            center: round ? CGPoint(x: x + 170, y: lineY - 4) : CGPoint(x: x + 196, y: lineY + 34), width: round ? p.stamp.width : min(p.stamp.width, 132),
            angle: impression == 1 ? -0.06 : 0.05)
        if p.signName.isEmpty {
            ink(ctx, p.seal2.flatMap { seal($0, impression: impression &* 31) }, center: CGPoint(x: x + 60, y: lineY + 24), width: p.seal2?.width ?? 60)
        } else {
            ink(ctx, signature(p, copy: Int(impression)), center: CGPoint(x: x + (round ? 70 : 60), y: lineY + 20), width: round ? 122 : 112)
        }
    }

    // MARK: - Helpers (x coordinates are given left-to-right and mirrored for RTL documents)

    private static func mx(_ x: CGFloat, _ w: CGFloat) -> CGFloat { rtl ? page.width - x - w : x }

    private static func fill(_ ctx: CGContext, _ r: CGRect, _ color: NSColor) {
        ctx.setFillColor(color.cgColor)
        ctx.fill(CGRect(x: mx(r.minX, r.width), y: r.minY, width: r.width, height: r.height))
    }

    private static func font(_ size: CGFloat, bold: Bool = false) -> NSFont {
        NSFont(name: bold ? "HelveticaNeue-Bold" : "HelveticaNeue", size: size) ?? .systemFont(ofSize: size)
    }

    private static func attributed(_ s: String, _ size: CGFloat, bold: Bool, color: NSColor) -> NSAttributedString {
        NSAttributedString(string: s, attributes: [.font: font(size, bold: bold), .foregroundColor: color])
    }

    private static func width(_ s: String, _ size: CGFloat, bold: Bool = false) -> CGFloat {
        CGFloat(CTLineGetTypographicBounds(CTLineCreateWithAttributedString(attributed(s, size, bold: bold, color: .black)), nil, nil, nil))
    }

    private static func text(_ ctx: CGContext, _ s: String, x: CGFloat, y: CGFloat, size: CGFloat, bold: Bool = false, color: NSColor = .black) {
        let line = CTLineCreateWithAttributedString(attributed(s, size, bold: bold, color: color))
        ctx.textPosition = CGPoint(x: mx(x, width(s, size, bold: bold)), y: y)
        CTLineDraw(line, ctx)
    }

    @discardableResult
    private static func paragraph(_ ctx: CGContext, _ s: String, x: CGFloat, top: CGFloat, width: CGFloat, size: CGFloat) -> CGFloat {
        let style = NSMutableParagraphStyle()
        style.lineSpacing = 2.5
        style.alignment = rtl ? .right : .natural
        style.baseWritingDirection = rtl ? .rightToLeft : .natural
        let a = NSAttributedString(string: s, attributes: [.font: font(size), .foregroundColor: NSColor(white: 0.15, alpha: 1), .paragraphStyle: style])
        let setter = CTFramesetterCreateWithAttributedString(a)
        let fit = CTFramesetterSuggestFrameSizeWithConstraints(setter, CFRange(), nil, CGSize(width: width, height: .greatestFiniteMagnitude), nil)
        let rect = CGRect(x: mx(x, width), y: top - fit.height, width: width, height: fit.height)
        CTFrameDraw(CTFramesetterCreateFrame(setter, CFRange(), CGPath(rect: rect, transform: nil), nil), ctx)
        return fit.height
    }

    private static func rule(_ ctx: CGContext, y: CGFloat) {
        fill(ctx, CGRect(x: 44, y: y, width: 507, height: 0.7), NSColor(white: 0.85, alpha: 1))
    }

    private static func footer(_ ctx: CGContext, _ s: String) {
        rule(ctx, y: 46)
        text(ctx, s, x: 551 - width(s, 8), y: 32, size: 8, color: .gray)
    }

    private static func signature(_ p: Party, copy: Int) -> CGImage? {
        let pen: PenType = ["gel": .gel, "fountain": .fountain, "felt": .felt][p.pen] ?? .ballpoint
        return SignatureGenerator.render(p.signName, design: p.design, impression: UInt64(4 + copy), ink: blue, pen: pen)
    }

    private static func seal(_ s: Stamp, impression: UInt64) -> CGImage? {
        guard let t = SealTemplate.named(s.id) else { print("unknown stamp \(s.id)"); return nil }
        return SealGenerator.render(SealSpec(template: t, values: s.values, ink: t.ink.stampPad, designSeed: 1), impression: impression)
    }

    /// Ink is multiplied with the page, like a pen or a stamp pad on paper.
    private static func ink(_ ctx: CGContext, _ image: CGImage?, center: CGPoint, width: CGFloat, angle: CGFloat = 0) {
        guard var image else { return }
        let target = Int(width * 300 / 72)
        if image.width > target, let small = InkDrawing.scaled(image, width: target) { image = small }
        let h = width * CGFloat(image.height) / CGFloat(max(1, image.width))
        ctx.saveGState()
        ctx.setBlendMode(.multiply)
        ctx.interpolationQuality = .high
        ctx.translateBy(x: rtl ? page.width - center.x : center.x, y: center.y)
        ctx.rotate(by: angle)
        ctx.draw(image, in: CGRect(x: -width / 2, y: -h / 2, width: width, height: h))
        ctx.restoreGState()
    }
}
