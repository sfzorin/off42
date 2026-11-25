import AppKit
import Kit42

/// File › New opens a scan window; File › Open imports into the front window; recent files open in their app.
final class ScanDocumentController: NSDocumentController {
    override func newDocument(_ sender: Any?) { AppDelegate.shared?.newWindow() }
    override func openDocument(_ sender: Any?) {
        let w = AppDelegate.shared?.frontController ?? AppDelegate.shared?.newWindow()
        w?.importAction(sender)
    }

    override func openDocument(withContentsOf url: URL, display displayDocument: Bool,
                               completionHandler: @escaping (NSDocument?, Bool, Error?) -> Void) {
        NSWorkspace.shared.open(url)
        completionHandler(nil, false, nil)
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    static weak var shared: AppDelegate?
    private(set) var windows: [ScanWindowController] = []

    private lazy var welcome = WelcomeWindowController(
        app: .scn, tagline: L("Scan documents, photos and receipts"),
        actions: [
            .init(L("New Scan"), symbol: "scanner", action: #selector(AppDelegate.newScanAction(_:))),
            .init(L("Scan with iPhone…"), symbol: "iphone", action: #selector(AppDelegate.phoneScanAction(_:))),
            .init(L("Import Images or PDF…"), symbol: "square.and.arrow.down", action: #selector(NSDocumentController.openDocument(_:))),
        ])

    override init() {
        super.init()
        AppDelegate.shared = self
    }

    func applicationWillFinishLaunching(_ notification: Notification) {
        DebugSnapshot.applyAppearanceIfRequested()
        NSApp.mainMenu = MenuBar.build(Cmd.menuBar())
        // Continuity Camera hands images and PDFs back through the services machinery.
        NSApp.registerServicesMenuSendTypes([], returnTypes: ContinuityImport.returnTypes)
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        if let i = CommandLine.arguments.firstIndex(of: "--audit"), CommandLine.arguments.indices.contains(i + 1) {
            Task { @MainActor in await ScanNativeAudit.run(CommandLine.arguments[i + 1]) }
            return
        }
        if CommandLine.arguments.contains("--uitest") {
            Task { @MainActor in await UITest.run() }
            return
        }
        NSApp.activate()
        DeviceManager.shared.start()
        let env = ProcessInfo.processInfo.environment
        if env["K42_WELCOME"] != nil {
            welcome.showWindow(nil)
        } else {
            let w = newWindow()
            // Files given on the command line or dropped on the icon are imported.
            let paths = CommandLine.arguments.dropFirst().filter { !$0.hasPrefix("-") && FileManager.default.fileExists(atPath: $0) }
            if !paths.isEmpty { w.importFiles(paths.map { URL(fileURLWithPath: $0) }, at: nil) }
        }
        if env["K42_SNAPSHOT"] != nil {
            // Demo pages are processed asynchronously; capture the finished document and allow a clean exit.
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.9) { [weak self] in self?.scheduleReadySnapshot() }
        }
        // Hooks that need time (scans, OCR) run before the snapshot (use K42_SNAPSHOT_DELAY).
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.6) { [weak self] in self?.earlyHooks(env) }
        // Debug runs never hang: quit after K42_WATCHDOG seconds.
        if let t = env["K42_WATCHDOG"].flatMap(Double.init) {
            DispatchQueue.main.asyncAfter(deadline: .now() + t) { print("watchdog: quitting"); exit(3) }
        }
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { false }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        if !flag { newWindow() }
        return true
    }

    func application(_ application: NSApplication, open urls: [URL]) {
        (frontController ?? newWindow()).importFiles(urls, at: nil)
    }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        if let active = windows.first(where: { $0.hasPendingWork }) {
            active.showWindow(nil)
            active.banner.show(.info, L("Wait for the current operation to finish."))
            return .terminateCancel
        }
        let dirty = windows.filter { $0.doc.isDirty && !$0.doc.isEmpty }
        guard !dirty.isEmpty, ProcessInfo.processInfo.environment["K42_SNAPSHOT"] == nil else { return .terminateNow }
        let alert = NSAlert()
        alert.messageText = L("Quit without saving the scans?")
        alert.informativeText = L("Unsaved scanned pages will be lost.")
        alert.addButton(withTitle: L("Quit"))
        alert.addButton(withTitle: L("Cancel"))
        return alert.runModal() == .alertFirstButtonReturn ? .terminateNow : .terminateCancel
    }

    func applicationWillTerminate(_ notification: Notification) {
        DeviceManager.shared.stop()
    }

    @discardableResult
    func newWindow() -> ScanWindowController {
        let w = ScanWindowController()
        windows.append(w)
        w.showWindow(nil)
        welcome.close()
        return w
    }

    func windowClosed(_ w: ScanWindowController) { windows.removeAll { $0 === w } }

    var frontController: ScanWindowController? {
        NSApp.orderedWindows.compactMap { $0.windowController as? ScanWindowController }.first ?? windows.first
    }

    @objc func showWelcomeAction(_ sender: Any?) { welcome.showWindow(nil) }
    @objc func newScanAction(_ sender: Any?) { newWindow(); welcome.close() }
    @objc func phoneScanAction(_ sender: Any?) {
        let w = frontController ?? newWindow()
        w.showWindow(nil)
        welcome.close()
        w.scanWithPhoneAction(w.toolbarImpl.phoneButton)
    }

    // MARK: - Debug hooks (K42_SNAPSHOT)

    /// K42_DEMO_PAGES=n adds generated pages; K42_AUTOSCAN=flatbed|feeder|overview|photos|rescan scans with the
    /// virtual scanner (K42_FAKE_SCANNER); K42_PRESET=n; K42_COLOR=gray|bw; K42_CONTINUITY_TEST=file feeds a
    /// private pasteboard to the Continuity Camera import; K42_SAVE_NOW saves into K42_SCAN_DIR; K42_DUMP_MENUS prints menus.
    private func earlyHooks(_ env: [String: String]) {
        setlinebuf(stdout)
        guard let w = frontController else { return }
        if let p = env["K42_PRESET"].flatMap(Int.init) { w.settingsPanel.choosePreset(p) }
        if let c = env["K42_COLOR"] {
            var s = w.settings
            s.colorMode = c == "bw" ? .bw : (c == "gray" ? .gray : .color)
            w.settingsPanel.apply(s)
        }
        if let src = env["K42_SOURCE"] {
            var s = w.settings
            s.source = src == "feeder" ? .feeder : .flatbed
            s.duplex = env["K42_DUPLEX"] != nil
            s.removeBlank = env["K42_REMOVE_BLANK"] != nil || s.removeBlank
            w.settingsPanel.apply(s)
        }
        w.settingsChanged()
        if let n = env["K42_DEMO_PAGES"].flatMap(Int.init), n > 0 {
            var o = w.settings.processing
            o.autoRotate = false
            for i in 0..<n {
                let img = i % 3 == 2 ? TestImages.documentOnGlass(angle: 4, small: true) : TestImages.feederPage(i + 1, skew: Double(i % 2) * 2, dpi: 100)
                if let img { w.process(RawScan(image: img, dpi: i % 3 == 2 ? 200 : 100), o, removeBlank: false) }
            }
        }
        if let file = env["K42_CONTINUITY_TEST"] {
            // A private pasteboard: the user's clipboard is not touched.
            let pb = NSPasteboard(name: NSPasteboard.Name("SCN42.test.\(UUID().uuidString)"))
            pb.clearContents()
            let url = URL(fileURLWithPath: file)
            if let data = try? Data(contentsOf: url) { pb.setData(data, forType: url.pathExtension.lowercased() == "pdf" ? .pdf : .png) }
            let requestor = w.preview.canvas.validRequestor(forSendType: nil, returnType: url.pathExtension.lowercased() == "pdf" ? .pdf : .png)
            print("continuity: requestor=\(requestor.map { String(describing: type(of: $0)) } ?? "nil")")
            print("continuity: read=\(w.preview.canvas.readSelection(from: pb))")
            pb.releaseGlobally()
        }
        if let mode = env["K42_AUTOSCAN"] {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.8) { self.autoscan(w, mode, env) }
        }
        if env["K42_DUMP_MENUS"] != nil { dumpMenus() }
        if let list = env["K42_ACTIONS"] { DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { self.runActions(w, list) } }
    }

