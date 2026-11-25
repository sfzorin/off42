import AppKit
import ImageCaptureCore
import Kit42
import UniformTypeIdentifiers

/// What a scanner is doing, in words a person understands.
enum ScannerStatus: Equatable {
    case connecting
    case ready
    case warmingUp
    case busy
    case scanning(Double)
    case error(String)
    case offline

    var text: String {
        switch self {
        case .connecting: return L("Connecting to the scanner…")
        case .ready: return L("Ready")
        case .warmingUp: return L("The scanner is warming up…")
        case .busy: return L("The scanner is busy")
        case .scanning(let p): return p.isFinite && p > 0 ? L("Scanning… %d%%", Int(min(100, p))) : L("Scanning…")
        case .error(let s): return s
        case .offline: return L("The scanner is not available")
        }
    }

    var canScan: Bool { self == .ready }

    static func afterWarmUp(selectingSource: Bool, acquiring: Bool) -> ScannerStatus {
        if selectingSource { return .busy }
        return acquiring ? .scanning(0) : .ready
    }
}

/// One scan job: where, how big, how fine.
struct ScanRequest {
    var source: ScanSource
    var resolution: Int
    var colorMode: ColorMode
    var bitDepth: Int
    /// Area on the glass, normalised 0…1 with the origin top-left (flatbed only).
    var area: CGRect
    var paper: CGSize?
    var duplex: Bool
    /// The last area of a job (the virtual scanner then moves on to its next glass image).
    var lastInJob = true
}

/// A scanned image with its real resolution.
struct RawScan {
    var image: CGImage
    var dpi: Double
}

protocol ScannerBackend: AnyObject {
    var id: String { get }
    var name: String { get }
    /// "USB", "Network", … for the picker.
    var connection: String { get }
    var sources: [ScanSource] { get }
    var supportsDuplex: Bool { get }
    var resolutions: [Int] { get }
    /// Glass size in inches (for areas and paper sizes).
    var glassSize: CGSize { get }
    var status: ScannerStatus { get }
    var onStatus: ((ScannerStatus) -> Void)? { get set }

    func open()
    func close()
    func prepare(source: ScanSource, completion: @escaping (Error?) -> Void)
    func overview(_ completion: @escaping (Result<CGImage, Error>) -> Void)
    /// `page` is called for each page (a feeder sends several), then `done` once.
    func scan(_ request: ScanRequest, page: @escaping (RawScan) -> Void, done: @escaping (Error?) -> Void)
    func cancel()
}

extension ScannerBackend {
    func prepare(source: ScanSource, completion: @escaping (Error?) -> Void) { completion(nil) }
}

struct ScanError: LocalizedError {
    let message: String
    var errorDescription: String? { message }

    /// Image Capture error codes in plain language.
    static func describe(_ error: Error) -> String {
        let code = (error as NSError).code
        switch code {
        case -9924, -9937: return L("The scan was cancelled.")
        case -9925, -9926: return L("Someone else is using the scanner. Try again when they are done.")
        case -9923, -9900: return L("The scanner does not answer. Check that it is switched on and connected.")
        case -9927, -21345, -21344, -9901, -9902: return L("Could not connect to the scanner. Switch it off and on, then choose it again.")
        case -21343: return L("macOS does not allow SCN42 to use this scanner.")
        case -9945, -9950: return L("This scanner needs a driver from its maker.")
        case -9946: return L("The scanner's software is being installed. Try again in a minute.")
        case -9953: return L("The scanner asks for a user name and password.")
        case -9929: return L("This part of the scanner (glass or feeder) is not available.")
        case -9930, -9931, -9955, -9956, -21350, -21349:
            return L("The scan failed. If the feeder is empty or jammed, fix it and press Scan again.")
        default:
            return (error as NSError).localizedDescription
        }
    }
}

// MARK: - Device discovery

/// Finds scanners: Image Capture (USB, network/Bonjour, AirScan/eSCL) or, with `K42_FAKE_SCANNER=dir`, only the
/// virtual scanner that "scans" the images in that folder (tests, demos — no real hardware is touched).
final class DeviceManager: NSObject, ICDeviceBrowserDelegate {
    static let shared = DeviceManager()
    private(set) var scanners: [ScannerBackend] = []
    private(set) var finishedLocalSearch = false
    private var browser: ICDeviceBrowser?
    /// Posted when scanners come or go (object: nil) and when one's status changes (object: the scanner).
    static let changed = Notification.Name("SCN42.devicesChanged")
    static let statusChanged = Notification.Name("SCN42.scannerStatus")

