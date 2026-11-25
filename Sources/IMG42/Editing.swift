import AppKit
import Kit42
import UniformTypeIdentifiers

/// Everything that can be undone: the working image, its marks, adjustments and description.
struct EditState {
    var base: CGImage?
    var marks: [Mark]
    var adjustments: Adjustments
    var imageDescription: String?
    var dpi: CGFloat
    var pixelSelection: PixelSelection? = nil
    var selectedMarkID: UUID? = nil
}

extension ImageWindowController {
    var isDirty: Bool {
        guard base != nil else { return false }
        return base !== originalBase || canvas.markup.hasPendingTextEdit || !canvas.markup.marks.isEmpty || !adjustments.isIdentity
            || imageDescription != originalDescription || imageDPI != originalDPI
    }

    /// Metadata-only edits can be written without re-encoding pixels.
    var onlyMetadataChanged: Bool {
        base === originalBase && canvas.markup.marks.isEmpty && adjustments.isIdentity
            && (imageDescription != originalDescription || imageDPI != originalDPI)
    }

    func resetEditing(base image: CGImage?) {
        editGeneration = UUID()
        // Reset is a discard boundary; Save/Revert take their committed snapshot before entering here.
        canvas.markup.cancelInteraction()
        canvas.markup.cancelTextEditing()
        renderWork?.cancel(); renderWork = nil
        Self.adjustStart.removeValue(forKey: ObjectIdentifier(self))
        showingOriginal = false
        canvas.markup.hideMarks = false
        originalBase = image
        base = image
        adjustments = Adjustments()
        autoFilters = nil
        displayed = image
        canvas.markup.marks = []
        canvas.markup.selectedID = nil
        canvas.markup.pixelSelection = nil
        originalDescription = url.flatMap { Self.description(of: $0) }
        imageDescription = originalDescription
        let savedDPI = url.flatMap { ImageLoader.properties($0)[kCGImagePropertyDPIWidth] as? CGFloat } ?? 72
        originalDPI = savedDPI.isFinite && savedDPI > 0 ? savedDPI : 72
        imageDPI = originalDPI
        confirmedOverwrite = false
        // Edits belong to this image; file operations (trash, lossless rotation) stay undoable.
        undo.removeAllActions(withTarget: self)
        if inspectorShows === adjustPanel { adjustPanel.set(adjustments) }
    }

    static func description(of url: URL) -> String? {
        let p = ImageLoader.properties(url)
        let iptc = p[kCGImagePropertyIPTCDictionary] as? [CFString: Any]
        let tiff = p[kCGImagePropertyTIFFDictionary] as? [CFString: Any]
        let s = iptc?[kCGImagePropertyIPTCCaptionAbstract] as? String ?? tiff?[kCGImagePropertyTIFFImageDescription] as? String
        return s?.isEmpty == false ? s : nil
    }

    // MARK: - Undo

    var state: EditState {
        EditState(base: base, marks: canvas.markup.marks, adjustments: adjustments, imageDescription: imageDescription,
                  dpi: imageDPI, pixelSelection: canvas.markup.pixelSelection, selectedMarkID: canvas.markup.selectedID)
    }

    func restore(_ s: EditState) {
        editGeneration = UUID()
        canvas.markup.cancelInteraction()
        canvas.markup.cancelTextEditing()
        renderWork?.cancel(); renderWork = nil
        showingOriginal = false; canvas.markup.hideMarks = false
        let sizeChanged = s.base?.width != base?.width || s.base?.height != base?.height
        base = s.base
        canvas.markup.marks = s.marks
        adjustments = s.adjustments
        imageDescription = s.imageDescription
        imageDPI = s.dpi
        autoFilters = nil
        if inspectorShows === adjustPanel { adjustPanel.set(adjustments) }
        refreshDisplay(resize: sizeChanged)
        canvas.markup.pixelSelection = s.pixelSelection
        canvas.markup.selectedID = s.selectedMarkID.flatMap { id in s.marks.contains(where: { $0.id == id }) ? id : nil }
        markupBar.selectionChanged()
    }

    func registerUndo(_ old: EditState, _ name: String) {
        editGeneration = UUID()
        undo.registerUndo(withTarget: self) { target in
            let current = target.state
            target.restore(old)
            target.registerUndo(current, name)
        }
        undo.setActionName(name)
        updateSubtitle()
    }

