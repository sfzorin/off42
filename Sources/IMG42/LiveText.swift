import AppKit
import VisionKit

/// Live Text like Preview: VisionKit's `ImageAnalysisOverlayView` over the image, inside the zoomable document view.
/// Text recognition runs in the background when an image is shown and is cached per image; the overlay only
/// takes the mouse over text (or links, phone numbers…) and only in the Select tool, so markup keeps working.
final class LiveTextHost: NSView, ImageAnalysisOverlayViewDelegate {
    let overlay = ImageAnalysisOverlayView()
    /// Whether the overlay may take a click at this point (document coordinates, y down).
    var allowsInteraction: (CGPoint) -> Bool = { _ in false }
    /// Keys the overlay does not use go to the markup view (navigation, Escape…).
    weak var keyTarget: NSResponder?
    var onSelectionChange: (() -> Void)?
    /// Debug hook: print the context menu when it opens, then close it.
    var debugMenu = false

    private let analyzer = ImageAnalyzer()
    private var task: Task<Void, Never>?
    /// The image the current analysis belongs to, and a few recent ones (flipping back is instant).
    private(set) var analysedImage: CGImage?
    private var cache: [(image: CGImage, analysis: ImageAnalysis)] = []
    private var waiting: [@MainActor () -> Void] = []
    private var isAnalyzing = false

    override var isFlipped: Bool { true }

    override init(frame: NSRect) {
        super.init(frame: frame)
        overlay.frame = bounds
        overlay.autoresizingMask = [.width, .height]
        overlay.delegate = self
        overlay.preferredInteractionTypes = [.textSelection, .dataDetectors]
        overlay.isSupplementaryInterfaceHidden = true
        addSubview(overlay)
    }

    required init?(coder: NSCoder) { fatalError("not supported") }

    // MARK: Analysis

    static var languages: [String] {
        let supported = Set(ImageAnalyzer.supportedTextRecognitionLanguages)
        var out: [String] = []
        for l in Locale.preferredLanguages + ["en-US", "ru-RU", "tr-TR"] {
            let match = supported.contains(l) ? l : supported.first { $0.hasPrefix(String(l.prefix(2))) }
            if let match, !out.contains(match) { out.append(match) }
        }
        return out
    }

    /// Starts recognising `image` (no-op when it is already the current one).
    func analyze(_ image: CGImage?) {
        guard image !== analysedImage else { return }
        task?.cancel()
        waiting.removeAll()
        isAnalyzing = false
        overlay.resetSelection()
        overlay.analysis = nil
        analysedImage = image
        guard let image, ImageAnalyzer.isSupported else { flushWaiting(); return }
        if let hit = cache.first(where: { $0.image === image }) {
            overlay.analysis = hit.analysis
            flushWaiting()
            return
        }
        let analyzer = analyzer
        isAnalyzing = true
        var config = ImageAnalyzer.Configuration([.text, .machineReadableCode])
        config.locales = Self.languages
        task = Task { @MainActor [weak self] in
            let analysis = try? await analyzer.analyze(image, orientation: .up, configuration: config)
            guard let self, !Task.isCancelled, self.analysedImage === image else { return }
            self.isAnalyzing = false
            if let analysis {
                self.overlay.analysis = analysis
                self.cache.append((image, analysis))
                if self.cache.count > 4 { self.cache.removeFirst() }
            }
            self.flushWaiting()
        }
    }

    /// Calls `done` once the current image's analysis has finished (debug hooks).
    func whenAnalysed(_ done: @escaping @MainActor () -> Void) {
        if !isAnalyzing { done() } else { waiting.append(done) }
    }

    private func flushWaiting() {
        let w = waiting
        waiting = []
        w.forEach { $0() }
    }

    var hasSelection: Bool { overlay.hasActiveTextSelection && !overlay.selectedText.isEmpty }
    var hasText: Bool { !(overlay.analysis?.transcript.isEmpty ?? true) }

    func selectAllText() {
        let t = overlay.text
        guard !t.isEmpty else { return }
        overlay.selectedRanges = [t.startIndex..<t.endIndex]
        onSelectionChange?()
    }

    /// Selects from the first occurrence of `from` to the end of the following `to` (debug hook).
    @discardableResult
    func select(from: String, to: String) -> String {
        let t = overlay.text
        guard let a = t.range(of: from, options: [.caseInsensitive]) else { return "" }
        let b = t.range(of: to, options: [.caseInsensitive], range: a.lowerBound..<t.endIndex) ?? a
        overlay.selectedRanges = [a.lowerBound..<max(a.upperBound, b.upperBound)]
        onSelectionChange?()
        return overlay.selectedText
    }

    func copySelection() {
        let text = overlay.selectedText
        guard !text.isEmpty else { return }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }

    // MARK: Events

    override func hitTest(_ point: NSPoint) -> NSView? {
        guard !isHidden, let superview else { return nil }
        let p = convert(point, from: superview)
        guard bounds.contains(p), overlay.analysis != nil, allowsInteraction(p) else { return nil }
        // VisionKit's hit tests take top-left-origin points in the overlay's bounds (same frame as this view).
        let hit = overlay.hasInteractiveItem(at: p) || overlay.hasText(at: p)
        if ProcessInfo.processInfo.environment["K42_LIVETEXT_TRACE"] != nil {
            print("livetext: hit \(Int(p.x)),\(Int(p.y)) text=\(hit) view=\(String(describing: super.hitTest(point)))")
        }
        guard hit else { return nil }
        // Away from links / phone numbers the data-detector layer (topmost) would swallow drags: give them to the text layer.
        if !overlay.hasDataDetector(at: p), let selection = textSelectionView { return selection }
        return super.hitTest(point)
    }

