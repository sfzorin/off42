import AppKit
import CoreText

/// A stamp to render: a catalogue template, the user's texts (missing keys use the template's examples) and ink.
public struct SealSpec {
    public var template: SealTemplate
    public var values: [String: String]
    public var ink: NSColor
    /// Reserved for layout variations of one stamp; impressions never change the layout.
    public var designSeed: UInt64

    public init(template: SealTemplate, values: [String: String], ink: NSColor, designSeed: UInt64) {
        self.template = template
        self.values = values
        self.ink = ink
        self.designSeed = designSeed
    }

    /// The text for a field; an empty date field means today.
    func text(_ key: String) -> String {
        let value = (values[key] ?? template.example(key)).trimmingCharacters(in: .whitespacesAndNewlines)
        if value.isEmpty, let field = template.fields.first(where: { $0.key == key }), field.label.hasPrefix("Date"),
           values[key] != " ", template.example(key) != " " {
            return Date().formatted(date: .numeric, time: .omitted)
        }
        return value
    }
}

/// Draws a stamp as if pressed on paper. The layout comes from the template and texts; every impression seed gives a
/// different pressing of that same stamp: tilt (round 2–8°, rectangular 0.3–2°), uneven pad pressure, grain, gaps and
/// an occasional faint double strike.
public enum SealGenerator {
    private static let sans = "HelveticaNeue-Bold", sansCondensed = "HelveticaNeue-CondensedBold"
    private static let serifBold = "TimesNewRomanPS-BoldMT"

    /// `scale`: pixels per layout unit (1: preview size, about 1000 px for a round seal); the impression is the same.
    public static func render(_ spec: SealSpec, impression: UInt64, scale: CGFloat = 1) -> CGImage? {
        var rng = SeededRandom(impression)
        let layout = CGMutablePath()
        let canvas = draw(spec, into: layout)
        let tilt = rng.range(spec.template.tilt) * (rng.chance(0.5) ? 1 : -1)
        let side = Int((max(canvas.width, canvas.height) * 1.25 * scale).rounded())
        guard let ctx = InkDraw.context(side, side) else { return nil }
        ctx.translateBy(x: CGFloat(side) / 2, y: CGFloat(side) / 2)
        ctx.scaleBy(x: scale, y: scale)
        ctx.rotate(by: tilt)
        ctx.translateBy(x: -canvas.width / 2, y: -canvas.height / 2)
        ctx.setFillColor(spec.ink.cgColor)
        ctx.addPath(layout)
        ctx.fillPath()
        // Sometimes the stamp rocks a little: a faint offset second impression.
        if rng.chance(0.3) {
            ctx.saveGState()
            ctx.setAlpha(rng.range(0.12...0.25))
            ctx.translateBy(x: rng.range(-6...6), y: rng.range(-6...6))
            ctx.addPath(layout)
            ctx.fillPath()
            ctx.restoreGState()
        }
        guard let image = ctx.makeImage() else { return nil }
        return InkDraw.trim(InkTexture.apply(image, seed: rng.next(), strength: InkTexture.stamp, scale: scale), pad: Int((10 * scale).rounded()))
    }

    /// A new impression of an imported stamp picture: tilt (round stamps more than rectangular), pad pressure,
    /// grain, gaps and sometimes a faint double strike. The picture keeps its own colours.
    /// `scale` enlarges the picture (smoothly) before pressing, so grain and bleed are finer than its pixels;
    /// below 1 it is drawn smaller. The result stays under about 4000 px.
    public static func impress(_ image: CGImage, round: Bool, impression: UInt64, scale: CGFloat = 1) -> CGImage? {
        var rng = SeededRandom(impression)
        let k = max(0.25, min(scale, 4000 / (CGFloat(max(image.width, image.height)) * 1.25)))
        let w = CGFloat(image.width) * k, h = CGFloat(image.height) * k
        let tilt = rng.range(round ? 0.035...0.14 : 0.005...0.035) * (rng.chance(0.5) ? 1 : -1)
        let side = Int((max(w, h) * 1.25).rounded())
        guard let ctx = InkDraw.context(side, side) else { return nil }
        ctx.translateBy(x: CGFloat(side) / 2, y: CGFloat(side) / 2)
        ctx.rotate(by: tilt)
        ctx.interpolationQuality = .high
        let rect = CGRect(x: -w / 2, y: -h / 2, width: w, height: h)
        ctx.draw(image, in: rect)
        if rng.chance(0.3) {
            ctx.setAlpha(rng.range(0.12...0.25))
            let d = max(w, h) / 160
            ctx.draw(image, in: rect.offsetBy(dx: rng.range(-d...d), dy: rng.range(-d...d)))
        }
        guard let pressed = ctx.makeImage() else { return nil }
        return InkDraw.trim(InkTexture.apply(pressed, seed: rng.next(), strength: InkTexture.stamp, scale: k), pad: Int((10 * k).rounded()))
    }

