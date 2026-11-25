import AppKit
import Kit42
import UniformTypeIdentifiers

extension ImageWindowController {
    // MARK: - Save

    @objc(saveDocument:) func saveDocument(_ sender: Any?) { save(confirmed: false) }

    @objc(saveDocumentAs:) func saveDocumentAs(_ sender: Any?) { runExportPanel(saveAs: true) }

    @objc(revertDocumentToSaved:) func revertDocumentToSaved(_ sender: Any?) {
        do { try revertSavedImage() } catch { showError(error) }
    }

    func revertSavedImage() throws {
        guard isDirty, let url else { return }
        let saved = ImageLoader.load(url, page: page)
        guard let image = saved.image, saved.error == nil else { throw CocoaError(.fileReadCorruptFile) }
        // Revert owns the undo snapshot, including any field that has not yet committed.
        undo.disableUndoRegistration()
        canvas.markup.endTextEditing()
        undo.enableUndoRegistration()
        registerUndo(state, L("Revert to Saved"))
        originalBase = image
        originalDescription = Self.description(of: url)
        let resolution = ImageLoader.properties(url)[kCGImagePropertyDPIWidth] as? CGFloat ?? 72
        originalDPI = resolution.isFinite && resolution > 0 ? resolution : 72
        loaded = saved
        invalidateCache(url)
        restore(EditState(base: image, marks: [], adjustments: Adjustments(), imageDescription: originalDescription, dpi: originalDPI))
    }

    /// Saves over the original (same format, metadata kept). Formats that cannot be written go to Save As.
    func save(confirmed: Bool, completion: ((Bool) -> Void)? = nil) {
        canvas.markup.endTextEditing()
        guard let url, let l = loaded, isDirty, let window else { completion?(true); return }
        if onlyMetadataChanged, Orientation.writeMetadata(description: imageDescription,
            resolution: imageDPI != originalDPI ? imageDPI : nil, to: url) {
            didSave(url)
            completion?(true)
            return
        }
        let type = l.typeIdentifier
        guard ImageWriter.canWrite(type), l.pageCount == 1, !l.isAnimated, !l.wasDownsampled, let image = finalImage() else {
            runExportPanel(saveAs: true, completion: completion)
            return
        }
        // Transparency would turn white in a JPEG: offer a PNG copy instead.
        let lostAlpha = (UTType(type ?? "")?.conforms(to: .jpeg) ?? false) && ImageWriter.hasTransparency(image)
        if lostAlpha { runExportPanel(saveAs: true, preferred: .png, completion: completion); return }

        let write = { [weak self] in
            guard let self else { return }
            do {
                try ImageWriter.write(image, to: url, type: type!, quality: 0.92, metadataFrom: url,
                                      description: .some(self.imageDescription), dpi: self.imageDPI)
                self.didSave(url)
                completion?(true)
            } catch {
                self.showError(error)
                completion?(false)
            }
        }
        if confirmed || confirmedOverwrite { write(); return }
        let alert = NSAlert()
        alert.messageText = L("Save changes to “%@”?", url.lastPathComponent)
        alert.informativeText = L("The original file will be replaced with the edited image. You can save a copy instead.")
        alert.addButton(withTitle: L("Save"))
        alert.addButton(withTitle: L("Cancel"))
        alert.addButton(withTitle: L("Save As Copy…"))
        alert.beginSheetModal(for: window) { [weak self] r in
            switch r {
            case .alertFirstButtonReturn:
                self?.confirmedOverwrite = true
                write()
            case .alertThirdButtonReturn: self?.runExportPanel(saveAs: true, completion: completion)
            default: completion?(false)
            }
        }
    }

    func didSave(_ url: URL) {
        let saved = ImageLoader.load(url, page: page)
        guard saved.image != nil, saved.error == nil else { showError(CocoaError(.fileReadCorruptFile)); return }
        invalidateCache(url)
        ImageLoader.thumbnailCache.removeObject(forKey: url as NSURL)
        display(saved)
        banner.show(.success, L("Saved"))
        DispatchQueue.main.asyncAfter(deadline: .now() + 2) { [weak self] in self?.banner.hide() }
    }

