import AppKit
import Kit42

/// Crop / selection aspect presets.
enum CropAspect: Int, CaseIterable {
    case free, square, fourThree, threeTwo, sixteenNine, original
    var title: String {
        switch self {
        case .free: return L("Free")
        case .square: return "1:1"
        case .fourThree: return "4:3"
        case .threeTwo: return "3:2"
        case .sixteenNine: return "16:9"
        case .original: return L("Original Ratio")
        }
    }
    func ratio(original: CGSize) -> CGFloat? {
        switch self {
        case .free: return nil
        case .square: return 1
        case .fourThree: return 4.0 / 3
        case .threeTwo: return 3.0 / 2
        case .sixteenNine: return 16.0 / 9
        case .original: return original.width / max(1, original.height)
        }
    }
}

/// Transparent layer over the image: draws the marks, edits them, makes pixel selections and pans the image.
final class MarkupView: NSView, NSTextFieldDelegate {
    var marks: [Mark] = [] { didSet { needsDisplay = true } }
    var selectedID: UUID? { didSet { if oldValue != selectedID { needsDisplay = true; onSelectionChange?() } } }
    var tool: MarkTool = .select {
        didSet {
            cancelInteraction()
            endTextEditing()
            if !tool.isSelectionTool { pixelSelection = nil }
            if tool == .smartLasso { SmartLasso.prewarm(baseImage?()) }
            needsDisplay = true
            window?.invalidateCursorRects(for: self)
        }
    }
    var markupActive = false { didSet { if !markupActive { selectedID = nil; tool = .select } } }
    var style = MarkStyle()
    var aspect: CropAspect = .free
    var pixelSelection: PixelSelection? {
        didSet {
            if pixelSelection == nil { invalidateSelectionWork() }
            needsDisplay = true; onPixelSelectionChange?()
        }
    }
    var hideMarks = false { didSet { needsDisplay = true } }
    var searchBoxes: [CGRect] = [] { didSet { needsDisplay = true } }
    var currentSearchBox: Int = -1 { didSet { needsDisplay = true } }

    var baseImage: (() -> CGImage?)?
    var magnification: (() -> CGFloat)?
    /// A finished change; `before` is the previous list for undo.
    var onCommit: ((_ before: [Mark], _ name: String) -> Void)?
    var onSelectionChange: (() -> Void)?
    var onPixelSelectionChange: (() -> Void)?
    var onKey: ((NSEvent) -> Bool)?
    var onDoubleClick: (() -> Void)?
    var onCropRequest: (() -> Void)?
    /// Any click on the canvas (outside Live Text): clears a text selection.
    var onMouseDown: (() -> Void)?
    var onTextEditingChange: (() -> Void)?

    private var drag: DragState?
    private var editor: NSTextField?
    private var editingID: UUID?

    private enum DragState {
        case pan(start: NSPoint, origin: NSPoint)
        case draw(Mark, before: [Mark], start: CGPoint)
        case move(id: UUID, start: CGPoint, original: Mark, before: [Mark])
        case resize(id: UUID, handle: Int, original: Mark, before: [Mark])
        case select(start: CGPoint, points: [CGPoint], original: PixelSelection?)
        case moveSelection(start: CGPoint, original: PixelSelection)
        case resizeSelection(handle: Int, original: PixelSelection)
        case alpha(seed: CGPoint, start: NSPoint, original: PixelSelection?)
    }

    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    private var scale: CGFloat { max(0.0001, magnification?() ?? 1) }
    var selected: Mark? { marks.first { $0.id == selectedID } }

    // MARK: Drawing