    private func onChange() { NotificationCenter.default.post(name: Self.changed, object: nil) }

    private func add(_ s: ScannerBackend) {
        s.onStatus = { [weak s] _ in if let s { NotificationCenter.default.post(name: DeviceManager.statusChanged, object: s) } }
        scanners.append(s)
    }

    func start() {
        guard browser == nil, scanners.isEmpty else { return }
        if let dir = ProcessInfo.processInfo.environment["K42_FAKE_SCANNER"] {
            add(FakeScanner(folder: URL(fileURLWithPath: dir)))
            finishedLocalSearch = true
            onChange()
            return
        }
        let b = ICDeviceBrowser()
        b.delegate = self
        let mask = ICDeviceTypeMask.scanner.rawValue | ICDeviceLocationTypeMask.local.rawValue | ICDeviceLocationTypeMask.shared.rawValue
            | ICDeviceLocationTypeMask.bonjour.rawValue | ICDeviceLocationTypeMask.bluetooth.rawValue | ICDeviceLocationTypeMask.remote.rawValue
        b.browsedDeviceTypeMask = ICDeviceTypeMask(rawValue: mask) ?? .scanner
        browser = b
        b.start()
        // Network scanners can take a while; stop calling it "searching" after a few seconds anyway.
        DispatchQueue.main.asyncAfter(deadline: .now() + 6) { [weak self] in
            guard let self, !self.finishedLocalSearch else { return }
            self.finishedLocalSearch = true
            self.onChange()
        }
    }

    func stop() {
        browser?.stop()
        browser = nil
        scanners.forEach { $0.close() }
    }

    func deviceBrowser(_ browser: ICDeviceBrowser, didAdd device: ICDevice, moreComing: Bool) {
        guard let scanner = device as? ICScannerDevice, !scanners.contains(where: { $0.id == (scanner.uuidString ?? scanner.name ?? "") }) else { return }
        add(ICScanner(device: scanner))
        if !moreComing { onChange() }
    }

    func deviceBrowser(_ browser: ICDeviceBrowser, didRemove device: ICDevice, moreGoing: Bool) {
        scanners.filter { ($0 as? ICScanner)?.device === device }.forEach { $0.close() }
        scanners.removeAll { ($0 as? ICScanner)?.device === device }
        if !moreGoing { onChange() }
    }

    func deviceBrowserDidEnumerateLocalDevices(_ browser: ICDeviceBrowser) {
        finishedLocalSearch = true
        onChange()
    }
}

// MARK: - Image Capture scanner

/// A real scanner through ImageCaptureCore. The session is opened when the scanner is chosen;
/// each scan selects the functional unit (flatbed / feeder / film), sets it up and saves pages as TIFF files.
final class ICScanner: NSObject, ScannerBackend, ICScannerDeviceDelegate {
    let device: ICScannerDevice
    var onStatus: ((ScannerStatus) -> Void)?
    private(set) var status: ScannerStatus = .offline { didSet { if status != oldValue { onStatus?(status) } } }
    private var pendingSelect: [(Error?) -> Void] = []
    private var overviewDone: ((Result<CGImage, Error>) -> Void)?
    private var scanPage: ((RawScan) -> Void)?
    private var scanDone: ((Error?) -> Void)?
    private var scanDPI = 300.0
    private var requestGeneration = UUID()
    private var progressTimer: Timer?
    private let folder = FileManager.default.temporaryDirectory.appendingPathComponent("SCN42-\(UUID().uuidString)", isDirectory: true)

    init(device: ICScannerDevice) {
        self.device = device
        super.init()
    }

    var id: String { device.uuidString ?? device.name ?? "scanner" }
    var name: String { device.name ?? L("Scanner") }
    var connection: String {
        let t = device.type.rawValue
        if t & ICDeviceLocationTypeMask.local.rawValue != 0 { return device.transportType == ICDeviceTransport.transportTypeUSB.rawValue ? "USB" : L("Local") }
        if t & ICDeviceLocationTypeMask.bonjour.rawValue != 0 { return L("Network") }
        if t & ICDeviceLocationTypeMask.shared.rawValue != 0 { return L("Shared") }
        if t & ICDeviceLocationTypeMask.bluetooth.rawValue != 0 { return "Bluetooth" }
        return L("Network")
    }

