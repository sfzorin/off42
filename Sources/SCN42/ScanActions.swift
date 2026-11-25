import AppKit
import Kit42
import PDFKit
import UniformTypeIdentifiers

extension ScanWindowController {
    // MARK: - Scanning

    @objc func scanAction(_ sender: Any?) {
        guard settingsPanel.commitPendingInput() else { return }
        guard let scanner, canScan else {
            explainCannotScan()
            return
        }
        let s = settings
        let intent = scanGeneration
        let freshDetection = overview != nil && !areaQuads.isEmpty && areaQuads.count == areas.count
        if s.source == .flatbed && s.detectPhotos && !freshDetection {
            // Photos: an overview first, every photo found becomes its own scan (new photos each time).
            runOverview { [weak self] ok in
                guard let self, ok else { return }
                let overviewToken = self.overviewGeneration
                self.detectPhotos { found in
                    guard !self.windowIsClosed, self.scanGeneration == intent,
                          self.overviewGeneration == overviewToken, self.scanner === scanner, self.settings == s else { return }
                    if found == 0 { self.startScan(scanner, areas: self.scanAreas(s), s) } else { self.startScan(scanner, areas: self.areas, s) }
                }
            }
            return
        }
        startScan(scanner, areas: scanAreas(s), s)
    }

    private func explainCannotScan() {
        if hasPendingWork { banner.show(.info, L("Wait for the current operation to finish.")); return }
        guard let scanner else {
            banner.show(.warning, L("No scanner found. Turn the scanner on, or use Scan with iPhone."))
            return
        }
        banner.show(.warning, scanner.status.text)
    }

    /// Flatbed: the marked areas, else the paper size from the top-left corner, else the whole glass.
    func scanAreas(_ s: ScanSettings) -> [CGRect] {
        let full = CGRect(x: 0, y: 0, width: 1, height: 1)
        guard s.source == .flatbed else { return [full] }
        let valid = areas.filter { $0.minX.isFinite && $0.minY.isFinite && $0.width.isFinite && $0.height.isFinite }
            .map { $0.intersection(full) }.filter { !$0.isNull && !$0.isEmpty }
        if !valid.isEmpty { return valid }
        if let paper = s.paper.inches(custom: s.customSize), let glass = scanner?.glassSize,
           glass.width.isFinite, glass.height.isFinite, glass.width > 0, glass.height > 0,
           paper.width.isFinite, paper.height.isFinite, paper.width > 0, paper.height > 0 {
            return [CGRect(x: 0, y: 0, width: min(1, paper.width / glass.width), height: min(1, paper.height / glass.height))]
        }
        return [full]
    }

    func startScan(_ scanner: ScannerBackend, areas list: [CGRect], _ s: ScanSettings) {
        guard !hasPendingWork, !windowIsClosed, scanner.status.canScan, !list.isEmpty else { return }
        scanGeneration = UUID()
        let token = scanGeneration
        isScanning = true
        scannedThisRun = []
        blanksThisRun = 0
        scanHadError = false
        banner.hide()
        if showingGlass && list.count <= 1 { showingGlass = false }
        updateStatus()
        var queue = Array(list.enumerated())
        let quads = list == areas ? areaQuads : []
        let paper = s.paper.inches(custom: s.customSize)
        func next() {
            guard !windowIsClosed, scanGeneration == token, isScanning else { return }
            guard !queue.isEmpty else { scanFinished(nil); return }
            let (index, area) = queue.removeFirst()
            var options = s.processing
            // A photo found on the overview: its outline, relative to this area.
            if quads.indices.contains(index), let q = quads[index], area.width > 0, area.height > 0 {
                func rel(_ p: CGPoint) -> CGPoint { CGPoint(x: (p.x - area.minX) / area.width, y: (p.y - area.minY) / area.height) }
                options.priorQuad = Quad(tl: rel(q.tl), tr: rel(q.tr), br: rel(q.br), bl: rel(q.bl))
            }
            let request = ScanRequest(source: s.source, resolution: s.resolution, colorMode: s.colorMode, bitDepth: s.bitDepth,
                                      area: area, paper: paper, duplex: s.duplex, lastInJob: queue.isEmpty)
            scanner.scan(request, page: { [weak self] raw in
                guard let self, !self.windowIsClosed, self.scanGeneration == token, self.isScanning else { return }
                self.process(raw, options, removeBlank: s.removeBlank)
            }, done: { [weak self] error in
                guard let self, !self.windowIsClosed, self.scanGeneration == token, self.isScanning else { return }
                if let error { self.scanFinished(error); return }
                next()
            })
        }
        next()
    }