    // MARK: - Layout families (filled outlines; rings are stroked into outlines). Returns the canvas size.

    private static func draw(_ s: SealSpec, into p: CGMutablePath) -> CGSize {
        switch s.template.layout {
        case .round(let r): return round(s, r, into: p)
        case .oval(let r): return oval(s, r, into: p)
        case .rect(let r): return rect(s, r, into: p)
        case .word(let w): return word(s, w, into: p)
        case .grid(let g): return grid(s, g, into: p)
        case .dater(let font): return dater(s, font: font, into: p)
        case .table: return table(s, into: p)
        case .triangle: return triangle(s, into: p)
        }
    }

    private static func round(_ s: SealSpec, _ o: SealLayout.Round, into p: CGMutablePath) -> CGSize {
        let c = CGPoint(x: 500, y: 500)
        var r: CGFloat = 470
        switch o.ring {
        case .solid: ring(p, c, r, 14); r -= 24; if o.double { ring(p, c, r, 4); r -= 12 }
        case .thick: ring(p, c, r, 24); r -= 34; if o.double { ring(p, c, r, 4); r -= 12 }
        case .thin: ring(p, c, r, 8); r -= 18; if o.double { ring(p, c, r, 3); r -= 10 }
        case .dotted:
            ring(p, c, r, 9); dots(p, c, r - 24, count: 72, size: 5); r -= 40
            if o.double { ring(p, c, r, 3); r -= 10 }
        case .rope:
            ring(p, c, r, 6); rope(p, c, r - 20, count: 96); ring(p, c, r - 40, 6); r -= 52
        case .serrated:
            serrated(p, c, outer: r + 18, inner: r - 4, teeth: 120); ring(p, c, r - 22, 5); r -= 34
        case .stars:
            ring(p, c, r, 10); starRing(p, c, r - 28, count: 40, size: 10); ring(p, c, r - 50, 4); r -= 60
        }
        let band: CGFloat = o.small ? 112 : 72
        let base = r - band - 4
        let edge = s.text("edge"), bottom = s.text("bottom")
        switch o.edge {
        case .full:
            arcText(p, edge, font: o.font, center: c, radius: base, band: band, fullCircle: true)
        case .topBottom:
            arcText(p, edge, font: o.font, center: c, radius: base, band: band, span: (.pi * 0.13, .pi * 0.87))
            arcText(p, bottom, font: o.font, center: c, radius: base, band: band, span: (.pi * 1.17, .pi * 1.83), bottom: true)
            for a in [CGFloat.pi, 0] { star(p, CGPoint(x: c.x + (base + band * 0.4) * cos(a), y: c.y + (base + band * 0.4) * sin(a)), 16, points: 5) }
        case .top:
            arcText(p, edge, font: o.font, center: c, radius: base - 20, band: band + 40, span: (-.pi * 0.22, .pi * 1.22), condensed: 0.78)
        }
        r = base - 16
        if o.double || o.ring == .solid && o.edge != .top { ring(p, c, r, 4); r -= 10 }
        let inner = s.text("inner")
        if o.inner, !inner.isEmpty {
            arcText(p, inner, font: o.font, center: c, radius: r - 50, band: 44, fullCircle: true)
            r -= 66
            ring(p, c, r, 3)
            r -= 8
        }
        let centre = s.text("centre")
        switch o.centre {
        case .lines:
            lines(p, centre, font: o.centreFont, center: c, box: CGSize(width: r * 1.45, height: r * 1.1), maxSize: o.small ? 130 : 96)
        case .initials:
            lines(p, centre, font: o.centreFont, center: c, box: CGSize(width: r * 1.3, height: r * 0.9), maxSize: r * 0.9)
        case .star:
            star(p, CGPoint(x: c.x, y: c.y + 10), r * 0.42, points: 5)
            if !bottom.isEmpty {
                lines(p, bottom, font: o.font, center: CGPoint(x: c.x, y: c.y - r * 0.62), box: CGSize(width: r * 1.15, height: r * 0.3), maxSize: 84)
            }
        case .vertical:
            vertical(p, centre, font: o.centreFont, center: c, box: CGSize(width: r * 1.0, height: r * 1.5))
        case .grid:
            break
        }
        return CGSize(width: 1000, height: 1000)
    }