    var sources: [ScanSource] {
        var s: [ScanSource] = []
        for n in device.availableFunctionalUnitTypes {
            switch ICScannerFunctionalUnitType(rawValue: n.uintValue) {
            case .flatbed: s.append(.flatbed)
            case .documentFeeder: s.append(.feeder)
            case .positiveTransparency, .negativeTransparency: if !s.contains(.transparency) { s.append(.transparency) }
            default: break
            }
        }
        return s.isEmpty ? [.flatbed] : s
    }

    var supportsDuplex: Bool { (device.selectedFunctionalUnit as? ICScannerFunctionalUnitDocumentFeeder)?.supportsDuplexScanning ?? false }

    var resolutions: [Int] {
        guard device.hasOpenSession else { return ScanSettings.resolutions }
        let fu = device.selectedFunctionalUnit
        let preferred = fu.preferredResolutions.map { $0 }
        let all = preferred.isEmpty ? fu.supportedResolutions.map { $0 } : preferred
        let r = all.filter { $0 >= 50 && $0 <= 4800 }
        return r.isEmpty ? ScanSettings.resolutions : r
    }

    var glassSize: CGSize {
        guard device.hasOpenSession else { return CGSize(width: 8.5, height: 11.69) }
        let fu = device.selectedFunctionalUnit
        let size = fu.physicalSize
        let perInch = unitsPerInch(fu.measurementUnit)
        return size.width > 0 ? CGSize(width: size.width / perInch, height: size.height / perInch) : CGSize(width: 8.5, height: 11.69)
    }

    private func unitsPerInch(_ u: ICScannerMeasurementUnit) -> CGFloat {
        switch u {
        case .inches: return 1
        case .centimeters: return 2.54
        case .picas: return 6
        case .points: return 72
        case .twips: return 1440
        case .pixels: return CGFloat(max(1, device.selectedFunctionalUnit.resolution))
        @unknown default: return 1
        }
    }

    func open() {
        guard !device.hasOpenSession else {
            if status == .offline || status == .connecting { status = .ready }
            return
        }
        status = .connecting
        device.delegate = self
        device.requestOpenSession()
    }

    func close() {
        progressTimer?.invalidate()
        finishPending(ScanError(message: L("The scanner is not available")))
        status = .offline
        if device.hasOpenSession { device.requestCloseSession() }
        try? FileManager.default.removeItem(at: folder)
    }

    private func select(_ source: ScanSource, _ completion: @escaping (Error?) -> Void) {
        let type: ICScannerFunctionalUnitType
        switch source {
        case .flatbed: type = .flatbed
        case .feeder: type = .documentFeeder
        case .transparency:
            type = device.availableFunctionalUnitTypes.contains(NSNumber(value: ICScannerFunctionalUnitType.positiveTransparency.rawValue)) ? .positiveTransparency : .negativeTransparency
        }
        if device.selectedFunctionalUnit.type == type { completion(nil); return }
        pendingSelect.append(completion)
        device.requestSelect(type)
    }

    func prepare(source: ScanSource, completion: @escaping (Error?) -> Void) {
        guard status.canScan, sources.contains(source) else {
            completion(ScanError(message: status.text)); return
        }
        status = .busy
        select(source) { [weak self] error in
            self?.status = error.map { .error(ScanError.describe($0)) } ?? .ready
            completion(error)
        }
    }

    private func finishPending(_ error: Error) {
        progressTimer?.invalidate()
        let selection = pendingSelect, overview = overviewDone, scan = scanDone
        pendingSelect = []; overviewDone = nil; scanDone = nil; scanPage = nil
        selection.forEach { $0(error) }
        overview?(.failure(error)); scan?(error)
    }