    func marksCommitted(before: [Mark], name: String) {
        var old = state
        old.marks = before
        registerUndo(old, name)
        markupBar.selectionChanged()
    }

    /// Replaces the working image (crop, resize, rotate, remove background); marks follow with `transform`.
    func replaceBase(_ image: CGImage, transform: CGAffineTransform?, name: String) {
        canvas.markup.cancelInteraction()
        canvas.markup.endTextEditing()
        registerUndo(state, name)
        stopAnimation()
        let t = transform ?? .identity
        canvas.markup.marks = canvas.markup.marks.map { $0.applying(t) }
        canvas.markup.pixelSelection = nil
        canvas.markup.selectedID = nil
        base = image
        autoFilters = nil
        canvas.document.showsCheckerboard = canvas.document.showsCheckerboard || image.alphaInfo != .none
        refreshDisplay(resize: true)
    }

    // MARK: - Display

    /// Re-renders the adjusted image; `live` renders a small proxy first (slider drags).
    func refreshDisplay(resize: Bool = false, live: Bool = false) {
        guard let base else { return }
        if adjustments.autoEnhance, autoFilters == nil { autoFilters = CIImage(cgImage: base).autoAdjustmentFilters() }
        let a = adjustments, filters = autoFilters
        let size = CGSize(width: base.width, height: base.height)
        if showingOriginal {
            let original = originalBase ?? base
            canvas.setImage(original, size: CGSize(width: original.width, height: original.height), resetZoom: false)
        } else if a.isIdentity {
            displayed = base
            canvas.setImage(base, size: size, resetZoom: false)
        } else if live, max(base.width, base.height) > 1600, let proxy = ImageLoader.scaled(base, maxPixels: 1600) {
            canvas.setImage(a.render(proxy, autoFilters: filters), size: size, resetZoom: false)
            scheduleFullRender()
        } else {
            displayed = a.render(base, autoFilters: filters)
            canvas.setImage(displayed, size: size, resetZoom: false)
        }
        if resize, !showingOriginal { canvas.setImage(canvas.imageView.image, size: size, resetZoom: false) }
        canvas.markup.needsDisplay = true
        updateSubtitle()
        window?.toolbar?.validateVisibleItems()
    }

    private func scheduleFullRender() {
        renderWork?.cancel()
        guard let base else { return }
        let a = adjustments, filters = autoFilters
        let work = DispatchWorkItem { [weak self] in
            let image = a.render(base, autoFilters: filters)
            DispatchQueue.main.async {
                guard let self, !self.showingOriginal, self.adjustments == a, self.base === base else { return }
                self.displayed = image
                self.canvas.imageView.image = image
                self.canvas.markup.needsDisplay = true
            }
        }
        renderWork = work
        DispatchQueue.global(qos: .userInitiated).asyncAfter(deadline: .now() + 0.15, execute: work)
    }

    /// The image as it will be saved: adjustments and marks flattened.
    func finalImage() -> CGImage? {
        guard let base else { return nil }
        if adjustments.autoEnhance, autoFilters == nil { autoFilters = CIImage(cgImage: base).autoAdjustmentFilters() }
        guard let adjusted = adjustments.renderChecked(base, autoFilters: autoFilters) else { return nil }
        return Mark.flattenChecked(canvas.markup.marks, onto: adjusted)
    }

    // MARK: - Adjustments

    private static var adjustStart: [ObjectIdentifier: EditState] = [:]

    func beginAdjusting() {
        editGeneration = UUID()
        stopAnimation()
        canvas.markup.endTextEditing()
        Self.adjustStart[ObjectIdentifier(self)] = state
    }

    func adjustmentsChanged(_ a: Adjustments, final: Bool) {
        adjustments = a
        stopAnimation()
        refreshDisplay(live: !final)
        if final, let start = Self.adjustStart.removeValue(forKey: ObjectIdentifier(self)), start.adjustments != a {
            registerUndo(start, L("Adjust Colour"))
        }
    }

    func showOriginal(_ on: Bool) {
        renderWork?.cancel(); renderWork = nil
        showingOriginal = on
        canvas.markup.hideMarks = on
        refreshDisplay()
    }

