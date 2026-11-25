import AppKit
import Kit42
import PDFKit

final class AppDelegate: NSObject, NSApplicationDelegate {
    private lazy var welcome = WelcomeWindowController(
        app: .pdf, tagline: L("View and edit PDF"),
        actions: [
            .init(L("Open PDF…"), symbol: "folder", action: #selector(NSDocumentController.openDocument(_:))),
            .init(L("New Document"), symbol: "doc.badge.plus", action: #selector(NSDocumentController.newDocument(_:))),
            .init(L("Signatures and Stamps"), symbol: "signature", action: #selector(showStampLibraryAction(_:))),
        ])

    func applicationWillFinishLaunching(_ notification: Notification) {
        // Debug hook for appearance checks: K42_APPEARANCE=dark|light, before any window exists.
        switch ProcessInfo.processInfo.environment["K42_APPEARANCE"] {
        case "dark": NSApp.appearance = NSAppearance(named: .darkAqua)
        case "light": NSApp.appearance = NSAppearance(named: .aqua)
        default: break
        }
        // Autosave every 30 s to a backup location (not over the file): documents survive a crash.
        NSDocumentController.shared.autosavingDelay = 30
        NSApp.mainMenu = MenuBar.build(Cmd.menuBar)
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.activate()
        // Started from the command line with paths (`PDF42 file.pdf`, also for snapshot runs).
        let paths = CommandLine.arguments.dropFirst().filter { !$0.hasPrefix("-") && FileManager.default.fileExists(atPath: $0) }
        for path in paths {
            NSDocumentController.shared.openDocument(withContentsOf: URL(fileURLWithPath: path), display: true) { _, _, error in
                if let error { FileHandle.standardError.write(Data("cannot open \(path): \(error)\n".utf8)) }
            }
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) { [self] in
            if NSDocumentController.shared.documents.isEmpty, paths.isEmpty { welcome.showWindow(nil) }
        }
        NotificationCenter.default.addObserver(forName: NSWindow.didBecomeMainNotification, object: nil, queue: .main) { [weak self] note in
            if (note.object as? NSWindow)?.windowController is EditorWindowController { self?.welcome.close() }
        }
        DebugSnapshot.scheduleIfRequested {
            let env = ProcessInfo.processInfo.environment
            let editor = NSApp.orderedWindows.compactMap { $0.windowController as? EditorWindowController }.first
            if let audit = env["K42_AUDIT"] {
                guard let editor else { print("FAIL PDF audit: no editor window"); exit(1) }
                let originallyEdited = editor.pdfDoc?.isDocumentEdited == true
                NSApp.activate(ignoringOtherApps: true)
                editor.window?.makeKeyAndOrderFront(nil)
                editor.window?.makeMain()
                RunLoop.main.run(until: Date(timeIntervalSinceNow: 0.15))
                let passed: Bool
                switch audit {
                case "markup": passed = editor.auditMarkup()
                case "pages": passed = editor.auditPages()
                case "core": passed = editor.auditCore()
                case "technical": passed = editor.auditTechnical()
                case "files": passed = PDFFilesAudit.runUI(owner: editor) == 0
                default: print("FAIL PDF audit: unknown scenario"); exit(1)
                }
                if !passed { exit(1) }
                // Each scenario restores its disposable source/Undo manager. Restore the original dirty
                // state too, so snapshot termination cannot open an unrelated Save prompt after PASS.
                editor.pdfDoc?.updateChangeCount(.changeCleared)
                if originallyEdited { editor.pdfDoc?.updateChangeCount(.changeDone) }
            }
            if env["K42_MARKUP"] != nil { editor?.toggleMarkupAction(nil) }
            if env["K42_STAMPS"] != nil { editor?.showStampsAction(nil) }
            if env["K42_COMPRESS"] != nil { editor?.reduceSizeAction(nil) }
            if let tool = env["K42_TOOL"].flatMap(Int.init).flatMap(Tool.init) { editor?.setTool(tool) }
            // K42_SIDEBAR=thumbnails|outline|attachments, K42_LINK_BORDERS=1, K42_DO=action,action (non-modal actions before the snapshot).
            if let mode = env["K42_SIDEBAR"], let editor {
                editor.showSidebarMode(["outline": .outline, "attachments": .attachments][mode] ?? .thumbnails)
            }
            if env["K42_LINK_BORDERS"] != nil { editor?.pdfView.showsLinkBorders = true }
            for action in (env["K42_DO"] ?? "").split(separator: ",") {
                let selector = NSSelectorFromString(action + ":")
                if let editor, editor.responds(to: selector) { editor.perform(selector, with: nil) }
            }
            if let text = env["K42_WATERMARK"], let editor, let pdf = editor.pdf {
                var spec = WatermarkSpec()
                spec.text = text
                for i in 0..<pdf.pageCount { if let p = pdf.page(at: i) { editor.add(WatermarkOverlay(spec: spec, page: p), to: p, name: "") } }
                for (i, t) in PageNumberSpec().labels(for: Array(0..<pdf.pageCount)) {
                    if let p = pdf.page(at: i) { editor.add(PageNumberOverlay(text: t, spec: PageNumberSpec(), page: p), to: p, name: "") }
                }
            }
            if let text = env["K42_EDIT_TEXT"], let editor, let hit = editor.pdf?.findString(text, withOptions: []).first, let page = hit.pages.first {
                editor.setTool(.editText)
                editor.editPageText(hit, on: page)
            }
            if let path = env["K42_COMPARE"], let other = PDFDocument(url: URL(fileURLWithPath: path)) {
                editor?.showComparison(with: other, name: (path as NSString).lastPathComponent)
            }
            if let marks = env["K42_REDACT"], let editor, let page = editor.pdf?.page(at: 0) {
                // K42_REDACT="x,y,w,h;…": redaction marks on page 1 (then e.g. K42_ACTION=applyRedactionsAction).
                for spec in marks.split(separator: ";") {
                    let n = spec.split(separator: ",").compactMap { Double($0) }
                    if n.count == 4 { editor.add(RedactOverlay(bounds: CGRect(x: n[0], y: n[1], width: n[2], height: n[3]), color: .black), to: page, name: "") }
                }
            }
            if let action = env["K42_ACTION"] {
                // K42_ACTION=<action method> opens a dialog for a snapshot (watermarkAction, pageNumbersAction,
                // splitAction, signDocumentAction, …; mergeList, textField, choiceField for the dialogs behind
                // a file panel or a drag). App-modal dialogs block quitting, so the debug run ends hard.
                // The main queue is not served while an app-modal alert runs: a timer in the common modes is.
                let timer = Timer(timeInterval: 1.2, repeats: false) { _ in
                    if let path = env["K42_SNAPSHOT"], let window = NSApp.modalWindow ?? NSApp.windows.compactMap(\.attachedSheet).first,
                       let frame = window.contentView?.superview, let rep = frame.bitmapImageRepForCachingDisplay(in: frame.bounds) {
                        frame.cacheDisplay(in: frame.bounds, to: rep)
                        try? rep.representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: path))
                        exit(0)
                    }
                }
                RunLoop.main.add(timer, forMode: .common)
                DispatchQueue.main.asyncAfter(deadline: .now() + 2.5) { exit(0) }
                DispatchQueue.main.async {
                    switch action {
                    case "mergeList":
                        _ = FileOrderList.run(NSApp.orderedDocuments.compactMap(\.fileURL) + [URL(fileURLWithPath: "/tmp/other.pdf")])
                    case "textField", "choiceField", "checkBox":
                        if let editor, let page = editor.pdf?.page(at: 0) {
                            let kind: PDFTools.FieldKind = action == "textField" ? .text : action == "checkBox" ? .checkBox : .choice
                            editor.createFormField(kind, in: CGRect(x: 72, y: 600, width: 180, height: 22), on: page)
                        }
                    case "linkDialog", "cropDialog":
                        if let editor, let page = editor.pdf?.page(at: 0) {
                            let rect = CGRect(x: 72, y: 600, width: 200, height: 20)
                            if action == "linkDialog" { editor.createLink(in: rect, on: page) } else { editor.cropDialog(rect: rect.insetBy(dx: 0, dy: -200), on: page) }
                        }
                    default:
                        let selector = NSSelectorFromString(action + ":")
                        if let editor, editor.responds(to: selector) { editor.perform(selector, with: nil) }
                        else { NSApp.sendAction(selector, to: nil, from: nil) }
                    }
                }
            }
        }
    }

    func applicationShouldOpenUntitledFile(_ sender: NSApplication) -> Bool { false }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        if !flag { welcome.showWindow(nil) }
        return true
    }

