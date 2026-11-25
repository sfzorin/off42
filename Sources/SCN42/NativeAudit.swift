import AppKit
import Kit42

@MainActor enum ScanNativeAudit {
    static func run(_ scenario: String) async {
        setlinebuf(stdout)
        if scenario == "vision" { exit(await ScanVisionAudit.run() == 0 ? 0 : 1) }
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("SCN42-audit-" + UUID().uuidString)
        setenv("K42_SCAN_DIR", folder.path, 1)
        unsetenv("K42_SNAPSHOT")
        var passed = false
        do {
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            guard let owner = AppDelegate.shared?.newWindow(), let window = owner.window else { throw CocoaError(.coderInvalidValue) }
            NSApp.activate(ignoringOtherApps: true); window.makeKeyAndOrderFront(nil)
            guard await UITest.wait({ window.isKeyWindow }) else { throw CocoaError(.coderInvalidValue) }
            switch scenario {
            case "core": passed = await owner.auditCore(folder: folder)
            case "scanner": passed = await owner.auditScanner(folder: folder)
            case "editing": passed = await owner.auditEditing()
            case "files": passed = await owner.auditFiles()
            case "marks": passed = await owner.auditMarks()
            default: print("FAIL SCN42 audit: unknown scenario")
            }
            UITest.capture(window, "scn-" + scenario)
            for controller in AppDelegate.shared?.windows ?? [] { controller.window?.close() }
        } catch { print("FAIL SCN42 audit fixture:", error) }
        try? FileManager.default.removeItem(at: folder)
        exit(passed ? 0 : 1)
    }
}

/// A controllable device for exercising callback ownership and settings without touching hardware.
final class AuditScanner: ScannerBackend {
    var id = UUID().uuidString
    var name = "Audit Scanner"
    var connection = "Virtual"
    var sources: [ScanSource] = [.flatbed, .feeder]
    var supportsDuplex: Bool { prepared == .feeder }
    var resolutions = [75, 173, 300]
    var glassSize = CGSize(width: 8, height: 10)
    var status: ScannerStatus = .ready
    var onStatus: ((ScannerStatus) -> Void)?
    var prepared: ScanSource = .flatbed
    var requests: [ScanRequest] = []
    var page: ((RawScan) -> Void)?
    var done: ((Error?) -> Void)?
    var overviewDone: ((Result<CGImage, Error>) -> Void)?
    var cancellationCount = 0
    var deferPreparation = false
    var preparations: [(ScanSource, (Error?) -> Void)] = []
    func open() {}
    func close() { status = .offline }
    func prepare(source: ScanSource, completion: @escaping (Error?) -> Void) {
        if deferPreparation { preparations.append((source, completion)); return }
        prepared = source; completion(nil)
    }
    func finishPreparation() {
        guard !preparations.isEmpty else { return }
        let (source, completion) = preparations.removeFirst()
        prepared = source; completion(nil)
    }
    func overview(_ completion: @escaping (Result<CGImage, Error>) -> Void) { overviewDone = completion }
    func scan(_ request: ScanRequest, page: @escaping (RawScan) -> Void, done: @escaping (Error?) -> Void) {
        requests.append(request); self.page = page; self.done = done
    }
    func cancel() { cancellationCount += 1 }
}