    override func draw(_ dirtyRect: NSRect) {
        let needsBase = marks.contains { $0.kind == .pixelate || $0.kind == .loupe }
        let base = needsBase ? baseImage?() : nil
        if !hideMarks {
            for m in marks where m.id != editingID && (m.kind == .spotlight || m.bounds.insetBy(dx: -m.width * 4 - 4, dy: -m.width * 4 - 4).intersects(dirtyRect)) {
                m.draw(base: base, canvas: bounds.size)
            }
        }
        if case .draw(let m, _, _) = drag {
            m.draw(base: base ?? (m.kind == .pixelate || m.kind == .loupe ? baseImage?() : nil), canvas: bounds.size)
        }
        drawSearch()
        if let m = selected, markupActive, !hideMarks { drawSelection(m) }
        if let sel = pixelSelection { drawPixelSelection(sel) }
        if case .select(_, let pts, _) = drag {
            if tool == .smartLasso { drawSmartBand(pts) } else if let path = selectionPath(pts) { drawOutline(path) }
        }
        if let pts = pendingSmartStroke { drawSmartBand(pts) }
    }

    private func drawSearch() {
        guard !searchBoxes.isEmpty else { return }
        for (i, box) in searchBoxes.enumerated() {
            let r = box.insetBy(dx: -box.height * 0.15, dy: -box.height * 0.1)
            let p = NSBezierPath(roundedRect: r, xRadius: r.height * 0.2, yRadius: r.height * 0.2)
            (i == currentSearchBox ? NSColor.systemYellow.withAlphaComponent(0.55) : NSColor.systemYellow.withAlphaComponent(0.3)).setFill()
            p.fill()
            if i == currentSearchBox {
                p.lineWidth = 2 / scale
                NSColor.systemOrange.setStroke()
                p.stroke()
            }
        }
    }

    private func drawSelection(_ m: Mark) {
        let b = m.bounds
        let path = NSBezierPath(rect: b)
        path.lineWidth = 1 / scale
        path.setLineDash([4 / scale, 3 / scale], count: 2, phase: 0)
        NSColor.controlAccentColor.setStroke()
        path.stroke()
        for p in handles(of: m) { drawHandle(p) }
    }

    private func drawHandle(_ p: CGPoint) {
        let r = 4.5 / scale
        let h = NSBezierPath(ovalIn: CGRect(x: p.x - r, y: p.y - r, width: r * 2, height: r * 2))
        NSColor.white.setFill()
        h.fill()
        h.lineWidth = 1.2 / scale
        NSColor.controlAccentColor.setStroke()
        h.stroke()
    }

    private func drawOutline(_ path: CGPath) {
        guard let ctx = NSGraphicsContext.current?.cgContext else { return }
        ctx.saveGState()
        ctx.addPath(path)
        ctx.setLineWidth(1.5 / scale)
        ctx.setStrokeColor(NSColor.white.cgColor)
        ctx.strokePath()
        ctx.addPath(path)
        ctx.setLineDash(phase: 0, lengths: [5 / scale, 4 / scale])
        ctx.setStrokeColor(NSColor.black.cgColor)
        ctx.strokePath()
        ctx.restoreGState()
    }

    private func drawPixelSelection(_ sel: PixelSelection) {
        guard let ctx = NSGraphicsContext.current?.cgContext else { return }
        if let outline = sel.outline {
            // Dim what is outside the selection, like a crop preview.
            ctx.saveGState()
            ctx.addRect(bounds)
            ctx.addPath(outline)
            ctx.setFillColor(NSColor.black.withAlphaComponent(0.35).cgColor)
            ctx.fillPath(using: .evenOdd)
            ctx.restoreGState()
            drawOutline(outline)
            if sel.isRectangle { for p in corners(sel.bounds) { drawHandle(p) } }
        } else {
            ctx.saveGState()
            ctx.translateBy(x: 0, y: bounds.height)
            ctx.scaleBy(x: 1, y: -1)
            ctx.clip(to: CGRect(origin: .zero, size: sel.size), mask: sel.mask)
            ctx.setFillColor(NSColor.systemPink.withAlphaComponent(0.45).cgColor)
            ctx.fill(CGRect(origin: .zero, size: sel.size))
            ctx.restoreGState()
        }
    }

