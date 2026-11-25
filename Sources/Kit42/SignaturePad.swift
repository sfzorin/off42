import AppKit

/// A box to sign in with the trackpad, mouse or a tablet. Records each stroke with time and pressure, so the
/// learned signature keeps the real rhythm of the hand. Also accepts a sketch or photo from an iPhone or iPad
/// (Continuity) through `onPicture`.
final class SignaturePad: NSView {
    private(set) var strokes: [[[Double]]] = []
    private var current: [[Double]] = []
    private var start: TimeInterval = 0
    var onChange: (() -> Void)?
    var onPicture: ((CGImage) -> Void)?

    override var isFlipped: Bool { false }
    override var acceptsFirstResponder: Bool { true }

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
    }

    required init?(coder: NSCoder) { fatalError("not supported") }

    var isEmpty: Bool { strokes.isEmpty }

    func clear() {
        strokes = []
        current = []
        needsDisplay = true
        onChange?()
    }

    var sample: SignatureSample? { strokes.isEmpty ? nil : SignatureSample(strokes: strokes) }

    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        if strokes.isEmpty { start = event.timestamp }
        current = [point(event)]
    }

    override func mouseDragged(with event: NSEvent) {
        current.append(point(event))
        needsDisplay = true
    }

    override func mouseUp(with event: NSEvent) {
        current.append(point(event))
        if current.count > 1 { strokes.append(current) }
        current = []
        needsDisplay = true
        onChange?()
    }

    private func point(_ event: NSEvent) -> [Double] {
        let p = convert(event.locationInWindow, from: nil)
        return [Double(p.x), Double(p.y), event.timestamp - start, Double(event.pressure)]
    }

    override func draw(_ dirtyRect: NSRect) {
        // Paper stays white in dark mode too.
        NSColor.white.setFill()
        bounds.fill()
        NSColor(white: 0.82, alpha: 1).setStroke()
        let base = NSBezierPath()
        base.move(to: NSPoint(x: 24, y: bounds.height * 0.28))
        base.line(to: NSPoint(x: bounds.width - 24, y: bounds.height * 0.28))
        base.lineWidth = 1
        base.stroke()
        if strokes.isEmpty && current.isEmpty {
            let text = L("Sign here") as NSString
            let attrs: [NSAttributedString.Key: Any] = [.font: NSFont.systemFont(ofSize: 15), .foregroundColor: NSColor(white: 0.6, alpha: 1)]
            text.draw(at: NSPoint(x: 28, y: bounds.height * 0.28 + 8), withAttributes: attrs)
        }
        NSColor(srgbRed: 0.1, green: 0.2, blue: 0.56, alpha: 1).setStroke()
        for stroke in strokes + [current] where stroke.count > 1 {
            let path = NSBezierPath()
            path.lineWidth = 2.2
            path.lineCapStyle = .round
            path.lineJoinStyle = .round
            path.move(to: NSPoint(x: stroke[0][0], y: stroke[0][1]))
            for p in stroke.dropFirst() { path.line(to: NSPoint(x: p[0], y: p[1])) }
            path.stroke()
        }
    }

    // MARK: Continuity: "Add Sketch" / "Take Photo" from an iPhone or iPad

    override func validRequestor(forSendType sendType: NSPasteboard.PasteboardType?, returnType: NSPasteboard.PasteboardType?) -> Any? {
        if let returnType, NSImage.imageTypes.contains(returnType.rawValue) { return self }
        return super.validRequestor(forSendType: sendType, returnType: returnType)
    }

    @objc func readSelection(from pasteboard: NSPasteboard) -> Bool {
        guard let image = NSImage(pasteboard: pasteboard) else { return false }
        var rect = CGRect(origin: .zero, size: image.size)
        guard let cg = image.cgImage(forProposedRect: &rect, context: nil, hints: nil) else { return false }
        onPicture?(cg)
        return true
    }
}