    private static func oval(_ s: SealSpec, _ o: SealLayout.Round, into p: CGMutablePath) -> CGSize {
        let c = CGPoint(x: 600, y: 430)
        var a: CGFloat = 580, b: CGFloat = 410
        if o.ring == .thick { ellipse(p, c, a, b, 22); a -= 30; b -= 30 } else { ellipse(p, c, a, b, 13); a -= 22; b -= 22 }
        if o.double, o.ring != .thick { ellipse(p, c, a, b, 4) }
        ellipseText(p, s.text("edge"), font: o.font, center: c, a: a - 70, b: b - 70, band: 60, from: .pi * 0.88, to: .pi * 0.12)
        ellipseText(p, s.text("bottom"), font: o.font, center: c, a: a - 70, b: b - 70, band: 60, from: .pi * 1.15, to: .pi * 1.85, bottom: true)
        ellipse(p, c, a - 106, b - 106, 4)
        lines(p, s.text("centre"), font: o.centreFont, center: c, box: CGSize(width: (a - 130) * 1.5, height: (b - 130) * 1.2), maxSize: 110)
        return CGSize(width: 1200, height: 860)
    }

    private static func rect(_ s: SealSpec, _ o: SealLayout.Rect, into p: CGMutablePath) -> CGSize {
        let framed = o.frame != .none
        let pad: CGFloat = framed ? 80 : 40
        let width = o.width, inner = width - pad * 2
        // Expand multi-line fields into one row per line, then measure.
        var rows: [(SealLayout.Row, String)] = []
        for row in o.rows {
            if case .text(let key, _, _, let upper) = row {
                let value = s.text(key)
                for line in value.split(separator: "\n", omittingEmptySubsequences: true) {
                    rows.append((row, upper ? line.uppercased() : String(line)))
                }
            } else {
                rows.append((row, ""))
            }
        }
        func height(_ row: SealLayout.Row) -> CGFloat {
            switch row {
            case .text(_, _, let size, _): return size * 1.32
            case .space(let h): return h
            case .line: return 26
            case .divider: return 30
            }
        }
        let total = rows.reduce(0) { $0 + height($1.0) }
        let h = total + pad * 2
        var y = h - pad
        for (row, text) in rows {
            let rh = height(row)
            switch row {
            case .text(_, let font, let size, _):
                let f = InkDraw.fit(font, text, width: inner, maxSize: size)
                let w = InkDraw.width(text, font: f)
                let x = o.left ? pad : (width - w) / 2
                let baseline = y - rh + (rh - CTFontGetCapHeight(f)) / 2
                for g in InkDraw.glyphs(text, font: f) {
                    var t = CGAffineTransform(translationX: x + g.position.x, y: baseline)
                    if let path = g.path.copy(using: &t) { p.addPath(path) }
                }
            case .line(let fraction):
                let lw = inner * fraction, x = o.left ? pad : (width - lw) / 2
                p.addRect(CGRect(x: x, y: y - rh / 2 - 2.5, width: lw, height: 5))
            case .divider:
                p.addRect(CGRect(x: pad - (framed ? 40 : 0), y: y - rh / 2 - 3, width: inner + (framed ? 80 : 0), height: 6))
            case .space:
                break
            }
            y -= rh
        }
        frame(p, CGRect(x: 0, y: 0, width: width, height: h), o.frame)
        return CGSize(width: width, height: h)
    }

    private static func frame(_ p: CGMutablePath, _ box: CGRect, _ style: SealLayout.Frame) {
        func outline(_ r: CGRect, _ radius: CGFloat, _ w: CGFloat) {
            p.addPath(CGPath(roundedRect: r, cornerWidth: radius, cornerHeight: radius, transform: nil)
                .copy(strokingWithWidth: w, lineCap: .round, lineJoin: .round, miterLimit: 10))
        }
        let r = box.insetBy(dx: 12, dy: 12)
        switch style {
        case .none: break
        case .single: outline(r, 6, 9)
        case .rounded: outline(r, 34, 9)
        case .thick: outline(r, 4, 18)
        case .double: outline(r, 22, 16); outline(r.insetBy(dx: 24, dy: 24), 12, 5)
        }
    }

