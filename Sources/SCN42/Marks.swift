import AppKit
import Kit42

// Signatures and stamps on scanned pages. The mark is placed over the page by hand and then printed into the page
// pixels (multiplied like ink on paper): it becomes part of the scan, also in a saved PDF, not a separate layer.

extension ScanPage {
    /// Prints `mark` into the page at `rect` (fractions of the page, origin top left). Both the geometry-corrected
    /// original and the shown image get it, so re-running clean-up or straightening keeps the mark.
    func printMark(_ mark: CGImage, at rect: CGRect) {
        guard let newBase = MarkPrinter.print(mark, into: base, at: rect),
              let newImage = MarkPrinter.print(mark, into: image, at: rect) else { return }
        replacePixels(base: ScanPage.compact(newBase), image: newImage)
    }
}

enum MarkPrinter {
    /// `page` with `mark` multiplied over it; keeps the page's kind (colour, gray or 1-bit black and white).
    static func print(_ mark: CGImage, into page: CGImage, at rect: CGRect) -> CGImage? {
        let oneBit = page.bitsPerPixel == 1
        let gray = oneBit || page.colorSpace?.model == .monochrome
        var bitmap = Bitmap(page, gray: gray)
        guard bitmap.width > 0, bitmap.height > 0, bitmap.data.count == bitmap.width * bitmap.height * bitmap.channels else { return nil }
        let w = CGFloat(bitmap.width), h = CGFloat(bitmap.height)
        let target = CGRect(x: rect.minX * w, y: (1 - rect.maxY) * h, width: rect.width * w, height: rect.height * h)
        let channels = bitmap.channels, width = bitmap.width, height = bitmap.height
        bitmap.data.withUnsafeMutableBytes { raw in
            guard let ctx = CGContext(data: raw.baseAddress, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * channels,
                                      space: gray ? Bitmap.gray : Bitmap.sRGB,
                                      bitmapInfo: gray ? CGImageAlphaInfo.none.rawValue : CGImageAlphaInfo.noneSkipLast.rawValue) else { return }
            ctx.interpolationQuality = .high
            ctx.setBlendMode(.multiply)
            ctx.draw(mark, in: target)
        }
        return oneBit ? bitmap.oneBitImage(threshold: 140) : bitmap.cgImage()
    }

    /// Initial size as a fraction of the page width: about 5 cm for signatures and wide stamps, 4 cm for round seals.
    static func defaultWidth(of mark: CGImage, pageWidth: Int, dpi: Double) -> CGFloat {
        let ratio = CGFloat(mark.width) / CGFloat(max(1, mark.height))
        let inches: CGFloat = ratio > 1.5 ? 2.0 : 1.6
        let pixels = inches * CGFloat(dpi > 0 ? dpi : 300)
        return min(0.6, max(0.08, pixels / CGFloat(max(1, pageWidth))))
    }
}

/// Placement of one mark over the preview: drag to move, drag a corner to resize, Return to print it into the page,
/// Esc to cancel. The mark is shown multiplied, as it will look once printed.
final class MarkPlacementView: NSView {
    let mark: CGImage
    /// Fractions of the page, origin top left.
    var rect: CGRect { didSet { needsDisplay = true } }
    let pageID: UUID
    var onApply: ((MarkPlacementView) -> Void)?
    var onCancel: ((MarkPlacementView) -> Void)?
    private var drag: (corner: Int?, start: CGPoint, original: CGRect)?
    private let accent = Theme42.App.scn.accent

    init(mark: CGImage, rect: CGRect, pageID: UUID) {
        self.mark = mark
        self.rect = rect
        self.pageID = pageID
        super.init(frame: .zero)
        setAccessibilityElement(true)
        setAccessibilityRole(.image)
        setAccessibilityLabel(L("Signature or stamp being placed"))
    }