    @objc func autoEnhanceAction(_ sender: Any?) {
        var a = adjustments
        a.autoEnhance.toggle()
        applyAdjustments(a, name: L("Auto Enhance"))
    }

    @objc func lookAction(_ sender: Any?) {
        var a = adjustments
        a.look = Adjustments.Look(rawValue: commandTag(of: sender)) ?? .none
        applyAdjustments(a, name: L("Look"))
    }

    func applyAdjustments(_ a: Adjustments, name: String) {
        guard adjustments != a else { return }
        stopAnimation()
        canvas.markup.endTextEditing()
        registerUndo(state, name)
        adjustments = a
        stopAnimation()
        adjustPanel.set(a)
        refreshDisplay()
    }

    // MARK: - Rotate & flip

    @objc func rotateLeftAction(_ sender: Any?) { transformImage(.rotateLeft) }
    @objc func rotateRightAction(_ sender: Any?) { transformImage(.rotateRight) }
    @objc func flipHorizontalAction(_ sender: Any?) { transformImage(.flipHorizontal) }
    @objc func flipVerticalAction(_ sender: Any?) { transformImage(.flipVertical) }

    func opName(_ op: Orientation.Op) -> String {
        switch op {
        case .rotateLeft: return L("Rotate Left")
        case .rotateRight: return L("Rotate Right")
        case .flipHorizontal: return L("Flip Horizontal")
        case .flipVertical: return L("Flip Vertical")
        }
    }

    /// Untouched JPEG/HEIF/PNG/TIFF files get a new orientation tag (lossless); anything else becomes an edit to save.
    func transformImage(_ op: Orientation.Op) {
        stopAnimation()
        canvas.markup.cancelInteraction()
        canvas.markup.endTextEditing()
        guard let url, let l = loaded, let base else { return }
        if !isDirty, l.pageCount == 1, !l.isAnimated, rotateFileLosslessly(url, op: op) { return }
        let size = CGSize(width: base.width, height: base.height)
        replaceBase(Orientation.apply(op.orientation, to: base), transform: Orientation.transform(op, size: size), name: opName(op))
    }

    private func losslessRotationSource(_ target: URL) -> LoadedImage? {
        let source = ImageLoader.load(target)
        guard source.error == nil, source.image != nil, source.pageCount == 1, !source.isAnimated,
              let type = source.typeIdentifier.flatMap(UTType.init),
              [UTType.jpeg, .heic, .heif, .png, .tiff].contains(where: { type.conforms(to: $0) }) else { return nil }
        return source
    }

    private func rotateFileLosslessly(_ target: URL, op: Orientation.Op) -> Bool {
        guard let source = losslessRotationSource(target) else { return false }
        return writeLosslessOrientation(target, from: source.orientation,
                                        to: Orientation.compose(op, after: source.orientation), name: opName(op))
    }

    /// File operations retain their own destination even when the viewer navigates to another image.
    /// Store both orientation states so Undo/Redo does not re-run a command against the current document.
    private func writeLosslessOrientation(_ target: URL, from previous: CGImagePropertyOrientation,
                                         to orientation: CGImagePropertyOrientation, name: String) -> Bool {
        let isCurrent = url?.standardizedFileURL == target.standardizedFileURL
        // Do not discard a draft that was started after this file operation.
        guard !(isCurrent && isDirty), Orientation.rewriteLosslessly(target, orientation: orientation) else { return false }
        invalidateCache(target)
        if isCurrent {
            // This also invalidates any outstanding load of the old orientation, without clearing file Undo.
            adoptSavedURL(target, loaded: ImageLoader.load(target))
        }
        undo.registerUndo(withTarget: fileUndo) { [weak self] _ in
            guard let self else { return }
            guard let source = self.losslessRotationSource(target), source.orientation == orientation,
                  self.writeLosslessOrientation(target, from: orientation, to: previous, name: name) else {
                self.showError(CocoaError(.fileWriteUnknown, userInfo: [NSFilePathErrorKey: target.path]))
                return
            }
        }
        undo.setActionName(name)
        return true
    }

    // MARK: - Markup

    @objc func toggleMarkupAction(_ sender: Any?) {
        markupBar.show(!markupBar.strip.isShown)
        rememberPanels()
    }