    func showError(_ error: Error) {
        guard let window else { return }
        NSAlert(error: error).beginSheetModal(for: window)
    }

    // MARK: - Export / Save As

    @objc func exportAction(_ sender: Any?) { runExportPanel(saveAs: false) }

    func runExportPanel(saveAs: Bool, preferred: UTType? = nil, completion: ((Bool) -> Void)? = nil) {
        canvas.markup.endTextEditing()
        guard let window, let image = finalImage() else { completion?(false); return }
        let panel = NSSavePanel()
        let options = ExportOptions(sourceType: preferred ?? loaded?.typeIdentifier.flatMap(UTType.init), hasAlpha: image.alphaInfo != .none, panel: panel)
        panel.accessoryView = options.view
        panel.title = saveAs ? L("Save As") : L("Export")
        panel.prompt = saveAs ? L("Save") : L("Export")
        panel.canCreateDirectories = true
        panel.isExtensionHidden = false
        if let url {
            panel.directoryURL = url.deletingLastPathComponent()
            panel.nameFieldStringValue = url.deletingPathExtension().lastPathComponent + (saveAs ? " " + L("copy") : "") + "." + options.fileExtension
        }
        panel.beginSheetModal(for: window) { [weak self] r in
            guard let self, r == .OK, let dest = panel.url else { completion?(false); return }
            do {
                try self.writeExport(to: dest, type: options.type.identifier, quality: options.quality, saveAs: saveAs,
                                     resize: options.resized)
                completion?(true)
            } catch {
                self.showError(error)
                completion?(false)
            }
        }
    }

    /// Save As adopts the chosen destination regardless of its folder or whether it is already in the image set.
    /// Export elsewhere preserves the source editing session; export over the current file establishes a new baseline.
    func writeExport(to destination: URL, type: String, quality: Double = 0.9, saveAs: Bool,
                     resize: (CGImage) -> CGImage? = { $0 }) throws {
        canvas.markup.endTextEditing()
        guard let rendered = finalImage(), let output = resize(rendered) else { throw CocoaError(.fileWriteUnknown) }
        try ImageWriter.write(output, to: destination, type: type, quality: quality, metadataFrom: url,
                              description: .some(imageDescription), dpi: imageDPI)
        // Export can replace a neighbour that was already decoded or is still being prefetched.
        invalidateCache(destination)
        if saveAs || destination.standardizedFileURL == url?.standardizedFileURL {
            let saved = ImageLoader.load(destination)
            guard saved.image != nil, saved.error == nil else { throw CocoaError(.fileReadCorruptFile) }
            adoptSavedURL(destination, loaded: saved)
        } else if destination.deletingLastPathComponent().standardizedFileURL == url?.deletingLastPathComponent().standardizedFileURL,
                  !urls.contains(destination) {
            insertIntoSetKeepingCurrent(destination, current: index)
        }
    }

    func insertIntoSetKeepingCurrent(_ new: URL, current: Int) {
        guard let cur = url else { return }
        var list = urls
        list.append(new)
        let sorted = sortOrder.sort(list)
        // Replace without reloading the current image (its edits stay).
        setURLsKeepingCurrent(sorted, current: cur)
    }

    // MARK: - Trash

    @objc func trashAction(_ sender: Any?) {
        guard let url else { return }
        confirmLeavingForTrash { [weak self] ok in
            guard ok, let self else { return }
            do {
                try self.moveImageToTrash(url)
            } catch {
                self.showError(error)
            }
        }
    }

    /// File Undo/Redo follows the captured file even after navigation changes the current image.
    func moveImageToTrash(_ original: URL) throws {
        var resulting: NSURL?
        try FileManager.default.trashItem(at: original, resultingItemURL: &resulting)
        removeFromSet(original)
        if let trashed = resulting as URL? {
            undo.registerUndo(withTarget: fileUndo) { [weak self] _ in self?.putBack(trashed, to: original) }
            undo.setActionName(L("Move to Trash"))
        }
    }

