import AppKit
import CoreImage
import CoreText

// Shared drawing helpers for the generated signatures and stamps: seeded randomness, glyph outlines,
// pen strokes with varying width, and an ink texture that makes a vector drawing look printed by hand.

/// Deterministic random numbers (SplitMix64): the same seed always gives the same signature or impression.
public struct SeededRandom {
    private var state: UInt64

    public init(_ seed: UInt64) { state = seed &+ 0x9E37_79B9_7F4A_7C15 }

    public mutating func next() -> UInt64 {
        state &+= 0x9E37_79B9_7F4A_7C15
        var z = state
        z = (z ^ (z >> 30)) &* 0xBF58_476D_1CE4_E5B9
        z = (z ^ (z >> 27)) &* 0x94D0_49BB_1331_11EB
        return z ^ (z >> 31)
    }

    /// Uniform value in 0..<1.
    public mutating func unit() -> CGFloat { CGFloat(next() >> 11) / CGFloat(1 << 53) }

    public mutating func range(_ r: ClosedRange<CGFloat>) -> CGFloat { r.lowerBound + unit() * (r.upperBound - r.lowerBound) }

    public mutating func chance(_ p: CGFloat) -> Bool { unit() < p }

    public mutating func pick<T>(_ items: [T]) -> T { items[Int(next() % UInt64(items.count))] }

    /// Index chosen with the given relative weights.
    public mutating func weighted(_ weights: [CGFloat]) -> Int {
        var x = unit() * weights.reduce(0, +)
        for (i, w) in weights.enumerated() {
            if x < w { return i }
            x -= w
        }
        return weights.count - 1
    }

    public static func freshSeed() -> UInt64 { UInt64.random(in: 1...UInt64.max) }
}

/// Ink colours offered for signatures and stamps; any other colour can be chosen too.
public enum InkColor: Int, CaseIterable {
    case blue, violet, black, red, green

    public var title: String {
        switch self {
        case .blue: return L("Blue ink")
        case .violet: return L("Violet ink")
        case .black: return L("Black ink")
        case .red: return L("Red ink")
        case .green: return L("Green ink")
        }
    }

    /// Ballpoint pen colours (signatures).
    public var pen: NSColor {
        switch self {
        case .blue: return NSColor(srgbRed: 0.10, green: 0.20, blue: 0.56, alpha: 1)
        case .violet: return NSColor(srgbRed: 0.30, green: 0.16, blue: 0.55, alpha: 1)
        case .black: return NSColor(srgbRed: 0.11, green: 0.11, blue: 0.13, alpha: 1)
        case .red: return NSColor(srgbRed: 0.72, green: 0.10, blue: 0.12, alpha: 1)
        case .green: return NSColor(srgbRed: 0.05, green: 0.40, blue: 0.22, alpha: 1)
        }
    }

    /// Stamp-pad colours (seals): lighter and more saturated than pen ink.
    public var stampPad: NSColor {
        switch self {
        case .blue: return NSColor(srgbRed: 0.16, green: 0.31, blue: 0.70, alpha: 1)
        case .violet: return NSColor(srgbRed: 0.38, green: 0.25, blue: 0.66, alpha: 1)
        case .black: return NSColor(srgbRed: 0.15, green: 0.15, blue: 0.17, alpha: 1)
        case .red: return NSColor(srgbRed: 0.80, green: 0.13, blue: 0.15, alpha: 1)
        case .green: return NSColor(srgbRed: 0.10, green: 0.48, blue: 0.27, alpha: 1)
        }
    }

    /// Small swatch for pop-up menus.
    public static func swatch(_ color: NSColor) -> NSImage {
        NSImage(size: NSSize(width: 14, height: 14), flipped: false) { rect in
            color.setFill()
            NSBezierPath(ovalIn: rect.insetBy(dx: 1, dy: 1)).fill()
            NSColor.black.withAlphaComponent(0.2).setStroke()
            NSBezierPath(ovalIn: rect.insetBy(dx: 1, dy: 1)).stroke()
            return true
        }
    }
}

