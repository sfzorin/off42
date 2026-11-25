import AppKit
import Kit42

/// Markup tools, as in Preview's markup bar.
enum MarkTool: Int, CaseIterable {
    case select
    // Selection tools (pixels)
    case selectRect, selectEllipse, selectLasso, smartLasso, instantAlpha
    // Drawing
    case sketch, draw, highlighter
    // Shapes
    case line, arrow, rectangle, roundedRect, oval, bubble, star, polygon, loupe, spotlight, pixelate
    case text

    var title: String {
        switch self {
        case .select: return L("Select and Move")
        case .selectRect: return L("Rectangular Selection")
        case .selectEllipse: return L("Elliptical Selection")
        case .selectLasso: return L("Lasso Selection")
        case .smartLasso: return L("Smart Lasso")
        case .instantAlpha: return L("Instant Alpha")
        case .sketch: return L("Sketch")
        case .draw: return L("Draw")
        case .highlighter: return L("Highlight")
        case .line: return L("Line")
        case .arrow: return L("Arrow")
        case .rectangle: return L("Rectangle")
        case .roundedRect: return L("Rounded Rectangle")
        case .oval: return L("Oval")
        case .bubble: return L("Speech Bubble")
        case .star: return L("Star")
        case .polygon: return L("Polygon")
        case .loupe: return L("Loupe")
        case .spotlight: return L("Spotlight")
        case .pixelate: return L("Pixelate")
        case .text: return L("Text")
        }
    }

    var symbol: String {
        switch self {
        case .select: return "cursorarrow"
        case .selectRect: return "rectangle.dashed"
        case .selectEllipse: return "circle.dashed"
        case .selectLasso, .smartLasso: return "lasso"
        case .instantAlpha: return "wand.and.rays"
        case .sketch: return "pencil.tip"
        case .draw: return "scribble"
        case .highlighter: return "highlighter"
        case .line: return "line.diagonal"
        case .arrow: return "arrow.up.right"
        case .rectangle: return "rectangle"
        case .roundedRect: return "app"
        case .oval: return "circle"
        case .bubble: return "bubble.left"
        case .star: return "star"
        case .polygon: return "hexagon"
        case .loupe: return "plus.magnifyingglass"
        case .spotlight: return "circle.dotted.and.circle"
        case .pixelate: return "checkerboard.rectangle"
        case .text: return "character.textbox"
        }
    }

    static let selectionTools: [MarkTool] = [.selectRect, .selectEllipse, .selectLasso, .smartLasso]
    static let shapeTools: [MarkTool] = [.line, .arrow, .rectangle, .roundedRect, .oval, .bubble, .star, .polygon, .loupe, .spotlight, .pixelate]

    var isSelectionTool: Bool { Self.selectionTools.contains(self) || self == .instantAlpha }

    var markKind: Mark.Kind? {
        switch self {
        case .sketch, .draw: return .pen
        case .highlighter: return .highlighter
        case .line, .arrow: return .line
        case .rectangle: return .rectangle
        case .roundedRect: return .roundedRect
        case .oval: return .oval
        case .bubble: return .bubble
        case .star: return .star
        case .polygon: return .polygon
        case .loupe: return .loupe
        case .spotlight: return .spotlight
        case .pixelate: return .pixelate
        case .text: return .text
        default: return nil
        }
    }
}

/// Style shared by new marks and applied to the selected one.
struct MarkStyle {
    var stroke: NSColor = .systemRed
    var fill: NSColor?
    var width: CGFloat = 3          // screen points at the zoom the mark was drawn at
    var dashed = false
    var shadow = false
    var arrowStart = false
    var arrowEnd = true
    var fontName = "Helvetica Neue"
    var fontSize: CGFloat = 24      // screen points
    var bold = false
    var italic = false
    var underline = false
    var alignment: NSTextAlignment = .left
    var textColor: NSColor = .black
    var loupeZoom: CGFloat = 2
}

/// One mark on the image, in image pixel coordinates (origin top left).
struct Mark {
    enum Kind { case pen, highlighter, line, rectangle, roundedRect, oval, bubble, star, polygon, loupe, spotlight, pixelate, text, image }

    var id = UUID()
    var kind: Kind
    var color: NSColor = .systemRed
    var fill: NSColor?
    var width: CGFloat = 4
    var dashed = false
    var shadow = false
    var arrowStart = false
    var arrowEnd = false
    var points: [CGPoint] = []
    var rect: CGRect = .zero
    var text = ""
    var fontName = "Helvetica Neue"
    var fontSize: CGFloat = 32
    var bold = false
    var italic = false
    var underline = false
    var alignment: NSTextAlignment = .left
    var textColor: NSColor = .black
    var image: CGImage?
    var zoom: CGFloat = 2

