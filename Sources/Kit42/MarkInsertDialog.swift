import AppKit

/// Where a signature or stamp will be placed: its width on the page and, for a scanned page, the scan's resolution.
public struct MarkPlacement {
    /// Width of the placed mark in points (1/72 inch).
    public var widthPoints: CGFloat
    /// Resolution of the scanned page under the mark; nil for a page drawn in lines and text (vector).
    public var scanDPI: CGFloat?

    public init(widthPoints: CGFloat, scanDPI: CGFloat? = nil) {
        self.widthPoints = widthPoints
        self.scanDPI = scanDPI
    }

    /// The usual size: about 6 cm for a wide signature, about 4.2 cm for a seal.
    public static func standard(aspect: CGFloat, scanDPI: CGFloat? = nil) -> MarkPlacement {
        MarkPlacement(widthPoints: aspect > 2 ? 170 : 120, scanDPI: scanDPI)
    }
}

/// Before a signature or stamp goes into a document: a preview of this impression and the resolution to render it at.
/// A scanned page offers the scan's own resolution (so the ink is as sharp as the paper around it); a vector page
/// takes the maximum.
public enum MarkInsertDialog {
    public enum Source {
        /// A generated or imported mark: each impression is drawn anew at the chosen resolution.
        case mark(GeneratedMark)
        /// A fixed picture: resampled to the chosen resolution.
        case picture(CGImage)
    }

    public static let maximumDPI: CGFloat = 1200
    static let choices: [CGFloat] = [300, 600, maximumDPI]

    /// Shows the preview as a sheet on `window` (modal without one). `insert` receives the image at the chosen
    /// resolution; nothing happens when cancelled. `placement` gives the placement for the mark's aspect ratio.
    public static func present(_ source: Source, over window: NSWindow?, placement: @escaping (CGFloat) -> MarkPlacement,
                               insert: @escaping (CGImage) -> Void) {
        // K42_MARK_DPI (audits): skip the dialog and render at this resolution ("max" or a number).
        if let fixed = ProcessInfo.processInfo.environment["K42_MARK_DPI"] {
            var seed = SeededRandom.freshSeed()
            guard let preview = previewImage(source, seed: &seed) else { return }
            let place = placement(aspect(preview))
            let dpi = fixed == "max" ? maximumDPI : CGFloat(Double(fixed) ?? Double(maximumDPI))
            if let image = render(source, seed: seed, preview: preview, widthPoints: place.widthPoints, dpi: dpi) { insert(image) }
            return
        }
        Controller(source: source, placement: placement).present(over: window, insert: insert)
    }

    // MARK: - Rendering

    /// The impression `seed` of `source` drawn so that it is `dpi` at `widthPoints` (what Insert produces).
    public static func image(_ source: Source, seed: UInt64, widthPoints: CGFloat, dpi: CGFloat) -> CGImage? {
        var s = seed
        guard let preview = previewImage(source, seed: &s) else { return nil }
        return render(source, seed: s, preview: preview, widthPoints: widthPoints, dpi: dpi)
    }

    static func aspect(_ image: CGImage) -> CGFloat { CGFloat(image.width) / CGFloat(max(1, image.height)) }

    /// The impression at the generator's base resolution (fast, for the preview). Pictures are shown as they are.
    static func previewImage(_ source: Source, seed: inout UInt64) -> CGImage? {
        switch source {
        case .mark(let mark): return mark.impression(seed, scale: 1)
        case .picture(let image): return image
        }
    }

    /// Pixel width for `dpi` at the placed width.
    static func pixelWidth(widthPoints: CGFloat, dpi: CGFloat) -> Int {
        Int((widthPoints / 72 * dpi).rounded())
    }

    /// The same impression as `preview`, drawn so that it is `dpi` at `widthPoints`.
    static func render(_ source: Source, seed: UInt64, preview: CGImage, widthPoints: CGFloat, dpi: CGFloat) -> CGImage? {
        let target = CGFloat(pixelWidth(widthPoints: widthPoints, dpi: dpi))
        switch source {
        case .mark(let mark):
            let k = max(0.25, min(8, target / CGFloat(max(1, preview.width))))
            return mark.impression(seed, scale: k)
        case .picture(let image):
            return resample(image, width: Int(target))
        }
    }

    /// High-quality resampling of a fixed picture (up or down) to `width` pixels.
    static func resample(_ image: CGImage, width: Int) -> CGImage? {
        guard width > 0, width != image.width else { return image }
        let height = max(1, Int((CGFloat(width) / aspect(image)).rounded()))
        guard let ctx = InkDraw.context(width, height) else { return image }
        ctx.interpolationQuality = .high
        ctx.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
        return ctx.makeImage() ?? image
    }

    // MARK: - Dialog

    private final class Controller: NSObject {
        let source: Source
        let placementFor: (CGFloat) -> MarkPlacement
        var seed = SeededRandom.freshSeed()
        var preview: CGImage?
        var placement = MarkPlacement.standard(aspect: 1)
        let imageView = NSImageView()
        let dpiPopup = NSPopUpButton(frame: .zero, pullsDown: false)
        let info = NSTextField(labelWithString: "")
        var dpis: [CGFloat] = []

        init(source: Source, placement: @escaping (CGFloat) -> MarkPlacement) {
            self.source = source
            self.placementFor = placement
        }