    @objc func selectToolAction(_ sender: Any?) {
        markupBar.setTool(MarkTool(rawValue: commandTag(of: sender)) ?? .select)
    }

    var highlightColor: NSColor { markupBar.style.stroke }

    /// Toolbar Highlight pen: tag 0 picks the tool, tags 1… pick a colour too.
    @objc func highlightAction(_ sender: Any?) {
        let tag = commandTag(of: sender)
        if tag > 0, ImageToolbar.highlightColors.indices.contains(tag - 1) {
            markupBar.style.stroke = ImageToolbar.highlightColors[tag - 1].1
        } else if !ImageToolbar.highlightColors.contains(where: { $0.1 == markupBar.style.stroke }) {
            markupBar.style.stroke = .systemYellow
        }
        markupBar.setTool(.highlighter)
        markupBar.selectionChanged()
    }

    @objc func showStampsAction(_ sender: Any?) {
        let popover = NSPopover()
        let vc = StampLibraryViewController { [weak self] image in
            guard let self else { return }
            if !self.markupBar.strip.isShown { self.markupBar.show(true) }
            self.markupBar.setTool(.select)
            self.canvas.markup.insertImage(image)
        }
        // A picture has its own resolution: the mark is drawn to match it.
        vc.placement = { [weak self] aspect in MarkPlacement.standard(aspect: aspect, scanDPI: self.map { max(72, $0.imageDPI) }) }
        vc.popover = popover
        popover.contentViewController = vc
        popover.behavior = .transient
        if let view = sender as? NSView, view.window != nil {
            popover.show(relativeTo: view.bounds, of: view, preferredEdge: .minY)
        } else {
            popover.show(relativeTo: NSRect(x: canvas.bounds.midX, y: canvas.bounds.maxY - 20, width: 1, height: 1), of: canvas, preferredEdge: .minY)
        }
    }

    // MARK: - Pixel selection

    @objc func cropToSelectionAction(_ sender: Any?) {
        stopAnimation()
        canvas.markup.cancelInteraction()
        canvas.markup.endTextEditing()
        guard let sel = canvas.markup.pixelSelection, let base, let cropped = PixelOps.extract(sel, from: base) else { return }
        let r = sel.bounds.integral.intersection(CGRect(x: 0, y: 0, width: base.width, height: base.height))
        replaceBase(cropped, transform: CGAffineTransform(translationX: -r.minX, y: -r.minY), name: L("Crop"))
    }

    @objc func deleteSelectionAction(_ sender: Any?) {
        stopAnimation()
        canvas.markup.cancelInteraction()
        canvas.markup.endTextEditing()
        guard let sel = canvas.markup.pixelSelection, let base, let cleared = PixelOps.delete(sel, from: base) else { return }
        canvas.document.showsCheckerboard = true
        replaceBase(cleared, transform: nil, name: L("Delete Selection"))
    }

    @objc func invertSelectionAction(_ sender: Any?) {
        canvas.markup.pixelSelection = canvas.markup.pixelSelection?.inverted()
    }

    @objc func deselectAction(_ sender: Any?) { canvas.markup.pixelSelection = nil }

    @objc func delete(_ sender: Any?) {
        if let id = canvas.markup.selectedID { canvas.markup.deleteMark(id) } else { deleteSelectionAction(sender) }
    }

    @objc override func selectAll(_ sender: Any?) {
        // ⌘A while text is selected selects all the text (like Preview).
        if canvas.liveText.overlay.hasActiveTextSelection { canvas.liveText.selectAllText(); return }
        if !markupBar.strip.isShown { markupBar.show(true) }
        if !canvas.markup.tool.isSelectionTool { markupBar.setTool(.selectRect) }
        canvas.markup.selectAllPixels()
    }

    // MARK: - Remove background

    struct BackgroundRemovalRequest {
        let image: CGImage
        let session: UUID
        let generation: UUID
    }

    func captureBackgroundRemoval() -> BackgroundRemovalRequest? {
        stopAnimation()
        canvas.markup.cancelInteraction()
        canvas.markup.endTextEditing()
        guard let base else { return nil }
        editGeneration = UUID()
        return BackgroundRemovalRequest(image: base, session: imageSession, generation: editGeneration)
    }

