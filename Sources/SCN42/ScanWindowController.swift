import AppKit
import Kit42

/// Settings or History in the right-hand inspector.
final class InspectorController: NSViewController {
    let switcher = NSSegmentedControl(labels: [L("Settings"), L("History")], trackingMode: .selectOne, target: nil, action: nil)
    let settings: SettingsPanelController
    let history: HistoryPanelController
    private let container = NSView()
    var showsHistory: Bool { switcher.selectedSegment == 1 }

    init(settings: SettingsPanelController, history: HistoryPanelController) {
        self.settings = settings
        self.history = history
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) { fatalError("not supported") }

    override func loadView() {
        let root = NSView()
        switcher.target = self
        switcher.action = #selector(switched)
        switcher.selectedSegment = 0
        switcher.segmentDistribution = .fillEqually
        for v in [switcher, container] as [NSView] {
            v.translatesAutoresizingMaskIntoConstraints = false
            root.addSubview(v)
        }
        NSLayoutConstraint.activate([
            switcher.topAnchor.constraint(equalTo: root.safeAreaLayoutGuide.topAnchor, constant: 10),
            switcher.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 16),
            switcher.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -16),
            container.topAnchor.constraint(equalTo: switcher.bottomAnchor, constant: 4),
            container.leadingAnchor.constraint(equalTo: root.leadingAnchor), container.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            container.bottomAnchor.constraint(equalTo: root.bottomAnchor),
        ])
        view = root
        show(history: false)
    }

    @objc private func switched() { show(history: switcher.selectedSegment == 1) }

    func show(history h: Bool) {
        switcher.selectedSegment = h ? 1 : 0
        container.subviews.forEach { $0.removeFromSuperview() }
        let v = h ? history.view : settings.view
        v.translatesAutoresizingMaskIntoConstraints = false
        container.addSubview(v)
        NSLayoutConstraint.activate([
            v.leadingAnchor.constraint(equalTo: container.leadingAnchor), v.trailingAnchor.constraint(equalTo: container.trailingAnchor),
            v.topAnchor.constraint(equalTo: container.topAnchor), v.bottomAnchor.constraint(equalTo: container.bottomAnchor),
        ])
        if h { history.reload() }
    }
}

/// One scanning window: pages on the left, the big preview in the middle, settings or history on the right.
final class ScanWindowController: NSWindowController, NSWindowDelegate, NSMenuItemValidation, NSToolbarItemValidation {
    let doc = ScanDocument()
    let split = NSSplitViewController()
    let pageList = PageListController()
    let preview = PreviewController()
    let settingsPanel = SettingsPanelController()
    let historyPanel = HistoryPanelController()
    lazy var inspector = InspectorController(settings: settingsPanel, history: historyPanel)
    let banner = BannerView()
    let statusIcon = NSImageView()
    let statusLabel = NSTextField(labelWithString: "")
    let spinner = NSProgressIndicator()
    let viewSwitch = NSSegmentedControl(labels: [L("Glass"), L("Pages")], trackingMode: .selectOne, target: nil, action: nil)
    let countLabel = NSTextField(labelWithString: "")
    let statusBar = NSStackView()
    let statusLine = NSBox()
    var sidebarItem: NSSplitViewItem!
    var inspectorItem: NSSplitViewItem!
    lazy var toolbarImpl = ScanToolbar(owner: self)