    var isRectBased: Bool { !isLineBased && kind != .pen && kind != .highlighter }
    var isLineBased: Bool { kind == .line }
    var keepsAspect: Bool { kind == .image || kind == .loupe }
    var supportsStroke: Bool { ![.image, .spotlight, .pixelate].contains(kind) }
    var supportsFill: Bool { [.rectangle, .roundedRect, .oval, .bubble, .star, .polygon, .text].contains(kind) }
    var supportsDash: Bool { supportsStroke && kind != .highlighter && kind != .loupe }
    var supportsShadow: Bool { kind != .spotlight }

    /// Used by editing commands to avoid creating Undo entries for clicks or unchanged styles/text.
    func hasSameContent(as other: Mark) -> Bool {
        func close(_ a: CGFloat, _ b: CGFloat) -> Bool { abs(a - b) < 1e-7 }
        func samePoint(_ a: CGPoint, _ b: CGPoint) -> Bool { close(a.x, b.x) && close(a.y, b.y) }
        return id == other.id && kind == other.kind && color.isEqual(other.color) && fill == other.fill
            && width == other.width && dashed == other.dashed && shadow == other.shadow
            && arrowStart == other.arrowStart && arrowEnd == other.arrowEnd
            && points.count == other.points.count && zip(points, other.points).allSatisfy({ samePoint($0.0, $0.1) })
            && samePoint(rect.origin, other.rect.origin) && close(rect.width, other.rect.width) && close(rect.height, other.rect.height) && text == other.text
            && fontName == other.fontName && fontSize == other.fontSize && bold == other.bold
            && italic == other.italic && underline == other.underline && alignment == other.alignment
            && textColor.isEqual(other.textColor) && image === other.image && zoom == other.zoom
    }

    var bounds: CGRect {
        if isRectBased { return rect.standardized }
        guard let first = points.first else { return .zero }
        var r = CGRect(origin: first, size: .zero)
        for p in points { r = r.union(CGRect(origin: p, size: .zero)) }
        let pad = (arrowStart || arrowEnd ? width * 4 : kind == .highlighter ? width * 2 : width) / 2 + 1
        return r.insetBy(dx: -pad, dy: -pad)
    }

    func applying(_ t: CGAffineTransform) -> Mark {
        var m = self
        m.points = points.map { $0.applying(t) }
        m.rect = rect.applying(t).standardized
        let scale = sqrt(abs(t.a * t.d - t.b * t.c))
        m.width *= scale
        m.fontSize *= scale
        if kind == .text {
            // Text stays horizontal: keep its centre, scale its box.
            let c = CGPoint(x: m.rect.midX, y: m.rect.midY)
            let size = m.textSize(width: rect.width * scale)
            m.rect = CGRect(x: c.x - size.width / 2, y: c.y - size.height / 2, width: size.width, height: size.height)
        }
        if kind == .image, let img = image {
            if t.b != 0 { m.image = Orientation.apply(t.b > 0 ? .right : .left, to: img) }
            else if t.a < 0 { m.image = Orientation.apply(.upMirrored, to: img) }
            else if t.d < 0 { m.image = Orientation.apply(.downMirrored, to: img) }
        }
        return m
    }

    // MARK: Text

    var font: NSFont {
        var font = NSFont(name: fontName, size: fontSize) ?? NSFont(name: "Helvetica Neue", size: fontSize) ?? .systemFont(ofSize: fontSize)
        var traits: NSFontTraitMask = []
        if bold { traits.insert(.boldFontMask) }
        if italic { traits.insert(.italicFontMask) }
        if !traits.isEmpty { font = NSFontManager.shared.convert(font, toHaveTrait: traits) }
        return font
    }

