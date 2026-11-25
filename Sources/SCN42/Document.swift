import AppKit
import Kit42

/// One page of the document being scanned.
final class ScanPage {
    let id = UUID()
    /// Geometry-corrected scan (cropped, straight, upright) before clean-up; kept JPEG-compressed in memory.
    private(set) var base: CGImage
    /// What is shown and saved.
    private(set) var image: CGImage
    var dpi: Double
    var options: ProcessOptions
    var report: ProcessReport
    /// Recognised text (nil = not recognised yet).
    var text: [TextLine]?
    private var cachedThumbnail: NSImage?
    private(set) var contentRevision = UUID()

    init(base: CGImage, image: CGImage, dpi: Double, options: ProcessOptions, report: ProcessReport) {
        self.base = ScanPage.compact(base)
        self.image = image
        self.dpi = dpi
        self.options = options
        self.report = report
    }

    /// Runs the whole pipeline on a raw scan.
    static func make(_ raw: RawScan, options: ProcessOptions) throws -> ScanPage {
        var o = options
        o.dpi = raw.dpi
        var geometry = o
        // Photos are straightened by their outline only (text skew would read their lines).
        geometry.deskew = o.deskew && o.enhancement != .photo
        geometry.enhancement = .none
        geometry.colorMode = .color
        geometry.brightness = 0
        geometry.contrast = 0
        let (base, report) = try Processor.processChecked(raw.image, geometry)
        guard let finished = Processor.finish(base, o) else { throw CocoaError(.fileReadCorruptFile) }
        var r = report
        r.outputSize = CGSize(width: finished.width, height: finished.height)
        return ScanPage(base: base, image: finished, dpi: raw.dpi, options: o, report: r)
    }

    /// A JPEG-backed copy (decoded when drawn): a page costs a few MB instead of tens.
    static func compact(_ image: CGImage) -> CGImage {
        guard image.bitsPerPixel > 1, let data = Exporter.jpegData(image, quality: 0.94, dpi: 300), let provider = CGDataProvider(data: data as CFData),
              let jpeg = CGImage(jpegDataProviderSource: provider, decode: nil, shouldInterpolate: true, intent: .defaultIntent) else { return image }
        return jpeg
    }

    var thumbnail: NSImage {
        if let cachedThumbnail { return cachedThumbnail }
        let side = 260
        let s = Double(side) / Double(max(image.width, image.height))
        let bmp = Bitmap(image, gray: image.colorSpace?.model == .monochrome).resized(width: max(1, Int(Double(image.width) * s)), height: max(1, Int(Double(image.height) * s)))
        let t = NSImage(cgImage: bmp.cgImage() ?? image, size: NSSize(width: bmp.width, height: bmp.height))
        cachedThumbnail = t
        return t
    }

    var exportPage: ExportPage {
        ExportPage(image: image, dpi: dpi, text: text, title: text.flatMap(OCR.title))
    }

    /// Compute immutable output on a worker, then restore it on the main thread.
    struct Snapshot {
        let base: CGImage, image: CGImage, dpi: Double, options: ProcessOptions, report: ProcessReport, text: [TextLine]?

        func hasSameContent(as other: Snapshot) -> Bool {
            base === other.base && image === other.image && dpi == other.dpi && options == other.options
                && report == other.report && text == other.text
        }

        func finished(_ options: ProcessOptions) throws -> Snapshot {
            var options = options; options.dpi = dpi
            guard let image = Processor.finish(base, options) else { throw CocoaError(.fileReadCorruptFile) }
            var report = report
            report.outputSize = CGSize(width: image.width, height: image.height)
            report.ink = Processor.inkCoverage(image)
            report.tone = Processor.toneSpread(image)
            report.blank = report.ink < options.blankLimit && report.tone < 10
            return Snapshot(base: base, image: image, dpi: dpi, options: options, report: report, text: nil)
        }
    }