    /// Pages are cleaned up one after another in the background while the scanner goes on.
    func process(_ raw: RawScan, _ options: ProcessOptions, removeBlank: Bool, documentScan: Bool = false) {
        guard !windowIsClosed else { return }
        let token = acquisitionGeneration
        pendingProcessing += 1
        updateStatus()
        var o = options
        if documentScan {
            // The phone already cut and straightened the page.
            o.crop = false
            o.deskew = false
        }
        processQueue.async { [weak self] in
            let result = Result { try ScanPage.make(raw, options: o) }
            DispatchQueue.main.async {
                guard let self else { return }
                self.pendingProcessing -= 1
                guard !self.windowIsClosed, self.acquisitionGeneration == token else { return }
                switch result {
                case .success(let page):
                    if removeBlank && page.report.blank { self.blanksThisRun += 1 }
                    else { self.add(page) }
                case .failure(let error):
                    self.scanHadError = true; self.copierCopies = nil
                    self.banner.show(.warning, error.localizedDescription)
                }
                self.processingMaybeDone()
                self.updateStatus()
            }
        }
    }

    private func add(_ page: ScanPage) {
        scannedThisRun.append(page)
        if let target = rescanTarget {
            doc.replace(target, with: [page])
            rescanTarget = nil
            if let i = pages.firstIndex(where: { $0 === page }) { insertionIndex = insertionIndex.map { max($0, i) } }
        } else if let ins = insertionIndex {
            doc.insert([page], at: ins + 1)
            insertionIndex = ins + 1
        } else {
            doc.append([page])
        }
        showingGlass = false
        if let i = pages.firstIndex(where: { $0 === page }) { pageList.select([i]) }
        refreshPreview()
    }

    private func scanFinished(_ error: Error?) {
        isScanning = false
        prepareScannerSource()
        // Detected photos have been scanned: the next batch needs a new overview.
        if !areaQuads.isEmpty {
            areaQuads = []
            areas = Store.lastAreas
            overview = nil
        }
        if let error {
            scanHadError = true
            let text = (error as? LocalizedError)?.errorDescription ?? error.localizedDescription
            banner.show(.warning, text)
            copierCopies = nil
        }
        updateStatus()
        processingMaybeDone()
    }

    /// When the scanner and the clean-up are both done: summary, and the copier prints.
    private func processingMaybeDone() {
        guard !isScanning, pendingProcessing == 0 else { return }
        // The scanner may finish before its last image has been processed and replaced the target.
        rescanTarget = nil
        let added = scannedThisRun.count
        if !scanHadError && (added > 0 || blanksThisRun > 0) {
            var text = L("Added %@.", pagesText(added))
            if blanksThisRun > 0 { text += " " + L("Blank pages removed: %d.", blanksThisRun) }
            let crops = scannedThisRun.filter { $0.report.cropMethod != nil }.count
            if crops > 0 { text += " " + L("Cropped and straightened: %d.", crops) }
            banner.show(.success, text)
            let token = scanGeneration
            DispatchQueue.main.asyncAfter(deadline: .now() + 6) { [weak self] in
                if self?.isScanning == false && self?.scanGeneration == token { self?.banner.hide() }
            }
        }
        if let copies = copierCopies {
            copierCopies = nil
            if !scannedThisRun.isEmpty { printPages(scannedThisRun, copies: copies, direct: true) }
        }
    }

    @objc func cancelScanAction(_ sender: Any?) {
        guard isScanning else { return }
        scanGeneration = UUID(); overviewGeneration = UUID()
        isScanning = false; copierCopies = nil; scanHadError = true
        scanner?.cancel()
        prepareScannerSource()
        banner.show(.info, L("The scan was cancelled."))
        processingMaybeDone(); updateStatus()
    }

    @objc func overviewAction(_ sender: Any?) {
        runOverview { [weak self] ok in
            guard let self, ok, self.settings.detectPhotos else { return }
            self.detectPhotos { _ in }
        }
    }

    func runOverview(_ done: @escaping (Bool) -> Void) {
        guard let scanner, canOverview else { explainCannotScan(); done(false); return }
        isScanning = true
        overviewGeneration = UUID(); let token = overviewGeneration
        overviewJobGeneration = UUID(); let job = overviewJobGeneration, intent = scanGeneration
        showingGlass = true
        refreshPreview()
        updateStatus()
        scanner.overview { [weak self] result in
            guard let self else { return }
            guard self.overviewJobGeneration == job, self.scanGeneration == intent, self.isScanning,
                  !self.windowIsClosed, self.scanner === scanner else { done(false); return }
            self.isScanning = false
            self.prepareScannerSource()
            guard self.overviewGeneration == token else { self.updateStatus(); done(false); return }
            switch result {
            case .success(let image):
                self.overview = image
                self.showingGlass = true
                self.refreshPreview()
                self.updateStatus()
                done(true)
            case .failure(let error):
                self.banner.show(.warning, (error as? LocalizedError)?.errorDescription ?? error.localizedDescription)
                self.updateStatus()
                done(false)
            }
        }
    }

    @objc func detectPhotosAction(_ sender: Any?) {
        if overview == nil { runOverview { [weak self] ok in if ok { self?.detectPhotos { _ in } } } } else { detectPhotos { _ in } }
    }