    /// K42_ACTIONS="select:2,rotateRight,moveUp,delete,undo,redo,enhance,insertHere,split,removeBlank,ocr,copyTextDry":
    /// runs page commands once the pages are ready and prints the page order (ids shortened) after each.
    private func runActions(_ w: ScanWindowController, _ list: String) {
        if w.isScanning || w.pendingProcessing > 0 || w.busyText != nil {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) { self.runActions(w, list) }
            return
        }
        var steps = list.split(separator: ",").map(String.init)
        guard !steps.isEmpty else { return }
        let step = steps.removeFirst()
        let parts = step.split(separator: ":").map(String.init)
        switch parts[0] {
        case "select": w.pageList.select(parts.dropFirst().compactMap { Int($0).map { $0 - 1 } })
        case "rotateRight": w.rotateRightAction(nil)
        case "rotateLeft": w.rotateLeftAction(nil)
        case "moveUp": w.movePageUpAction(nil)
        case "moveDown": w.movePageDownAction(nil)
        case "delete": w.deletePageAction(nil)
        case "undo": w.doc.undo.undo()
        case "redo": w.doc.undo.redo()
        case "enhance": w.enhanceToggleAction(nil)
        case "bw": w.pageColorAction(NSMenuItem.tagged(ColorMode.bw.rawValue + 1))
        case "insertHere": w.insertHereAction(nil)
        case "split": w.splitAction(nil)
        case "removeBlank": w.removeBlankAction(nil)
        case "ocr": w.recognizeTextAction(nil)
        case "hideAll": w.toggleAllPanelsAction(nil)
        case "history": w.showHistoryAction(nil)
        default: print("actions: unknown \(step)")
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) {
            func describe(_ c: ScanWindowController) -> String {
                c.pages.map { p in "\(p.id.uuidString.prefix(4))\(p.image.width > p.image.height ? "L" : "P")\(p.options.enhancement.rawValue)\(p.image.bitsPerPixel == 1 ? "b" : "")" }.joined(separator: " ")
            }
            let selected = w.pageList.selection.map { "\($0 + 1)" }.joined(separator: "+")
            print("actions: \(step) → [\(describe(w))] selected=\(selected) undo=\(w.doc.undo.canUndo) windows=\(self.windows.count)"
                  + (self.windows.count > 1 ? " other=[\(describe(self.windows.last!))]" : ""))
            if !steps.isEmpty { self.runActions(w, steps.joined(separator: ",")) }
        }
    }

    private func autoscan(_ w: ScanWindowController, _ mode: String, _ env: [String: String]) {
        print("autoscan: \(mode) scanner=\(w.scanner?.name ?? "none") status=\(w.scanner?.status.text ?? "-")")
        switch mode {
        case "overview": w.overviewAction(nil)
        case "photos":
            var s = w.settings
            s.detectPhotos = true
            s.enhancement = .photo
            s.autoRotate = false
            w.settingsPanel.apply(s)
            w.scanAction(nil)
        default: w.scanAction(nil)
        }
        func waitDone(_ tries: Int) {
            if (w.isScanning || w.pendingProcessing > 0 || w.busyText != nil) && tries > 0 {
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) { waitDone(tries - 1) }
                return
            }
            print("autoscan: done pages=\(w.pages.count) areas=\(w.areas.count) banner=\(w.banner.isHidden ? "-" : "shown")")
            for (i, p) in w.pages.enumerated() { print("autoscan: page \(i + 1) \(p.image.width)x\(p.image.height)@\(Int(p.dpi)) \(p.report.summary)") }
            if env["K42_SAVE_NOW"] != nil { self.saveNow(w, env) }
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { waitDone(400) }
    }

    /// Saves without the sheet (into K42_SCAN_DIR), prints the files and shows the history.
    private func saveNow(_ w: ScanWindowController, _ env: [String: String]) {
        var o = Store.saveOptions
        o.folder = nil
        o.openAfter = .none
        if let f = env["K42_SAVE_FORMAT"] { o.format = ["tiff": .tiff, "jpeg": .jpeg, "png": .png, "heic": .heic, "pdfa": .pdfA, "pages": .pdfPerPage][f] ?? .pdf }
        o.ocr = env["K42_SAVE_OCR"] != nil
        w.save(o) { url in
            print("saved: \(url?.path ?? "nil") history=\(History.shared.entries.count)")
            if env["K42_SHOW"] == "history" { w.inspector.show(history: true) }
        }
    }

    private func prepareSnapshot() {
        let env = ProcessInfo.processInfo.environment
        guard let w = frontController else { return }
        switch env["K42_SHOW"] {
        case "history": w.inspector.show(history: true)
        case "settings": w.inspector.show(history: false)
        case "more": w.settingsPanel.showMore(true)
        case "glass":
            w.showingGlass = true
            w.refreshPreview()
        default: break
        }
        if let p = env["K42_SELECT"].flatMap(Int.init) { w.pageList.select([p - 1]) }
        if env["K42_SAVE_SHEET"] != nil { w.saveDocument(nil) }
        if env["K42_NOSIDEBAR"] != nil { w.sidebarItem.isCollapsed = true }
        if env["K42_FULLSCREEN"] != nil { w.window?.toggleFullScreen(nil) }
        if let z = env["K42_ZOOM"].flatMap(Double.init) { w.preview.scroll.magnification = CGFloat(z) }
        if env["K42_PRINT_TEST"] != nil, let op = w.printOperation(w.pages, copies: 3) {
            print("print: pages=\(op.pageRange.length) copies=\(op.printInfo.dictionary()[NSPrintInfo.AttributeKey.copies] ?? "?") job=\(op.jobTitle ?? "")")
        }
    }

    private func scheduleReadySnapshot() {
        if windows.contains(where: { $0.hasPendingWork }) {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.15) { [weak self] in self?.scheduleReadySnapshot() }
        } else {
            DebugSnapshot.scheduleIfRequested { [weak self] in self?.prepareSnapshot() }
        }
    }

    private func dumpMenus() {
        func dump(_ menu: NSMenu, _ depth: Int) {
            for item in menu.items {
                if item.isSeparatorItem { continue }
                let key = item.keyEquivalent.isEmpty ? "" : " [\(item.keyEquivalentModifierMask.contains(.shift) ? "⇧" : "")\(item.keyEquivalentModifierMask.contains(.option) ? "⌥" : "")\(item.keyEquivalentModifierMask.contains(.control) ? "⌃" : "")⌘\(item.keyEquivalent)]"
                let id = item.identifier == NSMenuItem.importFromDeviceIdentifier ? " (Continuity Camera)" : ""
                print(String(repeating: "  ", count: depth) + item.title + key + id)
                if let sub = item.submenu, depth < 2 { dump(sub, depth + 1) }
            }
        }
        if let main = NSApp.mainMenu { dump(main, 0) }
    }
}

extension NSMenuItem {
    static func tagged(_ tag: Int) -> NSMenuItem {
        let i = NSMenuItem()
        i.tag = tag
        return i
    }
}