    func overview(_ completion: @escaping (Result<CGImage, Error>) -> Void) {
        guard status.canScan else { completion(.failure(ScanError(message: status.text))); return }
        requestGeneration = UUID(); let token = requestGeneration
        status = .busy
        select(.flatbed) { [self] error in
            if let error { status = .error(ScanError.describe(error)); completion(.failure(error)); return }
            guard requestGeneration == token else { completion(.failure(ScanError(message: L("The scan was cancelled.")))); return }
            let fu = device.selectedFunctionalUnit
            guard fu.canPerformOverviewScan else {
                status = .ready
                completion(.failure(ScanError(message: L("This scanner cannot make an overview scan."))))
                return
            }
            fu.overviewResolution = fu.supportedResolutions.integerGreaterThanOrEqualTo(75) ?? fu.overviewResolution
            overviewDone = completion
            status = .scanning(0)
            startProgress()
            device.requestOverviewScan()
        }
    }

    func scan(_ request: ScanRequest, page: @escaping (RawScan) -> Void, done: @escaping (Error?) -> Void) {
        guard status.canScan else { done(ScanError(message: status.text)); return }
        guard (50...4800).contains(request.resolution), sources.contains(request.source),
              request.area.minX.isFinite, request.area.minY.isFinite, request.area.width.isFinite, request.area.height.isFinite,
              request.area.width > 0, request.area.height > 0 else { done(CocoaError(.coderInvalidValue)); return }
        requestGeneration = UUID(); let token = requestGeneration
        status = .busy
        select(request.source) { [self] error in
            if let error { status = .error(ScanError.describe(error)); done(error); return }
            guard requestGeneration == token else { done(ScanError(message: L("The scan was cancelled."))); return }
            let fu = device.selectedFunctionalUnit
            if fu.supportedMeasurementUnits.contains(Int(ICScannerMeasurementUnit.inches.rawValue)) { fu.measurementUnit = .inches }
            let wanted = request.resolution
            let supported = fu.supportedResolutions
            let res = supported.contains(wanted) ? wanted : (supported.integerGreaterThanOrEqualTo(wanted) ?? supported.last ?? wanted)
            fu.resolution = res
            scanDPI = Double(res)
            // B&W is made from gray by SCN42's adaptive threshold: much better than most scanners' fixed one.
            fu.pixelDataType = request.colorMode == .color ? .RGB : .gray
            let depth: ICScannerBitDepth = request.bitDepth == 16 && fu.supportedBitDepths.contains(16) ? .depth16Bits : .depth8Bits
            fu.bitDepth = depth
            if let feeder = fu as? ICScannerFunctionalUnitDocumentFeeder {
                if let type = documentType(request.paper), feeder.supportedDocumentTypes.contains(Int(type.rawValue)) { feeder.documentType = type }
                if feeder.supportsDuplexScanning { feeder.duplexScanningEnabled = request.duplex }
            } else {
                let size = fu.physicalSize
                let a = request.area
                fu.scanArea = NSRect(x: a.minX * size.width, y: a.minY * size.height, width: a.width * size.width, height: a.height * size.height)
            }
            try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            device.transferMode = .fileBased
            device.downloadsDirectory = folder
            device.documentName = "scan-\(UUID().uuidString.prefix(8))"
            device.documentUTI = UTType.tiff.identifier
            scanPage = page
            scanDone = done
            status = .scanning(0)
            startProgress()
            device.requestScan()
        }
    }

    func cancel() {
        requestGeneration = UUID()
        device.cancelScan()
        finishPending(ScanError(message: L("The scan was cancelled.")))
    }

    private func documentType(_ paper: CGSize?) -> ICScannerDocumentType? {
        guard let paper else { return nil }
        let table: [(CGSize, ICScannerDocumentType)] = [
            (CGSize(width: 8.27, height: 11.69), .typeA4), (CGSize(width: 8.5, height: 11), .typeUSLetter),
            (CGSize(width: 8.5, height: 14), .typeUSLegal), (CGSize(width: 5.83, height: 8.27), .typeA5),
            (CGSize(width: 4.13, height: 5.83), .typeA6), (CGSize(width: 3.5, height: 2), .typeBusinessCard),
        ]
        return table.first { abs($0.0.width - paper.width) < 0.05 && abs($0.0.height - paper.height) < 0.05 }?.1
    }