    required init?(coder: NSCoder) { fatalError() }

    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }
    /// The mark can be grabbed even while the window is in the background.
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    var viewRect: CGRect {
        CGRect(x: rect.minX * bounds.width, y: rect.minY * bounds.height, width: rect.width * bounds.width, height: rect.height * bounds.height)
    }

    private var corners: [CGPoint] {
        let r = viewRect
        return [CGPoint(x: r.minX, y: r.minY), CGPoint(x: r.maxX, y: r.minY), CGPoint(x: r.minX, y: r.maxY), CGPoint(x: r.maxX, y: r.maxY)]
    }

    override func draw(_ dirtyRect: NSRect) {
        guard let ctx = NSGraphicsContext.current?.cgContext else { return }
        let r = viewRect
        ctx.saveGState()
        ctx.setBlendMode(.multiply)
        ctx.translateBy(x: r.minX, y: r.maxY)
        ctx.scaleBy(x: 1, y: -1)
        ctx.interpolationQuality = .high
        ctx.draw(mark, in: CGRect(origin: .zero, size: r.size))
        ctx.restoreGState()
        let scale = max(0.2, 1 / max(0.01, enclosingScrollView?.magnification ?? 1))
        let frame = NSBezierPath(rect: r)
        frame.lineWidth = scale
        frame.setLineDash([4 * scale, 3 * scale], count: 2, phase: 0)
        accent.setStroke()
        frame.stroke()
        for c in corners {
            let s = 9 * scale
            let handle = NSBezierPath(ovalIn: CGRect(x: c.x - s / 2, y: c.y - s / 2, width: s, height: s))
            NSColor.white.setFill(); handle.fill()
            handle.lineWidth = 1.5 * scale
            accent.setStroke(); handle.stroke()
        }
    }

    override func hitTest(_ point: NSPoint) -> NSView? {
        // Only the mark and its handles take clicks; elsewhere the page behaves as usual.
        let p = convert(point, from: superview)
        let slop = 8 / max(0.01, enclosingScrollView?.magnification ?? 1)
        return viewRect.insetBy(dx: -slop, dy: -slop).contains(p) ? self : nil
    }

    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        if event.clickCount == 2 { onApply?(self); return }
        let p = convert(event.locationInWindow, from: nil)
        let slop = 9 / max(0.01, enclosingScrollView?.magnification ?? 1)
        let corner = corners.firstIndex { hypot($0.x - p.x, $0.y - p.y) < slop }
        drag = (corner, p, rect)
    }

    override func mouseDragged(with event: NSEvent) {
        guard let drag, bounds.width > 0, bounds.height > 0 else { return }
        let p = convert(event.locationInWindow, from: nil)
        let dx = (p.x - drag.start.x) / bounds.width, dy = (p.y - drag.start.y) / bounds.height
        let o = drag.original
        guard let corner = drag.corner else {
            rect = o.offsetBy(dx: dx, dy: dy)
            return
        }
        // Resize from the opposite corner, keeping the mark's proportions.
        let aspect = o.height / max(0.0001, o.width)
        let left = corner == 0 || corner == 2, top = corner == 0 || corner == 1
        let fixed = CGPoint(x: left ? o.maxX : o.minX, y: top ? o.maxY : o.minY)
        let width = max(0.03, min(1.2, left ? o.width - dx : o.width + dx))
        let height = width * aspect
        rect = CGRect(x: left ? fixed.x - width : fixed.x, y: top ? fixed.y - height : fixed.y, width: width, height: height)
    }

    override func mouseUp(with event: NSEvent) { drag = nil }

    override func keyDown(with event: NSEvent) {
        let step: CGFloat = event.modifierFlags.contains(.shift) ? 0.02 : 0.004
        switch event.keyCode {
        case 36, 76: onApply?(self)                  // Return, Enter
        case 53: onCancel?(self)                     // Esc
        case 123: rect = rect.offsetBy(dx: -step, dy: 0)
        case 124: rect = rect.offsetBy(dx: step, dy: 0)
        case 125: rect = rect.offsetBy(dx: 0, dy: step)
        case 126: rect = rect.offsetBy(dx: 0, dy: -step)
        default: super.keyDown(with: event)
        }
    }

    override func cancelOperation(_ sender: Any?) { onCancel?(self) }
}

extension ScanWindowController {
    private static var placements: [ObjectIdentifier: (view: MarkPlacementView, bar: NSView)] = [:]

    var markPlacement: MarkPlacementView? { Self.placements[ObjectIdentifier(self)]?.view }
    /// The library popover last opened for a page (kept for closing it and for the audit).
    static weak var stampPopover: NSPopover?