/// One laid-out glyph: its outline in font units at the line origin, and where CoreText placed it.
struct LaidGlyph {
    let path: CGPath
    let position: CGPoint
    let advance: CGFloat
}

enum InkDraw {
    /// Glyph outlines of a line of text (with shaping, ligatures and font fallback for missing characters).
    static func glyphs(_ text: String, font: CTFont) -> [LaidGlyph] {
        let line = CTLineCreateWithAttributedString(NSAttributedString(string: text, attributes: [.font: font]))
        var out: [LaidGlyph] = []
        for run in CTLineGetGlyphRuns(line) as? [CTRun] ?? [] {
            let attributes = CTRunGetAttributes(run) as NSDictionary
            let runFont = attributes[kCTFontAttributeName] as! CTFont
            let n = CTRunGetGlyphCount(run)
            var glyphs = [CGGlyph](repeating: 0, count: n)
            var positions = [CGPoint](repeating: .zero, count: n)
            var advances = [CGSize](repeating: .zero, count: n)
            CTRunGetGlyphs(run, CFRange(location: 0, length: n), &glyphs)
            CTRunGetPositions(run, CFRange(location: 0, length: n), &positions)
            CTRunGetAdvances(run, CFRange(location: 0, length: n), &advances)
            for i in 0..<n {
                let path = CTFontCreatePathForGlyph(runFont, glyphs[i], nil) ?? CGMutablePath()
                out.append(LaidGlyph(path: path, position: positions[i], advance: advances[i].width))
            }
        }
        return out
    }

    static func width(_ text: String, font: CTFont) -> CGFloat {
        let line = CTLineCreateWithAttributedString(NSAttributedString(string: text, attributes: [.font: font]))
        return CGFloat(CTLineGetTypographicBounds(line, nil, nil, nil))
    }

    /// Whether the font itself has every character (no fallback), so a script font is only used where it fits.
    static func covers(_ font: CTFont, _ text: String) -> Bool {
        var chars = Array(text.filter { !$0.isWhitespace && $0 != "." && $0 != "-" }.utf16)
        guard !chars.isEmpty else { return true }
        var glyphs = [CGGlyph](repeating: 0, count: chars.count)
        return CTFontGetGlyphsForCharacters(font, &chars, &glyphs, chars.count)
    }

    /// Largest font size (≤ maxSize) at which the text fits the width.
    static func fit(_ name: String, _ text: String, width: CGFloat, maxSize: CGFloat, minSize: CGFloat = 6) -> CTFont {
        var size = maxSize
        var font = CTFontCreateWithName(name as CFString, size, nil)
        let w = self.width(text, font: font)
        if w > width, w > 0 {
            size = max(minSize, size * width / w)
            font = CTFontCreateWithName(name as CFString, size, nil)
        }
        return font
    }

    /// A pen stroke along the points: a filled outline whose width follows `width(t)`, t in 0...1.
    static func taperedStroke(_ points: [CGPoint], width: (CGFloat) -> CGFloat) -> CGPath {
        let path = CGMutablePath()
        guard points.count > 1 else { return path }
        var left: [CGPoint] = [], right: [CGPoint] = []
        for (i, p) in points.enumerated() {
            let a = points[max(0, i - 1)], b = points[min(points.count - 1, i + 1)]
            var dx = b.x - a.x, dy = b.y - a.y
            let len = max(0.0001, (dx * dx + dy * dy).squareRoot())
            dx /= len; dy /= len
            let w = width(CGFloat(i) / CGFloat(points.count - 1)) / 2
            left.append(CGPoint(x: p.x - dy * w, y: p.y + dx * w))
            right.append(CGPoint(x: p.x + dy * w, y: p.y - dx * w))
        }
        path.addLines(between: left + right.reversed())
        path.closeSubpath()
        return path
    }

