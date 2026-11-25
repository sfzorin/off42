import AppKit
import Kit42

/// Keeps the document centred when it is smaller than the visible area.
final class CenteringClipView: NSClipView {
    override func constrainBoundsRect(_ proposedBounds: NSRect) -> NSRect {
        var r = super.constrainBoundsRect(proposedBounds)
        guard let doc = documentView else { return r }
        if doc.frame.width < r.width { r.origin.x = (doc.frame.width - r.width) / 2 }
        if doc.frame.height < r.height { r.origin.y = (doc.frame.height - r.height) / 2 }
        return r
    }
}

/// The big preview: a scanned page, or the scanner glass (overview) with the scan areas drawn on it.
/// Areas are normalised (0…1, origin top-left): drag on the glass to add one, drag inside to move,
/// drag a corner to resize, ⌫ deletes the selected one.
final class PreviewCanvas: NSView {
    var image: CGImage? { didSet {
        if oldValue !== image { cancelInteraction() }
        display = image.map { NSImage(cgImage: $0, size: NSSize(width: $0.width, height: $0.height)) }; needsDisplay = true
    } }
    private var display: NSImage?
    var editsAreas = false { didSet { if !editsAreas { cancelInteraction() }; needsDisplay = true; window?.invalidateCursorRects(for: self) } }
    var areas: [CGRect] = [] { didSet { needsDisplay = true } }
    var selectedArea: Int? { didSet { needsDisplay = true } }
    var onAreasChanged: (([CGRect]) -> Void)?
    /// Detected document outline on the page view (normalised quad points), drawn faintly.
    var accent: NSColor = Theme42.App.scn.accent

