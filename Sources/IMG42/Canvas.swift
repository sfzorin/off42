import AppKit
import QuartzCore

extension NSAppearance {
    var isDark: Bool { bestMatch(from: [.aqua, .darkAqua]) == .darkAqua }
}

/// Keeps a document smaller than the viewport centred.
final class CenteringClipView: NSClipView {
    override func constrainBoundsRect(_ proposedBounds: NSRect) -> NSRect {
        var r = super.constrainBoundsRect(proposedBounds)
        guard let doc = documentView else { return r }
        let f = doc.frame
        if r.width > f.width { r.origin.x = f.minX - (r.width - f.width) / 2 }
        if r.height > f.height { r.origin.y = f.minY - (r.height - f.height) / 2 }
        return r
    }
}

/// Holds the image layer and the markup layer; draws the transparency checkerboard.
final class CanvasDocumentView: NSView {
    var showsCheckerboard = false { didSet { needsDisplay = true } }
    var magnification: () -> CGFloat = { 1 }
    override var isFlipped: Bool { true }

    /// Checkerboard squares: light in light mode, dark grey in dark mode (like Preview).
    static let checkerLight = NSColor(name: "IMG42.checkerLight") { $0.isDark ? NSColor(white: 0.30, alpha: 1) : .white }
    static let checkerDark = NSColor(name: "IMG42.checkerDark") { $0.isDark ? NSColor(white: 0.22, alpha: 1) : NSColor(white: 0.84, alpha: 1) }

    override func draw(_ rect: NSRect) {
        guard showsCheckerboard else { return }
        // Views do not clip to bounds (macOS 14+): the dirty rect may cover the whole viewport.
        let dirtyRect = rect.intersection(bounds)
        guard !dirtyRect.isEmpty else { return }
        let square = 8 / max(0.0001, magnification())
        Self.checkerLight.setFill()
        dirtyRect.fill()
        Self.checkerDark.setFill()
        let x0 = Int(floor(dirtyRect.minX / square)), x1 = Int(ceil(dirtyRect.maxX / square))
        let y0 = Int(floor(dirtyRect.minY / square)), y1 = Int(ceil(dirtyRect.maxY / square))
        guard (x1 - x0) * (y1 - y0) < 400_000 else { return }
        for y in y0..<y1 {
            for x in x0..<x1 where (x + y) % 2 == 0 {
                CGRect(x: CGFloat(x) * square, y: CGFloat(y) * square, width: square, height: square).intersection(bounds).fill()
            }
        }
    }
}

/// Plain layer-backed view showing a CGImage, scaled by the GPU.
final class ImageLayerView: NSView {
    var image: CGImage? {
        didSet { layer?.contents = image }
    }

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layer?.contentsGravity = .resize
        layer?.minificationFilter = .trilinear
        layer?.magnificationFilter = .linear
        layerContentsRedrawPolicy = .never
    }

    required init?(coder: NSCoder) { fatalError("not supported") }

    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    func setPixelated(_ on: Bool) {
        layer?.magnificationFilter = on ? .nearest : .linear
    }
}

/// Scroll view with zoom (pinch, ⌘-scroll, keyboard), centred image, and navigation gestures.
final class ImageCanvas: NSScrollView {
    let document = CanvasDocumentView()
    let imageView = ImageLayerView()
    let markup = MarkupView()
    let liveText = LiveTextHost()

    enum ZoomMode { case fit, custom }
    var zoomMode: ZoomMode = .fit
    var imageSize: CGSize = .zero
    var onNavigate: ((Int) -> Void)?
    var onZoomChanged: (() -> Void)?
    /// Whether wheel / swipe gestures may switch images.
    var navigationEnabled = true