    /// Smart Lasso: the wide translucent stroke the user paints around the object (Preview-style).
    private func drawSmartBand(_ pts: [CGPoint]) {
        guard pts.count > 1, let ctx = NSGraphicsContext.current?.cgContext else { return }
        ctx.saveGState()
        ctx.setLineCap(.round)
        ctx.setLineJoin(.round)
        ctx.addLines(between: pts)
        ctx.setLineWidth(SmartLasso.bandRadius * 2 / scale)
        ctx.setStrokeColor(NSColor.controlAccentColor.withAlphaComponent(0.35).cgColor)
        ctx.strokePath()
        ctx.addLines(between: pts)
        ctx.setLineWidth(1 / scale)
        ctx.setStrokeColor(NSColor.white.withAlphaComponent(0.8).cgColor)
        ctx.strokePath()
        ctx.restoreGState()
    }

    private func corners(_ r: CGRect) -> [CGPoint] {
        [CGPoint(x: r.minX, y: r.minY), CGPoint(x: r.maxX, y: r.minY), CGPoint(x: r.maxX, y: r.maxY), CGPoint(x: r.minX, y: r.maxY)]
    }

    private func handles(of m: Mark) -> [CGPoint] {
        if m.isLineBased, m.points.count >= 2 { return [m.points[0], m.points[1]] }
        if m.isRectBased { return corners(m.rect.standardized) }
        return []
    }

    private func selectionPath(_ pts: [CGPoint]) -> CGPath? {
        guard let a = pts.first, let b = pts.last else { return nil }
        switch tool {
        case .selectRect, .selectEllipse:
            let r = constrained(CGRect(x: min(a.x, b.x), y: min(a.y, b.y), width: abs(b.x - a.x), height: abs(b.y - a.y)), anchor: a, to: b)
            return tool == .selectRect ? CGPath(rect: r, transform: nil) : CGPath(ellipseIn: r, transform: nil)
        default:
            guard pts.count > 2 else { return nil }
            let p = CGMutablePath()
            p.addLines(between: pts)
            p.closeSubpath()
            return p
        }
    }

    private func constrained(_ r: CGRect, anchor: CGPoint, to p: CGPoint) -> CGRect {
        guard let ratio = aspect.ratio(original: bounds.size) else { return r.intersection(bounds) }
        // Shrink both dimensions together at the image edge; clipping only one breaks the chosen ratio.
        let availableWidth = p.x < anchor.x ? anchor.x - bounds.minX : bounds.maxX - anchor.x
        let availableHeight = p.y < anchor.y ? anchor.y - bounds.minY : bounds.maxY - anchor.y
        let width = max(0, min(r.width, availableWidth, availableHeight * ratio))
        let height = width / ratio
        return CGRect(x: p.x < anchor.x ? anchor.x - width : anchor.x,
                      y: p.y < anchor.y ? anchor.y - height : anchor.y, width: width, height: height)
    }

    // MARK: Mouse

    override func resetCursorRects() {
        let cursor: NSCursor
        switch tool {
        // Viewing / Select tool: no cursor rect, so Live Text can show the I-beam over text.
        case _ where !markupActive: return
        case .select: return
        case .text: cursor = .iBeam
        default: cursor = .crosshair
        }
        addCursorRect(visibleRect, cursor: cursor)
    }

    private func point(_ event: NSEvent) -> CGPoint { convert(event.locationInWindow, from: nil) }

    /// The topmost mark under `p` (image coordinates).
    func mark(at p: CGPoint) -> Mark? {
        hideMarks ? nil : marks.last { $0.hitTest(p, tolerance: 6 / scale) }
    }