    private static func word(_ s: SealSpec, _ o: SealLayout.Word, into p: CGMutablePath) -> CGSize {
        let size = CGSize(width: 1300, height: 640)
        let outer = CGRect(x: 30, y: 30, width: size.width - 60, height: size.height - 60)
        p.addPath(CGPath(roundedRect: outer, cornerWidth: o.rounded ? 26 : 4, cornerHeight: o.rounded ? 26 : 4, transform: nil)
            .copy(strokingWithWidth: 18, lineCap: .round, lineJoin: .round, miterLimit: 10))
        let inner = outer.insetBy(dx: 26, dy: 26)
        if o.frame == .double {
            p.addPath(CGPath(roundedRect: inner, cornerWidth: o.rounded ? 14 : 2, cornerHeight: o.rounded ? 14 : 2, transform: nil)
                .copy(strokingWithWidth: 6, lineCap: .round, lineJoin: .round, miterLimit: 10))
        }
        var rows: [(String, String, CGFloat)] = []
        let top = s.text("top"), date = s.text("date")
        if !top.isEmpty { rows.append((top.uppercased(), sans, 52)) }
        rows.append((s.text("word").uppercased(), o.font, 200))
        if !date.isEmpty { rows.append((date, sans, 62)) }
        stack(p, rows, in: inner.insetBy(dx: 40, dy: 30), alignLeft: false, frame: 0)
        return size
    }

    private static func grid(_ s: SealSpec, _ o: SealLayout.Grid, into p: CGMutablePath) -> CGSize {
        let chars = Array(s.text("chars").filter { !$0.isWhitespace })
        let size: CGSize, area: CGRect
        switch o.shape {
        case .square:
            size = CGSize(width: 1000, height: 1000)
            let box = CGRect(x: 40, y: 40, width: 920, height: 920)
            p.addPath(CGPath(roundedRect: box, cornerWidth: 18, cornerHeight: 18, transform: nil)
                .copy(strokingWithWidth: o.frame, lineCap: .round, lineJoin: .round, miterLimit: 10))
            area = box.insetBy(dx: o.frame + 34, dy: o.frame + 34)
        case .round:
            size = CGSize(width: 1000, height: 1000)
            ring(p, CGPoint(x: 500, y: 500), 470, o.frame)
            area = CGRect(x: 500 - 300, y: 500 - 330, width: 600, height: 660)
        case .oval:
            size = CGSize(width: 760, height: 1000)
            ellipse(p, CGPoint(x: 380, y: 500), 350, 480, o.frame)
            area = CGRect(x: 380 - 210, y: 500 - 340, width: 420, height: 680)
        }
        guard !chars.isEmpty else { return size }
        // CJK reads in columns from the right; Latin and other alphabets in rows from the left.
        let latin = chars.allSatisfy { $0.unicodeScalars.allSatisfy { $0.value < 0x2E80 } }
        let perColumn = o.perColumn ?? max(1, Int(Double(chars.count).squareRoot().rounded(.up)))
        let columns = Int((Double(chars.count) / Double(perColumn)).rounded(.up))
        let cellW = area.width / CGFloat(columns), cellH = area.height / CGFloat(perColumn)
        let f = font(o.font, 200)
        for (i, ch) in chars.enumerated() {
            let col = latin ? i % columns : i / perColumn, row = latin ? i / columns : i % perColumn
            let x = latin ? area.minX + CGFloat(col) * cellW : area.maxX - CGFloat(col + 1) * cellW
            let cell = CGRect(x: x, y: area.maxY - CGFloat(row + 1) * cellH, width: cellW, height: cellH)
            let glyphPath = CGMutablePath()
            for g in InkDraw.glyphs(String(ch), font: f) { glyphPath.addPath(g.path) }
            let b = glyphPath.boundingBoxOfPath
            guard b.width > 0, b.height > 0 else { continue }
            // Seal script fills its cell: scale each axis, but keep the proportions within 1.6×.
            var sx = cell.width * 0.88 / b.width, sy = cell.height * 0.88 / b.height
            sx = min(sx, sy * 1.6); sy = min(sy, sx * 1.6)
            var t = CGAffineTransform(translationX: cell.midX, y: cell.midY).scaledBy(x: sx, y: sy).translatedBy(x: -b.midX, y: -b.midY)
            if let path = glyphPath.copy(using: &t) { p.addPath(path) }
        }
        return size
    }