    /// Finds the photos on the overview and makes each one a scan area.
    func detectPhotos(_ done: @escaping (Int) -> Void) {
        guard let overview, busyText == nil, !windowIsClosed else { done(0); return }
        let token = overviewGeneration
        busyText = L("Looking for photos…")
        updateStatus()
        DispatchQueue.global(qos: .userInitiated).async {
            let quads = Processor.detectPhotos(overview).quads
            let found = quads.map { CLI.area(for: $0, in: overview) }
            let normalised = quads.map { $0.scaled(1 / CGFloat(overview.width), 1 / CGFloat(overview.height)) }
            DispatchQueue.main.async { [weak self] in
                guard let self else { return }
                self.busyText = nil
                guard !self.windowIsClosed, self.overviewGeneration == token, self.overview === overview else {
                    self.updateStatus(); done(0); return
                }
                // Found areas are not remembered: the next photos lie elsewhere.
                self.areas = found
                self.areaQuads = normalised
                self.showingGlass = true
                self.refreshPreview()
                self.updateStatus()
                self.banner.show(found.isEmpty ? .info : .success,
                                 found.isEmpty ? L("No separate photos found on the glass.") : L("Photos found: %d. Each is scanned as its own image.", found.count))
                done(found.count)
            }
        }
    }

    @objc func clearAreasAction(_ sender: Any?) {
        overviewGeneration = UUID()
        areas = []
        areaQuads = []
        Store.lastAreas = []
        refreshPreview()
        updateStatus()
    }

    @objc func showGlassAction(_ sender: Any?) {
        showingGlass.toggle()
        refreshPreview()
        updateStatus()
    }

    @objc func selectScannerAction(_ sender: Any?) {
        guard let item = sender as? NSMenuItem, let s = item.representedObject as? ScannerBackend else { return }
        if s !== scanner { choose(s) }
    }

    // MARK: - Presets

    @objc func presetAction(_ sender: Any?) { settingsPanel.choosePreset(commandTag(of: sender) - 1) }

    @objc func savePresetAction(_ sender: Any?) {
        guard let window else { return }
        let field = NSTextField(string: "")
        field.placeholderString = L("Preset name")
        let alert = NSAlert.form(L("Save Settings as Preset"), info: L("The current settings get a name in the Preset menu."), ok: L("Save"), rows: [(L("Name:"), field)])
        alert.window.initialFirstResponder = field
        alert.beginSheetModal(for: window) { [weak self] r in
            let name = field.stringValue.trimmingCharacters(in: .whitespaces)
            guard r == .alertFirstButtonReturn, !name.isEmpty, let self else { return }
            var list = Store.userPresets
            list.append(Preset(kind: .user, userName: name, settings: self.settings))
            Store.userPresets = list
            Store.presetIndex = Store.allPresets.count - 1
            self.presetsChanged()
        }
    }

    @objc func deletePresetAction(_ sender: Any?) {
        let i = Store.presetIndex - Preset.builtIn.count
        var list = Store.userPresets
        guard list.indices.contains(i) else { return }
        list.remove(at: i)
        Store.userPresets = list
        Store.presetIndex = -1
        presetsChanged()
    }

    func presetsChanged() {
        Cmd.rebuildPresets()
        AppDelegate.shared?.windows.forEach { $0.settingsPanel.reloadPresets() }
    }

    // MARK: - Import

    /// File › Open and Import: images and PDF pages become pages, cleaned up like scans.
    @objc func importAction(_ sender: Any?) {
        guard let window else { return }
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = true
        panel.allowedContentTypes = [.image, .pdf]
        panel.message = L("Choose images or PDF files to add as pages")
        panel.beginSheetModal(for: window) { [weak self] r in
            if r == .OK { self?.importFiles(panel.urls, at: nil) }
        }
    }

    @objc func openDocument(_ sender: Any?) { importAction(sender) }

    func importFiles(_ urls: [URL], at row: Int?) {
        guard !hasPendingWork, !windowIsClosed else { banner.show(.info, L("Wait for the current operation to finish.")); return }
        let items: [(RawScan, Bool)]
        do { items = try ContinuityImport.loadChecked(urls) }
        catch { showError(error); return }
        guard !items.isEmpty else { banner.show(.warning, L("No images found in these files.")); return }
        // -1 means before the first page; nil is reserved for appending at the end.
        if let row { insertionIndex = min(max(0, row), pages.count) - 1 }
        scannedThisRun = []
        blanksThisRun = 0
        scanHadError = false; scanGeneration = UUID()
        for (raw, isPDF) in items { process(raw, settings.processing, removeBlank: false, documentScan: isPDF) }
    }

