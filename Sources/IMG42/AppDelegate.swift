import AppKit
import Kit42
import UniformTypeIdentifiers

/// Routes File › Open, Open Recent and the welcome window's recent list to image windows (no NSDocument).
final class ImageDocumentController: NSDocumentController {
    override func openDocument(_ sender: Any?) { AppDelegate.shared?.runOpenPanel() }

    override func openDocument(withContentsOf url: URL, display displayDocument: Bool,
                               completionHandler: @escaping (NSDocument?, Bool, Error?) -> Void) {
        AppDelegate.shared?.open([url])
        completionHandler(nil, false, nil)
    }

    override func newDocument(_ sender: Any?) {}

    override func validateUserInterfaceItem(_ item: NSValidatedUserInterfaceItem) -> Bool {
        if item.action == #selector(openDocument(_:)) { return true }
        if item.action == #selector(newDocument(_:)) { return false }
        return super.validateUserInterfaceItem(item)
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate, NSMenuDelegate {
    static weak var shared: AppDelegate?
    private(set) var windows: [ImageWindowController] = []

    private lazy var welcome = WelcomeWindowController(
        app: .img, tagline: L("Images: view, flip through, mark up and adjust"),
        actions: [
            .init(L("Open Image…"), symbol: "photo", action: #selector(NSDocumentController.openDocument(_:))),
            .init(L("Open Folder…"), symbol: "folder", action: #selector(AppDelegate.openFolderAction(_:))),
        ])

    override init() {
        super.init()
        AppDelegate.shared = self
    }

    func applicationWillFinishLaunching(_ notification: Notification) {
        // K42_APPEARANCE=dark|light forces the appearance (dark mode checks with K42_SNAPSHOT).
        switch ProcessInfo.processInfo.environment["K42_APPEARANCE"] {
        case "dark": NSApp.appearance = NSAppearance(named: .darkAqua)
        case "light": NSApp.appearance = NSAppearance(named: .aqua)
        default: break
        }
        NSApp.mainMenu = MenuBar.build(Cmd.menuBar(openWithDelegate: self))
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        if let index = CommandLine.arguments.firstIndex(of: "--audit"), CommandLine.arguments.indices.contains(index + 1) {
            Task { @MainActor in await ImageNativeAudit.run(CommandLine.arguments[index + 1]) }
            return
        }
        if CommandLine.arguments.contains("--uitest") {
            Task { @MainActor in await UITest.run() }
            return
        }
        NSApp.activate()
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) { [self] in
            if windows.isEmpty {
                // Started from the command line with paths (`IMG42 photo.jpg`).
                let paths = CommandLine.arguments.dropFirst().filter { !$0.hasPrefix("-") && FileManager.default.fileExists(atPath: $0) }
                if !paths.isEmpty { open(paths.map { URL(fileURLWithPath: $0) }) } else { welcome.showWindow(nil) }
            }
        }
        DebugSnapshot.scheduleIfRequested { [weak self] in self?.prepareSnapshot() }
        // Runs before the snapshot (K42_SNAPSHOT_DELAY ≥ 3): the selection is computed in the background.
        if ProcessInfo.processInfo.environment["K42_SMART_LASSO"] != nil {
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.2) { [weak self] in
                if let w = self?.frontController { self?.smartLassoHook(w, ProcessInfo.processInfo.environment) }
            }
        }
    }

    func applicationShouldOpenUntitledFile(_ sender: NSApplication) -> Bool { false }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        if !flag { welcome.showWindow(nil) }
        return true
    }

    func application(_ application: NSApplication, open urls: [URL]) { open(urls) }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        let dirty = windows.filter(\.isDirty)
        // Debug snapshots quit on their own.
        guard !dirty.isEmpty, ProcessInfo.processInfo.environment["K42_SNAPSHOT"] == nil else { return .terminateNow }
        let alert = NSAlert()
        alert.messageText = L("Quit without saving your changes?")
        alert.informativeText = L("%d edited images have not been saved.", dirty.count)
        alert.addButton(withTitle: L("Quit"))
        alert.addButton(withTitle: L("Cancel"))
        return alert.runModal() == .alertFirstButtonReturn ? .terminateNow : .terminateCancel
    }

    // MARK: - Opening

    @objc func showWelcomeAction(_ sender: Any?) { welcome.showWindow(nil) }