    private static func dater(_ s: SealSpec, font: String, into p: CGMutablePath) -> CGSize {
        let c = CGPoint(x: 500, y: 500), r: CGFloat = 470
        ring(p, c, r, 16)
        for y in [CGFloat(120), -120] {
            let half = (r * r - y * y).squareRoot() - 6
            p.addRect(CGRect(x: c.x - half, y: c.y + y - 3, width: half * 2, height: 6))
        }
        lines(p, s.text("top"), font: font, center: CGPoint(x: c.x, y: c.y + 265), box: CGSize(width: 560, height: 170), maxSize: 150)
        lines(p, s.text("date"), font: "Courier-Bold", center: c, box: CGSize(width: 800, height: 150), maxSize: 130)
        lines(p, s.text("name"), font: font, center: CGPoint(x: c.x, y: c.y - 265), box: CGSize(width: 560, height: 170), maxSize: 150)
        return CGSize(width: 1000, height: 1000)
    }

    private static func table(_ s: SealSpec, into p: CGMutablePath) -> CGSize {
        let labels = s.text("rows").split(separator: "\n").map(String.init)
        let top = s.text("top")
        let rowH: CGFloat = 130, width: CGFloat = 1300
        let count = labels.count + (top.isEmpty ? 0 : 1)
        let h = CGFloat(max(1, count)) * rowH + 40
        let box = CGRect(x: 20, y: 20, width: width - 40, height: h - 40)
        p.addPath(CGPath(rect: box, transform: nil).copy(strokingWithWidth: 12, lineCap: .square, lineJoin: .miter, miterLimit: 10))
        var y = box.maxY
        if !top.isEmpty {
            lines(p, top.uppercased(), font: sans, center: CGPoint(x: box.midX, y: y - rowH / 2), box: CGSize(width: box.width - 80, height: rowH * 0.6), maxSize: 64)
            y -= rowH
            p.addRect(CGRect(x: box.minX, y: y - 3, width: box.width, height: 6))
        }
        let split = box.minX + box.width * 0.42
        p.addRect(CGRect(x: split - 3, y: box.minY, width: 6, height: y - box.minY))
        for (i, label) in labels.enumerated() {
            let f = InkDraw.fit("HelveticaNeue-Medium", label, width: split - box.minX - 60, maxSize: 54)
            for g in InkDraw.glyphs(label, font: f) {
                var t = CGAffineTransform(translationX: box.minX + 30 + g.position.x, y: y - rowH / 2 - CTFontGetCapHeight(f) / 2)
                if let path = g.path.copy(using: &t) { p.addPath(path) }
            }
            y -= rowH
            if i < labels.count - 1 { p.addRect(CGRect(x: box.minX, y: y - 3, width: box.width, height: 6)) }
        }
        return CGSize(width: width, height: h)
    }

    private static func triangle(_ s: SealSpec, into p: CGMutablePath) -> CGSize {
        let tri = CGMutablePath()
        tri.addLines(between: [CGPoint(x: 500, y: 940), CGPoint(x: 60, y: 160), CGPoint(x: 940, y: 160)])
        tri.closeSubpath()
        p.addPath(tri.copy(strokingWithWidth: 22, lineCap: .round, lineJoin: .round, miterLimit: 10))
        lines(p, s.text("word"), font: sans, center: CGPoint(x: 500, y: 300), box: CGSize(width: 520, height: 150), maxSize: 150)
        lines(p, s.text("number"), font: sans, center: CGPoint(x: 500, y: 520), box: CGSize(width: 300, height: 200), maxSize: 200)
        return CGSize(width: 1000, height: 1000)
    }

    private static func dots(_ p: CGMutablePath, _ c: CGPoint, _ r: CGFloat, count: Int, size: CGFloat) {
        for i in 0..<count {
            let a = CGFloat(i) / CGFloat(count) * 2 * .pi
            p.addEllipse(in: CGRect(x: c.x + r * cos(a) - size, y: c.y + r * sin(a) - size, width: size * 2, height: size * 2))
        }
    }