    var textAttributes: [NSAttributedString.Key: Any] {
        let p = NSMutableParagraphStyle()
        p.alignment = alignment
        var a: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: textColor, .paragraphStyle: p]
        if underline { a[.underlineStyle] = NSUnderlineStyle.single.rawValue }
        return a
    }

    var textPadding: CGFloat { fontSize * 0.25 }

    /// Box size for the text; `width` keeps a box the user widened.
    func textSize(width: CGFloat? = nil) -> CGSize {
        let s = ((text.isEmpty ? "W" : text) as NSString).boundingRect(with: CGSize(width: width.map { max(10, $0 - textPadding * 2) } ?? 100_000, height: 100_000),
                                                                      options: [.usesLineFragmentOrigin], attributes: textAttributes).size
        let w = max(width ?? 0, ceil(s.width) + textPadding * 2)
        return CGSize(width: w, height: ceil(s.height) + textPadding * 2)
    }

    // MARK: Hit testing

    func hitTest(_ p: CGPoint, tolerance: CGFloat) -> Bool {
        switch kind {
        case .rectangle, .roundedRect, .oval, .star, .polygon, .bubble, .spotlight:
            let r = rect.standardized
            guard r.insetBy(dx: -tolerance - width / 2, dy: -tolerance - width / 2).contains(p) else { return false }
            guard let path = shapePath() else { return false }
            if (fill != nil || kind == .spotlight) && path.contains(p) { return true }
            return path.cgPath.copy(strokingWithWidth: max(0, width) + tolerance * 2,
                                    lineCap: .round, lineJoin: .round, miterLimit: 10).contains(p)
        case .text, .image, .pixelate, .loupe:
            return rect.standardized.insetBy(dx: -tolerance, dy: -tolerance).contains(p)
        case .pen, .highlighter, .line:
            let tol = tolerance + (kind == .highlighter ? width * 2 : width / 2)
            if points.count == 1 { return hypot(points[0].x - p.x, points[0].y - p.y) <= tol }
            for i in 1..<max(1, points.count) where Self.distance(p, points[i - 1], points[i]) <= tol { return true }
            return false
        }
    }

    static func distance(_ p: CGPoint, _ a: CGPoint, _ b: CGPoint) -> CGFloat {
        let dx = b.x - a.x, dy = b.y - a.y
        let len2 = dx * dx + dy * dy
        guard len2 > 0 else { return hypot(p.x - a.x, p.y - a.y) }
        let t = max(0, min(1, ((p.x - a.x) * dx + (p.y - a.y) * dy) / len2))
        return hypot(p.x - (a.x + t * dx), p.y - (a.y + t * dy))
    }

    // MARK: Shapes

    func shapePath() -> NSBezierPath? {
        let r = rect.standardized
        switch kind {
        case .rectangle: return NSBezierPath(rect: r)
        case .roundedRect:
            let rad = min(r.width, r.height) * 0.18
            return NSBezierPath(roundedRect: r, xRadius: rad, yRadius: rad)
        case .oval, .loupe, .spotlight: return NSBezierPath(ovalIn: r)
        case .star: return Self.polygonPath(in: r, points: 5, inner: 0.42)
        case .polygon: return Self.polygonPath(in: r, points: 6, inner: nil)
        case .bubble:
            // Rounded body over the top 78 %, tail at the bottom left.
            let body = CGRect(x: r.minX, y: r.minY, width: r.width, height: r.height * 0.78)
            let rad = min(body.width, body.height) * 0.3
            let path = NSBezierPath(roundedRect: body, xRadius: rad, yRadius: rad)
            let tail = NSBezierPath()
            tail.move(to: CGPoint(x: body.minX + body.width * 0.22, y: body.maxY - 1))
            tail.line(to: CGPoint(x: body.minX + body.width * 0.12, y: r.maxY))
            tail.line(to: CGPoint(x: body.minX + body.width * 0.42, y: body.maxY - 1))
            tail.close()
            path.append(tail)
            path.windingRule = .nonZero
            return path
        default: return nil
        }
    }

    static func polygonPath(in r: CGRect, points n: Int, inner: CGFloat?) -> NSBezierPath {
        let path = NSBezierPath()
        let count = inner == nil ? n : n * 2
        for i in 0..<count {
            let angle = -CGFloat.pi / 2 + CGFloat(i) * 2 * .pi / CGFloat(count)
            let k: CGFloat = inner != nil && i % 2 == 1 ? inner! : 1
            let p = CGPoint(x: r.midX + cos(angle) * r.width / 2 * k, y: r.midY + sin(angle) * r.height / 2 * k)
            if i == 0 { path.move(to: p) } else { path.line(to: p) }
        }
        path.close()
        return path
    }

    // MARK: Drawing (flipped NSGraphicsContext in image coordinates)

    func draw(base: CGImage?, canvas: CGSize) {
        NSGraphicsContext.saveGraphicsState()
        defer { NSGraphicsContext.restoreGraphicsState() }
        if shadow, kind != .spotlight {
            let s = NSShadow()
            s.shadowColor = NSColor.black.withAlphaComponent(0.45)
            s.shadowBlurRadius = max(2, width * 1.5)
            s.shadowOffset = NSSize(width: width * 0.6, height: -width * 0.6)
            s.set()
        }
        let dash: [CGFloat] = [width * 2.5, width * 1.8]
        func stroke(_ path: NSBezierPath) {
            path.lineWidth = width
            path.lineJoinStyle = .round
            path.lineCapStyle = dashed ? .butt : .round
            if dashed { path.setLineDash(dash, count: 2, phase: 0) }
            color.setStroke()
            path.stroke()
        }
        switch kind {
        case .pen, .highlighter:
            guard !points.isEmpty else { return }
            let path = Self.smoothPath(points)
            if kind == .highlighter {
                path.lineWidth = width * 4
                path.lineCapStyle = .round
                path.lineJoinStyle = .round
                NSGraphicsContext.current?.cgContext.setBlendMode(.multiply)
                color.withAlphaComponent(0.5).setStroke()
                path.stroke()
            } else {
                stroke(path)
            }
        case .line:
            guard points.count >= 2 else { return }
            var a = points[0], b = points[1]
            let head = max(width * 4, 10)
            let angle = atan2(b.y - a.y, b.x - a.x)
            color.setFill()
            if arrowEnd {
                Self.arrowHead(at: b, angle: angle, size: head).fill()
                b = CGPoint(x: b.x - cos(angle) * head * 0.8, y: b.y - sin(angle) * head * 0.8)
            }
            if arrowStart {
                Self.arrowHead(at: a, angle: angle + .pi, size: head).fill()
                a = CGPoint(x: a.x + cos(angle) * head * 0.8, y: a.y + sin(angle) * head * 0.8)
            }
            let path = NSBezierPath()
            path.move(to: a)
            path.line(to: b)
            stroke(path)
        case .rectangle, .roundedRect, .oval, .star, .polygon, .bubble:
            guard let path = shapePath() else { return }
            if let fill { fill.setFill(); path.fill(); NSShadow().set() }
            stroke(path)
        case .spotlight:
            let outside = NSBezierPath(rect: CGRect(origin: .zero, size: canvas))
            if let hole = shapePath() { outside.append(hole.reversed) }
            outside.windingRule = .evenOdd
            NSColor.black.withAlphaComponent(0.55).setFill()
            outside.fill()
        case .loupe:
            let r = rect.standardized
            let circle = NSBezierPath(ovalIn: r)
            if let base {
                NSGraphicsContext.saveGraphicsState()
                circle.addClip()
                // The part of the image under the loupe, enlarged `zoom` times around its centre.
                let src = CGRect(x: r.midX - r.width / 2 / zoom, y: r.midY - r.height / 2 / zoom, width: r.width / zoom, height: r.height / zoom)
                NSImage(cgImage: base, size: .zero).draw(in: r, from: CGRect(x: src.minX, y: CGFloat(base.height) - src.maxY, width: src.width, height: src.height),
                                                         operation: .copy, fraction: 1, respectFlipped: true, hints: [.interpolation: NSImageInterpolation.high])
                NSGraphicsContext.restoreGraphicsState()
            }
            circle.lineWidth = max(width, 2)
            color.setStroke()
            circle.stroke()
        case .text:
            let r = rect.standardized
            let box = NSBezierPath(rect: r)
            if let fill { fill.setFill(); box.fill(); NSShadow().set() }
            if color.alphaComponent > 0 { stroke(box) }
            (text as NSString).draw(with: r.insetBy(dx: textPadding, dy: textPadding), options: [.usesLineFragmentOrigin], attributes: textAttributes)
        case .image:
            guard let image else { return }
            // Signatures and stamps are ink: multiply with the picture under them.
            NSImage(cgImage: image, size: .zero).draw(in: rect.standardized, from: .zero, operation: .multiply, fraction: 1,
                                                      respectFlipped: true, hints: [.interpolation: NSImageInterpolation.high])
        case .pixelate:
            // CGImage clips a crop to its image. Use that same visible rectangle when sampling and
            // drawing, or a partly off-canvas mark stretches the clipped pixels over its original box.
            let r = rect.standardized.integral.intersection(CGRect(origin: .zero, size: canvas))
            guard !r.isNull, !r.isEmpty else { return }
            guard let base, let crop = base.cropping(to: r), r.width >= 2, r.height >= 2 else {
                NSColor.gray.setFill()
                r.fill()
                return
            }
            let block = max(6, min(r.width, r.height) / 8)
            let small = CGSize(width: max(1, (r.width / block).rounded()), height: max(1, (r.height / block).rounded()))
            guard let tiny = ImageLoader.render(size: small, { ctx in
                ctx.interpolationQuality = .medium
                ctx.draw(crop, in: CGRect(origin: .zero, size: small))
            }) else { return }
            NSGraphicsContext.current?.imageInterpolation = .none
            NSImage(cgImage: tiny, size: .zero).draw(in: r, from: .zero, operation: .copy, fraction: 1, respectFlipped: true,
                                                     hints: [.interpolation: NSImageInterpolation.none])
        }
    }

    static func arrowHead(at tip: CGPoint, angle: CGFloat, size: CGFloat) -> NSBezierPath {
        let tri = NSBezierPath()
        tri.move(to: tip)
        tri.line(to: CGPoint(x: tip.x - size * cos(angle - .pi / 7), y: tip.y - size * sin(angle - .pi / 7)))
        tri.line(to: CGPoint(x: tip.x - size * cos(angle + .pi / 7), y: tip.y - size * sin(angle + .pi / 7)))
        tri.close()
        return tri
    }

    static func smoothPath(_ pts: [CGPoint]) -> NSBezierPath {
        let path = NSBezierPath()
        path.move(to: pts[0])
        if pts.count == 1 {
            path.line(to: pts[0])
            return path
        }
        for i in 1..<pts.count {
            let mid = CGPoint(x: (pts[i - 1].x + pts[i].x) / 2, y: (pts[i - 1].y + pts[i].y) / 2)
            if i == 1 { path.line(to: mid) } else { path.curve(to: mid, controlPoint1: pts[i - 1], controlPoint2: pts[i - 1]) }
        }
        path.line(to: pts[pts.count - 1])
        return path
    }

    /// Flattens marks onto an image of the same pixel size.
    static func flatten(_ marks: [Mark], onto image: CGImage) -> CGImage {
        flattenChecked(marks, onto: image) ?? image
    }

    /// Saving/export must fail when rendering cannot allocate its output, instead of dropping the marks.
    static func flattenChecked(_ marks: [Mark], onto image: CGImage) -> CGImage? {
        guard !marks.isEmpty else { return image }
        let size = CGSize(width: image.width, height: image.height)
        let cs = image.colorSpace.flatMap { $0.model == .rgb ? $0 : nil }
        return ImageLoader.render(size: size, colorSpace: cs) { ctx in
            ctx.draw(image, in: CGRect(origin: .zero, size: size))
            ctx.translateBy(x: 0, y: size.height)
            ctx.scaleBy(x: 1, y: -1)
            NSGraphicsContext.saveGraphicsState()
            NSGraphicsContext.current = NSGraphicsContext(cgContext: ctx, flipped: true)
            for m in marks { m.draw(base: image, canvas: size) }
            NSGraphicsContext.restoreGraphicsState()
        }
    }

    // MARK: Sketch recognition

    /// Turns a freehand stroke into a clean line, rectangle, oval or star when it resembles one.
    static func recognize(_ stroke: Mark) -> Mark {
        let pts = stroke.points
        guard pts.count >= 6 else { return stroke }
        var length: CGFloat = 0
        for i in 1..<pts.count { length += hypot(pts[i].x - pts[i - 1].x, pts[i].y - pts[i - 1].y) }
        let first = pts[0], last = pts[pts.count - 1]
        let chord = hypot(last.x - first.x, last.y - first.y)
        var box = CGRect(origin: first, size: .zero)
        for p in pts { box = box.union(CGRect(origin: p, size: .zero)) }
        let diag = hypot(box.width, box.height)
        guard diag > 4 else { return stroke }
        var m = stroke
        if chord / max(length, 1) > 0.92 {
            m.kind = .line
            m.points = [first, last]
            return m
        }
        let closed = chord < diag * 0.25
        guard closed, box.width > 8, box.height > 8 else { return stroke }
        // Oval: points stay near the inscribed ellipse.
        let ovalError = pts.map { p -> CGFloat in
            let nx = (p.x - box.midX) / (box.width / 2), ny = (p.y - box.midY) / (box.height / 2)
            return abs(sqrt(nx * nx + ny * ny) - 1)
        }.reduce(0, +) / CGFloat(pts.count)
        // Rectangle: points stay near the bounding box edges.
        let rectError = pts.map { p -> CGFloat in
            min(abs(p.x - box.minX), abs(p.x - box.maxX), abs(p.y - box.minY), abs(p.y - box.maxY)) / min(box.width, box.height)
        }.reduce(0, +) / CGFloat(pts.count)
        // Star: many sharp turns.
        var turns = 0
        let step = max(1, pts.count / 60)
        var prevAngle: CGFloat?
        for i in stride(from: step, to: pts.count, by: step) {
            let a = atan2(pts[i].y - pts[i - step].y, pts[i].x - pts[i - step].x)
            if let pa = prevAngle {
                var d = abs(a - pa)
                if d > .pi { d = 2 * .pi - d }
                if d > 1.6 { turns += 1 }
            }
            prevAngle = a
        }
        m.rect = box
        m.points = []
        if turns >= 8 { m.kind = .star } else if rectError < 0.06 && rectError < ovalError { m.kind = .rectangle }
        else if ovalError < 0.12 { m.kind = .oval } else { return stroke }
        return m
    }

    // MARK: Demo

    /// Sample marks of every kind (snapshot and CLI checks).
    static func demo(for size: CGSize) -> [Mark] {
        let w = size.width, h = size.height, s = max(w, h)
        let lw = s / 160
        var text = Mark(kind: .text, color: .clear, fill: NSColor.white.withAlphaComponent(0.85), text: "IMG42 ✓", fontSize: s / 22, bold: true, textColor: .systemBlue)
        text.rect = CGRect(origin: CGPoint(x: w * 0.06, y: h * 0.06), size: text.textSize())
        var bubble = Mark(kind: .bubble, color: .black, fill: .white, width: lw)
        bubble.rect = CGRect(x: w * 0.40, y: h * 0.05, width: w * 0.16, height: h * 0.16)
        return [
            Mark(kind: .spotlight, rect: CGRect(x: w * 0.30, y: h * 0.30, width: w * 0.40, height: h * 0.40)),
            Mark(kind: .rectangle, color: .systemRed, width: lw, rect: CGRect(x: w * 0.62, y: h * 0.06, width: w * 0.16, height: h * 0.16)),
            Mark(kind: .roundedRect, color: .systemOrange, width: lw, dashed: true, rect: CGRect(x: w * 0.80, y: h * 0.06, width: w * 0.16, height: h * 0.16)),
            Mark(kind: .oval, color: .systemGreen, fill: NSColor.systemGreen.withAlphaComponent(0.3), width: lw, rect: CGRect(x: w * 0.06, y: h * 0.30, width: w * 0.18, height: h * 0.18)),
            Mark(kind: .star, color: .systemYellow, fill: .systemYellow, width: lw, shadow: true, rect: CGRect(x: w * 0.06, y: h * 0.55, width: w * 0.16, height: h * 0.16)),
            Mark(kind: .polygon, color: .systemTeal, width: lw, rect: CGRect(x: w * 0.06, y: h * 0.78, width: w * 0.14, height: h * 0.16)),
            Mark(kind: .line, color: .systemRed, width: lw, arrowEnd: true, points: [CGPoint(x: w * 0.30, y: h * 0.90), CGPoint(x: w * 0.45, y: h * 0.72)]),
            Mark(kind: .line, color: .systemBlue, width: lw, arrowStart: true, arrowEnd: true, points: [CGPoint(x: w * 0.50, y: h * 0.92), CGPoint(x: w * 0.70, y: h * 0.92)]),
            Mark(kind: .highlighter, color: .systemYellow, width: lw * 1.5,
                 points: stride(from: 0.0, through: 1.0, by: 0.05).map { CGPoint(x: w * (0.30 + 0.35 * $0), y: h * (0.25 + 0.02 * sin($0 * 12))) }),
            Mark(kind: .pen, color: .systemPurple, width: lw,
                 points: stride(from: 0.0, through: 1.0, by: 0.02).map { CGPoint(x: w * (0.74 + 0.22 * $0), y: h * (0.80 + 0.05 * sin($0 * 18))) }),
            Mark(kind: .pixelate, rect: CGRect(x: w * 0.76, y: h * 0.32, width: w * 0.18, height: h * 0.14)),
            Mark(kind: .loupe, color: .white, width: lw, rect: CGRect(x: w * 0.76, y: h * 0.52, width: h * 0.2, height: h * 0.2), zoom: 2.5),
            bubble, text,
        ]
    }
}