    /// Tools › Signature or Stamp…: pick a signature or stamp (each insertion is a new impression), then place it.
    @objc func showStampsAction(_ sender: Any?) {
        guard shownPage != nil, busyText == nil else { NSSound.beep(); return }
        let popover = NSPopover()
        let vc = StampLibraryViewController { [weak self] image in self?.beginMarkPlacement(image) }
        // Drawn at the scan's resolution, at the width placement starts with (2 in for a signature, 1.6 in for a seal).
        vc.placement = { [weak self] aspect in
            let dpi = self?.shownPage.map { CGFloat($0.dpi) }
            return MarkPlacement(widthPoints: (aspect > 1.5 ? 2.0 : 1.6) * 72, scanDPI: dpi.flatMap { $0 > 0 ? $0 : nil })
        }
        vc.popover = popover
        popover.contentViewController = vc
        popover.behavior = .transient
        Self.stampPopover = popover
        if let view = sender as? NSView, view.window != nil {
            popover.show(relativeTo: view.bounds, of: view, preferredEdge: .minY)
        } else {
            let v = preview.view
            popover.show(relativeTo: NSRect(x: v.bounds.midX, y: v.bounds.maxY - 20, width: 1, height: 1), of: v, preferredEdge: .minY)
        }
    }

    func beginMarkPlacement(_ mark: CGImage) {
        guard let page = shownPage else { NSSound.beep(); return }
        cancelMarkPlacement()
        let width = MarkPrinter.defaultWidth(of: mark, pageWidth: page.image.width, dpi: page.dpi)
        let pageAspect = CGFloat(page.image.width) / CGFloat(max(1, page.image.height))
        let height = width * CGFloat(mark.height) / CGFloat(max(1, mark.width)) * pageAspect
        let rect = CGRect(x: min(0.95 - width, 0.62 - width / 2), y: min(0.95 - height, 0.8 - height / 2), width: width, height: height)
        let view = MarkPlacementView(mark: mark, rect: rect, pageID: page.id)
        view.frame = preview.canvas.bounds
        view.autoresizingMask = [.width, .height]
        view.onApply = { [weak self] v in self?.applyMarkPlacement(v) }
        view.onCancel = { [weak self] _ in self?.cancelMarkPlacement() }
        preview.canvas.addSubview(view)

        let hint = NSTextField(labelWithString: L("Drag to place, drag a corner to resize"))
        hint.font = .systemFont(ofSize: 12)
        hint.textColor = .secondaryLabelColor
        let cancel = NSButton(title: L("Cancel"), target: view, action: #selector(MarkPlacementView.cancelOperation(_:)))
        cancel.keyEquivalent = "\u{1b}"
        let apply = NSButton(title: L("Print into Page"), target: self, action: #selector(applyMarkPlacementAction(_:)))
        apply.keyEquivalent = "\r"
        apply.bezelColor = Theme42.App.scn.accent
        let bar = NSStackView(views: [hint, cancel, apply])
        bar.spacing = 10
        bar.edgeInsets = NSEdgeInsets(top: 6, left: 12, bottom: 6, right: 8)
        bar.wantsLayer = true
        bar.layer?.cornerRadius = 10
        bar.layer?.backgroundColor = NSColor.windowBackgroundColor.withAlphaComponent(0.94).cgColor
        bar.layer?.borderColor = NSColor.separatorColor.cgColor
        bar.layer?.borderWidth = 1
        bar.setAccessibilityIdentifier("SCN42.markPlacementBar")
        bar.translatesAutoresizingMaskIntoConstraints = false
        preview.view.addSubview(bar)
        NSLayoutConstraint.activate([
            bar.centerXAnchor.constraint(equalTo: preview.view.centerXAnchor),
            bar.topAnchor.constraint(equalTo: preview.view.topAnchor, constant: 10),
        ])
        Self.placements[ObjectIdentifier(self)] = (view, bar)
        window?.makeFirstResponder(view)
        view.scrollToVisible(view.viewRect.insetBy(dx: -20, dy: -20))
    }

    @objc func applyMarkPlacementAction(_ sender: Any?) {
        if let view = markPlacement { applyMarkPlacement(view) }
    }

    func applyMarkPlacement(_ view: MarkPlacementView) {
        guard busyText == nil, let page = pages.first(where: { $0.id == view.pageID }), page === shownPage else {
            NSSound.beep(); cancelMarkPlacement(); return
        }
        let mark = view.mark, rect = view.rect
        cancelMarkPlacement()
        doc.modify([page], name: L("Signature or Stamp")) { $0.printMark(mark, at: rect) }
    }

    func cancelMarkPlacement() {
        guard let placement = Self.placements.removeValue(forKey: ObjectIdentifier(self)) else { return }
        placement.view.removeFromSuperview()
        placement.bar.removeFromSuperview()
        window?.makeFirstResponder(preview.canvas)
    }

    /// A placement belongs to the page it was started on; showing another page or the glass drops it.
    func dropStalePlacement() {
        guard let view = markPlacement else { return }
        if shownPage?.id != view.pageID || busyText != nil { cancelMarkPlacement() }
    }
}