    func runOpenPanel(foldersOnly: Bool = false) {
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = !foldersOnly
        panel.canChooseDirectories = true
        panel.canChooseFiles = !foldersOnly
        if !foldersOnly { panel.allowedContentTypes = ImageFormats.readableTypes + [.folder] }
        panel.message = foldersOnly ? L("Choose a folder with images") : L("Choose images or a folder")
        guard panel.runModal() == .OK else { return }
        open(panel.urls)
    }

    @objc func openFolderAction(_ sender: Any?) { runOpenPanel(foldersOnly: true) }

    /// One file → its whole folder; one folder → its images; several items → exactly those (folders expanded).
    func open(_ items: [URL]) {
        guard !items.isEmpty else { return }
        var set: [URL] = []
        var start: URL?
        let isDir: (URL) -> Bool = { (try? $0.resourceValues(forKeys: [.isDirectoryKey]).isDirectory) == true }
        if items.count == 1, let only = items.first {
            if isDir(only) {
                set = ImageFormats.images(in: only)
            } else {
                set = ImageFormats.images(in: only.deletingLastPathComponent())
                if !set.contains(where: { $0.standardizedFileURL == only.standardizedFileURL }) { set.append(only) }
                start = only
            }
        } else {
            for item in items {
                if isDir(item) { set += ImageFormats.images(in: item) } else { set.append(item) }
            }
            start = items.first { !isDir($0) }
        }
        guard !set.isEmpty else {
            let alert = NSAlert()
            alert.messageText = L("No images found")
            alert.informativeText = L("The folder contains no images IMG42 can open.")
            alert.runModal()
            return
        }
        // Already open in a window: bring it to the front at that image.
        if let start, let w = windows.first(where: { $0.url?.standardizedFileURL == start.standardizedFileURL }) {
            w.showWindow(nil)
            return
        }
        let controller = ImageWindowController(urls: set, start: start)
        windows.append(controller)
        controller.showWindow(nil)
        welcome.close()
        for item in items where !isDir(item) { NSDocumentController.shared.noteNewRecentDocumentURL(item) }
    }

    func windowClosed(_ controller: ImageWindowController) {
        windows.removeAll { $0 === controller }
    }

    var frontController: ImageWindowController? {
        NSApp.orderedWindows.compactMap { $0.windowController as? ImageWindowController }.first
    }

