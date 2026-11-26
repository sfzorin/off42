import AppKit
import PDFKit
import Kit42

final class AppDelegate: NSObject, NSApplicationDelegate {
    private lazy var welcome = WelcomeWindowController(
        app: .xls, tagline: L("Spreadsheets with formulas"),
        actions: [
            .init(L("Open Spreadsheet…"), symbol: "folder", action: #selector(NSDocumentController.openDocument(_:))),
            .init(L("New Spreadsheet"), symbol: "tablecells.badge.ellipsis", action: #selector(NSDocumentController.newDocument(_:))),
        ])

    func applicationWillFinishLaunching(_ notification: Notification) {
        // Autosave every 30 s to a backup location (not over the file): documents survive a crash.
        NSDocumentController.shared.autosavingDelay = 30
        // Debug hook: K42_APPEARANCE=dark|light forces the appearance before any window opens (snapshot checks).
        if let mode = ProcessInfo.processInfo.environment["K42_APPEARANCE"] {
            NSApp.appearance = NSAppearance(named: mode == "dark" ? .darkAqua : .aqua)
        }
        NSApp.mainMenu = MenuBar.build(Cmd.menuBar)
        CellStyleMenu.install(in: NSApp.mainMenu)
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        if CommandLine.arguments.contains("--uitest") {
            DispatchQueue.main.async { UITest.run() }
            return
        }
        NSApp.activate()
        // Debug hook: K42_OPEN=/path/file.xlsx opens a document at launch (snapshot checks of the unbundled binary).
        if let path = ProcessInfo.processInfo.environment["K42_OPEN"] {
            NSDocumentController.shared.openDocument(withContentsOf: URL(fileURLWithPath: path), display: true) { document, _, error in
                if let error { FileHandle.standardError.write(Data("cannot open \(path): \(error)\n".utf8)) }
                // K42_SIGNATURE_AUDIT=1: signed-document lifecycle regression on the opened file (Kit42).
                guard ProcessInfo.processInfo.environment["K42_SIGNATURE_AUDIT"] != nil, let sheet = document as? SheetDocument else { return }
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
                    let ok = sheet.signatureState.audit(edit: {
                        // A cell entry as one closed undo group, as after a key event.
                        let undo = sheet.undoManager
                        undo?.groupsByEvent = false
                        undo?.beginUndoGrouping()
                        _ = sheet.editor?.setInput("42", at: CellRef(row: 0, col: 0))
                        undo?.endUndoGrouping()
                        undo?.groupsByEvent = true
                    })
                    if !ok { exit(1) }
                }
            }
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) { [self] in
            if NSDocumentController.shared.documents.isEmpty { welcome.showWindow(nil) }
        }
        NotificationCenter.default.addObserver(forName: NSWindow.didBecomeMainNotification, object: nil, queue: .main) { [weak self] note in
            if (note.object as? NSWindow)?.windowController is SheetWindowController { self?.welcome.close() }
        }
        DebugSnapshot.scheduleIfRequested {
            // K42_SCROLL=x,y scrolls the front sheet; K42_SELECT=C10 selects a cell, K42_SELECT=A1:C9 a range (UI checks).
            let env = ProcessInfo.processInfo.environment
            guard let editor = NSApp.orderedWindows.compactMap({ $0.windowController as? SheetWindowController }).first else { return }
            let selected = (env["K42_SELECT"] ?? "").split(separator: ":").compactMap { CellRef(String($0)) }
            if let first = selected.first {
                editor.grid.select(first)
                if selected.count > 1 { editor.grid.select(selected[1], extend: true) }
            }
            if let xy = env["K42_SCROLL"]?.split(separator: ",").compactMap({ Double($0) }), xy.count == 2 {
                editor.debugScroll(to: NSPoint(x: xy[0], y: xy[1]))
            }
            // K42_ACTION=insertFunctionAction: sends a command (non-modal dialogs: wizard, find panel).
            if let action = env["K42_ACTION"], let path = env["K42_SNAPSHOT"] {
                // The dialog itself goes to <snapshot>-dialog.png; a sheet is closed and a modal alert
                // cancelled so the app can quit. A run-loop timer scheduled first: modal dialogs block
                // sendAction, and nested main-queue blocks don't run while this one does.
                // K42_DIALOG_STEPS="segment:Codes;popup:None=SKU;field:Width in millimetres=40;click:Update Preview" operates
                // the dialog's own controls first (use with K42_SNAPSHOT_KEEP=1); K42_DIALOG_DELAY waits for its result.
                let steps = env["K42_DIALOG_STEPS"]
                func dialogWindow() -> NSWindow? {
                    NSApp.modalWindow ?? editor.window?.attachedSheet ?? editor.findPanel?.window ?? editor.functionWizard?.window
                }
                func captureDialog() {
                    let modal = NSApp.modalWindow
                    guard let dialog = dialogWindow(),
                          let frame = dialog.contentView?.superview, let rep = frame.bitmapImageRepForCachingDisplay(in: frame.bounds) else { return }
                    frame.cacheDisplay(in: frame.bounds, to: rep)
                    try? rep.representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: path.replacingOccurrences(of: ".png", with: "-dialog.png")))
                    // K42_DIALOG_PDF=path saves the document shown in the dialog's PDF preview (layer-backed, not cached).
                    if let pdfPath = env["K42_DIALOG_PDF"], let root = dialog.contentView {
                        func views(_ v: NSView) -> [NSView] { [v] + v.subviews.flatMap(views) }
                        views(root).compactMap { $0 as? PDFView }.first?.document?.write(toFile: pdfPath)
                    }
                    if steps != nil { exit(0) }
                    if modal != nil { NSApp.abortModal(); dialog.orderOut(nil) }
                    else if dialog.sheetParent != nil { editor.window?.endSheet(dialog) } else { dialog.orderOut(nil) }
                }
                let capture = Timer(timeInterval: 0.6, repeats: false) { _ in
                    guard let steps, let dialog = dialogWindow() else { captureDialog(); return }
                    DialogDriver.run(steps, in: dialog)
                    let later = Timer(timeInterval: Double(env["K42_DIALOG_DELAY"] ?? "") ?? 2, repeats: false) { _ in captureDialog() }
                    RunLoop.main.add(later, forMode: .common)
                }
                RunLoop.main.add(capture, forMode: .common)
                NSApp.sendAction(NSSelectorFromString(action), to: editor, from: nil)
            }
        }
    }

    func applicationShouldOpenUntitledFile(_ sender: NSApplication) -> Bool { false }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        if !flag { welcome.showWindow(nil) }
        return true
    }

    @objc func showWelcomeAction(_ sender: Any?) { welcome.showWindow(nil) }

    @objc func showFunctionsAction(_ sender: Any?) {
        let alert = NSAlert()
        alert.messageText = L("Supported Functions")
        alert.informativeText = L("Russian function names (СУММ, ЕСЛИ, ВПР…) are accepted too.") + "\n\n" + Self.functionList
        alert.runModal()
    }

    /// Function names grouped by category (from the engine's catalog).
    static var functionList: String {
        FunctionCatalog.Category.allCases.map { c in
            c.title + ": " + FunctionCatalog.all.filter { $0.category == c }.map(\.name).joined(separator: " ")
        }.joined(separator: "\n\n") + "\n\n" + L("Insert › Function… lists them with descriptions.")
    }
}