    /// Live Text may take the mouse: viewing, or the Select tool away from marks and while nothing is dragged.
    func allowsLiveText(at p: CGPoint) -> Bool {
        guard drag == nil, editor == nil else { return false }
        if !markupActive { return true }
        return tool == .select && mark(at: p) == nil
    }

    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        onMouseDown?()
        let p = point(event)
        let tol = 6 / scale
        if event.clickCount == 2 {
            if markupActive, tool == .select, let m = marks.last(where: { $0.hitTest(p, tolerance: tol) }), m.kind == .text {
                beginTextEditing(existing: m)
                return
            }
            if tool.isSelectionTool, let sel = pixelSelection, sel.bounds.contains(p) {
                onCropRequest?()
                return
            }
            if !markupActive || tool == .select { onDoubleClick?(); return }
        }
        endTextEditing()
        guard markupActive else { return startPan(event) }
        switch tool {
        case .select:
            if let m = selected, let h = handles(of: m).firstIndex(where: { hypot($0.x - p.x, $0.y - p.y) <= tol * 1.5 }) {
                drag = .resize(id: m.id, handle: h, original: m, before: marks)
            } else if let m = marks.last(where: { $0.hitTest(p, tolerance: tol) }) {
                selectedID = m.id
                drag = .move(id: m.id, start: p, original: m, before: marks)
            } else {
                selectedID = nil
                startPan(event)
            }
        case .selectRect, .selectEllipse, .selectLasso, .smartLasso:
            if let sel = pixelSelection, sel.isRectangle,
               let handle = corners(sel.bounds).firstIndex(where: { hypot($0.x - p.x, $0.y - p.y) <= tol * 1.5 }),
               !event.modifierFlags.contains(.shift) {
                drag = .resizeSelection(handle: handle, original: sel)
            } else if let sel = pixelSelection, let outline = sel.outline, outline.contains(p, using: .evenOdd), !event.modifierFlags.contains(.shift) {
                drag = .moveSelection(start: p, original: sel)
            } else {
                let previous = pixelSelection
                pixelSelection = nil
                invalidateSelectionWork()
                drag = .select(start: p, points: [p], original: previous)
            }
        case .instantAlpha:
            invalidateSelectionWork()
            drag = .alpha(seed: p, start: event.locationInWindow, original: pixelSelection)
            updateInstantAlpha(seed: p, distance: 0, preview: true)
        case .text:
            if let m = marks.last(where: { $0.kind == .text && $0.hitTest(p, tolerance: tol) }) {
                beginTextEditing(existing: m)
            } else {
                var m = newMark(kind: .text, at: p)
                m.rect = CGRect(origin: CGPoint(x: p.x, y: p.y - m.textSize().height / 2), size: m.textSize())
                beginTextEditing(new: m)
            }
        default:
            guard let kind = tool.markKind else { return }
            selectedID = nil
            drag = .draw(newMark(kind: kind, at: p), before: marks, start: p)
        }
    }

    func newMark(kind: Mark.Kind, at p: CGPoint) -> Mark {
        let s = style
        var m = Mark(kind: kind, color: s.stroke, fill: s.fill, width: s.width / scale, dashed: s.dashed, shadow: s.shadow)
        switch kind {
        case .pen, .highlighter:
            m.points = [p]
            m.fill = nil
            if kind == .highlighter { m.color = s.stroke == .systemRed ? .systemYellow : s.stroke; m.dashed = false }
        case .line:
            m.points = [p, p]
            m.fill = nil
            m.arrowEnd = tool == .arrow ? true : s.arrowEnd && tool != .line
            m.arrowStart = tool == .arrow ? s.arrowStart : false
        case .text:
            m.color = s.fill == nil ? .clear : s.stroke
            m.fontName = s.fontName
            m.fontSize = s.fontSize / scale
            m.bold = s.bold
            m.italic = s.italic
            m.underline = s.underline
            m.alignment = s.alignment
            m.textColor = s.textColor
        case .loupe:
            m.color = .white
            m.fill = nil
            m.zoom = s.loupeZoom
        default:
            m.rect = CGRect(origin: p, size: .zero)
        }
        return m
    }

    private func startPan(_ event: NSEvent) {
        guard let clip = enclosingScrollView?.contentView else { return }
        drag = .pan(start: event.locationInWindow, origin: clip.bounds.origin)
        NSCursor.closedHand.push()
    }

    override func mouseDragged(with event: NSEvent) {
        let p = point(event)
        let shift = event.modifierFlags.contains(.shift)
        switch drag {
        case .pan(let start, let origin):
            guard let scroll = enclosingScrollView else { return }
            let dx = (event.locationInWindow.x - start.x) / scale, dy = (event.locationInWindow.y - start.y) / scale
            scroll.contentView.scroll(to: NSPoint(x: origin.x - dx, y: origin.y + dy))
            scroll.reflectScrolledClipView(scroll.contentView)
        case .draw(var m, let before, let start):
            switch m.kind {
            case .pen, .highlighter:
                if let last = m.points.last, hypot(last.x - p.x, last.y - p.y) > 1.5 / scale { m.points.append(p) }
            case .line:
                m.points[1] = shift ? Self.snap45(from: m.points[0], to: p) : p
            default:
                var r = CGRect(x: min(start.x, p.x), y: min(start.y, p.y), width: abs(p.x - start.x), height: abs(p.y - start.y))
                if shift || m.keepsAspect {
                    let s = max(r.width, r.height)
                    r = CGRect(x: p.x < start.x ? start.x - s : start.x, y: p.y < start.y ? start.y - s : start.y, width: s, height: s)
                }
                m.rect = r
            }
            drag = .draw(m, before: before, start: start)
            needsDisplay = true
        case .move(let id, let start, let original, let before):
            let t = CGAffineTransform(translationX: p.x - start.x, y: p.y - start.y)
            replace(id) { _ in original.applying(t) }
            drag = .move(id: id, start: start, original: original, before: before)
        case .resize(let id, let handle, let original, _):
            replace(id) { m in
                var m = m
                if original.isLineBased { m.points[handle] = shift ? Self.snap45(from: original.points[1 - handle], to: p) : p; return m }
                let r = original.rect.standardized
                let anchor = corners(r)[(handle + 2) % 4]
                var nr = CGRect(x: min(anchor.x, p.x), y: min(anchor.y, p.y), width: abs(p.x - anchor.x), height: abs(p.y - anchor.y))
                if original.keepsAspect || shift, r.height > 0 {
                    nr.size.height = nr.width / (r.width / r.height)
                    if p.y < anchor.y { nr.origin.y = anchor.y - nr.height }
                }
                if original.kind == .text {
                    nr.size.height = m.textSize(width: nr.width).height
                    nr.origin.y = r.minY
                }
                m.rect = nr
                return m
            }
        case .select(let start, var pts, let original):
            if tool == .selectRect || tool == .selectEllipse { pts = [start, p] } else { pts.append(p) }
            drag = .select(start: start, points: pts, original: original)
            needsDisplay = true
        case .moveSelection(let start, let original):
            let dx = max(bounds.minX - original.bounds.minX, min(bounds.maxX - original.bounds.maxX, p.x - start.x))
            let dy = max(bounds.minY - original.bounds.minY, min(bounds.maxY - original.bounds.maxY, p.y - start.y))
            var t = CGAffineTransform(translationX: dx, y: dy)
            if let outline = original.outline, let moved = outline.copy(using: &t) {
                pixelSelection = PixelSelection.from(path: moved, size: original.size, isRectangle: original.isRectangle)
            }
        case .resizeSelection(let handle, let original):
            let anchor = corners(original.bounds)[(handle + 2) % 4]
            let proposed = CGRect(x: min(anchor.x, p.x), y: min(anchor.y, p.y), width: abs(p.x - anchor.x), height: abs(p.y - anchor.y))
            let rect = constrained(proposed, anchor: anchor, to: p)
            if rect.width >= 1, rect.height >= 1 {
                pixelSelection = PixelSelection.from(path: CGPath(rect: rect, transform: nil), size: original.size, isRectangle: true)
            }
        case .alpha(let seed, let start, _):
            updateInstantAlpha(seed: seed, distance: hypot(event.locationInWindow.x - start.x, event.locationInWindow.y - start.y), preview: true)
        case nil:
            break
        }
    }

    private var alphaWork: DispatchWorkItem?
    private var alphaToken = 0

    /// A tool/document change must not allow a background selection to reappear later.
    func invalidateSelectionWork() {
        alphaToken &+= 1; smartToken &+= 1
        alphaWork?.cancel(); alphaWork = nil; pendingSmartStroke = nil
    }

    /// Abort the preview without creating an Undo entry or leaving a half-applied move behind.
    func cancelInteraction() {
        switch drag {
        case .move(_, _, _, let before), .resize(_, _, _, let before): marks = before
        case .select(_, _, let original), .alpha(_, _, let original): pixelSelection = original
        case .moveSelection(_, let original), .resizeSelection(_, let original): pixelSelection = original
        case .pan: NSCursor.pop()
        default: break
        }
        drag = nil
        invalidateSelectionWork()
        needsDisplay = true
    }

    /// Instant Alpha: dragging farther selects a wider range of similar colours.
    private func updateInstantAlpha(seed: CGPoint, distance: CGFloat, preview: Bool) {
        guard let base = baseImage?() else { return }
        let tolerance = min(0.9, 0.04 + distance / 400)
        alphaWork?.cancel()
        alphaToken &+= 1
        let token = alphaToken
        let size = bounds.size
        let work = DispatchWorkItem { [weak self] in
            let sel = PixelSelection.similarColors(in: base, seed: seed, tolerance: tolerance)
            DispatchQueue.main.async {
                guard let self, token == self.alphaToken, self.tool == .instantAlpha,
                      self.bounds.size == size, self.baseImage?() === base else { return }
                self.pixelSelection = sel
                self.alphaWork = nil
            }
        }
        alphaWork = work
        DispatchQueue.global(qos: .userInitiated).async(execute: work)
    }

    static func snap45(from a: CGPoint, to p: CGPoint) -> CGPoint {
        let angle = (atan2(p.y - a.y, p.x - a.x) / (.pi / 4)).rounded() * (.pi / 4)
        let len = hypot(p.x - a.x, p.y - a.y)
        return CGPoint(x: a.x + cos(angle) * len, y: a.y + sin(angle) * len)
    }

    override func mouseUp(with event: NSEvent) {
        if drag != nil { mouseDragged(with: event) }
        defer { drag = nil }
        switch drag {
        case .pan:
            NSCursor.pop()
        case .draw(var m, let before, let start):
            let moved = hypot(point(event).x - start.x, point(event).y - start.y)
            if moved <= 3 / scale, m.kind != .pen, m.kind != .highlighter {
                // A click places a shape of a default size, like Preview.
                let s = 120 / scale
                if m.kind == .line {
                    m.points = [CGPoint(x: start.x - s / 2, y: start.y), CGPoint(x: start.x + s / 2, y: start.y)]
                } else {
                    m.rect = CGRect(x: start.x - s / 2, y: start.y - s / 2, width: s, height: m.keepsAspect || m.kind == .star || m.kind == .oval ? s : s * 0.75)
                }
            }
            if tool == .sketch { m = Mark.recognize(m) }
            if m.isRectBased { m.rect = m.rect.standardized }
            marks.append(m)
            selectedID = m.kind == .pen || m.kind == .highlighter ? nil : m.id
            onCommit?(before, tool.title)
        case .move(let id, _, let original, let before):
            if let current = marks.first(where: { $0.id == id }), !current.hasSameContent(as: original) { onCommit?(before, L("Move")) }
        case .resize(let id, _, let original, let before):
            if let current = marks.first(where: { $0.id == id }), !current.hasSameContent(as: original) { onCommit?(before, L("Resize")) }
        case .select(_, let pts, _) where tool == .smartLasso:
            finishSmartLasso(pts)
        case .select(_, let pts, _):
            if let path = selectionPath(pts), path.boundingBoxOfPath.width >= 2, path.boundingBoxOfPath.height >= 2 {
                pixelSelection = PixelSelection.from(path: path, size: bounds.size, isRectangle: tool == .selectRect)
            } else {
                pixelSelection = nil
            }
        default:
            break
        }
    }

    /// The stroke being snapped to edges (drawn until the selection arrives).
    private var pendingSmartStroke: [CGPoint]?
    private var smartToken = 0

    /// Smart Lasso: snaps the painted band to the object's edge in the background, then makes the selection.
    func finishSmartLasso(_ pts: [CGPoint]) {
        if ProcessInfo.processInfo.environment["K42_SMART_LASSO"] != nil { print("smartlasso: stroke of \(pts.count) points") }
        let size = bounds.size
        guard pts.count > 2, let image = baseImage?() else { pixelSelection = nil; return }
        let box = pts.reduce(CGRect.null) { $0.union(CGRect(origin: $1, size: .zero)) }
        guard box.width >= 2 || box.height >= 2 else { pixelSelection = nil; return }
        let radius = SmartLasso.bandRadius / scale
        smartToken += 1
        let token = smartToken
        pendingSmartStroke = pts
        needsDisplay = true
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let subject = SmartLasso.subjectMask(for: image)
            let path = SmartLasso.refine(stroke: pts, radius: radius, image: image, subject: subject)
            let sel = path.flatMap { PixelSelection.from(path: $0, size: size) }
            DispatchQueue.main.async {
                guard let self, token == self.smartToken, self.tool == .smartLasso,
                      self.bounds.size == size, self.baseImage?() === image else { return }
                self.pendingSmartStroke = nil
                self.pixelSelection = sel
                self.needsDisplay = true
            }
        }
    }

    private func replace(_ id: UUID, _ change: (Mark) -> Mark) {
        if let i = marks.firstIndex(where: { $0.id == id }) { marks[i] = change(marks[i]) }
    }

    // MARK: Keys

    override func keyDown(with event: NSEvent) {
        if markupActive {
            switch event.keyCode {
            case 51, 117:   // delete, forward delete
                if let id = selectedID { deleteMark(id); return }
            case 53:        // escape
                if drag != nil || pendingSmartStroke != nil || alphaWork != nil { cancelInteraction(); return }
                if pixelSelection != nil { pixelSelection = nil; return }
                if selectedID != nil { selectedID = nil; return }
            default: break
            }
        }
        if onKey?(event) == true { return }
        super.keyDown(with: event)
    }

    func deleteMark(_ id: UUID) {
        guard marks.contains(where: { $0.id == id }) else { return }
        let before = marks
        marks.removeAll { $0.id == id }
        selectedID = nil
        onCommit?(before, L("Delete"))
    }

    /// Changes the selected mark (style controls); returns false when nothing is selected.
    @discardableResult
    func updateSelected(_ name: String, _ change: (inout Mark) -> Void) -> Bool {
        endTextEditing()
        guard let id = selectedID, let i = marks.firstIndex(where: { $0.id == id }) else { return false }
        let before = marks
        change(&marks[i])
        if marks[i].kind == .text { marks[i].rect.size = marks[i].textSize(width: marks[i].rect.width) }
        guard !marks[i].hasSameContent(as: before[i]) else { return false }
        onCommit?(before, name)
        return true
    }

    var screenScale: CGFloat { scale }

    /// Adds an image mark (signature, stamp) centred in the visible part.
    func insertImage(_ image: CGImage) {
        let visible = visibleRect.isEmpty ? bounds : visibleRect
        let aspect = CGFloat(image.width) / max(1, CGFloat(image.height))
        let width = min(visible.width * 0.35, 220 / scale, visible.height * 0.7 * aspect)
        let size = CGSize(width: width, height: width / aspect)
        let before = marks
        let m = Mark(kind: .image, rect: CGRect(x: visible.midX - size.width / 2, y: visible.midY - size.height / 2, width: size.width, height: size.height), image: image)
        marks.append(m)
        selectedID = m.id
        onCommit?(before, L("Insert Signature"))
    }

    func selectAllPixels() {
        invalidateSelectionWork()
        pixelSelection = PixelSelection.from(path: CGPath(rect: bounds, transform: nil), size: bounds.size, isRectangle: true)
    }

    // MARK: Text editing

    private var editingBefore: [Mark] = []
    private var editingMark: Mark?

    private func beginTextEditing(new m: Mark) {
        editingBefore = marks
        editingMark = m
        showEditor(for: m)
    }

    private func beginTextEditing(existing m: Mark) {
        editingBefore = marks
        editingMark = m
        editingID = m.id
        selectedID = m.id
        showEditor(for: m)
        needsDisplay = true
    }

    private func showEditor(for m: Mark) {
        let field = NSTextField(string: m.text)
        field.isBordered = false
        field.drawsBackground = m.fill != nil
        field.backgroundColor = m.fill ?? .clear
        field.focusRingType = .none
        field.font = m.font
        field.textColor = m.textColor
        field.alignment = m.alignment
        field.delegate = self
        field.usesSingleLineMode = false
        field.cell?.wraps = true
        field.cell?.isScrollable = false
        let r = m.rect.standardized
        field.frame = CGRect(x: r.minX + m.textPadding - 2, y: r.minY + m.textPadding, width: max(r.width - m.textPadding * 2 + 4, m.fontSize * 6), height: max(r.height - m.textPadding * 2, m.fontSize * 1.3))
        addSubview(field)
        editor = field
        window?.makeFirstResponder(field)
    }

    func controlTextDidChange(_ obj: Notification) {
        guard let field = editor, var m = editingMark else { return }
        m.text = currentEditorText(field)
        let size = m.textSize(width: editingID == nil ? nil : m.rect.width)
        field.frame.size = CGSize(width: max(size.width, m.fontSize * 6), height: max(size.height - m.textPadding * 2, m.fontSize * 1.3))
        onTextEditingChange?()
    }

    func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        if selector == #selector(NSResponder.insertNewline(_:)), NSApp.currentEvent?.modifierFlags.contains(.option) == true {
            textView.insertNewlineIgnoringFieldEditor(nil)
            return true
        }
        if selector == #selector(NSResponder.cancelOperation(_:)) {
            cancelTextEditing()
            return true
        }
        return false
    }

    func controlTextDidEndEditing(_ obj: Notification) { endTextEditing() }

    func endTextEditing() {
        guard let field = editor, var m = editingMark else { return }
        let text = currentEditorText(field)
        editor = nil
        field.delegate = nil
        field.removeFromSuperview()
        let id = editingID
        editingID = nil
        editingMark = nil
        let changed = text != m.text
        m.text = text
        if changed { m.rect.size = m.textSize(width: id == nil ? nil : m.rect.width) }
        if let id, let i = marks.firstIndex(where: { $0.id == id }) {
            if m.text.isEmpty { marks.remove(at: i) } else { marks[i] = m }
        } else if !m.text.isEmpty {
            marks.append(m)
            selectedID = m.id
        }
        needsDisplay = true
        if changed { onCommit?(editingBefore, L("Text")) }
        window?.makeFirstResponder(self)
        onTextEditingChange?()
    }

    var isEditingText: Bool { editor != nil }
    var hasPendingTextEdit: Bool {
        guard let editor, let editingMark else { return false }
        return currentEditorText(editor) != editingMark.text
    }

    private func currentEditorText(_ field: NSTextField) -> String { field.currentEditor()?.string ?? field.stringValue }

    func cancelTextEditing() {
        guard let field = editor else { return }
        editor = nil; editingID = nil; editingMark = nil
        field.delegate = nil; field.removeFromSuperview()
        needsDisplay = true
        window?.makeFirstResponder(self)
        onTextEditingChange?()
    }
}