    // MARK: - Open With

    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        guard let url = frontController?.url else {
            menu.addItem(withTitle: L("No image"), action: nil, keyEquivalent: "").isEnabled = false
            return
        }
        let own = Bundle.main.bundleURL.standardizedFileURL
        let apps = NSWorkspace.shared.urlsForApplications(toOpen: url).filter { $0.standardizedFileURL != own }
        let fm = FileManager.default
        for app in apps.prefix(24) {
            let item = NSMenuItem(title: fm.displayName(atPath: app.path).replacingOccurrences(of: ".app", with: ""),
                                  action: #selector(openWithApp(_:)), keyEquivalent: "")
            item.target = self
            item.representedObject = app
            let icon = NSWorkspace.shared.icon(forFile: app.path)
            icon.size = NSSize(width: 16, height: 16)
            item.image = icon
            menu.addItem(item)
        }
        if apps.isEmpty { menu.addItem(withTitle: L("No other apps"), action: nil, keyEquivalent: "").isEnabled = false }
        menu.addItem(.separator())
        let other = NSMenuItem(title: L("Other…"), action: #selector(openWithOther(_:)), keyEquivalent: "")
        other.target = self
        menu.addItem(other)
    }

    @objc func openWithApp(_ sender: NSMenuItem) {
        guard let app = sender.representedObject as? URL, let url = frontController?.url else { return }
        NSWorkspace.shared.open([url], withApplicationAt: app, configuration: NSWorkspace.OpenConfiguration())
    }

    @objc func openWithOther(_ sender: Any?) {
        guard let url = frontController?.url else { return }
        let panel = NSOpenPanel()
        panel.directoryURL = URL(fileURLWithPath: "/Applications")
        panel.allowedContentTypes = [.application]
        panel.message = L("Choose an app to open the image")
        guard panel.runModal() == .OK, let app = panel.url else { return }
        NSWorkspace.shared.open([url], withApplicationAt: app, configuration: NSWorkspace.OpenConfiguration())
    }

    // MARK: - Debug hooks (K42_SNAPSHOT)

    /// K42_NEXT=n flips n images, K42_PAGE=n pages, K42_INFO / K42_FILMSTRIP / K42_ADJUST open panels,
    /// K42_MARKUP shows the markup strip, K42_DEMO_MARKS adds sample marks, K42_TOOL=<n> picks a tool,
    /// K42_SELECT=rect|ellipse|alpha makes a sample selection, K42_SEARCH=text runs Live Text search,
    /// K42_ZOOM=x zooms, K42_DUMP_MENUS prints the markup drop-downs, K42_STAMPS opens the signature popover,
    /// K42_RESIZE / K42_DESCRIPTION open those sheets, K42_BACKGROUND=n picks the background.
    private func prepareSnapshot() {
        let env = ProcessInfo.processInfo.environment
        guard let w = frontController else { return }
        if let n = env["K42_NEXT"].flatMap(Int.init) {
            for _ in 0..<abs(n) { w.step(n < 0 ? -1 : 1) }
        }
        if let p = env["K42_PAGE"].flatMap(Int.init) {
            for _ in 0..<p { w.nextPageAction(nil) }
        }
        if let b = env["K42_BACKGROUND"].flatMap(Int.init) { w.backgroundAction(NSMenuItem.tagged(b)) }
        if env["K42_FILMSTRIP"] != nil, w.filmstrip.isHidden { w.toggleFilmstripAction(nil) }
        if env["K42_NOFILMSTRIP"] != nil, !w.filmstrip.isHidden { w.toggleFilmstripAction(nil) }
        // Let the image load before panels and markup that depend on it.
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.4) {
            if env["K42_INFO"] != nil { w.toggleInspector(w.infoPanel, forceShow: true) }
            if env["K42_ADJUST"] != nil {
                var a = Adjustments()
                a[.exposure] = 10; a[.contrast] = 25; a[.saturation] = 30; a[.temperature] = 20; a[.vignette] = 40
                w.toggleInspector(w.adjustPanel, forceShow: true)
                w.applyAdjustments(a, name: "demo")
            }
            if env["K42_MARKUP"] != nil { w.markupBar.show(true) }
            if let t = env["K42_TOOL"].flatMap(Int.init), let tool = MarkTool(rawValue: t) { w.markupBar.setTool(tool) }
            if env["K42_DEMO_MARKS"] != nil, let base = w.base {
                w.markupBar.show(true)
                let before = w.canvas.markup.marks
                w.canvas.markup.marks = Mark.demo(for: CGSize(width: base.width, height: base.height))
                w.marksCommitted(before: before, name: "demo")
                if env["K42_SELECT_MARK"] != nil { w.canvas.markup.selectedID = w.canvas.markup.marks.last?.id }
            }
            if let sel = env["K42_SELECT"], let base = w.base {
                let r = CGRect(x: CGFloat(base.width) * 0.2, y: CGFloat(base.height) * 0.2, width: CGFloat(base.width) * 0.5, height: CGFloat(base.height) * 0.45)
                w.markupBar.show(true)
                switch sel {
                case "ellipse":
                    w.markupBar.setTool(.selectEllipse)
                    w.canvas.markup.pixelSelection = PixelSelection.from(path: CGPath(ellipseIn: r, transform: nil), size: CGSize(width: base.width, height: base.height))
                case "alpha":
                    w.markupBar.setTool(.instantAlpha)
                    w.canvas.markup.pixelSelection = PixelSelection.similarColors(in: base, seed: CGPoint(x: 2, y: 2), tolerance: 0.15)
                default:
                    w.markupBar.setTool(.selectRect)
                    w.canvas.markup.pixelSelection = PixelSelection.from(path: CGPath(rect: r, transform: nil), size: CGSize(width: base.width, height: base.height), isRectangle: true)
                }
                if env["K42_CROP"] != nil { w.cropToSelectionAction(nil) }
                if env["K42_DELETE"] != nil { w.deleteSelectionAction(nil) }
            }
            if let q = env["K42_SEARCH"], let image = w.displayed ?? w.base {
                // Recognise synchronously so the snapshot shows the result.
                w.textLines = PixelOps.recognizeText(image)
                w.textLinesFor = image
                w.toolbarImpl.searchField?.stringValue = q
                w.runSearch(q)
            }
            if let z = env["K42_ZOOM"].flatMap(Double.init) { w.canvas.zoom(to: CGFloat(z)) }
            if env["K42_DUMP_MENUS"] != nil { print(w.markupBar.dumpMenus()) }
            if env["K42_STAMPS"] != nil, let b = w.markupBar.strip.view.firstSubview(withToolTipPrefix: L("Insert a signature or stamp")) {
                w.showStampsAction(b)
            }
            if env["K42_RESIZE"] != nil { w.resizeAction(nil) }
            if env["K42_DESCRIPTION"] != nil { w.imageDescriptionAction(nil) }
            if env["K42_EXPORT"] != nil { w.exportAction(nil) }
            // K42_EXPORT_ACCESSORY: the export panel's options view in a plain window (the save panel itself is
            // out-of-process and blank in snapshots).
            if env["K42_EXPORT_ACCESSORY"] != nil {
                let panel = NSSavePanel()
                let options = ExportOptions(sourceType: .jpeg, hasAlpha: false, panel: panel)
                let win = NSWindow(contentRect: NSRect(x: 200, y: 200, width: 440, height: 140), styleMask: [.titled], backing: .buffered, defer: false)
                win.contentView = options.view
                objc_setAssociatedObject(win, "options", options, .OBJC_ASSOCIATION_RETAIN)
                objc_setAssociatedObject(win, "exportPanel", panel, .OBJC_ASSOCIATION_RETAIN)
                win.makeKeyAndOrderFront(nil)
            }
            if env["K42_UNDO"] != nil { w.undo.undo() }
            if env["K42_SAVE"] != nil { w.save(confirmed: true) { ok in print("saved:", ok) } }
            if env["K42_FULLSCREEN"] != nil { w.window?.toggleFullScreen(nil) }
            if env["K42_HIDE_ALL"] != nil { w.toggleAllPanelsAction(nil) }
            self.liveTextHooks(w, env)
            // K42_DUMP_CANVAS prints the canvas view tree with appearances and layer colours (dark mode checks).
            if env["K42_DUMP_CANVAS"] != nil {
                func dump(_ v: NSView, _ d: Int) {
                    print(String(repeating: "  ", count: d) + "\(type(of: v)) \(v.frame) appearance=\(v.effectiveAppearance.name.rawValue) layerBG=\(String(describing: v.layer?.backgroundColor))")
                    if d < 3 { v.subviews.forEach { dump($0, d + 1) } }
                }
                dump(w.canvas, 0)
            }
        }
    }