    static func cubic(_ p0: CGPoint, _ p1: CGPoint, _ p2: CGPoint, _ p3: CGPoint, steps: Int = 64) -> [CGPoint] {
        (0...steps).map { i in
            let t = CGFloat(i) / CGFloat(steps), u = 1 - t
            let a = u * u * u, b = 3 * u * u * t, c = 3 * u * t * t, d = t * t * t
            return CGPoint(x: a * p0.x + b * p1.x + c * p2.x + d * p3.x, y: a * p0.y + b * p1.y + c * p2.y + d * p3.y)
        }
    }

    /// Copies a path, moving every point through `f` (hand wobble, slant).
    static func warp(_ path: CGPath, _ f: @escaping (CGPoint) -> CGPoint) -> CGPath {
        let out = CGMutablePath()
        path.applyWithBlock { element in
            let e = element.pointee
            switch e.type {
            case .moveToPoint: out.move(to: f(e.points[0]))
            case .addLineToPoint: out.addLine(to: f(e.points[0]))
            case .addQuadCurveToPoint: out.addQuadCurve(to: f(e.points[1]), control: f(e.points[0]))
            case .addCurveToPoint: out.addCurve(to: f(e.points[2]), control1: f(e.points[0]), control2: f(e.points[1]))
            case .closeSubpath: out.closeSubpath()
            @unknown default: break
            }
        }
        return out
    }

    /// Transparent RGBA bitmap context of the given pixel size.
    static func context(_ width: Int, _ height: Int) -> CGContext? {
        CGContext(data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4,
                  space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
    }

    /// Crops fully transparent margins, keeping `pad` pixels.
    static func trim(_ image: CGImage, pad: Int = 6) -> CGImage {
        let w = image.width, h = image.height
        guard let ctx = context(w, h) else { return image }
        ctx.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
        guard let data = ctx.data?.assumingMemoryBound(to: UInt8.self) else { return image }
        var minX = w, minY = h, maxX = -1, maxY = -1
        for y in 0..<h {
            let row = data + y * w * 4
            for x in 0..<w where row[x * 4 + 3] > 8 {
                minX = min(minX, x); maxX = max(maxX, x); minY = min(minY, y); maxY = max(maxY, y)
            }
        }
        guard maxX >= minX, maxY >= minY else { return image }
        // Bitmap rows run top-down in memory for this context, matching CGImage cropping coordinates.
        let rect = CGRect(x: max(0, minX - pad), y: max(0, minY - pad),
                          width: min(w, maxX + pad + 1) - max(0, minX - pad), height: min(h, maxY + pad + 1) - max(0, minY - pad))
        return ctx.makeImage()?.cropping(to: rect) ?? image
    }
}

/// Makes a clean vector drawing look like real ink on paper: uneven pressure, grain, small gaps and slight bleed.
enum InkTexture {
    struct Strength {
        /// How much the overall pressure varies (0 = even, 1 = some areas almost blank).
        var pressure: CGFloat
        /// Share of fine gaps where the pad did not touch the paper.
        var gaps: CGFloat
        /// Edge softening in pixels.
        var bleed: CGFloat
        /// Pen only: pressure that rises and falls along the writing direction (left to right).
        var alongX: CGFloat = 0
        /// Paper fibres: fine elongated texture that takes up less ink (0 = smooth paper).
        var paper: CGFloat = 0
    }

    static let stamp = Strength(pressure: 0.55, gaps: 0.30, bleed: 0.9)

    /// Smooth value noise in 0...1.
    private struct Noise {
        let seed: UInt64
        let cell: CGFloat

        func hash(_ x: Int, _ y: Int) -> CGFloat {
            var h = UInt64(bitPattern: Int64(x)) &* 0x9E37_79B9_7F4A_7C15 ^ UInt64(bitPattern: Int64(y)) &* 0xC2B2_AE3D_27D4_EB4F ^ seed
            h = (h ^ (h >> 31)) &* 0xBF58_476D_1CE4_E5B9
            h ^= h >> 29
            return CGFloat(h & 0xFFFF) / 65535
        }