    private func startProgress() {
        progressTimer?.invalidate()
        progressTimer = Timer.scheduledTimer(withTimeInterval: 0.5, repeats: true) { [weak self] _ in
            guard let self, case .scanning = self.status else { return }
            self.status = .scanning(Double(self.device.selectedFunctionalUnit.scanProgressPercentDone))
        }
    }

    // ICDeviceDelegate

    func device(_ device: ICDevice, didOpenSessionWithError error: Error?) {
        if let error { status = .error(ScanError.describe(error)) } else { status = .ready }
    }

    func device(_ device: ICDevice, didCloseSessionWithError error: Error?) {
        status = .offline; finishPending(error ?? ScanError(message: L("The scanner is not available")))
    }
    func didRemove(_ device: ICDevice) {
        status = .offline; finishPending(ScanError(message: L("The scanner is not available")))
    }
    func deviceDidBecomeReady(_ device: ICDevice) { if status == .connecting { status = .ready } }

    func device(_ device: ICDevice, didEncounterError error: Error?) {
        if let error { status = .error(ScanError.describe(error)); finishPending(error) }
    }

    func device(_ device: ICDevice, didReceiveStatusInformation info: [ICDeviceStatus: Any]) {
        guard let note = info[.statusNotificationKey] as? String else { return }
        if note == ICScannerStatusWarmingUp { status = .warmingUp }
        else if note == ICScannerStatusWarmUpDone {
            guard device.hasOpenSession else { return }
            status = .afterWarmUp(selectingSource: !pendingSelect.isEmpty, acquiring: scanDone != nil || overviewDone != nil)
        }
    }

    // ICScannerDeviceDelegate

    func scannerDevice(_ scanner: ICScannerDevice, didSelect functionalUnit: ICScannerFunctionalUnit, error: Error?) {
        let waiting = pendingSelect
        pendingSelect = []
        waiting.forEach { $0(error) }
        if waiting.isEmpty, scanDone == nil, overviewDone == nil, device.hasOpenSession {
            status = error.map { .error(ScanError.describe($0)) } ?? .ready
        }
    }

    func scannerDevice(_ scanner: ICScannerDevice, didScanTo url: URL) {
        defer { try? FileManager.default.removeItem(at: url) }
        guard let src = CGImageSourceCreateWithURL(url as CFURL, nil), let image = CGImageSourceCreateImageAtIndex(src, 0, [kCGImageSourceShouldCache: true] as CFDictionary) else {
            let error = CocoaError(.fileReadCorruptFile)
            status = .error(error.localizedDescription); finishPending(error); device.cancelScan(); return
        }
        // Decode now: the file is removed.
        let copy = Bitmap(image, gray: image.colorSpace?.model == .monochrome).cgImage() ?? image
        scanPage?(RawScan(image: copy, dpi: scanDPI))
    }

    func scannerDevice(_ scanner: ICScannerDevice, didCompleteOverviewScanWithError error: Error?) {
        progressTimer?.invalidate()
        status = .ready
        let done = overviewDone
        overviewDone = nil
        if let error { done?(.failure(ScanError(message: ScanError.describe(error)))) }
        else if let image = scanner.selectedFunctionalUnit.overviewImage { done?(.success(image)) }
        else { done?(.failure(ScanError(message: L("The scanner sent no overview image.")))) }
    }

    func scannerDevice(_ scanner: ICScannerDevice, didCompleteScanWithError error: Error?) {
        progressTimer?.invalidate()
        status = .ready
        let done = scanDone
        scanDone = nil
        scanPage = nil
        done?(error.map { ScanError(message: ScanError.describe($0)) })
    }
}

// MARK: - Virtual scanner

/// Scans images from a folder as if they lay on a glass or in a feeder. `glass*` files are the flatbed
/// (one per scan, in turn), `feed*`/`page*` files fill the feeder; with neither, every image is both.
/// Image resolution comes from the file (DPI) or 200 dpi. Simulates warm-up (`K42_FAKE_WARMUP`), the time a scan takes,
/// areas, resolution, colour mode and duplex.
final class FakeScanner: ScannerBackend {
    let folder: URL
    var onStatus: ((ScannerStatus) -> Void)?
    private(set) var status: ScannerStatus = .offline { didSet { if status != oldValue { onStatus?(status) } } }
    private var glass: [URL] = []
    private var feeder: [URL] = []
    private var glassIndex = 0
    private var warmedUp = false
    private var generation = UUID()
    private var overviewDone: ((Result<CGImage, Error>) -> Void)?
    private var scanDone: ((Error?) -> Void)?
    /// Seconds per page (`K42_FAKE_SPEED`).
    private let pageTime: Double = {
        let t = Double(ProcessInfo.processInfo.environment["K42_FAKE_SPEED"] ?? "") ?? 0.35
        return t.isFinite ? min(30, max(0, t)) : 0.35
    }()