    var snapshot: Snapshot { Snapshot(base: base, image: image, dpi: dpi, options: options, report: report, text: text) }

    func restore(_ state: Snapshot) {
        base = state.base; image = state.image; dpi = state.dpi; options = state.options; report = state.report; text = state.text
        contentRevision = UUID()
        cachedThumbnail = nil
    }

    /// Same geometry, new pixels (a signature or stamp printed into the page); the recognised text stays valid.
    func replacePixels(base newBase: CGImage, image newImage: CGImage) {
        var report = report
        report.outputSize = CGSize(width: newImage.width, height: newImage.height)
        report.ink = Processor.inkCoverage(newImage)
        report.blank = false
        restore(Snapshot(base: newBase, image: newImage, dpi: dpi, options: options, report: report, text: text))
    }

    func copy() -> ScanPage {
        let page = ScanPage(base: base, image: image, dpi: dpi, options: options, report: report)
        page.restore(snapshot)
        return page
    }

    /// Re-runs clean-up with new options (enhancement, colour mode, brightness…).
    func refinish(_ options: ProcessOptions) throws { restore(try snapshot.finished(options)) }

    func rotate(clockwise degrees: Int) {
        let turn = (degrees % 360 + 360) % 360
        guard turn != 0, turn % 90 == 0,
              let rotatedBase = Processor.rotate90(base, clockwise: turn),
              let rotatedImage = Processor.rotate90(image, clockwise: turn) else { return }
        let output: CGImage
        if image.bitsPerPixel == 1 {
            guard let bits = Bitmap(rotatedImage, gray: true).oneBitImage(threshold: 128) else { return }
            output = bits
        } else { output = rotatedImage }
        var report = report
        report.outputSize = CGSize(width: output.width, height: output.height)
        report.rotation = (report.rotation + turn) % 360
        restore(Snapshot(base: ScanPage.compact(rotatedBase), image: output, dpi: dpi, options: options, report: report, text: nil))
    }
}

/// The pages of one window, with undo for every change.
final class ScanDocument {
    private(set) var pages: [ScanPage] = []
    let undo = UndoManager()
    var onChange: (() -> Void)?
    var captureSelection: (() -> [UUID])?
    var restoreSelection: (([UUID]) -> Void)?
    var isDirty = false
    /// Monotonic revision guards asynchronous saves; complete saved content determines dirty state after Undo.
    private(set) var revision = 0
    typealias SavedContents = [(UUID, ScanPage.Snapshot)]
    private var savedContents: SavedContents? = []
    var isEmpty: Bool { pages.isEmpty }

    init() { undo.groupsByEvent = false }

    private func step(_ body: () -> Void) {
        if undo.isUndoing || undo.isRedoing { body(); return }
        undo.beginUndoGrouping(); body(); undo.endUndoGrouping()
    }

    private func changed(_ name: String) {
        revision += 1
        isDirty = !matchesSavedContents
        undo.setActionName(name)
        onChange?()
    }

    private var matchesSavedContents: Bool {
        guard let savedContents else { return false }
        return pages.count == savedContents.count && zip(pages, savedContents).allSatisfy { page, saved in
            page.id == saved.0 && page.snapshot.hasSameContent(as: saved.1)
        }
    }

    func insert(_ new: [ScanPage], at index: Int, name: String = L("Add Pages")) {
        var ids = Set(pages.map(\.id))
        let unique = new.filter { ids.insert($0.id).inserted }
        guard !unique.isEmpty else { return }
        var order = pages
        order.insert(contentsOf: unique, at: min(max(0, index), pages.count))
        setOrder(order, name: name)
    }

    func append(_ new: [ScanPage]) { insert(new, at: pages.count) }

    func remove(ids: Set<UUID>, name: String = L("Delete Pages")) {
        setOrder(pages.filter { !ids.contains($0.id) }, name: name)
    }