    func isCurrentBackgroundRemoval(_ request: BackgroundRemovalRequest) -> Bool {
        request.image === base && request.session == imageSession && request.generation == editGeneration
            && !canvas.markup.hasPendingTextEdit
    }

    @discardableResult func applyBackgroundRemoval(_ image: CGImage, request: BackgroundRemovalRequest) -> Bool {
        guard isCurrentBackgroundRemoval(request) else { return false }
        replaceBase(image, transform: nil, name: L("Remove Background"))
        canvas.document.showsCheckerboard = true
        return true
    }

    @objc func removeBackgroundAction(_ sender: Any?) {
        guard let request = captureBackgroundRemoval() else { return }
        window?.subtitle = L("Removing background…")
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let result = Result { try PixelOps.removeBackground(request.image) }
            DispatchQueue.main.async {
                guard let self else { return }
                guard self.isCurrentBackgroundRemoval(request) else { self.updateSubtitle(); return }
                switch result {
                case .success(let image?):
                    self.applyBackgroundRemoval(image, request: request)
                case .success(nil):
                    self.banner.show(.info, L("No subject found to separate from the background."))
                    self.updateSubtitle()
                case .failure(let e):
                    self.banner.show(.error, e.localizedDescription)
                    self.updateSubtitle()
                }
            }
        }
    }

    // MARK: - Resize

    @objc func resizeAction(_ sender: Any?) {
        guard let base, let window else { return }
        let dialog = ResizeDialog(size: CGSize(width: base.width, height: base.height), dpi: dpi)
        dialog.alert.beginSheetModal(for: window) { [weak self] response in
            guard response == .alertFirstButtonReturn, dialog.isValid, let self, let base = self.base else { return }
            let target = dialog.pixelSize
            let resolution = dialog.resolutionDPI
            guard target.width >= 1, target.height >= 1 else { return }
            if target != CGSize(width: base.width, height: base.height) {
                guard self.resizeImage(to: target) else { return }
            } else {
                guard resolution != self.imageDPI else { return }
                self.registerUndo(self.state, L("Adjust Size"))
            }
            self.imageDPI = resolution
            self.updateSubtitle()
        }
    }

    var dpi: CGFloat { imageDPI }

    @discardableResult func resizeImage(to target: CGSize) -> Bool {
        stopAnimation()
        guard let base else { return false }
        let target = CGSize(width: target.width.rounded(), height: target.height.rounded())
        guard ImageLoader.validPixelSize(target) else { return false }
        guard target != CGSize(width: base.width, height: base.height) else { return true }
        canvas.markup.cancelInteraction()
        canvas.markup.endTextEditing()
        let cs = base.colorSpace.flatMap { $0.model == .rgb ? $0 : nil }
        guard let resized = ImageLoader.render(size: target, colorSpace: cs, { ctx in
            ctx.interpolationQuality = .high
            ctx.draw(base, in: CGRect(origin: .zero, size: target))
        }) else { return false }
        let t = CGAffineTransform(scaleX: target.width / CGFloat(base.width), y: target.height / CGFloat(base.height))
        replaceBase(resized, transform: t, name: L("Adjust Size"))
        canvas.zoomToFit()
        return true
    }

    // MARK: - Description

    @objc func imageDescriptionAction(_ sender: Any?) {
        guard let window else { return }
        let alert = NSAlert()
        alert.messageText = L("Image Description")
        alert.informativeText = L("Describe the image for people who use VoiceOver. The description is saved in the file's metadata.")
        alert.addButton(withTitle: L("OK"))
        alert.addButton(withTitle: L("Cancel"))
        let scroll = NSTextView.scrollableTextView()
        scroll.frame = NSRect(x: 0, y: 0, width: 340, height: 96)
        scroll.borderType = .bezelBorder
        let text = scroll.documentView as! NSTextView
        text.string = imageDescription ?? ""
        text.font = .systemFont(ofSize: 13)
        text.isRichText = false
        text.textContainerInset = NSSize(width: 6, height: 6)
        alert.accessoryView = scroll
        alert.window.initialFirstResponder = text
        alert.beginSheetModal(for: window) { [weak self] r in
            guard r == .alertFirstButtonReturn, let self else { return }
            let value = text.string.trimmingCharacters(in: .whitespacesAndNewlines)
            let new: String? = value.isEmpty ? nil : value
            guard new != self.imageDescription else { return }
            self.registerUndo(self.state, L("Image Description"))
            self.imageDescription = new
            self.updateSubtitle()
        }
    }

    // MARK: - Leaving an edited image

    /// Asks Save / Don't Save / Cancel when the image has unsaved changes.
    func confirmLeaving(_ done: @escaping (Bool) -> Void) {
        guard isDirty, let window, let url else { done(true); return }
        canvas.markup.endTextEditing()
        let alert = NSAlert()
        alert.messageText = L("Save changes to “%@”?", url.lastPathComponent)
        alert.informativeText = L("Your markup and adjustments will be lost if you don't save them.")
        alert.addButton(withTitle: L("Save"))
        alert.addButton(withTitle: L("Cancel"))
        alert.addButton(withTitle: L("Don't Save"))
        alert.beginSheetModal(for: window) { [weak self] r in
            guard let self else { return }
            switch r {
            case .alertFirstButtonReturn: self.save(confirmed: true) { ok in done(ok) }
            case .alertThirdButtonReturn:
                self.resetEditing(base: self.originalBase)
                done(true)
            default: done(false)
            }
        }
    }
}