    private enum Drag { case create(CGPoint), move(Int, CGPoint, CGRect), resize(Int, corner: Int, CGRect) }
    private var drag: Drag?
    private var areasBeforeDrag: [CGRect]?
    private var selectedBeforeDrag: Int?

    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }

    var imageRect: CGRect { bounds }

    override func draw(_ dirtyRect: NSRect) {
        guard let display else { return }
        // Paper shadow.
        NSGraphicsContext.saveGraphicsState()
        let shadow = NSShadow()
        shadow.shadowColor = NSColor.black.withAlphaComponent(0.28)
        shadow.shadowBlurRadius = 6
        shadow.shadowOffset = NSSize(width: 0, height: -1)
        shadow.set()
        NSColor.white.setFill()
        bounds.fill()
        NSGraphicsContext.restoreGraphicsState()
        display.draw(in: bounds, from: .zero, operation: .sourceOver, fraction: 1, respectFlipped: true, hints: [.interpolation: NSImageInterpolation.high.rawValue])

        guard editsAreas, !areas.isEmpty else { return }
        // Dim what will not be scanned.
        NSGraphicsContext.saveGraphicsState()
        for area in areas {
            let outside = NSBezierPath(rect: bounds)
            outside.append(NSBezierPath(rect: viewRect(area)))
            outside.windingRule = .evenOdd
            outside.addClip()
        }
        NSColor.black.withAlphaComponent(0.35).setFill()
        bounds.fill()
        NSGraphicsContext.restoreGraphicsState()
        for (i, a) in areas.enumerated() {
            let r = viewRect(a)
            let path = NSBezierPath(rect: r.insetBy(dx: 0.5, dy: 0.5))
            path.lineWidth = i == selectedArea ? 2.5 : 1.5
            accent.setStroke()
            if i != selectedArea { path.setLineDash([6, 3], count: 2, phase: 0) }
            path.stroke()
            if i == selectedArea {
                for c in corners(r) {
                    let h = NSBezierPath(ovalIn: CGRect(x: c.x - 5, y: c.y - 5, width: 10, height: 10))
                    NSColor.white.setFill()
                    h.fill()
                    accent.setStroke()
                    h.lineWidth = 1.5
                    h.stroke()
                }
            }
            if areas.count > 1 {
                let label = NSAttributedString(string: "\(i + 1)", attributes: [.font: NSFont.systemFont(ofSize: 12, weight: .bold), .foregroundColor: NSColor.white])
                let s = label.size()
                let badge = CGRect(x: r.minX + 6, y: r.minY + 6, width: max(20, s.width + 10), height: 20)
                accent.setFill()
                NSBezierPath(roundedRect: badge, xRadius: 10, yRadius: 10).fill()
                label.draw(at: NSPoint(x: badge.midX - s.width / 2, y: badge.midY - s.height / 2))
            }
        }
    }

    func viewRect(_ a: CGRect) -> CGRect {
        CGRect(x: bounds.minX + a.minX * bounds.width, y: bounds.minY + a.minY * bounds.height, width: a.width * bounds.width, height: a.height * bounds.height)
    }

    private func normalised(_ p: CGPoint) -> CGPoint {
        CGPoint(x: min(1, max(0, (p.x - bounds.minX) / max(1, bounds.width))), y: min(1, max(0, (p.y - bounds.minY) / max(1, bounds.height))))
    }

    private func corners(_ r: CGRect) -> [CGPoint] {
        [CGPoint(x: r.minX, y: r.minY), CGPoint(x: r.maxX, y: r.minY), CGPoint(x: r.maxX, y: r.maxY), CGPoint(x: r.minX, y: r.maxY)]
    }

    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        guard editsAreas, image != nil else { return }
        let p = convert(event.locationInWindow, from: nil)
        guard bounds.contains(p) else { return }
        cancelInteraction()
        areasBeforeDrag = areas
        selectedBeforeDrag = selectedArea
        // Corner of the selected area, then any area, then a new one.
        if let s = selectedArea, areas.indices.contains(s),
           let c = corners(viewRect(areas[s])).firstIndex(where: { hypot($0.x - p.x, $0.y - p.y) < 9 }) {
            drag = .resize(s, corner: c, areas[s])
            return
        }
        if let i = areas.indices.reversed().first(where: { viewRect(areas[$0]).contains(p) }) {
            selectedArea = i
            drag = .move(i, normalised(p), areas[i])
            return
        }
        selectedArea = nil
        drag = .create(normalised(p))
    }

    override func mouseDragged(with event: NSEvent) {
        guard editsAreas, let drag else { return }
        let n = normalised(convert(event.locationInWindow, from: nil))
        switch drag {
        case .create(let start):
            let r = CGRect(x: min(start.x, n.x), y: min(start.y, n.y), width: abs(n.x - start.x), height: abs(n.y - start.y))
            guard r.width > 0.01, r.height > 0.01 else { return }
            if let s = selectedArea, areas.indices.contains(s) { areas[s] = r } else { areas.append(r); selectedArea = areas.count - 1 }
        case .move(let i, let start, let original):
            guard areas.indices.contains(i) else { return }
            var r = original.offsetBy(dx: n.x - start.x, dy: n.y - start.y)
            r.origin.x = min(max(0, r.origin.x), 1 - r.width)
            r.origin.y = min(max(0, r.origin.y), 1 - r.height)
            areas[i] = r
        case .resize(let i, let corner, let original):
            guard areas.indices.contains(i) else { return }
            // The opposite corner stays.
            let fixed = [CGPoint(x: original.maxX, y: original.maxY), CGPoint(x: original.minX, y: original.maxY),
                         CGPoint(x: original.minX, y: original.minY), CGPoint(x: original.maxX, y: original.minY)][corner]
            let r = CGRect(x: min(fixed.x, n.x), y: min(fixed.y, n.y), width: abs(n.x - fixed.x), height: abs(n.y - fixed.y))
            if r.width > 0.01, r.height > 0.01 { areas[i] = r }
        }
    }

    override func mouseUp(with event: NSEvent) {
        let changed = drag != nil && areasBeforeDrag != areas
        drag = nil
        areasBeforeDrag = nil
        selectedBeforeDrag = nil
        if changed { onAreasChanged?(areas) }
    }

    func cancelInteraction() {
        if let before = areasBeforeDrag { areas = before; selectedArea = selectedBeforeDrag }
        drag = nil; areasBeforeDrag = nil; selectedBeforeDrag = nil
    }

    override func cancelOperation(_ sender: Any?) { cancelInteraction() }

    override func keyDown(with event: NSEvent) {
        if event.keyCode == 53 { cancelInteraction(); return }
        if drag != nil, event.keyCode == 51 || event.keyCode == 117 { cancelInteraction(); return }
        if editsAreas, let s = selectedArea, areas.indices.contains(s), event.keyCode == 51 || event.keyCode == 117 {
            cancelInteraction()
            guard areas.indices.contains(s) else { return }
            areas.remove(at: s)
            selectedArea = nil
            onAreasChanged?(areas)
            return
        }
        super.keyDown(with: event)
    }

    override func resetCursorRects() {
        if editsAreas { addCursorRect(bounds, cursor: .crosshair) }
    }

    // Continuity Camera: importing from an iPhone works while the preview has the focus.
    override func validRequestor(forSendType sendType: NSPasteboard.PasteboardType?, returnType: NSPasteboard.PasteboardType?) -> Any? {
        if sendType == nil, let returnType, ContinuityImport.accepts(returnType) { return self }
        return super.validRequestor(forSendType: sendType, returnType: returnType)
    }
}

extension PreviewCanvas: NSServicesMenuRequestor {
    func readSelection(from pboard: NSPasteboard) -> Bool {
        (window?.windowController as? ScanWindowController)?.importContinuity(pboard) ?? false
    }

    func writeSelection(to pboard: NSPasteboard, types: [NSPasteboard.PasteboardType]) -> Bool { false }
}