    /// VisionKit's text selection layer (private class; nil if a later macOS renames it, then the default hit test applies).
    private var textSelectionView: NSView? {
        func find(_ v: NSView) -> NSView? {
            if String(describing: type(of: v)).contains("TextSelectionView") { return v }
            for s in v.subviews { if let f = find(s) { return f } }
            return nil
        }
        return find(overlay)
    }

    override func keyDown(with event: NSEvent) {
        if let keyTarget { keyTarget.keyDown(with: event) } else { super.keyDown(with: event) }
    }

    override func cancelOperation(_ sender: Any?) {
        guard overlay.hasActiveTextSelection else { super.cancelOperation(sender); return }
        overlay.resetSelection()
        onSelectionChange?()
    }

    // MARK: ImageAnalysisOverlayViewDelegate

    func contentsRect(for overlayView: ImageAnalysisOverlayView) -> CGRect { CGRect(x: 0, y: 0, width: 1, height: 1) }

    func overlayView(_ overlayView: ImageAnalysisOverlayView, shouldBeginAt point: CGPoint,
                     forAnalysisType analysisType: ImageAnalysisOverlayView.InteractionTypes) -> Bool {
        allowsInteraction(convert(point, from: overlayView))
    }

    func overlayView(_ overlayView: ImageAnalysisOverlayView, shouldHandleKeyDownEvent event: NSEvent) -> Bool {
        // VisionKit does not forward a rejected key to this host's keyDown. Clear its selection here,
        // and handle the standard cancelOperation route above when the text responder interprets Escape.
        if event.keyCode == 53, overlayView.hasActiveTextSelection { cancelOperation(nil) }
        // ⌘C / ⌘A go through the Edit menu (window controller).
        return false
    }

    func textSelectionDidChange(_ overlayView: ImageAnalysisOverlayView) { onSelectionChange?() }

    func overlayView(_ overlayView: ImageAnalysisOverlayView, willOpen menu: NSMenu) {
        guard debugMenu else { return }
        print("livetext: menu=" + menu.items.map { $0.isSeparatorItem ? "|" : $0.title }.joined(separator: ", "))
        DispatchQueue.main.async { menu.cancelTracking() }
    }
}

/// Synthesised mouse input for debug hooks (points in image pixels of the canvas document view).
enum MouseScript {
    private static var clock: TimeInterval = 0

    /// Queued (not dispatched): a control's mouse-tracking loop pulls them from the queue. Timestamps are spaced out.
    private static func send(_ type: NSEvent.EventType, _ p: CGPoint, in view: NSView, clicks: Int = 1, gap: TimeInterval = 0.02) {
        guard let window = view.window else { return }
        let loc = view.convert(p, to: nil)
        guard let e = NSEvent.mouseEvent(with: type, location: loc, modifierFlags: [], timestamp: nextTime(gap),
                                         windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: clicks,
                                         pressure: type == .leftMouseUp ? 0 : 1) else { return }
        // Drag deltas (gesture recognisers read them); NSEvent.mouseEvent leaves them at 0.
        if let cg = e.cgEvent {
            cg.setIntegerValueField(.mouseEventDeltaX, value: Int64((loc.x - last.x).rounded()))
            cg.setIntegerValueField(.mouseEventDeltaY, value: Int64((last.y - loc.y).rounded()))
            cg.setIntegerValueField(.mouseEventClickState, value: Int64(clicks))
            cg.setIntegerValueField(.mouseEventButtonNumber, value: 0)
            last = loc
            if let ns = NSEvent(cgEvent: cg) { NSApp.postEvent(ns, atStart: false); return }
        }
        last = loc
        NSApp.postEvent(e, atStart: false)
    }
    private static var last = CGPoint.zero

    private static func nextTime(_ gap: TimeInterval) -> TimeInterval {
        clock = max(clock + gap, ProcessInfo.processInfo.systemUptime)
        return clock
    }

    private static func run(_ steps: [() -> Void], interval: TimeInterval = 0) { steps.forEach { $0() } }

    static func drag(_ pts: [CGPoint], in view: NSView, substeps: Int = 8) {
        guard let a = pts.first, let b = pts.last else { return }
        var steps: [() -> Void] = [{ send(.leftMouseDown, a, in: view) }]
        var prev = a
        for p in pts.dropFirst() {
            for i in 1...substeps {
                let t = CGFloat(i) / CGFloat(substeps), q = CGPoint(x: prev.x + (p.x - prev.x) * t, y: prev.y + (p.y - prev.y) * t)
                steps.append { send(.leftMouseDragged, q, in: view) }
            }
            prev = p
        }
        steps.append { send(.leftMouseUp, b, in: view) }
        run(steps)
    }

    static func click(_ p: CGPoint, count: Int, in view: NSView) {
        var steps: [() -> Void] = []
        for n in 1...max(1, count) {
            steps.append { send(.leftMouseDown, p, in: view, clicks: n, gap: 0.08) }
            steps.append { send(.leftMouseUp, p, in: view, clicks: n) }
        }
        run(steps, interval: 0.05)
    }

    static func key(_ code: UInt16, chars: String, in view: NSView) {
        guard let window = view.window,
              let e = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [], timestamp: nextTime(0.05), windowNumber: window.windowNumber,
                                       context: nil, characters: chars, charactersIgnoringModifiers: chars, isARepeat: false, keyCode: code) else { return }
        NSApp.postEvent(e, atStart: false)
    }

    static func rightClick(_ p: CGPoint, in view: NSView) {
        send(.rightMouseDown, p, in: view)
        send(.rightMouseUp, p, in: view)
    }
}
