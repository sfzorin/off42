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
        if ProcessInfo.processInfo.environment["K42_SIGNING_AUDIT"] != nil {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { SigningUIAudit.run() }
        }
        let prepareSnapshot: () -> Void = {
            let env = ProcessInfo.processInfo.environment
            let editor = NSApp.orderedWindows.compactMap { $0.windowController as? EditorWindowController }.first
            if env["K42_MARKUP"] != nil { editor?.toggleMarkupAction(nil) }
            if env["K42_STAMPS"] != nil { editor?.showStampsAction(nil) }
            if env["K42_COMPRESS"] != nil { editor?.reduceSizeAction(nil) }
            if let tool = env["K42_TOOL"].flatMap(Int.init).flatMap(Tool.init) { editor?.setTool(tool) }
            // K42_SIDEBAR=thumbnails|outline|attachments, K42_LINK_BORDERS=1, K42_DO=action,action (non-modal actions before the snapshot).
            if let mode = env["K42_SIDEBAR"], let editor {
                editor.showSidebarMode(["outline": .outline, "attachments": .attachments][mode] ?? .thumbnails)
            }
            if env["K42_LINK_BORDERS"] != nil { editor?.pdfView.showsLinkBorders = true }
            // K42_CERT_WIZARD=0…4 opens Get a Certificate on that tab (keys, Russia, Türkiye, EU, free).
            if let tab = env["K42_CERT_WIZARD"].flatMap(Int.init) { CertificateWizard.show(tab: tab) }
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
        if let audit = ProcessInfo.processInfo.environment["K42_AUDIT"] {
            // Native activation and PDFKit field editors need the main queue to return between
            // events. Schedule the snapshot only after the asynchronous audit has completed.
            Task { @MainActor in
                let deadline = Date(timeIntervalSinceNow: 3)
                var editor: EditorWindowController?
                repeat {
                    editor = NSApp.orderedWindows.compactMap { $0.windowController as? EditorWindowController }.first
                    if editor == nil { try? await Task.sleep(nanoseconds: 20_000_000) }
                } while editor == nil && Date() < deadline
                guard let editor else { print("FAIL PDF audit: no editor window"); exit(1) }
                let originallyEdited = editor.pdfDoc?.isDocumentEdited == true
                guard await editor.activateForNativeAudit("start " + audit) else { exit(1) }
                let passed: Bool
                switch audit {
                case "markup": passed = await editor.auditMarkup()
                case "pages": passed = await editor.auditPages()
                case "printer": passed = await editor.auditPrinterFiles()
                case "core": passed = editor.auditCore()
                case "technical":
                    // This scenario pumps the run loop for native sheets and drags; nested pumping only delivers
                    // events from a run-loop timer, not from inside a Swift task, so run it from one and await it.
                    passed = await withCheckedContinuation { continuation in
                        let timer = Timer(timeInterval: 0, repeats: false) { _ in continuation.resume(returning: editor.auditTechnical()) }
                        RunLoop.main.add(timer, forMode: .common)
                    }
                case "files": passed = PDFFilesAudit.runUI(owner: editor) == 0
                default: print("FAIL PDF audit: unknown scenario"); exit(1)
                }
                if !passed { exit(1) }
                // Each scenario restores its disposable source/Undo manager. Restore the original
                // dirty state so snapshot termination cannot open an unrelated Save prompt.
                editor.pdfDoc?.updateChangeCount(.changeCleared)
                if originallyEdited { editor.pdfDoc?.updateChangeCount(.changeDone) }
                if ProcessInfo.processInfo.environment["K42_SNAPSHOT"] != nil {
                    DebugSnapshot.scheduleIfRequested(prepare: prepareSnapshot)
                } else { exit(0) }
            }
        } else {
            DebugSnapshot.scheduleIfRequested(prepare: prepareSnapshot)
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

    @objc func batchSignAction(_ sender: Any?) { BatchSignUI.start() }

    @objc func importCertificateAction(_ sender: Any?) {
        if let identity = SignDialog.importCertificate() {
            info(L("Certificate added"), L("“%@” is saved in the Keychain and can be used for signing.", identity.displayName))
        }
    }

    @objc func getCertificateAction(_ sender: Any?) { CertificateWizard.show() }

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

    @objc func cryptoProAction(_ sender: Any?) { SignDialog.showCryptoPro() }

    /// Detached CAdES signature (.sig) of any file with a CryptoPro certificate.
    @objc func gostSignFileAction(_ sender: Any?) {
        guard CryptoPro.isInstalled else { SignDialog.showCryptoPro(); return }
        let open = NSOpenPanel()
        open.message = L("Choose the file to sign")
        guard open.runModal() == .OK, let file = open.url else { return }
        let certs = (try? CryptoPro.certificates()) ?? []
        guard !certs.isEmpty else {
            info(L("No GOST certificates"), L("CryptoPro CSP has no certificate linked to a private key. Insert the token or install the certificate from its key container."))
            SignDialog.showCryptoPro()
            return
        }
        let popup = NSPopUpButton(frame: .zero, pullsDown: false)
        popup.addItems(withTitles: certs.map(\.displayName))
        let pin = NSSecureTextField()
        pin.placeholderString = L("Container PIN (if it has one)")
        let tsa = NSTextField(string: "")
        tsa.placeholderString = L("Optional: TSA URL for CAdES-T")
        let alert = NSAlert.form(L("Sign File with GOST"), info: L("A detached signature “%@.sig” (CAdES) is saved next to the file.", file.lastPathComponent),
                                 ok: L("Sign"), rows: [(L("Certificate:"), popup), (L("PIN:"), pin), (L("TSA server:"), tsa)])
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        let cert = certs[max(0, popup.indexOfSelectedItem)]
        let out = file.appendingPathExtension("sig")
        do {
            let url = URL(string: tsa.stringValue.trimmingCharacters(in: .whitespaces)).flatMap { $0.scheme == nil ? nil : $0 }
            try CryptoPro.signDetached(file: file, to: out, options: CryptoPro.SignOptions(thumbprint: cert.thumbprint,
                                       pin: pin.stringValue.isEmpty ? nil : pin.stringValue, tsa: url))
            NSWorkspace.shared.activateFileViewerSelecting([out])
        } catch { NSApp.presentError(error) }
    }

    /// Verifies a detached signature; an МЧД can be checked along with it.
    @objc func gostVerifyFileAction(_ sender: Any?) {
        let open = NSOpenPanel()
        open.message = L("Choose the signature file (.sig, .p7s)")
        guard open.runModal() == .OK, let sigURL = open.url, let sig = try? Data(contentsOf: sigURL) else { return }
        var file = sigURL.deletingPathExtension()
        if !FileManager.default.fileExists(atPath: file.path) {
            let pick = NSOpenPanel()
            pick.message = L("Choose the signed file")
            guard pick.runModal() == .OK, let url = pick.url else { return }
            file = url
        }
        let parsed = CMSInfo(sig)
        var s = SignatureInfo()
        s.coversWholeFile = true
        PDFSignatureVerifier.describe(parsed, into: &s)
        s.isGOST = parsed?.isGOST ?? false
        if CryptoPro.isInstalled {
            do {
                let v = try CryptoPro.verifyDetached(file: file, signature: sigURL)
                switch v.status {
                case .valid: s.status = .valid
                case .untrusted: s.status = .untrusted; s.trustMessage = v.message
                case .invalid: s.status = .invalid; s.problem = L("The document was changed after signing") + (v.message.map { "\n" + $0 } ?? "")
                }
            } catch { s.status = .invalid; s.problem = error.localizedDescription }
        } else {
            s.status = .notChecked
            s.trustMessage = CryptoPro.installHelp
        }
        let ask = NSAlert()
        ask.messageText = L("Check a power of attorney (МЧД) too?")
        ask.addButton(withTitle: L("Choose МЧД…"))
        ask.addButton(withTitle: L("No"))
        if ask.runModal() == .alertFirstButtonReturn, let files = SignDialog.choosePowerOfAttorney(),
           let xml = files.first, let poa = PowerOfAttorney(xml: xml.data) {
            s.powerOfAttorney = poa.check(signer: s.certificate, signedAt: s.date, xml: xml.data, poaSignature: files.dropFirst().first?.data)
        }
        SignDialog.showResults([s], window: nil)
    }

    private func info(_ title: String, _ text: String) {
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = text
        alert.runModal()
    }
}