    /// Twisted-rope border: short slanted bars around the circle.
    private static func rope(_ p: CGMutablePath, _ c: CGPoint, _ r: CGFloat, count: Int) {
        for i in 0..<count {
            let a = CGFloat(i) / CGFloat(count) * 2 * .pi
            let bar = CGMutablePath()
            bar.move(to: CGPoint(x: c.x + (r - 12) * cos(a), y: c.y + (r - 12) * sin(a)))
            bar.addLine(to: CGPoint(x: c.x + (r + 12) * cos(a + 0.035), y: c.y + (r + 12) * sin(a + 0.035)))
            p.addPath(bar.copy(strokingWithWidth: 7, lineCap: .round, lineJoin: .round, miterLimit: 10))
        }
    }

    private static func starRing(_ p: CGMutablePath, _ c: CGPoint, _ r: CGFloat, count: Int, size: CGFloat) {
        for i in 0..<count {
            let a = CGFloat(i) / CGFloat(count) * 2 * .pi
            star(p, CGPoint(x: c.x + r * cos(a), y: c.y + r * sin(a)), size, points: 5)
        }
    }

    // MARK: - Primitives

    private static func ring(_ p: CGMutablePath, _ c: CGPoint, _ r: CGFloat, _ width: CGFloat) {
        p.addPath(CGPath(ellipseIn: CGRect(x: c.x - r, y: c.y - r, width: 2 * r, height: 2 * r), transform: nil)
            .copy(strokingWithWidth: width, lineCap: .round, lineJoin: .round, miterLimit: 10))
    }

    private static func ellipse(_ p: CGMutablePath, _ c: CGPoint, _ a: CGFloat, _ b: CGFloat, _ width: CGFloat) {
        p.addPath(CGPath(ellipseIn: CGRect(x: c.x - a, y: c.y - b, width: 2 * a, height: 2 * b), transform: nil)
            .copy(strokingWithWidth: width, lineCap: .round, lineJoin: .round, miterLimit: 10))
    }

    /// Saw-tooth edge of a corporate seal.
    private static func serrated(_ p: CGMutablePath, _ c: CGPoint, outer: CGFloat, inner: CGFloat, teeth: Int) {
        let path = CGMutablePath()
        for i in 0..<(teeth * 2) {
            let a = CGFloat(i) / CGFloat(teeth * 2) * 2 * .pi, r = i % 2 == 0 ? outer : inner
            let pt = CGPoint(x: c.x + r * cos(a), y: c.y + r * sin(a))
            if i == 0 { path.move(to: pt) } else { path.addLine(to: pt) }
        }
        path.closeSubpath()
        p.addPath(path.copy(strokingWithWidth: 5, lineCap: .round, lineJoin: .round, miterLimit: 10))
    }

    private static func star(_ p: CGMutablePath, _ c: CGPoint, _ r: CGFloat, points n: Int) {
        let path = CGMutablePath()
        for i in 0..<(n * 2) {
            let a = .pi / 2 + CGFloat(i) * .pi / CGFloat(n), rr = i % 2 == 0 ? r : r * 0.38
            let pt = CGPoint(x: c.x + rr * cos(a), y: c.y + rr * sin(a))
            if i == 0 { path.move(to: pt) } else { path.addLine(to: pt) }
        }
        path.closeSubpath()
        p.addPath(path)
    }

    private static func font(_ name: String, _ size: CGFloat) -> CTFont { CTFontCreateWithName(name as CFString, size, nil) }