    /// Continuity Camera result (Take Photo → image to clean up; Scan Documents → PDF pages from the phone).
    func importContinuity(_ pboard: NSPasteboard) -> Bool {
        guard !hasPendingWork, !windowIsClosed else { banner.show(.info, L("Wait for the current operation to finish.")); return false }
        guard let imported = ContinuityImport.read(pboard), !imported.pages.isEmpty else { return false }
        scannedThisRun = []
        blanksThisRun = 0
        scanHadError = false; scanGeneration = UUID()
        for raw in imported.pages { process(raw, settings.processing, removeBlank: false, documentScan: imported.isDocumentScan) }
        return true
    }

    /// Shows the Continuity Camera menu (Take Photo / Scan Documents for each nearby iPhone or iPad).
    @objc func scanWithPhoneAction(_ sender: Any?) {
        window?.makeFirstResponder(preview.canvas)
        let menu = NSMenu()
        menu.addItem(Cmd.importFromDeviceItem())
        let view = (sender as? NSView) ?? toolbarImpl.phoneButton
        menu.popUp(positioning: nil, at: NSPoint(x: 0, y: view.bounds.maxY + 4), in: view)
    }

    // MARK: - Pages

    func indices(of list: [ScanPage]) -> IndexSet { IndexSet(list.compactMap { p in pages.firstIndex { $0 === p } }) }

    @objc func rotateLeftAction(_ sender: Any?) { rotate(270) }
    @objc func rotateRightAction(_ sender: Any?) { rotate(90) }

    private func rotate(_ degrees: Int) {
        let targets = targetPages
        guard !targets.isEmpty else { return }
        let sel = pageList.selection
        doc.modify(targets, name: degrees == 90 ? L("Rotate Right") : L("Rotate Left")) { $0.rotate(clockwise: degrees) }
        pageList.select(sel, scroll: false)
    }

    @objc func deletePageAction(_ sender: Any?) {
        let targets = targetPages
        guard !targets.isEmpty else { return }
        let first = indices(of: targets).first ?? 0
        doc.remove(ids: Set(targets.map(\.id)), name: targets.count == 1 ? L("Delete Page") : L("Delete Pages"))
        if !pages.isEmpty { pageList.select([min(first, pages.count - 1)]) }
    }

    @objc func delete(_ sender: Any?) { deletePageAction(sender) }

    override func selectAll(_ sender: Any?) {
        showingGlass = false
        pageList.select(Array(0..<pages.count), scroll: false)
        refreshPreview()
    }

    @objc func movePageUpAction(_ sender: Any?) { movePages(-1) }
    @objc func movePageDownAction(_ sender: Any?) { movePages(1) }

    private func movePages(_ delta: Int) {
        let sel = IndexSet(pageList.selection)
        guard let first = sel.first, let last = sel.last else { return }
        let dest = delta < 0 ? first - 1 : last + 2
        guard dest >= 0, dest <= pages.count else { return }
        let ids = sel.map { pages[$0].id }
        doc.move(sel, to: dest)
        pageList.select(ids.compactMap { id in pages.firstIndex { $0.id == id } })
    }

    @objc func rescanPageAction(_ sender: Any?) {
        guard canScan, pendingProcessing == 0, let page = targetPages.first else { return }
        rescanTarget = page
        updateStatus()
        scanAction(nil)
    }

    @objc func insertHereAction(_ sender: Any?) {
        if insertionIndex != nil { insertionIndex = nil; return }
        guard let i = indices(of: targetPages).last else { return }
        insertionIndex = i
        banner.show(.info, L("The next scans are inserted after page %d.", i + 1))
    }

    /// The selected page and everything after it go to a new window.
    @objc func splitAction(_ sender: Any?) {
        guard let i = pageList.selection.first, i > 0, i < pages.count, let other = AppDelegate.shared?.newWindow() else { return }
        let moving = Array(pages[i...])
        doc.remove(ids: Set(moving.map(\.id)), name: L("Split Document Here"))
        other.doc.append(moving.map { $0.copy() })
        other.doc.undo.removeAllActions()
        other.pageList.select([0])
    }

    // MARK: - Tools

    @objc func enhanceToggleAction(_ sender: Any?) {
        let targets = targetPages
        guard !targets.isEmpty else { return }
        let to: Enhancement = targets.allSatisfy { $0.options.enhancement == .document } ? .none : .document
        refinish(targets, name: L("Enhance")) { $0.enhancement = to }
    }

    @objc func enhancementAction(_ sender: Any?) {
        guard let e = Enhancement(rawValue: commandTag(of: sender) - 1) else { return }
        refinish(targetPages, name: L("Clean-up")) { $0.enhancement = e }
    }

    @objc func pageColorAction(_ sender: Any?) {
        guard let m = ColorMode(rawValue: commandTag(of: sender) - 1) else { return }
        refinish(targetPages, name: L("Colour")) { $0.colorMode = m }
    }