    private func confirmLeavingForTrash(_ done: @escaping (Bool) -> Void) {
        guard isDirty else { done(true); return }
        confirmLeaving(done)
    }

    func putBack(_ trashed: URL, to original: URL) {
        do {
            try FileManager.default.moveItem(at: trashed, to: original)
        } catch {
            showError(error)
            return
        }
        insertIntoSet(original)
        undo.registerUndo(withTarget: fileUndo) { [weak self] _ in
            guard let self else { return }
            do { try self.moveImageToTrash(original) } catch { self.showError(error) }
        }
        undo.setActionName(L("Move to Trash"))
    }

    // MARK: - Finder, Copy, Share, Print

    @objc func showInFinderAction(_ sender: Any?) {
        if let url { NSWorkspace.shared.activateFileViewerSelecting([url]) }
    }

    @objc func copy(_ sender: Any?) {
        // Text selected with Live Text wins over the image.
        if canvas.liveText.hasSelection { canvas.liveText.copySelection(); return }
        copyImage(to: .general)
    }

    func copyImage(to pb: NSPasteboard) {
        canvas.markup.endTextEditing()
        guard let image = finalImage() else { return }
        pb.clearContents()
        if let sel = canvas.markup.pixelSelection, let part = PixelOps.extract(sel, from: image) {
            pb.writeObjects([NSImage(cgImage: part, size: .zero)])
            return
        }
        let ns = NSImage(cgImage: image, size: .zero)
        if !isDirty, let url { pb.writeObjects([url as NSURL, ns]) } else { pb.writeObjects([ns]) }
    }

    func shareItems() -> [Any] {
        canvas.markup.endTextEditing()
        guard let url else { return [] }
        if isDirty, let image = finalImage() { return [NSImage(cgImage: image, size: .zero)] }
        return [url]
    }

    @objc(printDocument:) func printDocument(_ sender: Any?) {
        guard let window, let op = imagePrintOperation() else { return }
        op.runModal(for: window, delegate: nil, didRun: nil, contextInfo: nil)
    }

    func imagePrintOperation() -> NSPrintOperation? {
        canvas.markup.endTextEditing()
        guard let image = finalImage() else { return nil }
        let info = (NSPrintInfo.shared.copy() as? NSPrintInfo) ?? NSPrintInfo()
        info.orientation = image.width > image.height ? .landscape : .portrait
        info.horizontalPagination = .fit
        info.verticalPagination = .fit
        info.isHorizontallyCentered = true
        info.isVerticallyCentered = true
        let page = NSSize(width: info.paperSize.width - info.leftMargin - info.rightMargin,
                          height: info.paperSize.height - info.topMargin - info.bottomMargin)
        let view = NSImageView(frame: NSRect(origin: .zero, size: page))
        view.image = NSImage(cgImage: image, size: .zero)
        view.imageScaling = .scaleProportionallyUpOrDown
        let op = NSPrintOperation(view: view, printInfo: info)
        op.jobTitle = url?.lastPathComponent ?? "IMG42"
        return op
    }

    @objc(runPageLayout:) func runPageLayout(_ sender: Any?) {
        guard let window else { return }
        NSPageLayout().beginSheet(with: NSPrintInfo.shared, modalFor: window, delegate: nil, didEnd: nil, contextInfo: nil)
    }

    // MARK: - Live Text