    /// Text along a circle. Letters stand on `radius` with their tops outward and run clockwise; a full-circle
    /// text (CIS seals) is spread evenly around, so the bottom half reads upside down as on real seals.
    /// `bottom` text reads left to right along the lower arc with tops toward the centre.
    private static func arcText(_ p: CGMutablePath, _ text: String, font name: String, center c: CGPoint, radius: CGFloat,
                                band: CGFloat, fullCircle: Bool = false, span: (CGFloat, CGFloat) = (0, 0),
                                bottom: Bool = false, condensed: CGFloat = 1) {
        guard !text.isEmpty else { return }
        let chars = Array(text)
        let f = font(name, band * 0.78)
        let widths = chars.map { InkDraw.width(String($0), font: f) * condensed }
        let total = widths.reduce(0, +)
        let available = fullCircle ? 2 * .pi * radius : abs(span.1 - span.0) * radius
        // Shrink when it does not fit, otherwise add letter spacing to fill the arc.
        let squeeze = min(1, available * 0.97 / max(1, total))
        let gap = fullCircle ? (available - total * squeeze) / CGFloat(chars.count)
                             : max(0, (available - total * squeeze) / CGFloat(max(1, chars.count - 1)))
        let letterGap = fullCircle ? gap : min(gap, band * 0.35)
        let used = total * squeeze + letterGap * CGFloat(fullCircle ? chars.count : chars.count - 1)
        // Clockwise from the start angle (CIS: from the left, so the text begins at about 9 o'clock).
        var angle: CGFloat
        if fullCircle { angle = .pi * 0.98 }
        else if bottom { angle = (span.0 + span.1) / 2 - used / radius / 2 }
        else { angle = (span.0 + span.1) / 2 + used / radius / 2 }
        for (i, ch) in chars.enumerated() {
            let w = widths[i] * squeeze
            let mid = bottom ? angle + w / 2 / radius : angle - w / 2 / radius
            for g in InkDraw.glyphs(String(ch), font: f) {
                let gw = g.advance
                var t = CGAffineTransform.identity
                if bottom {
                    let r = radius + band * 0.62
                    t = t.translatedBy(x: c.x + r * cos(mid), y: c.y + r * sin(mid))
                        .rotated(by: mid + .pi / 2)
                } else {
                    t = t.translatedBy(x: c.x + radius * cos(mid), y: c.y + radius * sin(mid))
                        .rotated(by: mid - .pi / 2)
                }
                t = t.scaledBy(x: squeeze * condensed, y: 1).translatedBy(x: -gw / 2, y: 0)
                if let path = g.path.copy(using: &t) { p.addPath(path) }
            }
            angle += bottom ? (w + letterGap) / radius : -(w + letterGap) / radius
        }
    }

    /// Text along an ellipse arc between parameter angles (top: tops outward, clockwise; bottom: upright).
    private static func ellipseText(_ p: CGMutablePath, _ text: String, font name: String, center c: CGPoint, a: CGFloat, b: CGFloat,
                                    band: CGFloat, from: CGFloat, to: CGFloat, bottom: Bool = false) {
        guard !text.isEmpty else { return }
        let f = font(name, band * 0.8)
        let chars = Array(text)
        let widths = chars.map { InkDraw.width(String($0), font: f) }
        // Arc length table along the parameter.
        let steps = 600
        var lengths: [CGFloat] = [0]
        func point(_ t: CGFloat) -> CGPoint { CGPoint(x: c.x + a * cos(t), y: c.y + b * sin(t)) }
        for i in 1...steps {
            let t0 = from + (to - from) * CGFloat(i - 1) / CGFloat(steps), t1 = from + (to - from) * CGFloat(i) / CGFloat(steps)
            let p0 = point(t0), p1 = point(t1)
            lengths.append(lengths[i - 1] + hypot(p1.x - p0.x, p1.y - p0.y))
        }
        let arc = lengths.last!, total = widths.reduce(0, +)
        let squeeze = min(1, arc * 0.96 / max(1, total))
        let gap = min(band * 0.3, max(0, (arc - total * squeeze) / CGFloat(max(1, chars.count - 1))))
        let used = total * squeeze + gap * CGFloat(chars.count - 1)
        var s = (arc - used) / 2
        func param(at length: CGFloat) -> CGFloat {
            let i = lengths.firstIndex { $0 >= length } ?? steps
            return from + (to - from) * CGFloat(i) / CGFloat(steps)
        }
        for (i, ch) in chars.enumerated() {
            let w = widths[i] * squeeze
            let t = param(at: s + w / 2)
            let pt = point(t)
            // Outward normal of the ellipse.
            let normal = atan2(a * sin(t), b * cos(t))
            for g in InkDraw.glyphs(String(ch), font: f) {
                var m: CGAffineTransform
                if bottom {
                    let off = band * 0.62
                    m = CGAffineTransform(translationX: pt.x + off * cos(normal), y: pt.y + off * sin(normal)).rotated(by: normal + .pi / 2)
                } else {
                    m = CGAffineTransform(translationX: pt.x, y: pt.y).rotated(by: normal - .pi / 2)
                }
                m = m.scaledBy(x: squeeze, y: 1).translatedBy(x: -g.advance / 2, y: 0)
                if let path = g.path.copy(using: &m) { p.addPath(path) }
            }
            s += w + gap
        }
    }