    /// Re-runs the clean-up of some pages in the background (one undo step).
    private func refinish(_ targets: [ScanPage], name: String, _ change: @escaping (inout ProcessOptions) -> Void) {
        guard busyText == nil, !targets.isEmpty else { return }
        let jobs = targets.compactMap { page -> (ScanPage, UUID, ScanPage.Snapshot, ProcessOptions)? in
            var options = page.options; change(&options)
            guard options != page.options else { return nil }
            return (page, page.contentRevision, page.snapshot, options)
        }
        guard !jobs.isEmpty else { return }
        let token = UUID(); pageWorkGeneration = token
        busyText = L("Cleaning up…")
        updateStatus()
        processQueue.async {
            let result = Result { try jobs.map { ($0.0, $0.1, try $0.2.finished($0.3)) } }
            DispatchQueue.main.async { [weak self] in
                guard let self, self.pageWorkGeneration == token else { return }
                self.busyText = nil
                switch result {
                case .success(let states): _ = self.applyPageSnapshots(states, name: name)
                case .failure(let error): self.showError(error)
                }
                self.updateStatus()
            }
        }
    }

    /// Only the exact pixels used by a worker can receive its result; a removed or edited page stays untouched.
    @discardableResult func applyPageSnapshots(_ states: [(ScanPage, UUID, ScanPage.Snapshot)], name: String) -> Int {
        let valid = states.filter { page, revision, _ in pages.contains { $0 === page } && page.contentRevision == revision }
        doc.modify(valid.map { $0.0 }, name: name) { page in
            if let state = valid.first(where: { $0.0 === page }) { page.restore(state.2) }
        }
        return valid.count
    }

    @objc func restraightenAction(_ sender: Any?) {
        let targets = targetPages
        guard busyText == nil, !targets.isEmpty else { return }
        let jobs = targets.map { ($0, $0.contentRevision, $0.snapshot) }
        let token = UUID(); pageWorkGeneration = token
        busyText = L("Cleaning up…")
        updateStatus()
        processQueue.async {
            let result = Result {
                try jobs.map { job -> (ScanPage, UUID, ScanPage.Snapshot) in
                    var options = job.2.options; options.crop = true; options.deskew = true
                    let page = try ScanPage.make(RawScan(image: job.2.base, dpi: job.2.dpi), options: options)
                    return (job.0, job.1, page.snapshot)
                }
            }
            DispatchQueue.main.async { [weak self] in
                guard let self, self.pageWorkGeneration == token else { return }
                self.busyText = nil
                switch result {
                case .success(let states): _ = self.applyPageSnapshots(states, name: L("Crop and Straighten Again"))
                case .failure(let error): self.showError(error)
                }
                self.updateStatus()
            }
        }
    }

    @objc func removeBlankAction(_ sender: Any?) {
        let blank = pages.filter { $0.report.blank }
        guard !blank.isEmpty else { banner.show(.info, L("No blank pages found.")); return }
        doc.remove(ids: Set(blank.map(\.id)), name: L("Remove Blank Pages"))
        banner.show(.success, L("Blank pages removed: %d.", blank.count))
    }

    /// Recognises text on every page that has none yet; `then` runs on the main thread afterwards.
    func ensureText(_ targets: [ScanPage], then: @escaping (Bool) -> Void) {
        let jobs = targets.filter { $0.text == nil }.map { ($0, $0.contentRevision, $0.image) }
        guard !jobs.isEmpty else { then(true); return }
        guard busyText == nil else { then(false); return }
        let token = UUID(); pageWorkGeneration = token
        busyText = L("Recognizing text…")
        updateStatus()
        processQueue.async {
            let result = Result { try jobs.map { ($0.0, $0.1, try OCR.recognizeChecked($0.2)) } }
            DispatchQueue.main.async { [weak self] in
                guard let self, self.pageWorkGeneration == token else { then(false); return }
                self.busyText = nil
                switch result {
                case .success(let results):
                    let accepted = results.map { self.doc.setText($0.0, $0.2, expectedContentRevision: $0.1) }.allSatisfy { $0 }
                    self.updateStatus(); then(accepted)
                case .failure(let error): self.updateStatus(); self.showError(error); then(false)
                }
            }
        }
    }

    @objc func recognizeTextAction(_ sender: Any?) {
        ensureText(pages) { [weak self] success in
            guard success, let self else { return }
            let lines = self.pages.reduce(0) { $0 + ($1.text?.count ?? 0) }
            self.banner.show(.success, L("Text recognized: %d lines. Saved PDFs are searchable.", lines))
        }
    }

    @objc func copyTextAction(_ sender: Any?) {
        let targets = targetPages
        copyText(targets, to: .general)
    }

    func copyText(_ targets: [ScanPage], to pasteboard: NSPasteboard, completion: ((Bool) -> Void)? = nil) {
        ensureText(targets) { [weak self] success in
            guard success else { completion?(false); return }
            let text = targets.compactMap(\.text).map(OCR.text).joined(separator: "\n\n")
            guard !text.isEmpty else { self?.banner.show(.info, L("No text found on these pages.")); completion?(false); return }
            pasteboard.clearContents()
            let written = pasteboard.setString(text, forType: .string)
            self?.banner.show(.success, L("Text copied."))
            completion?(written)
        }
    }

