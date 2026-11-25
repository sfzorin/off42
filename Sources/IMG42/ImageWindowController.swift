import AppKit
import Kit42
import UniformTypeIdentifiers

/// One viewer window: an image set (folder or chosen files) with next/previous navigation.
final class ImageWindowController: NSWindowController, NSWindowDelegate, NSMenuItemValidation, NSToolbarItemValidation, NSSearchFieldDelegate {
    static let defaults = UserDefaults.standard

    let canvas = ImageCanvas()
    let filmstrip = Filmstrip()
    let banner = BannerView()
    let messageLabel = NSTextField(wrappingLabelWithString: "")
    let messageIcon = NSImageView()
    let splitController = NSSplitViewController()
    let infoPanel = InfoPanelController()
    let adjustPanel = AdjustPanelController()
    let inspectorContainer = NSViewController()
    var inspectorItem: NSSplitViewItem!
    lazy var toolbarImpl = ImageToolbar(owner: self)
    let undo = UndoManager()
    /// Undo target for file operations, so that clearing an image's edits keeps them.
    let fileUndo = NSObject()

    // Navigation set
    private(set) var urls: [URL] = []
    private(set) var index = 0
    var sortOrder: SortOrder = SortOrder(rawValue: defaults.integer(forKey: "sortOrder")) ?? .name
    var wraps: Bool = defaults.object(forKey: "wraps") as? Bool ?? true
    var background: Background = Background(rawValue: defaults.integer(forKey: "background")) ?? .neutral

    // Current image
    var loaded: LoadedImage?
    var page = 0
    private var loadToken = 0
    private var cacheGeneration = UUID()
    var imageSession = UUID()
    var searchGeneration = UUID()
    var editGeneration = UUID()
    private var cache: [URL: LoadedImage] = [:]
    private var animationTimer: Timer?
    private var frameIndex = 0
    var slideshowTimer: Timer?
    var slideshowPaused = false
    private var slideshowEnteredFullScreen = false
    var panelsBeforeFullScreen: Panels?
    var panelsBeforeHideAll: Panels?

    // Editing session (see Editing.swift)
    var originalBase: CGImage?
    var base: CGImage? {
        didSet {
            if base !== oldValue {
                textLines = nil; textLinesFor = nil
                if animationTimer == nil { canvas.liveText.analyze(base) }
            }
        }
    }
    var adjustments = Adjustments()
    var imageDescription: String?
    var originalDescription: String?
    var imageDPI: CGFloat = 72
    var originalDPI: CGFloat = 72
    var autoFilters: [CIFilter]?
    var displayed: CGImage?
    var confirmedOverwrite = false
    var showingOriginal = false
    var renderWork: DispatchWorkItem?

    // Markup strip (see Markup bar in Editing.swift)
    lazy var markupBar = MarkupBar(owner: self)

    // Live Text
    var textLines: [PixelOps.TextLine]?
    var textLinesFor: CGImage?
    var searchIndex = -1

    var url: URL? { urls.indices.contains(index) ? urls[index] : nil }