/// Debug snapshots: operates a dialog's controls the way a person would (selection, typing, button press).
private enum DialogDriver {
    static func run(_ script: String, in window: NSWindow) {
        guard let root = window.contentView else { return }
        func views(_ v: NSView) -> [NSView] { [v] + v.subviews.flatMap(views) }
        for step in script.split(separator: ";") {
            let parts = step.split(separator: ":", maxSplits: 1).map(String.init)
            guard parts.count == 2 else { continue }
            let kv = parts[1].split(separator: "=", maxSplits: 1).map(String.init)
            let all = views(root)
            switch parts[0] {
            case "popup":
                guard kv.count == 2, let popup = all.compactMap({ $0 as? NSPopUpButton })
                        .first(where: { $0.itemTitles.first == kv[0] || $0.accessibilityLabel() == kv[0] }),
                      popup.indexOfItem(withTitle: kv[1]) >= 0 else { continue }
                popup.selectItem(withTitle: kv[1])
                if let action = popup.action { NSApp.sendAction(action, to: popup.target, from: popup) }
            case "combo":
                guard kv.count == 2, let combo = all.compactMap({ $0 as? NSComboBox }).first,
                      let index = (0..<combo.numberOfItems).first(where: { (combo.itemObjectValue(at: $0) as? String)?.hasPrefix(kv[1]) == true }) else { continue }
                combo.selectItem(at: index)
                combo.stringValue = combo.itemObjectValue(at: index) as? String ?? ""
                (combo.delegate as? NSComboBoxDelegate)?.comboBoxSelectionDidChange?(Notification(name: NSComboBox.selectionDidChangeNotification, object: combo))
            case "field":
                guard kv.count == 2, let field = all.compactMap({ $0 as? NSTextField })
                        .first(where: { $0.isEditable && ($0.accessibilityLabel() == kv[0] || $0.placeholderString == kv[0]) }) else { continue }
                field.stringValue = kv[1]
                field.delegate?.controlTextDidChange?(Notification(name: NSControl.textDidChangeNotification, object: field))
            case "segment":
                guard let control = all.compactMap({ $0 as? NSSegmentedControl }).first(where: { c in
                    (0..<c.segmentCount).contains { c.label(forSegment: $0) == parts[1] } }) else { continue }
                control.selectedSegment = (0..<control.segmentCount).first { control.label(forSegment: $0) == parts[1] } ?? 0
                if let action = control.action { NSApp.sendAction(action, to: control.target, from: control) }
            case "click":
                all.compactMap { $0 as? NSButton }.first { $0.title == parts[1] && $0.isEnabled }?.performClick(nil)
            default: continue
            }
        }
    }
}