        func present(over window: NSWindow?, insert: @escaping (CGImage) -> Void) {
            guard renderPreview() else { NSSound.beep(); return }
            let alert = NSAlert()
            alert.messageText = L("Insert Signature or Stamp")
            alert.informativeText = placement.scanDPI != nil
                ? L("The page is a scan: matching its resolution makes the ink as sharp as the paper around it.")
                : L("The page is drawn in lines and text: the maximum resolution keeps the ink sharp at any zoom.")
            alert.addButton(withTitle: L("Insert"))
            alert.addButton(withTitle: L("Cancel"))
            alert.accessoryView = accessory()
            alert.window.initialFirstResponder = dpiPopup
            // The controller stays alive with the sheet's handler.
            let finish: (NSApplication.ModalResponse) -> Void = { response in
                guard response == .alertFirstButtonReturn, let preview = self.preview else { return }
                let dpi = self.dpis[max(0, self.dpiPopup.indexOfSelectedItem)]
                if let image = MarkInsertDialog.render(self.source, seed: self.seed, preview: preview,
                                                       widthPoints: self.placement.widthPoints, dpi: dpi) { insert(image) }
            }
            if let window, window.attachedSheet == nil {
                alert.beginSheetModal(for: window, completionHandler: finish)
            } else {
                finish(alert.runModal())
            }
        }

        @discardableResult
        func renderPreview() -> Bool {
            guard let image = MarkInsertDialog.previewImage(source, seed: &seed) else { return false }
            preview = image
            placement = placementFor(MarkInsertDialog.aspect(image))
            imageView.image = NSImage(cgImage: image, size: NSSize(width: image.width, height: image.height))
            return true
        }

        func accessory() -> NSView {
            imageView.imageScaling = .scaleProportionallyDown
            imageView.wantsLayer = true
            // Ink on paper: the preview stays white in dark mode too.
            imageView.layer?.backgroundColor = NSColor.white.cgColor
            imageView.layer?.cornerRadius = Theme42.cornerRadius
            imageView.layer?.borderWidth = 0.5
            imageView.layer?.borderColor = NSColor(white: 0.5, alpha: 0.5).cgColor
            imageView.setAccessibilityIdentifier("Kit42.markInsertPreview")
            imageView.translatesAutoresizingMaskIntoConstraints = false

            var titles: [String] = []
            if let scan = placement.scanDPI {
                let rounded = (scan / 10).rounded() * 10
                dpis.append(rounded)
                titles.append(L("Match the scan (%d dpi)", Int(rounded)))
            }
            for d in MarkInsertDialog.choices where !dpis.contains(d) {
                dpis.append(d)
                titles.append(d == MarkInsertDialog.maximumDPI ? L("%d dpi (maximum)", Int(d)) : L("%d dpi", Int(d)))
            }
            dpiPopup.addItems(withTitles: titles)
            dpiPopup.selectItem(at: placement.scanDPI != nil ? 0 : dpis.count - 1)
            dpiPopup.target = self
            dpiPopup.action = #selector(updateInfo)
            dpiPopup.setAccessibilityIdentifier("Kit42.markInsertDPI")

            let label = NSTextField(labelWithString: L("Resolution:"))
            info.textColor = .secondaryLabelColor
            info.font = .systemFont(ofSize: 11)
            let row = NSStackView(views: [label, dpiPopup])
            row.spacing = 8
            // Honest about what this is: a picture, not a signature.
            let note = NSTextField(wrappingLabelWithString: L("A picture of a signature does not prove who signed or that the document is unchanged. For that, sign the PDF digitally."))
            note.textColor = .secondaryLabelColor
            note.font = .systemFont(ofSize: 11)
            note.preferredMaxLayoutWidth = 340
            note.translatesAutoresizingMaskIntoConstraints = false
            note.widthAnchor.constraint(equalToConstant: 340).isActive = true
            var views: [NSView] = [imageView, row, info, note]
            if case .mark = source {
                let again = NSButton(title: L("New Impression"), target: self, action: #selector(newImpression))
                again.bezelStyle = .push
                again.toolTip = L("Every insertion is a new impression; draw another one")
                views.insert(again, at: 1)
            }
            let stack = NSStackView(views: views)
            stack.orientation = .vertical
            stack.alignment = .leading
            stack.spacing = 10
            NSLayoutConstraint.activate([
                imageView.widthAnchor.constraint(equalToConstant: 340),
                imageView.heightAnchor.constraint(equalToConstant: 160),
            ])
            updateInfo()
            stack.frame = NSRect(origin: .zero, size: stack.fittingSize)
            return stack
        }

        @objc func newImpression() {
            seed = SeededRandom.freshSeed()
            renderPreview()
            updateInfo()
        }

        @objc func updateInfo() {
            guard let preview else { return }
            let dpi = dpis[max(0, dpiPopup.indexOfSelectedItem)]
            let w = MarkInsertDialog.pixelWidth(widthPoints: placement.widthPoints, dpi: dpi)
            let h = Int((CGFloat(w) / MarkInsertDialog.aspect(preview)).rounded())
            let mm = placement.widthPoints / 72 * 25.4
            info.stringValue = L("%d × %d px, %d mm wide", w, h, Int(mm.rounded()))
        }
    }
}