    /// K42_SMART_LASSO="x,y;x,y;…" (image pixels, or @file with that text) paints a Smart Lasso stroke with synthesised
    /// mouse events; prints the selection; K42_SMART_CROP then crops to it.
    private func smartLassoHook(_ w: ImageWindowController, _ env: [String: String]) {
        guard var spec = env["K42_SMART_LASSO"] else { return }
        if spec.hasPrefix("@") { spec = (try? String(contentsOfFile: String(spec.dropFirst()), encoding: .utf8)) ?? "" }
        let size = w.canvas.imageSize
        let pts: [CGPoint] = spec.split(separator: ";").compactMap { item -> CGPoint? in
            let v = item.split(separator: ",").compactMap { Double($0) }
            guard v.count == 2 else { return nil }
            return CGPoint(x: min(max(CGFloat(v[0]), 1), size.width - 1), y: min(max(CGFloat(v[1]), 1), size.height - 1))
        }
        setlinebuf(stdout)
        w.markupBar.show(true)
        w.markupBar.setTool(.smartLasso)
        print("smartlasso: \(pts.count) points, tool=\(w.canvas.markup.tool) active=\(w.canvas.markup.markupActive)")
        MouseScript.drag(pts, in: w.canvas.document, substeps: 1)
        func report(_ tries: Int) {
            if let sel = w.canvas.markup.pixelSelection {
                print("smartlasso: selection bounds=\(NSStringFromRect(sel.bounds.integral))")
                if env["K42_SMART_CROP"] != nil {
                    w.cropToSelectionAction(nil)
                    print("smartlasso: cropped to \(w.base?.width ?? 0)x\(w.base?.height ?? 0)")
                }
            } else if tries > 0 {
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) { report(tries - 1) }
            } else { print("smartlasso: no selection") }
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) { report(40) }
    }

    /// K42_LIVETEXT_SELECT="from..to" selects recognised text and copies it; K42_LIVETEXT_DRAG="x1,y1;x2,y2" drags,
    /// K42_LIVETEXT_CLICK="x,y,n" clicks n times (image pixels) with synthesised mouse events; K42_LIVETEXT_ALL presses ⌘A,
    /// K42_LIVETEXT_MENU prints the right-click menu at the selection. Results are printed as `livetext: …`.
    private func liveTextHooks(_ w: ImageWindowController, _ env: [String: String]) {
        let keys = ["K42_LIVETEXT_KEY", "K42_LIVETEXT_PROBE", "K42_LIVETEXT_SELECT", "K42_LIVETEXT_DRAG", "K42_LIVETEXT_CLICK", "K42_LIVETEXT_ALL", "K42_LIVETEXT_MENU", "K42_LIVETEXT_HIGHLIGHT"]
        guard keys.contains(where: { env[$0] != nil }) else { return }
        setlinebuf(stdout)
        if env["K42_LIVETEXT_TRACE"] != nil {
            NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .leftMouseDragged, .leftMouseUp]) { e in
                print("livetext: event \(e.type.rawValue) \(e.locationInWindow) clicks=\(e.clickCount)")
                return e
            }
        }
        let live = w.canvas.liveText
        let started = Date()
        let points: (String) -> [CGPoint] = { s in
            s.split(separator: ";").map { $0.split(separator: ",").compactMap { Double($0) } }.filter { $0.count >= 2 }.map { CGPoint(x: $0[0], y: $0[1]) }
        }
        live.whenAnalysed {
            print(String(format: "livetext: analysed in %.2fs, text=%@", Date().timeIntervalSince(started), live.overlay.text.debugDescription))
            if env["K42_LIVETEXT_TRACE"] != nil {
                func dump(_ v: NSView, _ d: Int) {
                    print("livetext: view " + String(repeating: "  ", count: d) + "\(type(of: v)) \(v.frame) gr=\(v.gestureRecognizers.map { "\(type(of: $0))" })")
                    v.subviews.forEach { dump($0, d + 1) }
                }
                dump(live, 0)
            }
            // K42_LIVETEXT_PROBE="x,y;x,y": what lies under each point (no click).
            for p in points(env["K42_LIVETEXT_PROBE"] ?? "") {
                print("livetext: probe \(Int(p.x)),\(Int(p.y)) text=\(live.overlay.hasText(at: p)) detector=\(live.overlay.hasDataDetector(at: p))")
            }
            if env["K42_LIVETEXT_HIGHLIGHT"] != nil { live.overlay.selectableItemsHighlighted = true }
            if let spec = env["K42_LIVETEXT_SELECT"] {
                let parts = spec.components(separatedBy: "..")
                live.select(from: parts[0], to: parts.count > 1 ? parts[1] : parts[0])
            }
            if let spec = env["K42_LIVETEXT_DRAG"] { MouseScript.drag(points(spec), in: w.canvas.document) }
            if let spec = env["K42_LIVETEXT_CLICK"] {
                let v = spec.split(separator: ",").compactMap { Double($0) }
                if v.count >= 2 { MouseScript.click(CGPoint(x: v[0], y: v[1]), count: v.count > 2 ? Int(v[2]) : 1, in: w.canvas.document) }
            }
            // K42_LIVETEXT_KEY=53 presses Escape (key code) after the mouse script.
            if let code = env["K42_LIVETEXT_KEY"].flatMap(UInt16.init) { MouseScript.key(code, chars: code == 53 ? "\u{1b}" : " ", in: w.canvas.document) }
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) {
                if env["K42_LIVETEXT_ALL"] != nil { w.selectAll(nil) }
                print("livetext: selected=\(live.overlay.selectedText.debugDescription) marks=\(w.canvas.markup.marks.count) tool=\(w.canvas.markup.tool)")
                if live.hasSelection {
                    w.copy(nil)
                    print("livetext: copied=\((NSPasteboard.general.string(forType: .string) ?? "").debugDescription)")
                }
                if env["K42_LIVETEXT_MENU"] != nil, let r = live.overlay.selectedRanges.first {
                    _ = r
                    live.debugMenu = true
                    let p = env["K42_LIVETEXT_MENU"].flatMap { s -> CGPoint? in
                        let v = s.split(separator: ",").compactMap { Double($0) }
                        return v.count == 2 ? CGPoint(x: v[0], y: v[1]) : nil
                    }
                    if let p { MouseScript.rightClick(p, in: w.canvas.document) }
                }
            }
        }
    }
}

extension NSMenuItem {
    static func tagged(_ tag: Int) -> NSMenuItem {
        let i = NSMenuItem()
        i.tag = tag
        return i
    }
}

extension NSView {
    func firstSubview(withToolTipPrefix prefix: String) -> NSView? {
        if toolTip?.hasPrefix(prefix) == true { return self }
        for s in subviews { if let v = s.firstSubview(withToolTipPrefix: prefix) { return v } }
        return nil
    }
}