    init(urls: [URL], start: URL?) {
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1100, height: 760),
                              styleMask: [.titled, .closable, .miniaturizable, .resizable],
                              backing: .buffered, defer: false)
        window.minSize = NSSize(width: 640, height: 420)
        window.toolbarStyle = .unified
        window.collectionBehavior.insert(.fullScreenPrimary)
        super.init(window: window)
        window.delegate = self
        shouldCascadeWindows = true
        setupContent()
        window.toolbar = toolbarImpl.toolbar
        window.addTitlebarAccessoryViewController(markupBar.strip)
        markupBar.strip.isShown = false
        window.setFrameAutosaveName("IMG42.window")
        if window.frame.width < 700 { window.setContentSize(NSSize(width: 1100, height: 760)); window.center() }

        setSet(urls, current: start ?? urls.first)
        let saved = Self.savedPanels
        DispatchQueue.main.async { [weak self] in self?.setPanels(saved, remember: false) }
    }

    required init?(coder: NSCoder) { fatalError("not supported") }

    deinit {
        animationTimer?.invalidate()
        slideshowTimer?.invalidate()
    }

    // MARK: - Layout

    private func setupContent() {
        let main = NSViewController()
        let root = NSView()
        canvas.backgroundColor = background.color
        canvas.drawsBackground = true
        canvas.onNavigate = { [weak self] step in self?.step(step) }
        canvas.onZoomChanged = { [weak self] in self?.updateSubtitle() }
        let markup = canvas.markup
        markup.baseImage = { [weak self] in self?.displayed ?? self?.base }
        markup.onCommit = { [weak self] before, name in self?.marksCommitted(before: before, name: name) }
        markup.onSelectionChange = { [weak self] in self?.markupBar.selectionChanged() }
        markup.onTextEditingChange = { [weak self] in self?.updateSubtitle() }
        markup.onPixelSelectionChange = { [weak self] in
            guard let self else { return }
            if self.canvas.markup.pixelSelection != nil, self.canvas.liveText.overlay.hasActiveTextSelection { self.canvas.liveText.overlay.resetSelection() }
            self.window?.toolbar?.validateVisibleItems()
        }
        markup.onKey = { [weak self] event in self?.handleKey(event) ?? false }
        markup.onDoubleClick = { [weak self] in
            guard let self else { return }
            let p = self.canvas.document.convert(NSApp.currentEvent?.locationInWindow ?? .zero, from: nil)
            self.canvas.toggleFitActual(at: p)
        }
        markup.onCropRequest = { [weak self] in self?.cropToSelectionAction(nil) }
        markup.onMouseDown = { [weak self] in
            self?.editGeneration = UUID()
            self?.stopAnimation()
            if self?.canvas.liveText.overlay.hasActiveTextSelection == true { self?.canvas.liveText.overlay.resetSelection() }
        }
        canvas.liveText.allowsInteraction = { [weak markup] p in markup?.allowsLiveText(at: p) ?? false }

        filmstrip.onSelect = { [weak self] i in self?.go(to: i) }
        filmstrip.isHidden = true

        messageLabel.alignment = .center
        messageLabel.font = .systemFont(ofSize: 15, weight: .medium)
        messageLabel.textColor = .secondaryLabelColor
        messageIcon.image = Theme42.symbol("photo.badge.exclamationmark", size: 44, weight: .light)
        messageIcon.contentTintColor = .tertiaryLabelColor
        let message = NSStackView(views: [messageIcon, messageLabel])
        message.orientation = .vertical
        message.spacing = 12
        message.isHidden = true
        message.identifier = NSUserInterfaceItemIdentifier("message")

        let stack = NSStackView(views: [banner, canvas, filmstrip])
        stack.orientation = .vertical
        stack.spacing = 0
        stack.distribution = .fill
        for v in [banner, canvas, filmstrip] { v.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true }
        banner.heightAnchor.constraint(equalToConstant: Theme42.bannerHeight).isActive = true
        canvas.setContentHuggingPriority(.defaultLow, for: .vertical)
        canvas.setContentCompressionResistancePriority(.defaultLow, for: .vertical)
        for v in [stack, message] {
            v.translatesAutoresizingMaskIntoConstraints = false
            root.addSubview(v)
        }
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: root.leadingAnchor), stack.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            stack.topAnchor.constraint(equalTo: root.topAnchor), stack.bottomAnchor.constraint(equalTo: root.bottomAnchor),
            message.centerXAnchor.constraint(equalTo: canvas.centerXAnchor), message.centerYAnchor.constraint(equalTo: canvas.centerYAnchor),
            message.widthAnchor.constraint(lessThanOrEqualTo: canvas.widthAnchor, constant: -64),
            messageLabel.widthAnchor.constraint(lessThanOrEqualToConstant: 460),
            root.widthAnchor.constraint(greaterThanOrEqualToConstant: 400), root.heightAnchor.constraint(greaterThanOrEqualToConstant: 300),
        ])
        main.view = root

        adjustPanel.onBeginChange = { [weak self] in self?.beginAdjusting() }
        adjustPanel.onChange = { [weak self] a, final in self?.adjustmentsChanged(a, final: final) }
        adjustPanel.onShowOriginal = { [weak self] on in self?.showOriginal(on) }

        inspectorContainer.view = NSView()
        let mainItem = NSSplitViewItem(viewController: main)
        mainItem.minimumThickness = 360
        inspectorItem = NSSplitViewItem(inspectorWithViewController: inspectorContainer)
        inspectorItem.minimumThickness = 260
        inspectorItem.maximumThickness = 420
        inspectorItem.canCollapse = true
        inspectorItem.isCollapsed = true
        splitController.splitViewItems = [mainItem, inspectorItem]
        contentViewController = splitController
    }

    var messageView: NSView? { canvas.superview?.superview?.subviews.first { $0.identifier?.rawValue == "message" } }

    // MARK: - Image set

    func setSet(_ newURLs: [URL], current: URL?) {
        var seen = Set<URL>()
        urls = sortOrder.sort(newURLs.filter { seen.insert($0.standardizedFileURL).inserted })
        filmstrip.urls = urls
        let start = current.flatMap { c in urls.firstIndex { $0.standardizedFileURL == c.standardizedFileURL } } ?? 0
        index = start
        cache.removeAll()
        cacheGeneration = UUID()
        show(index: start)
    }

    /// The window's set changed on disk or by sorting: keep the current image.
    func resort() {
        let current = url
        urls = sortOrder.sort(urls)
        filmstrip.urls = urls
        if let current, let i = urls.firstIndex(of: current) { index = i }
        filmstrip.current = index
        updateSubtitle()
    }

    func step(_ delta: Int) {
        guard !urls.isEmpty else { return }
        var i = index + delta
        if wraps { i = (i % urls.count + urls.count) % urls.count } else { i = max(0, min(urls.count - 1, i)) }
        if i != index { go(to: i) } else if !wraps { NSSound.beep() }
    }

    func go(to i: Int) {
        guard urls.indices.contains(i), i != index || loaded == nil else {
            filmstrip.current = index
            return
        }
        let destination = urls[i]
        confirmLeaving { [weak self] proceed in
            guard let self else { return }
            if proceed, let target = self.urls.firstIndex(of: destination) { self.show(index: target) }
            else { self.filmstrip.current = self.index }
        }
    }

    func show(index i: Int, page newPage: Int = 0) {
        guard urls.indices.contains(i) else { showEmpty(); return }
        index = i
        page = newPage
        let url = urls[i]
        loadToken += 1
        let token = loadToken
        imageSession = UUID(); searchGeneration = UUID()
        stopAnimation(); renderWork?.cancel(); renderWork = nil
        window?.title = url.lastPathComponent
        window?.representedURL = url
        filmstrip.current = i
        if newPage == 0, let hit = cache[url] {
            display(hit)
        } else {
            // The selected URL must never coexist with editable pixels from the previous image.
            loaded = nil
            resetEditing(base: nil)
            canvas.setImage(nil, size: .zero, resetZoom: true)
            canvas.markup.searchBoxes = []; searchIndex = -1
            banner.hide(); window?.isDocumentEdited = false
            updateControlAvailability()
            DispatchQueue.global(qos: .userInitiated).async { [weak self] in
                let result = ImageLoader.load(url, page: newPage)
                DispatchQueue.main.async {
                    guard let self, token == self.loadToken else { return }
                    if newPage == 0 { self.cache[url] = result }
                    self.display(result)
                }
            }
        }
        NSDocumentController.shared.noteNewRecentDocumentURL(url)
    }

    private func showEmpty() {
        loadToken += 1; imageSession = UUID(); searchGeneration = UUID(); cacheGeneration = UUID()
        stopAnimation(); renderWork?.cancel(); renderWork = nil
        loaded = nil
        resetEditing(base: nil)
        canvas.setImage(nil, size: .zero, resetZoom: true)
        window?.title = "IMG42"
        window?.subtitle = ""
        window?.isDocumentEdited = false
        updateControlAvailability()
        showMessage(L("No images"))
    }

    func display(_ result: LoadedImage) {
        imageSession = UUID(); searchGeneration = UUID()
        stopAnimation()
        loaded = result
        textLines = nil
        textLinesFor = nil; searchIndex = -1
        canvas.markup.searchBoxes = []
        banner.hide()
        if let error = result.error {
            resetEditing(base: nil)
            canvas.setImage(nil, size: .zero, resetZoom: true)
            canvas.document.showsCheckerboard = false
            showMessage(error)
        } else if let image = result.image {
            hideMessage()
            canvas.document.showsCheckerboard = result.hasAlpha || image.alphaInfo == .premultipliedLast || image.alphaInfo == .last
            resetEditing(base: image)
            canvas.setImage(image, size: CGSize(width: image.width, height: image.height), resetZoom: true)
            if result.isAnimated, result.pages.count > 1 { startAnimation() }
            if result.wasDownsampled { banner.show(.info, L("This image is displayed at reduced resolution. Save creates a copy.")) }
        }
        if !inspectorItem.isCollapsed, inspectorContainer.children.first === infoPanel { infoPanel.show(url: result.url, loaded: result) }
        updateSubtitle()
        preloadNeighbours()
        updateControlAvailability()
        markupBar.selectionChanged()
        if let q = toolbarImpl.searchField?.stringValue, !q.isEmpty { runSearch(q) }
    }

    private func preloadNeighbours() {
        guard urls.count > 1 else { return }
        let wanted = Set([index - 1, index + 1].map { (($0 % urls.count) + urls.count) % urls.count }.map { urls[$0] } + [urls[index]])
        cache = cache.filter { wanted.contains($0.key) }
        for u in wanted where cache[u] == nil {
            let generation = cacheGeneration
            DispatchQueue.global(qos: .utility).async { [weak self] in
                let result = ImageLoader.load(u)
                DispatchQueue.main.async {
                    guard let self, generation == self.cacheGeneration, self.urls.contains(u) else { return }
                    self.cache[u] = result
                }
            }
        }
    }

    func invalidateCache(_ url: URL) {
        cacheGeneration = UUID()
        cache[url] = nil
        filmstrip.reloadItem(url)
    }

    private func showMessage(_ text: String) {
        messageLabel.stringValue = text
        messageView?.isHidden = false
    }

    private func hideMessage() { messageView?.isHidden = true }

    func removeFromSet(_ url: URL) {
        guard let i = urls.firstIndex(of: url) else { return }
        let current = self.url
        urls.remove(at: i)
        filmstrip.urls = urls
        cacheGeneration = UUID()
        cache[url] = nil
        if urls.isEmpty { showEmpty(); return }
        if current != url, let current, let currentIndex = urls.firstIndex(of: current) {
            index = currentIndex
            filmstrip.current = index
            updateSubtitle()
            return
        }
        show(index: min(i, urls.count - 1))
    }

    func setURLsKeepingCurrent(_ list: [URL], current: URL) {
        urls = list
        filmstrip.urls = urls
        index = urls.firstIndex(of: current) ?? 0
        filmstrip.current = index
        updateSubtitle()
    }

    /// Save As changes the editing destination and the visible decoded baseline together.
    func adoptSavedURL(_ destination: URL, loaded result: LoadedImage) {
        loadToken += 1; cacheGeneration = UUID(); cache.removeAll()
        let destination = destination.standardizedFileURL
        if !urls.contains(where: { $0.standardizedFileURL == destination }) { urls.append(destination) }
        urls = sortOrder.sort(urls)
        index = urls.firstIndex(where: { $0.standardizedFileURL == destination }) ?? 0
        page = 0
        filmstrip.urls = urls; filmstrip.current = index; filmstrip.reloadItem(destination)
        window?.title = destination.lastPathComponent; window?.representedURL = destination
        NSDocumentController.shared.noteNewRecentDocumentURL(destination)
        cache[destination] = result
        display(result)
    }

    func insertIntoSet(_ url: URL) {
        urls.append(url)
        urls = sortOrder.sort(urls)
        filmstrip.urls = urls
        if let i = urls.firstIndex(of: url) { show(index: i) }
    }

    // MARK: - Animation

    private func startAnimation() {
        guard let l = loaded, l.pages.count > 1 else { return }
        frameIndex = 0
        canvas.liveText.analyze(nil)
        scheduleFrame(after: l.frameDelays.first ?? 0.1)
    }

    private func scheduleFrame(after delay: Double) {
        animationTimer = Timer.scheduledTimer(withTimeInterval: delay, repeats: false) { [weak self] _ in
            guard let self, let l = self.loaded, l.pages.count > 1, !self.isDirty else { return }
            self.advanceAnimationFrame()
            self.scheduleFrame(after: l.frameDelays.indices.contains(self.frameIndex) ? l.frameDelays[self.frameIndex] : 0.1)
        }
    }

    func advanceAnimationFrame() {
        guard let l = loaded, l.isAnimated, l.pages.count > 1, !isDirty else { return }
        frameIndex = (frameIndex + 1) % l.pages.count
        let frame = l.pages[frameIndex]
        originalBase = frame; base = frame; displayed = frame
        canvas.setImage(frame, size: CGSize(width: frame.width, height: frame.height), resetZoom: false)
    }

    func stopAnimation() {
        let wasPlaying = animationTimer != nil
        animationTimer?.invalidate()
        animationTimer = nil
        if wasPlaying { canvas.liveText.analyze(base) }
    }

    func freezeDisplayedAnimationFrame() { stopAnimation() }

    // MARK: - Subtitle

    func updateSubtitle() {
        guard let url, let l = loaded else { return }
        var parts: [String] = []
        if urls.count > 1 { parts.append("\(index + 1) / \(urls.count)") }
        if let image = base ?? l.image { parts.append("\(image.width) × \(image.height)") }
        if l.pageCount > 1 { parts.append(L("Page %d of %d", page + 1, l.pageCount)) }
        if l.isAnimated { parts.append(L("%d frames", l.pages.count)) }
        if l.error == nil { parts.append("\(Int((canvas.magnification * 100).rounded())) %") }
        if isDirty { parts.append(L("Edited")) }
        if slideshowTimer != nil { parts.append(slideshowPaused ? L("Slideshow paused") : L("Slideshow")) }
        window?.subtitle = parts.joined(separator: " · ")
        window?.isDocumentEdited = isDirty
        _ = url
    }

    // MARK: - Keys

    /// Keys that are not menu shortcuts: Space, ⇧Space, Home, End, Page Up/Down, Escape.
    func handleKey(_ event: NSEvent) -> Bool {
        let mods = event.modifierFlags.intersection([.command, .option, .control, .shift])
        switch event.keyCode {
        case 49: // space
            if slideshowTimer != nil, mods.isEmpty { slideshowPaused.toggle(); updateSubtitle(); return true }
            step(mods.contains(.shift) ? -1 : 1)
            return true
        case 115: go(to: 0); return true                           // home
        case 119: go(to: urls.count - 1); return true              // end
        case 116: step(-1); return true                            // page up
        case 121: step(1); return true                             // page down
        case 123 where mods.isEmpty: step(-1); return true         // ← (when the menu item is disabled)
        case 124 where mods.isEmpty: step(1); return true          // →
        case 51 where canvas.markup.pixelSelection != nil:         // ⌫ deletes the selected pixels
            deleteSelectionAction(nil)
            return true
        case 3 where mods.isEmpty:                                  // F: full screen
            window?.toggleFullScreen(nil)
            return true
        case 53 where canvas.liveText.overlay.hasActiveTextSelection:  // escape clears the text selection first
            canvas.liveText.overlay.resetSelection()
            return true
        case 53:                                                    // escape
            if slideshowTimer != nil { stopSlideshow(); return true }
            if window?.styleMask.contains(.fullScreen) == true { window?.toggleFullScreen(nil); return true }
            return false
        default:
            return false
        }
    }

    // MARK: - Navigation actions

    @objc func previousAction(_ sender: Any?) { step(-1) }
    @objc func nextAction(_ sender: Any?) { step(1) }
    @objc func firstAction(_ sender: Any?) { go(to: 0) }
    @objc func lastAction(_ sender: Any?) { go(to: urls.count - 1) }

    @objc func nextPageAction(_ sender: Any?) { changePage(1) }
    @objc func previousPageAction(_ sender: Any?) { changePage(-1) }

    private func changePage(_ d: Int) {
        guard let l = loaded, l.pageCount > 1 else { return }
        let p = page + d
        guard p >= 0, p < l.pageCount else { NSSound.beep(); return }
        let sourceURL = l.url
        confirmLeaving { [weak self] ok in
            guard ok, let self, let target = self.urls.firstIndex(of: sourceURL) else { return }
            self.show(index: target, page: p)
        }
    }

    // MARK: - Zoom

    @objc func zoomInAction(_ sender: Any?) { canvas.zoomBy(1.25) }
    @objc func zoomOutAction(_ sender: Any?) { canvas.zoomBy(0.8) }
    @objc func actualSizeAction(_ sender: Any?) { canvas.zoom(to: 1) }
    @objc func zoomToFitAction(_ sender: Any?) { canvas.zoomToFit() }

    // MARK: - Panels

    @objc func toggleInfoAction(_ sender: Any?) { toggleInspector(infoPanel) }
    @objc func toggleAdjustAction(_ sender: Any?) { toggleInspector(adjustPanel) }

    func toggleInspector(_ panel: NSViewController, forceShow: Bool = false, remember: Bool = true) {
        let showing = !inspectorItem.isCollapsed && inspectorContainer.children.first === panel
        if showing && !forceShow {
            inspectorItem.animator().isCollapsed = true
            if remember { DispatchQueue.main.async { self.rememberPanels() } }
            return
        }
        if inspectorContainer.children.first !== panel {
            inspectorContainer.children.forEach { $0.view.removeFromSuperview(); $0.removeFromParent() }
            inspectorContainer.addChild(panel)
            panel.view.translatesAutoresizingMaskIntoConstraints = false
            inspectorContainer.view.addSubview(panel.view)
            NSLayoutConstraint.activate([
                panel.view.leadingAnchor.constraint(equalTo: inspectorContainer.view.leadingAnchor),
                panel.view.trailingAnchor.constraint(equalTo: inspectorContainer.view.trailingAnchor),
                panel.view.topAnchor.constraint(equalTo: inspectorContainer.view.topAnchor),
                panel.view.bottomAnchor.constraint(equalTo: inspectorContainer.view.bottomAnchor),
            ])
        }
        if panel === infoPanel { infoPanel.show(url: url, loaded: loaded) }
        if panel === adjustPanel { adjustPanel.set(adjustments) }
        if inspectorItem.isCollapsed { inspectorItem.isCollapsed = false }
        if remember { rememberPanels() }
    }

    var inspectorShows: NSViewController? { inspectorItem.isCollapsed ? nil : inspectorContainer.children.first }

    @objc func toggleFilmstripAction(_ sender: Any?) {
        filmstrip.isHidden.toggle()
        if !filmstrip.isHidden { DispatchQueue.main.async { self.filmstrip.current = self.index } }
        rememberPanels()
    }

    // MARK: - View options

    @objc func sortAction(_ sender: Any?) {
        sortOrder = SortOrder(rawValue: commandTag(of: sender)) ?? .name
        Self.defaults.set(sortOrder.rawValue, forKey: "sortOrder")
        resort()
    }

    @objc func backgroundAction(_ sender: Any?) {
        background = Background(rawValue: commandTag(of: sender)) ?? .neutral
        Self.defaults.set(background.rawValue, forKey: "background")
        canvas.backgroundColor = background.color
    }

    @objc func toggleWrapAction(_ sender: Any?) {
        wraps.toggle()
        Self.defaults.set(wraps, forKey: "wraps")
    }

    // MARK: - Slideshow

    @objc func slideshowAction(_ sender: Any?) {
        if slideshowTimer != nil { stopSlideshow(); return }
        startSlideshow()
    }

    func startSlideshow(enterFullScreen: Bool = true) {
        guard slideshowTimer == nil, !urls.isEmpty else { return }
        slideshowPaused = false
        slideshowEnteredFullScreen = enterFullScreen && window?.styleMask.contains(.fullScreen) == false
        if slideshowEnteredFullScreen { window?.toggleFullScreen(nil) }
        canvas.backgroundColor = .black
        slideshowTimer = Timer.scheduledTimer(withTimeInterval: 3, repeats: true) { [weak self] _ in
            guard let self, !self.slideshowPaused else { return }
            if !self.wraps && self.index == self.urls.count - 1 { self.stopSlideshow(); return }
            if self.isDirty { return }
            self.step(1)
        }
        updateSubtitle()
    }

    func stopSlideshow() {
        slideshowTimer?.invalidate()
        slideshowTimer = nil
        canvas.backgroundColor = window?.styleMask.contains(.fullScreen) == true ? .black : background.color
        if slideshowEnteredFullScreen, window?.styleMask.contains(.fullScreen) == true { window?.toggleFullScreen(nil) }
        slideshowEnteredFullScreen = false
        updateSubtitle()
    }

    // MARK: - Window

    func windowWillReturnUndoManager(_ window: NSWindow) -> UndoManager? { undo }

    func windowShouldClose(_ sender: NSWindow) -> Bool {
        guard isDirty else { return true }
        confirmLeaving { ok in if ok { sender.close() } }
        return false
    }

    func windowWillClose(_ notification: Notification) {
        loadToken += 1; imageSession = UUID(); searchGeneration = UUID(); cacheGeneration = UUID()
        renderWork?.cancel(); renderWork = nil
        rememberPanels()
        stopAnimation()
        slideshowTimer?.invalidate()
        AppDelegate.shared?.windowClosed(self)
    }

    func windowDidResize(_ notification: Notification) {
        if canvas.zoomMode == .fit { canvas.applyZoom() }
    }

    // MARK: - Full screen & panels

    func window(_ window: NSWindow, willUseFullScreenPresentationOptions proposedOptions: NSApplication.PresentationOptions = []) -> NSApplication.PresentationOptions {
        [.fullScreen, .autoHideToolbar, .autoHideMenuBar, .autoHideDock]
    }

    /// Full screen shows only the image on black; the toolbar slides in at the top edge.
    func windowWillEnterFullScreen(_ notification: Notification) {
        panelsBeforeFullScreen = visiblePanels
        setPanels([], remember: false)
        canvas.backgroundColor = .black
    }

    func windowDidEnterFullScreen(_ notification: Notification) {
        canvas.zoomToFit()
        window?.makeFirstResponder(canvas.markup)
    }

    func windowWillExitFullScreen(_ notification: Notification) {
        slideshowEnteredFullScreen = false
        if slideshowTimer != nil {
            slideshowTimer?.invalidate()
            slideshowTimer = nil
        }
    }

    func windowDidExitFullScreen(_ notification: Notification) {
        canvas.backgroundColor = background.color
        setPanels(panelsBeforeFullScreen ?? Self.savedPanels, remember: false)
        panelsBeforeFullScreen = nil
        updateSubtitle()
    }

    struct Panels: OptionSet {
        let rawValue: Int
        static let markup = Panels(rawValue: 1)
        static let filmstrip = Panels(rawValue: 2)
        static let info = Panels(rawValue: 4)
        static let adjust = Panels(rawValue: 8)
        static let toolbar = Panels(rawValue: 16)
    }

    var visiblePanels: Panels {
        var p: Panels = []
        if markupBar.strip.isShown { p.insert(.markup) }
        if !filmstrip.isHidden { p.insert(.filmstrip) }
        if inspectorShows === infoPanel { p.insert(.info) }
        if inspectorShows === adjustPanel { p.insert(.adjust) }
        if window?.toolbar?.isVisible == true { p.insert(.toolbar) }
        return p
    }

    static var savedPanels: Panels {
        Panels(rawValue: defaults.object(forKey: "panels") as? Int ?? Panels.toolbar.rawValue)
    }

    func rememberPanels() {
        guard window?.styleMask.contains(.fullScreen) != true else { return }
        Self.defaults.set(visiblePanels.rawValue, forKey: "panels")
    }

    func setPanels(_ p: Panels, remember: Bool = true) {
        if markupBar.strip.isShown != p.contains(.markup) { markupBar.show(p.contains(.markup)) }
        filmstrip.isHidden = !p.contains(.filmstrip)
        if p.contains(.info) { toggleInspector(infoPanel, forceShow: true, remember: false) }
        else if p.contains(.adjust) { toggleInspector(adjustPanel, forceShow: true, remember: false) }
        else if !inspectorItem.isCollapsed { inspectorItem.isCollapsed = true }
        if window?.styleMask.contains(.fullScreen) != true { window?.toolbar?.isVisible = p.contains(.toolbar) }
        if !filmstrip.isHidden { DispatchQueue.main.async { self.filmstrip.current = self.index } }
        if remember { rememberPanels() }
    }

    /// Hides every bar and panel so only the image is left; again brings them back.
    @objc func toggleAllPanelsAction(_ sender: Any?) {
        let current = visiblePanels
        if current.isEmpty || current == [] {
            setPanels(panelsBeforeHideAll ?? [.toolbar])
            panelsBeforeHideAll = nil
        } else {
            panelsBeforeHideAll = current
            setPanels([])
        }
    }

    // MARK: - Validation

    func validateMenuItem(_ menuItem: NSMenuItem) -> Bool {
        let ok = validate(menuItem.action, tag: menuItem.tag)
        switch menuItem.action {
        case #selector(toggleInfoAction(_:)): menuItem.state = inspectorShows === infoPanel ? .on : .off
        case #selector(toggleFilmstripAction(_:)): menuItem.state = filmstrip.isHidden ? .off : .on
        case #selector(toggleMarkupAction(_:)): menuItem.state = markupBar.strip.isShown ? .on : .off
        case #selector(toggleAllPanelsAction(_:)): menuItem.state = visiblePanels.isEmpty ? .on : .off
        case #selector(toggleAdjustAction(_:)):
            menuItem.state = inspectorShows === adjustPanel ? .on : .off
        case #selector(sortAction(_:)): menuItem.state = menuItem.tag == sortOrder.rawValue ? .on : .off
        case #selector(backgroundAction(_:)): menuItem.state = menuItem.tag == background.rawValue ? .on : .off
        case #selector(toggleWrapAction(_:)): menuItem.state = wraps ? .on : .off
        case #selector(slideshowAction(_:)): menuItem.title = slideshowTimer == nil ? L("Slideshow") : L("Stop Slideshow")
        case #selector(lookAction(_:)): menuItem.state = adjustments.look.rawValue == menuItem.tag ? .on : .off
        case #selector(autoEnhanceAction(_:)): menuItem.state = adjustments.autoEnhance ? .on : .off
        case #selector(selectToolAction(_:)): menuItem.state = markupBar.strip.isShown && canvas.markup.tool.rawValue == menuItem.tag ? .on : .off
        default: break
        }
        return ok
    }

    func validateToolbarItem(_ item: NSToolbarItem) -> Bool { validate(item.action, tag: item.tag) }

    func updateControlAvailability() {
        let enabled = base != nil
        toolbarImpl.highlight.isEnabled = enabled
        toolbarImpl.share.isEnabled = enabled
        toolbarImpl.searchField?.isEnabled = enabled
        window?.toolbar?.validateVisibleItems()
    }


    func validate(_ action: Selector?, tag: Int) -> Bool {
        let hasImage = loaded?.image != nil
        let editingText = canvas.markup.isEditingText || window?.firstResponder is NSText && !(window?.firstResponder === canvas.markup)
        switch action {
        case #selector(previousAction(_:)), #selector(nextAction(_:)):
            // Plain arrow keys belong to text fields while one is being edited.
            return !editingText && urls.count > 1
        case #selector(firstAction(_:)), #selector(lastAction(_:)): return urls.count > 1
        case #selector(nextPageAction(_:)): return !editingText && (loaded?.pageCount ?? 1) > page + 1
        case #selector(previousPageAction(_:)): return !editingText && page > 0
        case #selector(zoomInAction(_:)), #selector(zoomOutAction(_:)), #selector(actualSizeAction(_:)), #selector(zoomToFitAction(_:)),
             #selector(rotateLeftAction(_:)), #selector(rotateRightAction(_:)), #selector(flipHorizontalAction(_:)), #selector(flipVerticalAction(_:)),
             #selector(toggleAdjustAction(_:)), #selector(resizeAction(_:)), #selector(autoEnhanceAction(_:)), #selector(lookAction(_:)),
             #selector(exportAction(_:)), #selector(printDocument(_:)), #selector(runPageLayout(_:)), #selector(removeBackgroundAction(_:)),
             #selector(imageDescriptionAction(_:)), #selector(copyTextAction(_:)), #selector(selectToolAction(_:)),
             #selector(showStampsAction(_:)), #selector(toggleMarkupAction(_:)), #selector(highlightAction(_:)), #selector(saveDocumentAs(_:)):
            return hasImage
        case #selector(copy(_:)): return hasImage && !editingText
        case #selector(selectAll(_:)): return hasImage && !editingText
        case #selector(delete(_:)): return canvas.markup.selectedID != nil || canvas.markup.pixelSelection != nil
        case #selector(cropToSelectionAction(_:)), #selector(deleteSelectionAction(_:)), #selector(invertSelectionAction(_:)), #selector(deselectAction(_:)):
            return canvas.markup.pixelSelection != nil
        case #selector(saveDocument(_:)), #selector(revertDocumentToSaved(_:)): return isDirty
        case #selector(trashAction(_:)), #selector(showInFinderAction(_:)): return url != nil && !editingText
        case #selector(slideshowAction(_:)): return urls.count > 1 || slideshowTimer != nil
        case #selector(toggleFilmstripAction(_:)), #selector(toggleInfoAction(_:)), #selector(sortAction(_:)), #selector(toggleAllPanelsAction(_:)),
             #selector(backgroundAction(_:)), #selector(toggleWrapAction(_:)):
            return true
        case #selector(NSDocument.duplicate(_:)): return false
        default: return true
        }
    }
}

extension ImageWindowController: PanelHiding {}