    @objc func copy(_ sender: Any?) {
        _ = copyPageImage(to: .general)
    }

    @discardableResult func copyPageImage(to pasteboard: NSPasteboard) -> Bool {
        guard let page = targetPages.first else { return false }
        pasteboard.clearContents()
        return pasteboard.writeObjects([NSImage(cgImage: page.image, size: NSSize(width: page.image.width, height: page.image.height))])
    }

    @objc func exportTextAction(_ sender: Any?) {
        guard let window else { return }
        ensureText(pages) { [weak self] success in
            guard success, let self else { return }
            let panel = NSSavePanel()
            panel.nameFieldStringValue = self.baseName() + ".txt"
            let options = TextExportOptions(panel: panel)
            panel.accessoryView = options.view
            panel.beginSheetModal(for: window) { r in
                guard r == .OK, var url = panel.url else { return }
                let rtf = options.rtf
                url = url.deletingPathExtension().appendingPathExtension(rtf ? "rtf" : "txt")
                do {
                    try self.writeText(to: url, rtf: rtf)
                    self.banner.show(.success, L("Saved: %@", url.lastPathComponent))
                } catch { self.showError(error) }
            }
        }
    }

    func writeText(to url: URL, rtf: Bool) throws {
        guard let data = Exporter.textData(pages.compactMap(\.text), rtf: rtf) else { throw CocoaError(.fileWriteUnknown) }
        try data.write(to: url, options: .atomic)
    }

    // MARK: - Saving

    func baseName(_ options: SaveOptions = Store.saveOptions) -> String {
        Exporter.expand(options.nameTemplate, counter: options.counter, title: pages.first?.text.flatMap(OCR.title))
    }

    @IBAction func saveDocument(_ sender: Any?) {
        presentSaveSheet()
    }

    func presentSaveSheet(completion: ((URL?) -> Void)? = nil) {
        guard let window, !pages.isEmpty else { return }
        var options = Store.saveOptions
        if Store.testDirectory != nil { options.folder = nil; options.openAfter = .none }
        let sheet = SaveSheetController(pages: pages.map(\.exportPage), title: pages.first?.text.flatMap(OCR.title), options: options)
        sheet.onSave = { [weak self] o in self?.save(o, completion: completion) }
        saveSheet = sheet
        guard let w = sheet.window else { return }
        window.beginSheet(w) { [weak self] _ in self?.saveSheet = nil }
    }

    @IBAction func saveDocumentAs(_ sender: Any?) { exportAction(sender) }

    /// Writes the document with `options` into its folder (OCR first when wanted), notes it in the history, opens it.
    func save(_ options: SaveOptions, completion: ((URL?) -> Void)? = nil) {
        let targets = pages
        let needsText = (options.ocr && options.format.isPDF) || options.nameTemplate.contains("{title}")
        if needsText {
            ensureText(targets) { [weak self] success in
                guard success, let self else { completion?(nil); return }
                self.writeFiles(options, targets: targets, completion)
            }
        } else { writeFiles(options, targets: targets, completion) }
    }

    private func writeFiles(_ options: SaveOptions, targets: [ScanPage], _ completion: ((URL?) -> Void)?) {
        let contents = doc.captureSavedContents(targets)
        let exportPages: [ExportPage] = targets.map(\.exportPage)
        let base = Exporter.expand(options.nameTemplate, counter: options.counter, title: exportPages.first?.title)
        busyText = L("Saving…")
        updateStatus()
        DispatchQueue.global(qos: .userInitiated).async {
            var urls: [URL] = []
            var failure: Error?
            do { urls = try Exporter.write(exportPages, folder: options.folderURL, baseName: base, options: options) } catch { failure = error }
            DispatchQueue.main.async { [weak self] in
                guard let self else { return }
                self.busyText = nil
                self.updateStatus()
                if let failure {
                    self.showError(failure)
                    completion?(nil)
                    return
                }
                var next = options
                next.counter = options.counter == Int.max ? Int.max : options.counter + 1
                Store.saveOptions = next
                self.didSave(urls, options, contents: contents, snapshot: exportPages)
                completion?(urls.first)
            }
        }
    }