    /// Destination is an index before removal; invalid/stale row indices are ignored.
    func move(_ indices: IndexSet, to destination: Int) {
        let valid = indices.filteredIndexSet { pages.indices.contains($0) }
        guard !valid.isEmpty else { return }
        let moving = valid.map { pages[$0] }
        var rest = pages.enumerated().filter { !valid.contains($0.offset) }.map(\.element)
        let position = min(max(0, destination), pages.count)
        let shift = valid.filter { $0 < position }.count
        rest.insert(contentsOf: moving, at: min(max(0, position - shift), rest.count))
        setOrder(rest, name: L("Move Pages"))
    }

    func setOrder(_ order: [ScanPage], name: String) { setOrder(order, name: name, selection: nil) }

    private func setOrder(_ order: [ScanPage], name: String, selection: [UUID]?) {
        var ids = Set<UUID>()
        let order = order.filter { ids.insert($0.id).inserted }
        guard order.map(\.id) != pages.map(\.id) else { return }
        step {
            let old = pages, oldSelection = captureSelection?()
            pages = order
            undo.registerUndo(withTarget: self) { $0.setOrder(old, name: name, selection: oldSelection) }
            changed(name)
            if let selection { restoreSelection?(selection) }
        }
    }

    func replace(_ page: ScanPage, with new: [ScanPage], name: String = L("Rescan Page")) {
        guard let index = pages.firstIndex(where: { $0 === page }) else { return }
        var order = pages; order.replaceSubrange(index...index, with: new)
        setOrder(order, name: name)
    }

    func modify(_ targets: [ScanPage], name: String, _ change: (ScanPage) -> Void) {
        var seen = Set<UUID>()
        let targets = targets.filter { target in pages.contains { $0 === target } && seen.insert(target.id).inserted }
        guard !targets.isEmpty else { return }
        let before = targets.map { ($0, $0.snapshot) }, selection = captureSelection?()
        targets.forEach(change)
        let changedStates = before.filter { !$0.0.snapshot.hasSameContent(as: $0.1) }
        guard !changedStates.isEmpty else { return }
        step {
            undo.registerUndo(withTarget: self) { $0.restore(changedStates, name: name, selection: selection) }
            changed(name)
        }
    }

    private func restore(_ states: [(ScanPage, ScanPage.Snapshot)], name: String, selection: [UUID]?) {
        let valid = states.filter { page, _ in pages.contains { $0 === page } }
        guard !valid.isEmpty else { return }
        let redo = valid.map { ($0.0, $0.0.snapshot) }, redoSelection = captureSelection?()
        valid.forEach { $0.0.restore($0.1) }
        undo.registerUndo(withTarget: self) { $0.restore(redo, name: name, selection: redoSelection) }
        changed(name)
        if let selection { restoreSelection?(selection) }
    }

    /// Recognition is not undoable; a result must still belong to these exact page pixels.
    @discardableResult func setText(_ page: ScanPage, _ lines: [TextLine], expectedContentRevision: UUID? = nil) -> Bool {
        guard pages.contains(where: { $0 === page }), expectedContentRevision == nil || expectedContentRevision == page.contentRevision else { return false }
        guard page.text != lines else { return true }
        page.text = lines
        revision += 1; isDirty = !matchesSavedContents
        onChange?()
        return true
    }

    func markSaved(revision savedRevision: Int) {
        guard revision == savedRevision else {
            // Legacy callers without the saved pixels cannot establish a baseline after concurrent edits.
            savedContents = nil; isDirty = true; onChange?()
            return
        }
        markSaved(contents: captureSavedContents(pages))
    }

    func captureSavedContents(_ targets: [ScanPage]) -> SavedContents { targets.map { ($0.id, $0.snapshot) } }

    /// The saved snapshot, not the state at callback time, becomes the new disk baseline.
    func markSaved(contents: SavedContents) {
        savedContents = contents; isDirty = !matchesSavedContents
        onChange?()
    }
}