    /// Recognises text once per shown image (background), then calls `done`.
    func withRecognizedText(_ done: @escaping ([PixelOps.TextLine]) -> Void) {
        guard let image = displayed ?? base else { done([]); return }
        if let lines = textLines, textLinesFor === image { done(lines); return }
        let session = imageSession
        window?.subtitle = L("Recognising text…")
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let lines = PixelOps.recognizeText(image)
            DispatchQueue.main.async {
                guard let self, self.acceptRecognizedText(lines, image: image, session: session) else { return }
                self.updateSubtitle()
                done(lines)
            }
        }
    }

    @discardableResult
    func acceptRecognizedText(_ lines: [PixelOps.TextLine], image: CGImage, session: UUID) -> Bool {
        guard imageSession == session, (displayed ?? base) === image else { return false }
        textLines = lines; textLinesFor = image
        return true
    }

    @objc func searchChanged(_ sender: NSSearchField) {
        let isEnter = NSApp.currentEvent?.type == .keyDown && NSApp.currentEvent?.keyCode == 36
        if isEnter, !canvas.markup.searchBoxes.isEmpty {
            searchIndex = (searchIndex + 1) % canvas.markup.searchBoxes.count
            canvas.markup.currentSearchBox = searchIndex
            reveal(canvas.markup.searchBoxes[searchIndex])
            return
        }
        runSearch(sender.stringValue)
    }

    func runSearch(_ query: String) {
        searchGeneration = UUID()
        let generation = searchGeneration
        let q = query.trimmingCharacters(in: .whitespaces)
        guard !q.isEmpty, let image = displayed ?? base else {
            canvas.markup.searchBoxes = []
            searchIndex = -1; canvas.markup.currentSearchBox = -1
            banner.hide()
            return
        }
        withRecognizedText { [weak self] lines in
            self?.applySearch(q, lines: lines, image: image, generation: generation)
        }
    }

    @discardableResult
    func applySearch(_ query: String, lines: [PixelOps.TextLine], image: CGImage, generation: UUID) -> Bool {
        guard searchGeneration == generation, (displayed ?? base) === image else { return false }
        let boxes = PixelOps.find(query, in: lines, size: CGSize(width: image.width, height: image.height))
        canvas.markup.searchBoxes = boxes
        searchIndex = boxes.isEmpty ? -1 : 0
        canvas.markup.currentSearchBox = searchIndex
        if let first = boxes.first { reveal(first) }
        if lines.isEmpty { banner.show(.info, L("No text found in this image")) }
        else { banner.show(boxes.isEmpty ? .warning : .info, boxes.isEmpty ? L("“%@” not found", query) : L("Matches for “%@”: %d", query, boxes.count)) }
        return true
    }

    private func reveal(_ box: CGRect) {
        guard !canvas.imageFits else { return }
        canvas.markup.scrollToVisible(box.insetBy(dx: -box.width, dy: -box.height * 3))
    }

    @objc func copyTextAction(_ sender: Any?) {
        withRecognizedText { [weak self] lines in
            guard let self else { return }
            guard !lines.isEmpty else { self.banner.show(.info, L("No text found in this image")); return }
            let text = lines.map(\.text).joined(separator: "\n")
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(text, forType: .string)
            self.banner.show(.success, L("Copied %d lines of text", lines.count))
            DispatchQueue.main.asyncAfter(deadline: .now() + 2.5) { [weak self] in self?.banner.hide() }
        }
    }
}

/// Format, quality and size controls for the export / Save As panel.
final class ExportOptions: NSObject {
    let view: NSView
    private let formatPopup = NSPopUpButton()
    private let qualitySlider = NSSlider(value: 0.9, minValue: 0.1, maxValue: 1, target: nil, action: nil)
    private let qualityLabel = NSTextField(labelWithString: L("Quality:"))
    private let qualityValue = NSTextField(labelWithString: "")
    private let sizePopup = NSPopUpButton()
    private weak var panel: NSSavePanel?
    let types: [UTType]

    static let sizes: [(String, CGFloat?, CGFloat?)] = [
        (L("Original Size"), nil, nil), ("75 %", 0.75, nil), ("50 %", 0.5, nil), ("25 %", 0.25, nil),
        (L("Fit in %d px", 3840), nil, 3840), (L("Fit in %d px", 2048), nil, 2048), (L("Fit in %d px", 1024), nil, 1024),
    ]