/// Scroll view, canvas, empty state and zoom.
final class PreviewController: NSViewController {
    let scroll = NSScrollView()
    let canvas = PreviewCanvas()
    let empty = NSStackView()
    let emptyTitle = NSTextField(labelWithString: "")
    let emptyHint = NSTextField(wrappingLabelWithString: "")
    private var imageSize: CGSize = .zero

    static let background = NSColor(name: "SCN42.previewBackground") { a in
        a.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua ? NSColor(white: 0.13, alpha: 1) : NSColor(white: 0.90, alpha: 1)
    }

    override func loadView() {
        let root = NSView()
        scroll.contentView = CenteringClipView()
        scroll.documentView = canvas
        scroll.hasVerticalScroller = true
        scroll.hasHorizontalScroller = true
        scroll.autohidesScrollers = true
        scroll.allowsMagnification = true
        scroll.minMagnification = 0.5
        scroll.maxMagnification = 8
        scroll.drawsBackground = true
        scroll.backgroundColor = Self.background
        scroll.borderType = .noBorder

        let icon = NSImageView(image: Theme42.symbol("scanner", size: 54, weight: .ultraLight) ?? NSImage())
        icon.contentTintColor = .tertiaryLabelColor
        emptyTitle.font = .systemFont(ofSize: 17, weight: .semibold)
        emptyTitle.textColor = .secondaryLabelColor
        emptyTitle.alignment = .center
        emptyHint.font = .systemFont(ofSize: 13)
        emptyHint.textColor = .tertiaryLabelColor
        emptyHint.alignment = .center
        emptyHint.preferredMaxLayoutWidth = 380
        empty.setViews([icon, emptyTitle, emptyHint], in: .center)
        empty.orientation = .vertical
        empty.spacing = 10
        empty.setCustomSpacing(16, after: icon)

        for v in [scroll, empty] as [NSView] {
            v.translatesAutoresizingMaskIntoConstraints = false
            root.addSubview(v)
        }
        NSLayoutConstraint.activate([
            scroll.leadingAnchor.constraint(equalTo: root.leadingAnchor), scroll.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            scroll.topAnchor.constraint(equalTo: root.topAnchor), scroll.bottomAnchor.constraint(equalTo: root.bottomAnchor),
            empty.centerXAnchor.constraint(equalTo: root.centerXAnchor), empty.centerYAnchor.constraint(equalTo: root.centerYAnchor, constant: -20),
            empty.widthAnchor.constraint(lessThanOrEqualTo: root.widthAnchor, constant: -48),
            emptyHint.widthAnchor.constraint(lessThanOrEqualToConstant: 380),
        ])
        view = root
        NotificationCenter.default.addObserver(self, selector: #selector(resized), name: NSView.frameDidChangeNotification, object: scroll)
        scroll.postsFrameChangedNotifications = true
    }

    func show(_ image: CGImage?, areas: [CGRect]? = nil) {
        canvas.cancelInteraction()
        let changed = image?.width != canvas.image?.width || image?.height != canvas.image?.height
        canvas.image = image
        canvas.editsAreas = areas != nil
        canvas.areas = areas ?? []
        if areas == nil { canvas.selectedArea = nil }
        imageSize = image.map { CGSize(width: $0.width, height: $0.height) } ?? .zero
        canvas.isHidden = image == nil
        empty.isHidden = image != nil
        if changed { scroll.magnification = 1 }
        layoutCanvas()
    }

    /// The canvas fills the visible area at magnification 1 (zoom to fit), with a margin.
    func layoutCanvas() {
        guard imageSize.width > 0 else { canvas.frame = .zero; return }
        let avail = scroll.contentSize
        let pad: CGFloat = 24
        let s = min((avail.width - 2 * pad) / imageSize.width, (avail.height - 2 * pad) / imageSize.height)
        let size = NSSize(width: max(10, (imageSize.width * s).rounded()), height: max(10, (imageSize.height * s).rounded()))
        // Room for the margin around the page, centred by the clip view.
        canvas.frame = NSRect(origin: .zero, size: size)
        scroll.documentView?.needsDisplay = true
        scroll.reflectScrolledClipView(scroll.contentView)
    }

    @objc private func resized() {
        let m = scroll.magnification
        layoutCanvas()
        scroll.magnification = m
    }

    func zoom(by factor: CGFloat) {
        guard factor.isFinite, factor > 0, canvas.image != nil else { return }
        scroll.animator().magnification = min(scroll.maxMagnification, max(scroll.minMagnification, scroll.magnification * factor))
    }
    func zoomToFit() { scroll.animator().magnification = 1 }
    func actualSize() {
        guard canvas.frame.width > 0, let scale = view.window?.backingScaleFactor else { return }
        let magnification = imageSize.width / scale / canvas.frame.width
        guard magnification.isFinite, magnification > 0 else { return }
        scroll.minMagnification = min(scroll.minMagnification, magnification / 2)
        scroll.maxMagnification = max(scroll.maxMagnification, magnification * 2)
        scroll.animator().magnification = magnification
    }
}