    private func didSave(_ urls: [URL], _ o: SaveOptions, contents: ScanDocument.SavedContents, snapshot: [ExportPage]) {
        guard let first = urls.first else { return }
        savedURL = first
        doc.markSaved(contents: contents)
        window?.isDocumentEdited = doc.isDirty
        window?.title = urls.count == 1 ? first.lastPathComponent : L("%d files", urls.count)
        let bytes = urls.reduce(0) { $0 + ((try? $1.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0) }
        for (index, url) in urls.enumerated() {
            let thumbnail = snapshot.indices.contains(index) ? snapshot[index].image : snapshot.first?.image
            History.shared.add(url, pages: urls.count == 1 ? snapshot.count : 1, thumbnail: thumbnail)
        }
        if Store.testDirectory == nil { urls.forEach { NSDocumentController.shared.noteNewRecentDocumentURL($0) } }
        let name = urls.count == 1 ? first.lastPathComponent : L("%d files", urls.count)
        banner.show(.success, L("Saved “%@” (%@).", name, Exporter.sizeText(bytes)), action: L("Show in Finder")) {
            NSWorkspace.shared.activateFileViewerSelecting(urls)
        }
        open(urls, with: o.openAfter)
    }

    func open(_ urls: [URL], with choice: OpenAfter) {
        switch choice {
        case .none: return
        case .finder: NSWorkspace.shared.activateFileViewerSelecting(urls)
        case .defaultApp: urls.forEach { NSWorkspace.shared.open($0) }
        case .pdf42, .img42:
            if let id = choice.bundleID, let app = NSWorkspace.shared.urlForApplication(withBundleIdentifier: id) {
                NSWorkspace.shared.open(urls, withApplicationAt: app, configuration: NSWorkspace.OpenConfiguration())
            } else {
                urls.forEach { NSWorkspace.shared.open($0) }
            }
        }
    }

    /// Save to a place and name of your choice, in any format.
    @objc func exportAction(_ sender: Any?) {
        guard let window, !pages.isEmpty else { return }
        var o = Store.saveOptions
        let panel = NSSavePanel()
        panel.nameFieldStringValue = baseName(o) + "." + o.format.ext
        panel.allowedContentTypes = [o.format.type]
        let popup = NSPopUpButton(frame: NSRect(x: 0, y: 0, width: 240, height: 26), pullsDown: false)
        popup.addItems(withTitles: SaveFormat.allCases.map(\.title))
        popup.selectItem(at: o.format.rawValue)
        let box = NSStackView(views: [NSTextField(labelWithString: L("Format:")), popup])
        box.edgeInsets = NSEdgeInsets(top: 8, left: 8, bottom: 8, right: 8)
        panel.accessoryView = box
        let handler = FormatPopupHandler(panel: panel)
        popup.target = handler
        popup.action = #selector(FormatPopupHandler.changed(_:))
        objc_setAssociatedObject(panel, "handler", handler, .OBJC_ASSOCIATION_RETAIN)
        panel.beginSheetModal(for: window) { [weak self] r in
            guard let self, r == .OK, let url = panel.url else { return }
            o.format = SaveFormat(rawValue: popup.indexOfSelectedItem) ?? .pdf
            o.folder = url.deletingLastPathComponent().path
            o.nameTemplate = url.deletingPathExtension().lastPathComponent
            o.openAfter = .none
            // The chosen name is final: write it directly.
            let target = url.deletingPathExtension().appendingPathExtension(o.format.ext)
            let targets = self.pages
            let run = {
                do {
                    try self.writeExport(to: target, options: o, targets: targets)
                } catch { self.showError(error) }
            }
            if o.ocr && o.format.isPDF { self.ensureText(targets) { success in if success { run() } } } else { run() }
        }
    }

    /// Writes the chosen destination from one immutable document snapshot.
    @discardableResult func writeExport(to target: URL, options: SaveOptions, targets: [ScanPage]? = nil) throws -> [URL] {
        let targets = targets ?? pages
        let contents = doc.captureSavedContents(targets)
        let snapshot = targets.map(\.exportPage)
        let urls = try Exporter.write(snapshot, folder: target.deletingLastPathComponent(),
                                      baseName: target.deletingPathExtension().lastPathComponent, options: options, replaceExisting: true)
        didSave(urls, options, contents: contents, snapshot: snapshot)
        return urls
    }

    /// A temporary PDF of the document for Share and Mail.
    func temporaryPDFChecked(targets: [ScanPage]? = nil) throws -> URL {
        var o = Store.saveOptions
        o.format = .pdf
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("SCN42-share-" + UUID().uuidString, isDirectory: true)
        do {
            guard let result = try Exporter.write((targets ?? pages).map(\.exportPage), folder: dir, baseName: baseName(o), options: o).first else {
                throw CocoaError(.fileWriteUnknown)
            }
            return result
        } catch { try? FileManager.default.removeItem(at: dir); throw error }
    }

    func temporaryPDF(targets: [ScanPage]? = nil) -> URL? {
        guard !(targets ?? pages).isEmpty else { return nil }
        do { return try temporaryPDFChecked(targets: targets) }
        catch { showError(error); return nil }
    }

    @objc func mailAction(_ sender: Any?) {
        let targets = pages
        ensureText(targets) { [weak self] success in
            guard success, let url = self?.temporaryPDF(targets: targets), let service = NSSharingService(named: .composeEmail) else { return }
            service.perform(withItems: [url])
        }
    }

    func shareItems() -> [Any] { temporaryPDF().map { [$0] } ?? [] }

    // MARK: - Printing

    @objc func printDocument(_ sender: Any?) { printPages(pages, copies: 1, direct: false) }
    @objc func runPageLayout(_ sender: Any?) { NSPageLayout().beginSheet(with: NSPrintInfo.shared, modalFor: window!, delegate: nil, didEnd: nil, contextInfo: nil) }

    func printOperationChecked(_ list: [ScanPage], copies: Int) throws -> NSPrintOperation {
        guard (1...99).contains(copies) else { throw CocoaError(.fileWriteUnknown) }
        let data = try Exporter.pdfChecked(list.map(\.exportPage), compression: .original, ocr: false, archival: false)
        guard let doc = PDFDocument(data: data) else { throw CocoaError(.fileWriteUnknown) }
        let info = (NSPrintInfo.shared.copy() as? NSPrintInfo) ?? NSPrintInfo.shared
        info.dictionary()[NSPrintInfo.AttributeKey.copies] = copies
        guard let op = doc.printOperation(for: info, scalingMode: .pageScaleToFit, autoRotate: true) else { throw CocoaError(.fileWriteUnknown) }
        op.jobTitle = baseName()
        op.printPanel.options.formUnion([.showsCopies, .showsPaperSize, .showsOrientation, .showsScaling, .showsPreview])
        return op
    }

    func printOperation(_ list: [ScanPage], copies: Int) -> NSPrintOperation? {
        try? printOperationChecked(list, copies: copies)
    }

    /// `direct`: the copier prints the fresh scan without the print dialog.
    func printPages(_ list: [ScanPage], copies: Int, direct: Bool) {
        guard let window, !list.isEmpty else { return }
        do {
            let op = try printOperationChecked(list, copies: copies)
            op.showsPrintPanel = !direct
            op.showsProgressPanel = true
            if let printOperationRunner { printOperationRunner(op, direct) }
            else { op.runModal(for: window, delegate: nil, didRun: nil, contextInfo: nil) }
        } catch { showError(error) }
    }

    /// Copier: asks how many copies, scans, prints on the default printer.
    @objc func scanAndPrintAction(_ sender: Any?) {
        guard let window, canScan else { explainCannotScan(); return }
        let field = NSTextField(string: "1")
        let stepper = NSStepper()
        stepper.minValue = 1
        stepper.maxValue = 99
        stepper.integerValue = 1
        stepper.target = field
        stepper.action = #selector(NSTextField.takeIntegerValueFrom(_:))
        let row = NSStackView(views: [field, stepper])
        field.translatesAutoresizingMaskIntoConstraints = false
        field.widthAnchor.constraint(equalToConstant: 50).isActive = true
        let printer = NSPrintInfo.defaultPrinter?.name ?? L("no printer")
        let alert = NSAlert.form(L("Copy (Scan and Print)"), info: L("Scans with the current settings and prints right away on %@.", printer),
                                 ok: L("Scan and Print"), rows: [(L("Copies:"), row)])
        alert.beginSheetModal(for: window) { [weak self] r in
            guard r == .alertFirstButtonReturn, let self else { return }
            self.copierCopies = max(1, min(99, field.integerValue))
            self.scanAction(nil)
        }
    }

    // MARK: - View

    @objc func togglePagesAction(_ sender: Any?) { sidebarItem.animator().isCollapsed.toggle() }

    @objc func showSettingsAction(_ sender: Any?) { toggleInspector(history: false) }
    @objc func showHistoryAction(_ sender: Any?) { toggleInspector(history: true) }

    func toggleInspector(history: Bool) {
        if !inspectorItem.isCollapsed && inspector.showsHistory == history {
            inspectorItem.animator().isCollapsed = true
        } else {
            inspector.show(history: history)
            inspectorItem.animator().isCollapsed = false
        }
    }

    @objc func zoomInAction(_ sender: Any?) { preview.zoom(by: 1.25) }
    @objc func zoomOutAction(_ sender: Any?) { preview.zoom(by: 0.8) }
    @objc func actualSizeAction(_ sender: Any?) { preview.actualSize() }
    @objc func zoomToFitAction(_ sender: Any?) { preview.zoomToFit() }

    func showError(_ error: Error) {
        guard let window else { return }
        NSAlert(error: error).beginSheetModal(for: window)
    }
}

/// Keeps the save panel's file type in step with the format popup.
final class FormatPopupHandler: NSObject {
    weak var panel: NSSavePanel?
    init(panel: NSSavePanel) { self.panel = panel }
    @objc func changed(_ sender: NSPopUpButton) {
        guard let panel, let f = SaveFormat(rawValue: sender.indexOfSelectedItem) else { return }
        panel.allowedContentTypes = [f.type]
        let base = (panel.nameFieldStringValue as NSString).deletingPathExtension
        panel.nameFieldStringValue = base + "." + f.ext
    }
}