    init(sourceType: UTType?, hasAlpha: Bool, panel: NSSavePanel) {
        self.panel = panel
        var list: [UTType] = [.png, .jpeg, .heic, .tiff]
        if let avif = UTType("public.avif"), ImageWriter.writableTypes.contains(avif) { list.append(avif) }
        if let s = sourceType, ImageWriter.canWrite(s.identifier), !list.contains(s) { list.append(s) }
        types = list
        for t in list { formatPopup.addItem(withTitle: t.preferredFilenameExtension?.uppercased() ?? t.identifier) }
        var start = sourceType.flatMap { s in list.firstIndex { s.conforms(to: $0) } } ?? 0
        if hasAlpha, list[start] == .jpeg { start = 0 }
        formatPopup.selectItem(at: start)
        for s in Self.sizes { sizePopup.addItem(withTitle: s.0) }
        qualitySlider.doubleValue = 0.9
        let grid = NSGridView(views: [
            [NSTextField(labelWithString: L("Format:")), formatPopup],
            [qualityLabel, NSStackView(views: [qualitySlider, qualityValue])],
            [NSTextField(labelWithString: L("Size:")), sizePopup],
        ])
        grid.rowSpacing = 8
        grid.columnSpacing = 8
        grid.column(at: 0).xPlacement = .trailing
        grid.rowAlignment = .firstBaseline
        qualitySlider.translatesAutoresizingMaskIntoConstraints = false
        qualitySlider.widthAnchor.constraint(equalToConstant: 180).isActive = true
        qualityValue.translatesAutoresizingMaskIntoConstraints = false
        qualityValue.widthAnchor.constraint(equalToConstant: 40).isActive = true
        let box = NSView()
        grid.translatesAutoresizingMaskIntoConstraints = false
        box.addSubview(grid)
        NSLayoutConstraint.activate([
            grid.topAnchor.constraint(equalTo: box.topAnchor, constant: 12),
            grid.bottomAnchor.constraint(equalTo: box.bottomAnchor, constant: -12),
            grid.centerXAnchor.constraint(equalTo: box.centerXAnchor),
            grid.leadingAnchor.constraint(greaterThanOrEqualTo: box.leadingAnchor, constant: 20),
        ])
        view = box
        super.init()
        formatPopup.target = self
        formatPopup.action = #selector(formatChanged)
        qualitySlider.target = self
        qualitySlider.action = #selector(qualityChanged)
        formatChanged()
        qualityChanged()
    }

    var type: UTType { types[max(0, formatPopup.indexOfSelectedItem)] }
    var fileExtension: String { type.preferredFilenameExtension ?? "png" }
    var quality: Double { qualitySlider.doubleValue }

    @objc private func formatChanged() {
        let lossy = type.conforms(to: .jpeg) || type == .heic || type.identifier == "public.avif"
        qualityLabel.isHidden = !lossy
        qualitySlider.isHidden = !lossy
        qualityValue.isHidden = !lossy
        panel?.allowedContentTypes = [type]
        if let panel {
            let name = (panel.nameFieldStringValue as NSString).deletingPathExtension
            if !name.isEmpty { panel.nameFieldStringValue = name + "." + fileExtension }
        }
    }

    @objc private func qualityChanged() {
        qualityValue.stringValue = "\(Int((quality * 100).rounded())) %"
    }

    func resized(_ image: CGImage) -> CGImage? {
        let s = Self.sizes[max(0, sizePopup.indexOfSelectedItem)]
        let w = CGFloat(image.width), h = CGFloat(image.height)
        var k: CGFloat = 1
        if let f = s.1 { k = f }
        if let m = s.2 { k = min(1, m / max(w, h)) }
        guard k < 1 else { return image }
        let size = CGSize(width: max(1, (w * k).rounded()), height: max(1, (h * k).rounded()))
        return ImageLoader.render(size: size, colorSpace: image.colorSpace.flatMap { $0.model == .rgb ? $0 : nil }) { ctx in
            ctx.interpolationQuality = .high
            ctx.draw(image, in: CGRect(origin: .zero, size: size))
        }
    }
}