    override init(frame: NSRect) {
        super.init(frame: frame)
        contentView = CenteringClipView()
        hasHorizontalScroller = true
        hasVerticalScroller = true
        autohidesScrollers = true
        scrollerStyle = .overlay
        allowsMagnification = true
        minMagnification = 0.01
        maxMagnification = 64
        borderType = .noBorder
        usesPredominantAxisScrolling = false
        documentView = document
        document.addSubview(imageView)
        document.addSubview(markup)
        document.addSubview(liveText)
        liveText.keyTarget = markup
        document.magnification = { [weak self] in self?.magnification ?? 1 }
        markup.magnification = { [weak self] in self?.magnification ?? 1 }
        NotificationCenter.default.addObserver(self, selector: #selector(liveMagnifyEnded), name: NSScrollView.didEndLiveMagnifyNotification, object: self)
    }

    required init?(coder: NSCoder) { fatalError("not supported") }

    deinit { NotificationCenter.default.removeObserver(self) }

    func setImage(_ image: CGImage?, size: CGSize, resetZoom: Bool) {
        let sizeChanged = size != imageSize
        if sizeChanged || image !== imageView.image { markup.invalidateSelectionWork() }
        imageSize = size
        imageView.image = image
        if sizeChanged {
            document.frame = CGRect(origin: .zero, size: size)
            imageView.frame = document.bounds
            markup.frame = document.bounds
            liveText.frame = document.bounds
        }
        if resetZoom { zoomMode = .fit }
        if resetZoom || sizeChanged { applyZoom() }
    }

    var fitMagnification: CGFloat {
        guard imageSize.width > 0, imageSize.height > 0 else { return 1 }
        let viewport = contentSize
        let margin: CGFloat = 0
        let m = min((viewport.width - margin) / imageSize.width, (viewport.height - margin) / imageSize.height)
        // Small images are not blown up: they show at 100 %.
        return max(minMagnification, min(1, m))
    }

    /// True when the whole image is visible (navigation gestures are safe).
    var imageFits: Bool { magnification <= fitMagnification + 0.0001 }

    func applyZoom() {
        if zoomMode == .fit {
            magnification = fitMagnification
            centerImage()
        }
        updateFilters()
        onZoomChanged?()
    }

    func centerImage() {
        let clip = contentView
        let r = clip.constrainBoundsRect(NSRect(x: (document.frame.width - clip.bounds.width) / 2, y: (document.frame.height - clip.bounds.height) / 2,
                                                width: clip.bounds.width, height: clip.bounds.height))
        clip.scroll(to: r.origin)
        reflectScrolledClipView(clip)
    }

    func zoom(to m: CGFloat, around point: NSPoint? = nil) {
        guard m.isFinite, m > 0 else { return }
        zoomMode = .custom
        let target = max(minMagnification, min(maxMagnification, m))
        let center = point ?? NSPoint(x: contentView.bounds.midX, y: contentView.bounds.midY)
        setMagnification(target, centeredAt: center)
        updateFilters()
        onZoomChanged?()
    }

    func zoomBy(_ factor: CGFloat) { zoom(to: magnification * factor) }

    func zoomToFit() {
        zoomMode = .fit
        applyZoom()
    }

    func toggleFitActual(at point: NSPoint? = nil) {
        if zoomMode == .fit && abs(magnification - 1) > 0.001 { zoom(to: 1, around: point) } else { zoomToFit() }
    }

    private func updateFilters() {
        imageView.setPixelated(magnification >= 3)
        document.needsDisplay = true
        markup.needsDisplay = true
    }

    override func layout() {
        super.layout()
        if zoomMode == .fit { applyZoom() }
    }

    @objc private func liveMagnifyEnded() {
        zoomMode = abs(magnification - fitMagnification) < 0.001 ? .fit : .custom
        updateFilters()
        onZoomChanged?()
    }

    override func smartMagnify(with event: NSEvent) {
        toggleFitActual(at: document.convert(event.locationInWindow, from: nil))
    }

    // MARK: Gestures

    private var swipeAccumulator: CGFloat = 0
    private var swipeConsumed = false

    override func scrollWheel(with event: NSEvent) {
        if event.modifierFlags.contains(.command) {
            let delta = event.hasPreciseScrollingDeltas ? event.scrollingDeltaY / 200 : event.scrollingDeltaY / 20
            zoom(to: magnification * (1 + delta), around: document.convert(event.locationInWindow, from: nil))
            return
        }
        if navigationEnabled, imageFits {
            if !event.hasPreciseScrollingDeltas {
                // Mouse wheel: one notch → next / previous image.
                let d = abs(event.scrollingDeltaY) >= abs(event.scrollingDeltaX) ? event.scrollingDeltaY : event.scrollingDeltaX
                if d != 0 { onNavigate?(d < 0 ? 1 : -1) }
                return
            }
            // Trackpad: a horizontal two-finger swipe flips the image.
            if event.phase == .began { swipeAccumulator = 0; swipeConsumed = false }
            if event.phase != [] || event.momentumPhase == [] {
                if abs(event.scrollingDeltaX) > abs(event.scrollingDeltaY) || swipeAccumulator != 0 {
                    swipeAccumulator += event.scrollingDeltaX
                    if !swipeConsumed, abs(swipeAccumulator) > 50 {
                        swipeConsumed = true
                        onNavigate?(swipeAccumulator < 0 ? 1 : -1)
                    }
                    return
                }
            }
            if event.momentumPhase != [] && swipeAccumulator != 0 { return }
        }
        super.scrollWheel(with: event)
    }

    override func swipe(with event: NSEvent) {
        if navigationEnabled, imageFits, event.deltaX != 0 { onNavigate?(event.deltaX < 0 ? 1 : -1) }
    }
}