/// Adjust Size dialog: width/height in pixels, percent, cm or inches, resolution, keep proportions.
final class ResizeDialog: NSObject, NSTextFieldDelegate {
    let alert = NSAlert()
    private let original: CGSize
    private let width = NSTextField()
    private let height = NSTextField()
    private let units = NSPopUpButton()
    private let resolution = NSTextField()
    private let keep = NSButton(checkboxWithTitle: L("Scale proportionally"), target: nil, action: nil)
    private let resample = NSButton(checkboxWithTitle: L("Resample image"), target: nil, action: nil)
    private let result = NSTextField(labelWithString: "")
    private var dpi: CGFloat
    private let originalDPI: CGFloat

    enum Unit: Int, CaseIterable { case pixels, percent, cm, inches
        var title: String { [L("pixels"), L("percent"), L("cm"), L("inches")][rawValue] }
    }
    /// The unit of the values currently displayed; a popup action sees the newly selected unit.
    private var fieldUnit: Unit = .pixels
    private var exactPixelSize: CGSize?
    private(set) var isValid = false

    init(size: CGSize, dpi: CGFloat) {
        original = size
        let validDPI = dpi.isFinite && dpi > 0 ? dpi : 72
        self.dpi = validDPI
        originalDPI = validDPI
        super.init()
        alert.messageText = L("Adjust Size")
        alert.addButton(withTitle: L("OK"))
        alert.addButton(withTitle: L("Cancel"))
        for u in Unit.allCases { units.addItem(withTitle: u.title) }
        units.target = self
        units.action = #selector(unitChanged)
        for f in [width, height, resolution] {
            f.delegate = self
            f.alignment = .right
            f.translatesAutoresizingMaskIntoConstraints = false
            f.widthAnchor.constraint(equalToConstant: 90).isActive = true
        }
        resolution.stringValue = Self.format(validDPI)
        keep.state = .on
        resample.state = .on
        resample.target = self
        resample.action = #selector(unitChanged)
        result.textColor = .secondaryLabelColor
        result.font = .systemFont(ofSize: 11)
        let grid = NSGridView(views: [
            [NSTextField(labelWithString: L("Width:")), width, units],
            [NSTextField(labelWithString: L("Height:")), height, NSGridCell.emptyContentView],
            [NSTextField(labelWithString: L("Resolution:")), resolution, NSTextField(labelWithString: L("pixels/inch"))],
            [NSGridCell.emptyContentView, keep, NSGridCell.emptyContentView],
            [NSGridCell.emptyContentView, resample, NSGridCell.emptyContentView],
            [NSGridCell.emptyContentView, result, NSGridCell.emptyContentView],
        ])
        grid.rowSpacing = 8
        grid.columnSpacing = 8
        grid.column(at: 0).xPlacement = .trailing
        grid.rowAlignment = .firstBaseline
        for r in 3...5 { grid.cell(atColumnIndex: 1, rowIndex: r).row?.mergeCells(in: NSRange(location: 1, length: 2)) }
        result.translatesAutoresizingMaskIntoConstraints = false
        result.widthAnchor.constraint(greaterThanOrEqualToConstant: 280).isActive = true
        fill(original)
        grid.frame.size = grid.fittingSize
        alert.accessoryView = grid
    }