    // MARK: - App-level actions

    @objc func showWelcomeAction(_ sender: Any?) { welcome.showWindow(nil) }

    @objc func newFromImagesAction(_ sender: Any?) {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.image]
        panel.allowsMultipleSelection = true
        panel.message = L("Each image becomes an A4 page")
        guard panel.runModal() == .OK else { return }
        let pdf = PDFDocument()
        for url in panel.urls.sorted(by: { $0.lastPathComponent.localizedStandardCompare($1.lastPathComponent) == .orderedAscending }) {
            if let image = NSImage(contentsOf: url), let page = PDFDoc.page(from: image) { pdf.insert(page, at: pdf.pageCount) }
        }
        guard pdf.pageCount > 0 else { return }
        let doc = PDFDoc(pdf: pdf)
        NSDocumentController.shared.addDocument(doc)
        doc.makeWindowControllers()
        doc.showWindows()
        doc.updateChangeCount(.changeDone)
    }

    @objc func showStampLibraryAction(_ sender: Any?) {
        let popover = NSPopover()
        let vc = StampLibraryViewController { _ in }
        vc.canInsert = false
        vc.popover = popover
        popover.contentViewController = vc
        popover.behavior = .transient
        if let view = sender as? NSView { popover.show(relativeTo: view.bounds, of: view, preferredEdge: .maxX) }
    }

    @objc func importCertificateAction(_ sender: Any?) {
        if let identity = SignDialog.importCertificate() {
            info(L("Certificate added"), L("“%@” is saved in the Keychain and can be used for signing.", identity.displayName))
        }
    }

    @objc func createCertificateAction(_ sender: Any?) {
        if let identity = SignDialog.createCertificate() {
            info(L("Certificate created"), L("“%@” is saved in the Keychain. It is a self-signed certificate: the signature will be intact, but other apps will show that the certificate is not confirmed by a certification authority.", identity.displayName))
        }
    }

    @objc func showShortcutsAction(_ sender: Any?) {
        let lines = [
            L("⌘1…⌘9 — markup tools, Esc — select"),
            L("⇧⌘A — markup toolbar, ⇧⌘K — signature or stamp"),
            L("⌘E — edit selected text, ⌘⌫ — delete annotation"),
            L("⌥⌘M — merge files, ⌘E — edit text (click a line)"),
            L("⌘L / ⌘R — rotate page, ⌥⌘⌫ — delete pages"),
            L("⇧⌘N — blank page, ⇧⌥⌘I — pages from file"),
            L("⌘D — add bookmark, ⌥⌘I — document properties, ⌥⌘K — crop"),
            L("⌥⌘D — sign, ⌥⌘V — verify signatures"),
            L("⌘F / ⌘G — find, ⌥⌘G — go to page"),
            L("⌘+ / ⌘- / ⌘0 — zoom, ⌃⌘S — sidebar"),
        ]
        info(L("Keyboard Shortcuts"), lines.joined(separator: "\n"))
    }

    private func info(_ title: String, _ text: String) {
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = text
        alert.runModal()
    }
}