    init(folder: URL) {
        self.folder = folder
        let files = ((try? FileManager.default.contentsOfDirectory(at: folder, includingPropertiesForKeys: nil)) ?? [])
            .filter { ["png", "jpg", "jpeg", "tif", "tiff", "heic"].contains($0.pathExtension.lowercased()) }
            .sorted { $0.lastPathComponent.localizedStandardCompare($1.lastPathComponent) == .orderedAscending }
        glass = files.filter { $0.lastPathComponent.lowercased().hasPrefix("glass") }
        feeder = files.filter { let n = $0.lastPathComponent.lowercased(); return n.hasPrefix("feed") || n.hasPrefix("page") }
        if glass.isEmpty && feeder.isEmpty { glass = files; feeder = files }
        if let start = ProcessInfo.processInfo.environment["K42_FAKE_GLASS"].flatMap(Int.init) { glassIndex = max(0, start) % max(1, glass.count) }
    }

    var id: String { "virtual:" + folder.path }
    var name: String { L("Virtual Scanner") }
    var connection: String { L("Virtual") }
    var sources: [ScanSource] {
        var s: [ScanSource] = []
        if !glass.isEmpty { s.append(.flatbed) }
        if !feeder.isEmpty { s.append(.feeder) }
        return s
    }
    var supportsDuplex: Bool { true }
    var resolutions: [Int] { [75, 100, 150, 200, 300, 400, 600, 1200] }

    private var currentGlass: URL? { glass.isEmpty ? nil : glass[glassIndex % glass.count] }

    private static func load(_ url: URL) -> RawScan? {
        guard let src = CGImageSourceCreateWithURL(url as CFURL, nil), let image = CGImageSourceCreateImageAtIndex(src, 0, nil) else { return nil }
        let props = CGImageSourceCopyPropertiesAtIndex(src, 0, nil) as? [CFString: Any]
        let dpi = (props?[kCGImagePropertyDPIWidth] as? Double).flatMap { $0.isFinite && $0 > 10 && abs($0 - 72) > 0.5 ? $0 : nil } ?? 200
        return RawScan(image: image, dpi: dpi)
    }

    var glassSize: CGSize {
        guard let url = currentGlass, let s = FakeScanner.load(url) else { return CGSize(width: 8.5, height: 11.69) }
        return CGSize(width: Double(s.image.width) / s.dpi, height: Double(s.image.height) / s.dpi)
    }