    // Scanning
    var scanner: ScannerBackend?
    var overview: CGImage?
    var areas: [CGRect] = Store.lastAreas
    /// Photo outlines found on the overview (normalised to the glass), one per area; nil for hand-drawn areas.
    var areaQuads: [Quad?] = []
    var showingGlass = false
    var isScanning = false
    var pendingProcessing = 0
    let processQueue = DispatchQueue(label: "scn42.process", qos: .userInitiated)
    var pageWorkGeneration = UUID()
    var acquisitionGeneration = UUID()
    var scanGeneration = UUID()
    var overviewGeneration = UUID()
    var overviewJobGeneration = UUID()
    var windowIsClosed = false
    var preparingSource = false
    var preparedSource: ScanSource?
    private var lastSettingsSource = Store.settings.source
    private var insertionAnchorID: UUID?
    private var insertsBeforeFirst = false
    var insertionIndex: Int? {
        get { insertsBeforeFirst ? -1 : insertionAnchorID.flatMap { id in pages.firstIndex { $0.id == id } } }
        set {
            insertsBeforeFirst = newValue == -1
            insertionAnchorID = newValue.flatMap { pages.indices.contains($0) ? pages[$0].id : nil }
            pageList.insertionIndex = insertionIndex
        }
    }
    var rescanTarget: ScanPage?
    var copierCopies: Int?
    var scannedThisRun: [ScanPage] = []
    var blanksThisRun = 0
    var scanHadError = false
    var busyText: String?
    var savedURL: URL?
    var saveSheet: SaveSheetController?
    /// Optional execution seam for native print-to-file checks; ordinary windows use AppKit's print UI.
    var printOperationRunner: ((NSPrintOperation, Bool) -> Void)?

