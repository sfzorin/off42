import AppKit
import PDFKit

extension EditorWindowController {
    @MainActor func auditPrinterFiles() async -> Bool {
        let fixture = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString + ".zpl")
        let saved = fixture.deletingPathExtension().appendingPathExtension("pdf")
        let exported = fixture.deletingPathExtension().appendingPathExtension("out.zpl")
        defer { try? FileManager.default.removeItem(at: fixture); try? FileManager.default.removeItem(at: saved); try? FileManager.default.removeItem(at: exported) }
        do { try Data("^XA^MNY^PW9^LL2^LH0,0^FO0,0^GFA,4,4,2,80804000^FS^XZ".utf8).write(to: fixture) } catch { print("FAIL PDF printer: fixture", error); return false }
        let before = Set(NSDocumentController.shared.documents.map { ObjectIdentifier($0) })
        var dialog = false, opened = false
        func pressReturn(_ panel: NSSavePanel) -> Bool {
            guard let event = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [], timestamp: 0, windowNumber: panel.windowNumber, context: nil, characters: "\r", charactersIgnoringModifiers: "\r", isARepeat: false, keyCode: 36) else { return false }
            NSApp.postEvent(event, atStart: false); return true
        }
        func descendants(_ v: NSView) -> [NSView] { [v] + v.subviews.flatMap(descendants) }
        var openReady: Date?
        let deadline = Date(timeIntervalSinceNow: 6)
        let timer = Timer(timeInterval: 0.05, repeats: true) { timer in
            if Date() > deadline { print("PDF printer dialog diagnostic:", NSApp.windows.map { "\(type(of: $0))|\($0.title)|\($0.isVisible)" }, "modal", NSApp.modalWindow.map { "\(type(of: $0))|\($0.title)" } ?? "none"); timer.invalidate(); NSApp.abortModal(); return }
            if !opened, let panel = NSApp.windows.compactMap({ $0 as? NSOpenPanel }).first(where: { $0.isVisible }) {
                if openReady == nil {
                    panel.directoryURL = fixture.deletingLastPathComponent(); panel.nameFieldStringValue = fixture.lastPathComponent
                    openReady = Date(timeIntervalSinceNow: 0.4)
                } else if let ready = openReady, Date() >= ready {
                    opened = pressReturn(panel)
                }
            } else if !dialog, let content = NSApp.modalWindow?.contentView,
                      let button = descendants(content).compactMap({ $0 as? NSButton }).first(where: { $0.title == "Continue" }) {
                dialog = true; button.performClick(nil)
            }
        }
        RunLoop.current.add(timer, forMode: .modalPanel)
        await withCheckedContinuation { (continuation: CheckedContinuation<Void, Never>) in
            Timer.scheduledTimer(withTimeInterval: 0.01, repeats: false) { _ in
                _ = NSApp.sendAction(#selector(AppDelegate.importPrinterFileAction(_:)), to: NSApp.delegate, from: nil)
                continuation.resume()
            }
        }
        timer.invalidate()
        guard dialog, opened, let doc = NSDocumentController.shared.documents.compactMap({ $0 as? PDFDoc }).first(where: { !before.contains(ObjectIdentifier($0)) }), doc.fileURL == nil, doc.pdf.pageCount == 1 else {
            print("FAIL PDF printer: import controls, new unsaved document, and page count", dialog, opened); return false
        }
        defer { doc.close() }
        do {
            let bytes = try doc.data(ofType: "com.adobe.pdf"); try bytes.write(to: saved, options: .atomic)
            guard let reopened = PDFDocument(url: saved), reopened.pageCount == 1, let page = reopened.page(at: 0), abs(page.bounds(for: .mediaBox).width * 25.4 / 72 * 8 - 9) < 0.02 else { print("FAIL PDF printer: saved/reopened geometry"); return false }
        } catch { print("FAIL PDF printer: saved/reopened data", error); return false }
        guard let editor = doc.editor else { print("FAIL PDF printer: missing imported editor"); return false }
        let sourceBefore = try? Data(contentsOf: saved)
        var choseExport = false, savedJob = false
        var saveReady: Date?
        let exportDeadline = Date(timeIntervalSinceNow: 6)
        let exportTimer = Timer(timeInterval: 0.05, repeats: true) { timer in
            if Date() > exportDeadline { timer.invalidate(); NSApp.windows.compactMap { $0 as? NSSavePanel }.first(where: { $0.isVisible })?.cancel(nil); NSApp.abortModal(); return }
            if !savedJob, let panel = NSApp.windows.compactMap({ $0 as? NSSavePanel }).first(where: { $0.isVisible && !($0 is NSOpenPanel) }) {
                if saveReady == nil {
                    panel.directoryURL = exported.deletingLastPathComponent(); panel.nameFieldStringValue = exported.lastPathComponent
                    saveReady = Date(timeIntervalSinceNow: 0.4)
                } else if let ready = saveReady, Date() >= ready {
                    savedJob = pressReturn(panel)
                }
            } else if !choseExport, let content = NSApp.modalWindow?.contentView,
                      let button = descendants(content).compactMap({ $0 as? NSButton }).first(where: { $0.title == "Continue" }) {
                choseExport = true; button.performClick(nil)
            }
        }
        RunLoop.current.add(exportTimer, forMode: .modalPanel); RunLoop.current.add(exportTimer, forMode: .common)
        _ = NSApp.sendAction(#selector(EditorWindowController.exportPrinterFileAction(_:)), to: editor, from: nil)
        while Date() < exportDeadline && !FileManager.default.fileExists(atPath: exported.path) { try? await Task.sleep(nanoseconds: 50_000_000) }
        exportTimer.invalidate()
        let expected = Data("^XA^MNY^PW9^LL2^LH0,0^FO0,0^GFA,4,4,2,80804000^FS^XZ".utf8)
        guard choseExport, savedJob, (try? Data(contentsOf: exported)) == expected, sourceBefore == (try? Data(contentsOf: saved)) else {
            print("FAIL PDF printer: export controls, exact raster, and unchanged source", choseExport, savedJob); return false
        }
        print("PDF printer UI regressions: 4/4 passed")
        return true
    }
}