    private var unit: Unit { Unit(rawValue: units.indexOfSelectedItem) ?? .pixels }
    var resolutionDPI: CGFloat { dpi }

    static func format(_ v: CGFloat) -> String {
        guard v.isFinite, abs(v) < CGFloat(Int.max) else { return String(format: "%.4g", v) }
        return v == v.rounded() ? String(Int(v)) : String(format: "%.2f", v)
    }

    private func fill(_ px: CGSize) {
        width.isEnabled = resample.state == .on
        height.isEnabled = resample.state == .on
        keep.isEnabled = resample.state == .on
        fieldUnit = unit
        exactPixelSize = px
        width.stringValue = Self.format(toUnit(px.width, original.width))
        height.stringValue = Self.format(toUnit(px.height, original.height))
        updateResult()
    }

    private func toUnit(_ px: CGFloat, _ full: CGFloat) -> CGFloat {
        switch unit {
        case .pixels: return px
        case .percent: return (px / full * 1000).rounded() / 10
        case .cm: return (px / dpi * 2.54 * 100).rounded() / 100
        case .inches: return (px / dpi * 100).rounded() / 100
        }
    }

    private func toPixels(_ v: CGFloat, _ full: CGFloat) -> CGFloat {
        switch fieldUnit {
        case .pixels: return v
        case .percent: return full * v / 100
        case .cm: return v / 2.54 * dpi
        case .inches: return v * dpi
        }
    }

    var pixelSize: CGSize {
        guard resample.state == .on else { return original }
        if let exactPixelSize { return exactPixelSize }
        guard [width.doubleValue, height.doubleValue].allSatisfy({ $0.isFinite && $0 > 0 }) else { return .zero }
        return CGSize(width: max(1, toPixels(CGFloat(width.doubleValue), original.width).rounded()),
                      height: max(1, toPixels(CGFloat(height.doubleValue), original.height).rounded()))
    }

    @objc private func unitChanged() {
        let px = pixelSize
        guard ImageLoader.validPixelSize(px) else {
            units.selectItem(at: fieldUnit.rawValue)
            updateResult()
            return
        }
        fill(px)
    }

    func controlTextDidChange(_ obj: Notification) {
        guard let field = obj.object as? NSTextField else { return }
        if field === resolution {
            let new = CGFloat(resolution.doubleValue)
            if new.isFinite, new > 0 {
                // Without resampling, a new resolution changes only the print size.
                let px = pixelSize
                dpi = new
                if resample.state == .on, unit == .cm || unit == .inches {
                    exactPixelSize = nil
                    updateResult()
                } else { fill(px) }
            } else { updateResult() }
            return
        }
        guard resample.state == .on else { fill(original); return }
        exactPixelSize = nil
        if keep.state == .on {
            let ratio = original.width / original.height
            if field === width {
                let px = toPixels(CGFloat(width.doubleValue), original.width)
                height.stringValue = Self.format(toUnit((px / ratio).rounded(), original.height))
            } else {
                let px = toPixels(CGFloat(height.doubleValue), original.height)
                width.stringValue = Self.format(toUnit((px * ratio).rounded(), original.width))
            }
        }
        updateResult()
    }

    private func updateResult() {
        let px = pixelSize
        let estimatedBytes = px.width * px.height * 4
        let validDimensions = exactPixelSize != nil || [width.doubleValue, height.doubleValue].allSatisfy { $0.isFinite && $0 > 0 }
        let valid = validDimensions && resolution.doubleValue.isFinite && resolution.doubleValue > 0 && ImageLoader.validPixelSize(px)
            && estimatedBytes.isFinite && estimatedBytes < CGFloat(Int64.max)
        isValid = valid
        alert.buttons.first?.isEnabled = valid
        guard valid else {
            result.stringValue = L("The image size or resolution is invalid.")
            return
        }
        let bytes = Int64(estimatedBytes)
        result.stringValue = L("Result: %@ × %@ px, about %@", Self.format(px.width), Self.format(px.height),
                               ByteCountFormatter.string(fromByteCount: bytes, countStyle: .memory))
        _ = originalDPI
    }
}