    init() {
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1180, height: 780),
                              styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
                              backing: .buffered, defer: false)
        window.minSize = NSSize(width: 860, height: 560)
        window.toolbarStyle = .unified
        window.collectionBehavior.insert(.fullScreenPrimary)
        window.title = L("New Scan")
        super.init(window: window)
        window.delegate = self
        shouldCascadeWindows = true
        setupContent()
        window.toolbar = toolbarImpl.toolbar
        window.setFrameAutosaveName("SCN42.window")
        if window.frame.width < 860 { window.setContentSize(NSSize(width: 1180, height: 780)) }
        window.center()

        doc.onChange = { [weak self] in self?.documentChanged() }
        doc.captureSelection = { [weak self] in
            guard let self else { return [] }
            return self.pageList.selection.compactMap { self.pages.indices.contains($0) ? self.pages[$0].id : nil }
        }
        doc.restoreSelection = { [weak self] ids in
            guard let self else { return }
            let selected = Set(ids)
            self.pageList.select(self.pages.indices.filter { selected.contains(self.pages[$0].id) })
        }
        NotificationCenter.default.addObserver(self, selector: #selector(devicesChanged), name: DeviceManager.changed, object: nil)
        NotificationCenter.default.addObserver(self, selector: #selector(statusChanged(_:)), name: DeviceManager.statusChanged, object: nil)
        devicesChanged()
        refreshPreview()
        updateStatus()
    }

    required init?(coder: NSCoder) { fatalError("not supported") }

    // MARK: - Layout

    private func setupContent() {
        let main = NSViewController()
        let root = NSView()

        statusIcon.image = Theme42.symbol("circle.fill", size: 8)
        spinner.style = .spinning
        spinner.controlSize = .small
        spinner.isDisplayedWhenStopped = false
        statusLabel.font = .systemFont(ofSize: 11.5)
        statusLabel.textColor = .secondaryLabelColor
        statusLabel.lineBreakMode = .byTruncatingTail
        statusLabel.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        countLabel.font = .monospacedDigitSystemFont(ofSize: 11.5, weight: .regular)
        countLabel.textColor = .secondaryLabelColor
        viewSwitch.controlSize = .small
        viewSwitch.target = self
        viewSwitch.action = #selector(viewSwitched)
        viewSwitch.setToolTip(L("The scanner glass (overview and scan areas)"), forSegment: 0)
        viewSwitch.setToolTip(L("The scanned pages"), forSegment: 1)
        statusBar.setViews([statusIcon, spinner, statusLabel], in: .leading)
        statusBar.setViews([viewSwitch], in: .center)
        statusBar.setViews([countLabel], in: .trailing)
        statusBar.spacing = 6
        statusBar.edgeInsets = NSEdgeInsets(top: 0, left: 12, bottom: 0, right: 12)

        let line = statusLine
        line.boxType = .separator
        let stack = NSStackView(views: [banner, preview.view, line, statusBar])
        stack.orientation = .vertical
        stack.spacing = 0
        for v in [banner, preview.view, line, statusBar] { v.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true }
        banner.heightAnchor.constraint(equalToConstant: Theme42.bannerHeight).isActive = true
        statusBar.heightAnchor.constraint(equalToConstant: 30).isActive = true
        preview.view.setContentHuggingPriority(.defaultLow, for: .vertical)
        stack.translatesAutoresizingMaskIntoConstraints = false
        root.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: root.leadingAnchor), stack.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            stack.topAnchor.constraint(equalTo: root.safeAreaLayoutGuide.topAnchor), stack.bottomAnchor.constraint(equalTo: root.bottomAnchor),
            root.widthAnchor.constraint(greaterThanOrEqualToConstant: 380), root.heightAnchor.constraint(greaterThanOrEqualToConstant: 320),
        ])
        main.view = root

        preview.canvas.onAreasChanged = { [weak self] a in
            self?.areas = a
            self?.areaQuads = []
            Store.lastAreas = a
            self?.updateStatus()
        }
        pageList.onSelect = { [weak self] _ in
            guard let self else { return }
            if self.showingGlass && !self.pageList.selection.isEmpty { self.showingGlass = false }
            self.refreshPreview()
        }
        pageList.onMove = { [weak self] rows, to in self?.doc.move(rows, to: to) }
        pageList.onDropFiles = { [weak self] urls, row in self?.importFiles(urls, at: row) }
        settingsPanel.onChange = { [weak self] _ in self?.settingsChanged() }
        settingsPanel.onPreset = { [weak self] i in self?.settingsPanel.choosePreset(i) }

        sidebarItem = NSSplitViewItem(sidebarWithViewController: pageList)
        sidebarItem.minimumThickness = 160
        sidebarItem.maximumThickness = 280
        sidebarItem.canCollapse = true
        let mainItem = NSSplitViewItem(viewController: main)
        mainItem.minimumThickness = 380
        inspectorItem = NSSplitViewItem(inspectorWithViewController: inspector)
        inspectorItem.minimumThickness = 300
        inspectorItem.maximumThickness = 380
        inspectorItem.canCollapse = true
        split.splitViewItems = [sidebarItem, mainItem, inspectorItem]
        contentViewController = split
        window?.setContentSize(NSSize(width: 1180, height: 780))
    }

    // MARK: - State

    var settings: ScanSettings { settingsPanel.settings }
    var pages: [ScanPage] { doc.pages }

    /// Pages the page commands act on: the selection, or the one shown.
    var targetPages: [ScanPage] {
        let sel = pageList.selection.filter { $0 < pages.count }
        if !sel.isEmpty { return sel.map { pages[$0] } }
        return shownPage.map { [$0] } ?? []
    }

    var shownPage: ScanPage? {
        guard !showingGlass else { return nil }
        if let i = pageList.selection.last, i < pages.count { return pages[i] }
        return pages.last
    }

    func documentChanged() {
        pageList.pages = pages
        if let id = insertionAnchorID, !pages.contains(where: { $0.id == id }) { insertionAnchorID = nil }
        pageList.insertionIndex = insertionIndex
        refreshPreview()
        updateStatus()
        window?.isDocumentEdited = doc.isDirty && !pages.isEmpty
        window?.toolbar?.validateVisibleItems()
    }

    func settingsChanged() {
        if lastSettingsSource != settings.source {
            lastSettingsSource = settings.source
            overviewGeneration = UUID(); overview = nil; areas = []; areaQuads = []
            Store.lastAreas = []
        }
        prepareScannerSource()
        refreshPreview()
        updateStatus()
    }

    func refreshPreview() {
        if showingGlass {
            preview.emptyTitle.stringValue = L("Nothing on the glass yet")
            preview.emptyHint.stringValue = L("Press Overview to see what lies on the glass, then drag to mark the parts to scan.")
            preview.show(overview, areas: areas)
        } else {
            preview.emptyTitle.stringValue = scanner == nil ? L("No scanner yet") : L("Put a document on the scanner")
            preview.emptyHint.stringValue = scanner == nil
                ? L("Connect a scanner or turn it on. You can also scan with your iPhone or import images.")
                : L("Press Scan. Every scan adds pages here; save them as one PDF when you are done.")
            preview.show(shownPage?.image)
        }
        dropStalePlacement()
        viewSwitch.selectedSegment = showingGlass ? 0 : 1
        viewSwitch.isHidden = !(scanner?.sources.contains(.flatbed) ?? false)
    }

    func updateStatus() {
        let name = scanner?.name ?? (DeviceManager.shared.finishedLocalSearch ? L("No scanner found") : L("Looking for scanners…"))
        var text = name
        var color = NSColor.tertiaryLabelColor
        if let scanner {
            text += " — " + scanner.status.text
            switch scanner.status {
            case .ready: color = .systemGreen
            case .warmingUp, .connecting, .busy, .scanning: color = .systemOrange
            case .error, .offline: color = .systemRed
            }
        }
        if let busyText { text += " · " + busyText }
        if showingGlass, settings.source == .flatbed, !areas.isEmpty { text += " · " + L("%d scan areas", areas.count) }
        statusLabel.stringValue = text
        statusIcon.contentTintColor = color
        let working = hasPendingWork
        spinner.isHidden = !working
        if working { spinner.startAnimation(nil) } else { spinner.stopAnimation(nil) }
        countLabel.stringValue = pages.isEmpty ? "" : pagesText(pages.count)
        settingsPanel.scanButton.isEnabled = canScan
        settingsPanel.overviewButton.isEnabled = canOverview
        toolbarImpl.share.isEnabled = !pages.isEmpty && !hasPendingWork
        toolbarImpl.phoneButton.isEnabled = !hasPendingWork
        toolbarImpl.scanners.isEnabled = !hasPendingWork
        settingsPanel.scanButton.title = isScanning ? L("Scanning…") : (rescanTarget != nil ? L("Rescan Page") : L("Scan"))
        window?.subtitle = pages.isEmpty ? "" : pagesText(pages.count)
        window?.toolbar?.validateVisibleItems()
    }

    var canScan: Bool { scanner?.status.canScan == true && !isScanning && !preparingSource && pendingProcessing == 0 && busyText == nil && !windowIsClosed && settingsPanel.hasValidInput }
    var hasPendingWork: Bool { isScanning || preparingSource || pendingProcessing > 0 || busyText != nil }
    var canOverview: Bool { canScan && settings.source == .flatbed && (scanner?.sources.contains(.flatbed) ?? false) }

    @objc func viewSwitched() {
        showingGlass = viewSwitch.selectedSegment == 0
        refreshPreview()
        updateStatus()
    }

    // MARK: - Scanners

    @objc func devicesChanged() {
        let list = DeviceManager.shared.scanners
        if let s = scanner, !list.contains(where: { $0 === s }) {
            scanGeneration = UUID(); overviewGeneration = UUID()
            isScanning = false; preparingSource = false; preparedSource = nil
            copierCopies = nil; scanner = nil; overview = nil; areaQuads = []
        }
        if scanner == nil {
            let preferred = list.first { $0.id == Store.lastScanner } ?? (list.count == 1 ? list.first : nil) ?? list.first
            if let preferred { choose(preferred) }
        }
        toolbarImpl.reloadScanners(list, selected: scanner)
        settingsPanel.update(scanner: scanner, capabilitiesReady: preparedSource == settings.source && !preparingSource)
        refreshPreview()
        updateStatus()
    }

    func choose(_ s: ScannerBackend) {
        guard !isScanning, pendingProcessing == 0, busyText == nil, !preparingSource else { return }
        overviewGeneration = UUID(); overview = nil; areaQuads = []
        preparedSource = nil
        scanner = s
        Store.lastScanner = s.id
        s.open()
        settingsPanel.update(scanner: s, capabilitiesReady: false)
        // A scanner without the chosen source: take what it has.
        if !s.sources.contains(settings.source), let first = s.sources.first {
            var st = settings
            st.source = first
            settingsPanel.apply(st)
        }
        toolbarImpl.reloadScanners(DeviceManager.shared.scanners, selected: s)
        lastSettingsSource = settings.source
        prepareScannerSource()
        updateStatus()
    }

    /// Selecting a feeder before scanning exposes its actual duplex/resolution capabilities.
    func prepareScannerSource() {
        guard let scanner, scanner.status.canScan, !isScanning, !preparingSource,
              scanner.sources.contains(settings.source), preparedSource != settings.source else { return }
        let source = settings.source
        preparingSource = true
        settingsPanel.update(scanner: scanner, capabilitiesReady: false)
        scanner.prepare(source: source) { [weak self, weak scanner] error in
            guard let self, let scanner, self.scanner === scanner, !self.windowIsClosed else { return }
            self.preparingSource = false
            if let error { self.banner.show(.warning, error.localizedDescription) }
            else { self.preparedSource = source }
            self.settingsPanel.update(scanner: scanner, capabilitiesReady: error == nil && self.settings.source == source)
            self.updateStatus()
            if error == nil && self.settings.source != source { self.prepareScannerSource() }
        }
    }

    @objc func statusChanged(_ note: Notification) {
        guard let s = note.object as? ScannerBackend, s === scanner else { return }
        // Capabilities are refreshed after this window prepares its chosen source, not on every
        // progress notification (another window may be using a different functional unit).
        prepareScannerSource()
        updateStatus()
    }

    // MARK: - Window

    func windowWillReturnUndoManager(_ window: NSWindow) -> UndoManager? { doc.undo }

    func windowShouldClose(_ sender: NSWindow) -> Bool {
        guard !isScanning, pendingProcessing == 0, busyText == nil, !preparingSource else {
            banner.show(.info, L("Wait for scanning and processing to finish before closing."))
            return false
        }
        guard doc.isDirty, !pages.isEmpty, ProcessInfo.processInfo.environment["K42_SNAPSHOT"] == nil else { return true }
        let alert = NSAlert()
        alert.messageText = L("Save the scanned pages?")
        alert.informativeText = L("%@ will be lost if you close the window without saving.", pagesText(pages.count))
        alert.addButton(withTitle: L("Save…"))
        alert.addButton(withTitle: L("Don't Save"))
        alert.addButton(withTitle: L("Cancel"))
        alert.beginSheetModal(for: sender) { [weak self] r in
            if r == .alertFirstButtonReturn {
                self?.presentSaveSheet { [weak self] url in
                    guard url != nil, let self, !self.doc.isDirty, !self.isScanning, self.pendingProcessing == 0 else { return }
                    sender.close()
                }
            }
            else if r == .alertSecondButtonReturn { self?.doc.isDirty = false; sender.close() }
        }
        return false
    }

    func windowWillClose(_ notification: Notification) {
        cancelMarkPlacement()
        windowIsClosed = true
        pageWorkGeneration = UUID(); acquisitionGeneration = UUID(); scanGeneration = UUID(); overviewGeneration = UUID()
        if isScanning { scanner?.cancel() }
        AppDelegate.shared?.windowClosed(self)
    }

    func window(_ window: NSWindow, willUseFullScreenPresentationOptions proposedOptions: NSApplication.PresentationOptions = []) -> NSApplication.PresentationOptions {
        Panels.fullScreenOptions
    }

    @objc func toggleAllPanelsAction(_ sender: Any?) {
        Panels.toggleAll(in: window) { [weak self] hidden in
            self?.statusBar.isHidden = hidden
            self?.statusLine.isHidden = hidden
            if hidden { self?.banner.hide() }
        }
    }

    // MARK: - Validation

    func validateMenuItem(_ item: NSMenuItem) -> Bool {
        switch item.action {
        case #selector(togglePagesAction(_:)): item.state = sidebarItem.isCollapsed ? .off : .on
        case #selector(showSettingsAction(_:)): item.state = !inspectorItem.isCollapsed && !inspector.showsHistory ? .on : .off
        case #selector(showHistoryAction(_:)): item.state = !inspectorItem.isCollapsed && inspector.showsHistory ? .on : .off
        case #selector(showGlassAction(_:)): item.state = showingGlass ? .on : .off
        case #selector(toggleAllPanelsAction(_:)): item.state = Panels.allHidden(in: window) ? .on : .off
        case #selector(insertHereAction(_:)): item.state = insertionIndex != nil ? .on : .off
        case #selector(presetAction(_:)): item.state = Store.presetIndex == item.tag - 1 ? .on : .off
        case #selector(enhancementAction(_:)):
            item.state = targetPages.contains { $0.options.enhancement.rawValue == item.tag - 1 } ? .on : .off
        case #selector(pageColorAction(_:)):
            item.state = targetPages.contains { $0.options.colorMode.rawValue == item.tag - 1 } ? .on : .off
        case #selector(selectScannerAction(_:)): item.state = (item.representedObject as AnyObject?) === scanner ? .on : .off
        default: break
        }
        return validate(item.action)
    }

    func validateToolbarItem(_ item: NSToolbarItem) -> Bool { validate(item.action) }

    func validate(_ action: Selector?) -> Bool {
        let hasPages = !pages.isEmpty
        let hasTarget = !targetPages.isEmpty
        switch action {
        case #selector(scanAction(_:)): return canScan
        case #selector(overviewAction(_:)), #selector(detectPhotosAction(_:)): return canOverview
        case #selector(cancelScanAction(_:)): return isScanning
        case #selector(importAction(_:)), #selector(openDocument(_:)), #selector(scanWithPhoneAction(_:)), #selector(selectScannerAction(_:)):
            return !hasPendingWork
        case #selector(clearAreasAction(_:)): return !areas.isEmpty
        case #selector(scanAndPrintAction(_:)): return canScan
        case #selector(deletePresetAction(_:)): return Store.presetIndex >= Preset.builtIn.count
        case #selector(saveDocument(_:)), #selector(saveDocumentAs(_:)), #selector(exportAction(_:)), #selector(printDocument(_:)),
             #selector(mailAction(_:)), #selector(exportTextAction(_:)), #selector(recognizeTextAction(_:)), #selector(removeBlankAction(_:)),
             #selector(runPageLayout(_:)):
            return hasPages && busyText == nil
        case #selector(rotateLeftAction(_:)), #selector(rotateRightAction(_:)), #selector(deletePageAction(_:)), #selector(delete(_:)),
             #selector(enhanceToggleAction(_:)), #selector(enhancementAction(_:)), #selector(pageColorAction(_:)), #selector(copyTextAction(_:)),
             #selector(restraightenAction(_:)):
            return hasTarget && busyText == nil
        case #selector(copy(_:)): return hasTarget
        case #selector(showStampsAction(_:)): return shownPage != nil && busyText == nil
        case #selector(selectAll(_:)): return hasPages
        case #selector(rescanPageAction(_:)): return hasTarget && canScan && pendingProcessing == 0 && targetPages.count == 1
        case #selector(insertHereAction(_:)): return hasTarget || insertionIndex != nil
        case #selector(splitAction(_:)): return pages.count > 1 && (pageList.selection.first ?? 0) > 0
        case #selector(movePageUpAction(_:)): return pageList.selection.first.map { $0 > 0 } ?? false
        case #selector(movePageDownAction(_:)): return pageList.selection.last.map { $0 < pages.count - 1 } ?? false
        case #selector(zoomInAction(_:)), #selector(zoomOutAction(_:)), #selector(actualSizeAction(_:)), #selector(zoomToFitAction(_:)):
            return preview.canvas.image != nil
        case #selector(showGlassAction(_:)): return scanner?.sources.contains(.flatbed) ?? false
        case #selector(NSDocument.duplicate(_:)), #selector(NSDocument.revertToSaved(_:)): return false
        default: return true
        }
    }
}

extension ScanWindowController: PanelHiding {}