    func open() {
        guard status == .offline else { return }
        generation = UUID()
        let token = generation
        status = .connecting
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) {
            guard self.generation == token, self.status == .connecting else { return }
            self.status = .ready
        }
    }

    func close() { cancel(); generation = UUID(); status = .offline }

    private func warmUp(_ token: UUID, _ then: @escaping () -> Void) {
        guard !warmedUp, ProcessInfo.processInfo.environment["K42_FAKE_WARMUP"] != nil else { warmedUp = true; then(); return }
        warmedUp = true
        status = .warmingUp
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) { if self.generation == token { then() } }
    }

    func overview(_ completion: @escaping (Result<CGImage, Error>) -> Void) {
        guard status.canScan else { completion(.failure(ScanError(message: status.text))); return }
        guard let url = currentGlass, let raw = FakeScanner.load(url) else {
            completion(.failure(ScanError(message: L("This part of the scanner (glass or feeder) is not available."))))
            return
        }
        generation = UUID(); let token = generation
        overviewDone = completion
        warmUp(token) { [self] in
            status = .scanning(0)
            DispatchQueue.global(qos: .userInitiated).asyncAfter(deadline: .now() + pageTime) {
                let s = 75 / raw.dpi
                let img = Bitmap(raw.image, gray: false).resized(width: Int(Double(raw.image.width) * s), height: Int(Double(raw.image.height) * s)).cgImage()
                DispatchQueue.main.async {
                    guard self.generation == token else { return }
                    self.status = .ready
                    let done = self.overviewDone; self.overviewDone = nil
                    if let img { done?(.success(img)) } else { done?(.failure(ScanError(message: L("The scanner sent no overview image.")))) }
                }
            }
        }
    }

    func scan(_ request: ScanRequest, page: @escaping (RawScan) -> Void, done: @escaping (Error?) -> Void) {
        guard status.canScan else { done(ScanError(message: status.text)); return }
        guard (50...4800).contains(request.resolution), request.area.minX.isFinite, request.area.minY.isFinite,
              request.area.width.isFinite, request.area.height.isFinite, request.area.width > 0, request.area.height > 0 else {
            done(CocoaError(.coderInvalidValue)); return
        }
        let urls: [URL]
        switch request.source {
        case .feeder:
            if feeder.isEmpty { done(ScanError(message: L("The document feeder is empty. Put the pages in and press Scan."))); return }
            // Simplex scans every other file (the front sides); duplex scans them all.
            urls = request.duplex ? feeder : feeder.filter { !Self.isBackSide($0) }
        default:
            guard let g = currentGlass else { done(ScanError(message: L("This part of the scanner (glass or feeder) is not available."))); return }
            urls = [g]
        }
        generation = UUID(); let token = generation
        scanDone = done
        warmUp(token) { [self] in
            status = .scanning(0)
            let queue = DispatchQueue.global(qos: .userInitiated)
            func step(_ i: Int) {
                guard self.generation == token else { return }
                guard i < urls.count else {
                    self.status = .ready
                    if request.source == .flatbed && request.lastInJob { self.glassIndex = (self.glassIndex + 1) % max(1, self.glass.count) }
                    let completion = self.scanDone; self.scanDone = nil; completion?(nil)
                    return
                }
                queue.asyncAfter(deadline: .now() + pageTime) {
                    let result = FakeScanner.load(urls[i]).flatMap { self.simulate($0, request) }
                    DispatchQueue.main.async {
                        guard self.generation == token else { return }
                        guard let result else {
                            self.status = .ready
                            let completion = self.scanDone; self.scanDone = nil
                            completion?(CocoaError(.fileReadCorruptFile)); return
                        }
                        self.status = .scanning(Double(i + 1) / Double(urls.count) * 100)
                        page(result)
                        step(i + 1)
                    }
                }
            }
            step(0)
        }
    }

    /// Simplex skips back sides only when the files say they are backs (`feed-02b.png`).
    private static func isBackSide(_ url: URL) -> Bool { url.deletingPathExtension().lastPathComponent.lowercased().hasSuffix("b") }

    func cancel() {
        generation = UUID()
        let overview = overviewDone, scan = scanDone
        overviewDone = nil; scanDone = nil
        if status != .offline { status = .ready }
        let error = ScanError(message: L("The scan was cancelled."))
        overview?(.failure(error)); scan?(error)
    }

    /// Area crop, resampling to the requested resolution and the colour mode, like a scanner would deliver.
    private func simulate(_ raw: RawScan, _ r: ScanRequest) -> RawScan? {
        var image = raw.image
        if r.source == .flatbed {
            let w = CGFloat(image.width), h = CGFloat(image.height)
            let a = r.area.intersection(CGRect(x: 0, y: 0, width: 1, height: 1))
            guard !a.isNull, !a.isEmpty else { return nil }
            let px = CGRect(x: a.minX * w, y: a.minY * h, width: a.width * w, height: a.height * h).integral
            guard let c = image.cropping(to: px) else { return nil }; image = c
        }
        let s = Double(r.resolution) / raw.dpi
        let width = Double(image.width) * s, height = Double(image.height) * s
        guard width.isFinite, height.isFinite, width >= 1, height >= 1, width <= 32768, height <= 32768,
              width * height <= 256_000_000 else { return nil }
        let gray = r.colorMode != .color
        let bmp = Bitmap(image, gray: gray).resized(width: Int(width), height: Int(height))
        guard let out = bmp.cgImage() else { return nil }
        return RawScan(image: out, dpi: Double(r.resolution))
    }
}