    /// Centred lines inside a box, each fitted to the width; the block is centred vertically.
    private static func lines(_ p: CGMutablePath, _ text: String, font name: String, center c: CGPoint, box: CGSize, maxSize: CGFloat) {
        let rows = text.split(separator: "\n").map(String.init).filter { !$0.isEmpty }
        guard !rows.isEmpty else { return }
        let lineSize = min(maxSize, box.height / CGFloat(rows.count) / 1.15)
        let fonts = rows.map { InkDraw.fit(name, $0, width: box.width, maxSize: lineSize) }
        let heights = fonts.map { CTFontGetCapHeight($0) }
        let spacing = lineSize * 0.42
        let total = heights.reduce(0, +) + spacing * CGFloat(rows.count - 1)
        var y = c.y + total / 2
        for (i, row) in rows.enumerated() {
            y -= heights[i]
            let w = InkDraw.width(row, font: fonts[i])
            for g in InkDraw.glyphs(row, font: fonts[i]) {
                var t = CGAffineTransform(translationX: c.x - w / 2 + g.position.x, y: y)
                if let path = g.path.copy(using: &t) { p.addPath(path) }
            }
            y -= spacing
        }
    }

    /// Rectangular stamp: rows of (text, font, size) stacked in the rect, optional frame.
    private static func stack(_ p: CGMutablePath, _ rows: [(String, String, CGFloat)], in rect: CGRect, alignLeft: Bool, frame: CGFloat) {
        guard !rows.isEmpty else { return }
        let inner = frame > 0 ? rect.insetBy(dx: 44, dy: 36) : rect
        let fonts = rows.map { InkDraw.fit($0.1, $0.0, width: inner.width, maxSize: $0.2) }
        let heights = fonts.map { CTFontGetCapHeight($0) }
        let gaps = fonts.map { CTFontGetSize($0) * 0.5 }
        var total = heights.reduce(0, +) + gaps.dropLast().reduce(0, +)
        var scale: CGFloat = 1
        if total > inner.height { scale = inner.height / total; total = inner.height }
        var y = inner.midY + total / 2
        var maxWidth: CGFloat = 0
        for (i, row) in rows.enumerated() {
            let f = scale < 1 ? CTFontCreateCopyWithAttributes(fonts[i], CTFontGetSize(fonts[i]) * scale, nil, nil) : fonts[i]
            y -= heights[i] * scale
            let w = InkDraw.width(row.0, font: f)
            maxWidth = max(maxWidth, w)
            let x = alignLeft ? inner.minX : inner.midX - w / 2
            for g in InkDraw.glyphs(row.0, font: f) {
                var t = CGAffineTransform(translationX: x + g.position.x, y: y)
                if let path = g.path.copy(using: &t) { p.addPath(path) }
            }
            y -= gaps[i] * scale
        }
        if frame > 0 {
            // The frame hugs the text block (a rubber stamp is cut to its content).
            let width = min(rect.width, maxWidth + 88), height = total + 72
            let box = CGRect(x: alignLeft ? inner.minX - 44 : rect.midX - width / 2, y: inner.midY - height / 2, width: width, height: height)
            p.addPath(CGPath(roundedRect: box, cornerWidth: 18, cornerHeight: 18, transform: nil)
                .copy(strokingWithWidth: frame, lineCap: .round, lineJoin: .round, miterLimit: 10))
        }
    }

    /// Hanko: characters in vertical columns, right to left.
    private static func vertical(_ p: CGMutablePath, _ text: String, font name: String, center c: CGPoint, box: CGSize) {
        let chars = Array(text.filter { !$0.isWhitespace })
        guard !chars.isEmpty else { return }
        let perColumn = chars.count <= 4 ? chars.count : Int((Double(chars.count) / 2).rounded(.up))
        let columns = Int((Double(chars.count) / Double(perColumn)).rounded(.up))
        let cell = min(box.height / CGFloat(perColumn), box.width / CGFloat(columns))
        let f = font(name, cell * 0.92)
        for (i, ch) in chars.enumerated() {
            let col = i / perColumn, row = i % perColumn
            let x = c.x + (CGFloat(columns) / 2 - CGFloat(col) - 0.5) * cell
            let y = c.y + (CGFloat(perColumn) / 2 - CGFloat(row) - 1) * cell + cell * 0.14
            for g in InkDraw.glyphs(String(ch), font: f) {
                var t = CGAffineTransform(translationX: x - g.advance / 2, y: y)
                if let path = g.path.copy(using: &t) { p.addPath(path) }
            }
        }
    }
}
