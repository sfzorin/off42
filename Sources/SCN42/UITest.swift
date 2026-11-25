import AppKit
import Kit42

/// `SCN42 --uitest`: real controller and sheets with an in-memory scanner; no device discovery.
@MainActor enum UITest {
    static func wait(_ condition: () -> Bool, seconds: Double = 10) async -> Bool {
        let deadline = Date().addingTimeInterval(seconds)
        while !condition(), Date() < deadline { try? await Task.sleep(nanoseconds: 20_000_000) }
        return condition()
    }

    static func capture(_ window: NSWindow, _ name: String) {
        guard let directory = ProcessInfo.processInfo.environment["K42_UI_TEST_DIR"],
              let frame = window.contentView?.superview,
              let bitmap = frame.bitmapImageRepForCachingDisplay(in: frame.bounds) else { return }
        frame.cacheDisplay(in: frame.bounds, to: bitmap)
        let folder = URL(fileURLWithPath: directory)
        try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        try? bitmap.representation(using: .png, properties: [:])?.write(to: folder.appendingPathComponent(name + ".png"))
    }

    static func run() async {
        setlinebuf(stdout)
        var failed = 0
        func check(_ name: String, _ ok: Bool) {
            print("\(ok ? "PASS" : "FAIL") \(name)")
            if !ok { failed += 1 }
        }
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("SCN42-uitest-\(UUID().uuidString)")
        setenv("K42_SCAN_DIR", folder.path, 1)
        unsetenv("K42_SNAPSHOT") // Close must exercise the normal unsaved-changes dialog.
        do {
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            guard let image = Bitmap(width: 240, height: 320, channels: 1, fill: 240).cgImage() else { throw CocoaError(.fileWriteUnknown) }
            let w = AppDelegate.shared!.newWindow()
            guard let window = w.window else { throw CocoaError(.fileReadUnknown) }
            var settings = ScanSettings()
            settings.autoCrop = false; settings.deskew = false; settings.autoRotate = false
            settings.enhancement = .none; settings.removeBlank = false
            settings.resolution = 100
            w.settingsPanel.apply(settings)
            check("signatures and stamps print into page pixels (see SCN marks lines)", await w.auditMarks())
            func page() -> ScanPage {
                ScanPage(base: image, image: image, dpi: 100, options: settings.processing, report: ProcessReport())
            }
            let first = page(), second = page()
            w.doc.append([first, second])
            w.pageList.select([0])
            let scanner = ImmediateScanner(image: image)
            w.choose(scanner)
            w.rescanPageAction(nil)
            w.pageList.select([1])
            w.rescanPageAction(nil)
            check("rescan cannot replace an in-flight target", scanner.requests == 1)
            check("rescan replaces selected page after processing", await wait { !w.isScanning && w.pendingProcessing == 0 }
                && w.pages.count == 2 && w.pages.first !== first && w.pages.last === second)
            capture(window, "scn-rescan")

            let beforeDrop = w.pages.map(\.id)
            let input = folder.appendingPathComponent("drop.png")
            var imageOptions = SaveOptions(); imageOptions.format = .png
            try Exporter.data([first.exportPage], imageOptions).write(to: input)
            w.importFiles([input], at: 0)
            check("drop before first page inserts at beginning", await wait { w.pendingProcessing == 0 }
                && w.pages.count == 3 && Array(w.pages.dropFirst().map(\.id)) == beforeDrop)
            capture(window, "scn-drop-first")

            window.performClose(nil)
            check("close opens unsaved-changes sheet", await wait { window.attachedSheet != nil })
            if let sheet = window.attachedSheet { window.endSheet(sheet, returnCode: .alertThirdButtonReturn) }
            _ = await wait { window.attachedSheet == nil }
            check("Cancel close keeps document open", window.isVisible && w.doc.isDirty)

            var save = SaveOptions()
            save.folder = folder.path; save.openAfter = .none; save.ocr = false
            save.nameTemplate = "close-regression"
            Store.saveOptions = save
            func closeWithSave(beforeSave: (() -> Void)? = nil) async {
                window.performClose(nil)
                _ = await wait { window.attachedSheet != nil }
                if let sheet = window.attachedSheet { window.endSheet(sheet, returnCode: .alertFirstButtonReturn) }
                check("Save in close prompt opens save sheet", await wait { w.saveSheet?.window?.sheetParent === window })
                if let sheet = w.saveSheet, let sheetWindow = sheet.window {
                    capture(sheetWindow, "scn-close-save")
                    func buttons(_ view: NSView) -> [NSButton] {
                        (view as? NSButton).map { [$0] } ?? view.subviews.flatMap(buttons)
                    }
                    let saveButton = sheetWindow.contentView.flatMap { buttons($0).first { $0.title == L("Save") } }
                    check("save sheet has actionable Save button", saveButton != nil)
                    beforeSave?()
                    saveButton?.performClick(nil)
                }
            }
            // Keep a real scan queued until the first save's completion has run.
            let processingGate = DispatchSemaphore(value: 0)
            defer { processingGate.signal() }
            w.processQueue.async { processingGate.wait() }
            await closeWithSave {
                w.process(RawScan(image: image, dpi: 100), settings.processing, removeBlank: false)
            }
            _ = await wait { w.savedURL != nil && w.busyText == nil }
            check("save does not close over a pending scan", window.isVisible && w.pendingProcessing > 0)
            window.performClose(nil)
            check("repeated close preserves pending scan", window.isVisible && window.attachedSheet == nil)
            processingGate.signal()
            check("queued page remains available after save", await wait { w.pendingProcessing == 0 }
                && w.pages.count == 4 && w.doc.isDirty)
            await closeWithSave()
            check("successful save closes the requested window", await wait { !window.isVisible })
            check("closed document exists on disk", w.savedURL.map { FileManager.default.fileExists(atPath: $0.path) } ?? false)
            check("closed document is clean", !w.doc.isDirty)
            if window.isVisible { window.close() }
        } catch {
            print("FAIL SCN42 UI fixture: \(error)")
            failed += 1
        }
        try? FileManager.default.removeItem(at: folder)
        print("SCN42 UI: \(failed) failures")
        exit(failed == 0 ? 0 : 1)
    }
}

/// Delivers an image and completion in the same callback, before the real processing queue can finish.
private final class ImmediateScanner: ScannerBackend {
    let image: CGImage
    init(image: CGImage) { self.image = image }
    var id: String { "uitest:memory" }
    var name: String { "UI Test Scanner" }
    var connection: String { "Virtual" }
    var sources: [ScanSource] { [.flatbed] }
    var supportsDuplex: Bool { false }
    var resolutions: [Int] { [100] }
    var glassSize: CGSize { CGSize(width: 2.4, height: 3.2) }
    var status: ScannerStatus = .ready
    private(set) var requests = 0
    var onStatus: ((ScannerStatus) -> Void)?
    func open() { status = .ready }
    func close() { status = .offline }
    func cancel() {}
    func overview(_ completion: @escaping (Result<CGImage, Error>) -> Void) { completion(.success(image)) }
    func scan(_ request: ScanRequest, page: @escaping (RawScan) -> Void, done: @escaping (Error?) -> Void) {
        requests += 1
        page(RawScan(image: image, dpi: 100))
        done(nil)
    }
}