        func value(_ px: CGFloat, _ py: CGFloat) -> CGFloat {
            let x = px / cell, y = py / cell
            let x0 = Int(x.rounded(.down)), y0 = Int(y.rounded(.down))
            var fx = x - CGFloat(x0), fy = y - CGFloat(y0)
            fx = fx * fx * (3 - 2 * fx); fy = fy * fy * (3 - 2 * fy)
            let a = hash(x0, y0), b = hash(x0 + 1, y0), c = hash(x0, y0 + 1), d = hash(x0 + 1, y0 + 1)
            return (a + (b - a) * fx) * (1 - fy) + (c + (d - c) * fx) * fy
        }
    }

    /// `scale`: pixels per pixel of the base rendering. Fine detail (paper fibres, the finest grain, bleed) is sized in
    /// base pixels, so a high-resolution impression looks the same, only sharper.
    static func apply(_ image: CGImage, seed: UInt64, strength: Strength, scale: CGFloat = 1) -> CGImage {
        let w = image.width, h = image.height
        guard let ctx = InkDraw.context(w, h) else { return image }
        ctx.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
        guard let data = ctx.data?.assumingMemoryBound(to: UInt8.self) else { return image }
        var rng = SeededRandom(seed)
        let size = CGFloat(max(w, h))
        let pressure = Noise(seed: rng.next(), cell: size / 3.5)
        let mottle = Noise(seed: rng.next(), cell: size / 28)
        let grain = Noise(seed: rng.next(), cell: max(2.2 * scale, size / 260))
        let stroke = Noise(seed: rng.next(), cell: CGFloat(w) / 7)
        let fibre = Noise(seed: rng.next(), cell: 2.2 * scale)
        let fibreAngle = rng.range(0...(.pi)), fc = cos(fibreAngle), fs = sin(fibreAngle)
        // The pad presses harder on one side: a gentle gradient in a random direction.
        let angle = rng.range(0...(2 * .pi)), tilt = rng.range(0.15...0.45) * strength.pressure
        let gx = cos(angle), gy = sin(angle)
        for y in 0..<h {
            let row = data + y * w * 4
            let fy = CGFloat(y)
            for x in 0..<w {
                let a = row[x * 4 + 3]
                guard a > 0 else { continue }
                let fx = CGFloat(x)
                let side = ((fx / size - 0.5) * gx + (fy / size - 0.5) * gy) * 2   // -1...1 across the image
                var k = 1 - strength.pressure * 0.55 * (1 - pressure.value(fx, fy)) - tilt * max(0, side)
                k -= strength.pressure * 0.25 * (1 - mottle.value(fx, fy))
                if strength.alongX > 0 { k -= strength.alongX * (1 - stroke.value(fx, 0.5)) }
                if strength.paper > 0 {
                    // Fibres run in one direction: noise stretched ten times along it.
                    let u = (fx * fc + fy * fs) * 0.1, v = -fx * fs + fy * fc
                    k -= strength.paper * (1 - fibre.value(u, v))
                }
                let g = grain.value(fx, fy)
                let gapLimit = strength.gaps * (0.55 + 0.9 * (1 - k))
                if g < gapLimit { k *= 0.12 + g / max(0.001, gapLimit) * 0.5 }
                k = min(1, max(0.05, k * rng.range(0.94...1.0)))
                for c in 0..<4 { row[x * 4 + c] = UInt8(CGFloat(row[x * 4 + c]) * k) }
            }
        }
        guard let textured = ctx.makeImage() else { return image }
        return blur(textured, radius: strength.bleed * scale) ?? textured
    }

    static func blur(_ image: CGImage, radius: CGFloat) -> CGImage? {
        guard radius > 0 else { return image }
        let input = CIImage(cgImage: image)
        let blurred = input.clampedToExtent().applyingGaussianBlur(sigma: Double(radius)).cropped(to: input.extent)
        return CIContext(options: [.workingColorSpace: CGColorSpace(name: CGColorSpace.sRGB)!])
            .createCGImage(blurred, from: input.extent)
    }
}
