import AppKit
import Kit42

/// Where on the scanner the original lies.
enum ScanSource: Int, CaseIterable, Codable {
    case flatbed, feeder, transparency

    var title: String {
        switch self {
        case .flatbed: return L("Flatbed")
        case .feeder: return L("Document Feeder")
        case .transparency: return L("Transparency (film)")
        }
    }
    var symbol: String {
        switch self {
        case .flatbed: return "rectangle.portrait.on.rectangle.portrait"
        case .feeder: return "doc.on.doc"
        case .transparency: return "film"
        }
    }
}

enum ColorMode: Int, CaseIterable, Codable {
    case color, gray, bw

    var title: String {
        switch self {
        case .color: return L("In colour")
        case .gray: return L("Greyscale")
        case .bw: return L("Black & White")
        }
    }
}

/// Paper sizes in inches (portrait).
enum PaperSize: Int, CaseIterable, Codable {
    case auto, a4, letter, legal, a5, a6, businessCard, custom

    var title: String {
        switch self {
        case .auto: return L("Auto (whole glass)")
        case .a4: return "A4"
        case .letter: return L("US Letter")
        case .legal: return L("US Legal")
        case .a5: return "A5"
        case .a6: return "A6"
        case .businessCard: return L("Business card")
        case .custom: return L("Custom…")
        }
    }

    /// Width × height in inches; nil = whole glass.
    func inches(custom: CGSize) -> CGSize? {
        switch self {
        case .auto: return nil
        case .a4: return CGSize(width: 8.27, height: 11.69)
        case .letter: return CGSize(width: 8.5, height: 11)
        case .legal: return CGSize(width: 8.5, height: 14)
        case .a5: return CGSize(width: 5.83, height: 8.27)
        case .a6: return CGSize(width: 4.13, height: 5.83)
        case .businessCard: return CGSize(width: 3.5, height: 2.0)
        case .custom: return custom
        }
    }
}

/// What "Enhance" does after the scan.
enum Enhancement: Int, CaseIterable, Codable {
    case none, document, photo

    var title: String {
        switch self {
        case .none: return L("None")
        case .document: return L("Document (white paper, sharp text)")
        case .photo: return L("Photo (auto colour and contrast)")
        }
    }
}

/// Everything that decides how a scan is made and cleaned up.
struct ScanSettings: Codable, Equatable {
    var source: ScanSource = .flatbed
    var colorMode: ColorMode = .color
    var resolution: Int = 300
    var bitDepth: Int = 8
    var paper: PaperSize = .auto
    var customWidth: Double = 8.27
    var customHeight: Double = 11.69
    var duplex = false
    /// −1…1 (0 = unchanged).
    var brightness: Double = 0
    var contrast: Double = 0
    /// Black & white threshold 0…1 on the cleaned-up page (0.5 = automatic).
    var threshold: Double = 0.5
    // Processing
    var autoCrop = true
    var deskew = true
    var autoRotate = true
    var removeBlank = false
    var enhancement: Enhancement = .document
    var descreen = false
    var detectPhotos = false

    static let resolutions = [75, 100, 150, 200, 300, 400, 600, 1200]

    var customSize: CGSize { CGSize(width: customWidth, height: customHeight) }

    var normalized: ScanSettings {
        var s = self
        s.resolution = min(4800, max(50, resolution))
        s.bitDepth = bitDepth == 16 ? 16 : 8
        func finite(_ n: Double, _ fallback: Double, _ range: ClosedRange<Double>) -> Double {
            n.isFinite ? min(range.upperBound, max(range.lowerBound, n)) : fallback
        }
        s.customWidth = finite(customWidth, 8.27, (1 / 2.54)...(60 / 2.54))
        s.customHeight = finite(customHeight, 11.69, (1 / 2.54)...(60 / 2.54))
        s.brightness = finite(brightness, 0, -1...1)
        s.contrast = finite(contrast, 0, -1...1)
        s.threshold = finite(threshold, 0.5, 0.1...0.9)
        return s
    }

    var processing: ProcessOptions {
        var o = ProcessOptions()
        o.crop = autoCrop
        o.deskew = deskew
        o.autoRotate = autoRotate
        o.detectBlank = removeBlank
        o.enhancement = enhancement
        o.colorMode = colorMode
        o.brightness = brightness
        o.contrast = contrast
        o.threshold = threshold
        o.descreen = descreen
        return o
    }
}

/// A named group of settings. Built-in presets can't be deleted; user presets are saved in the defaults.
struct Preset: Codable, Equatable {
    enum Kind: Int, Codable { case document, photo, receipt, draft, user }
    var kind: Kind
    var userName: String?
    var settings: ScanSettings

    var name: String {
        switch kind {
        case .document: return L("Document")
        case .photo: return L("Photo")
        case .receipt: return L("Receipt")
        case .draft: return L("Draft")
        case .user: return userName ?? L("Custom")
        }
    }

    static var builtIn: [Preset] {
        var doc = ScanSettings()
        doc.colorMode = .color
        doc.resolution = 300
        doc.enhancement = .document

        var photo = ScanSettings()
        photo.colorMode = .color
        photo.resolution = 600
        photo.enhancement = .photo
        photo.autoRotate = false
        photo.detectPhotos = true

        var receipt = ScanSettings()
        receipt.colorMode = .gray
        receipt.resolution = 300
        receipt.enhancement = .document

        var draft = ScanSettings()
        draft.colorMode = .gray
        draft.resolution = 150
        draft.enhancement = .document

        return [Preset(kind: .document, settings: doc), Preset(kind: .photo, settings: photo),
                Preset(kind: .receipt, settings: receipt), Preset(kind: .draft, settings: draft)]
    }
}

/// Saved state: settings, user presets, last areas, last scanner, save options. One place for every key.
enum Store {
    static let defaults = UserDefaults.standard

    static func load<T: Decodable>(_ type: T.Type, _ key: String) -> T? {
        guard let data = defaults.data(forKey: key) else { return nil }
        return try? JSONDecoder().decode(T.self, from: data)
    }

    static func save<T: Encodable>(_ value: T, _ key: String) {
        if let data = try? JSONEncoder().encode(value) { defaults.set(data, forKey: key) }
    }

    static var settings: ScanSettings {
        get { (load(ScanSettings.self, "settings") ?? Preset.builtIn[0].settings).normalized }
        set { save(newValue.normalized, "settings") }
    }

    /// Index into `allPresets` the settings came from (−1 = changed by hand: "Custom").
    static var presetIndex: Int {
        get { defaults.object(forKey: "preset") as? Int ?? 0 }
        set { defaults.set(newValue, forKey: "preset") }
    }

    static var userPresets: [Preset] {
        get { load([Preset].self, "presets") ?? [] }
        set { save(newValue, "presets") }
    }

    static var allPresets: [Preset] { Preset.builtIn + userPresets }

    /// Scan areas on the glass, normalised (0…1, origin top-left).
    static var lastAreas: [CGRect] {
        get { load([CGRect].self, "areas") ?? [] }
        set { save(newValue, "areas") }
    }

    static var lastScanner: String? {
        get { defaults.string(forKey: "scanner") }
        set { defaults.set(newValue, forKey: "scanner") }
    }

    static var saveOptions: SaveOptions {
        get { load(SaveOptions.self, "save") ?? SaveOptions() }
        set { save(newValue, "save") }
    }

    /// `K42_SCAN_DIR` (tests) redirects the default destination and the history away from the user's folders.
    static var testDirectory: URL? {
        ProcessInfo.processInfo.environment["K42_SCAN_DIR"].map { URL(fileURLWithPath: $0, isDirectory: true) }
    }

    static var defaultDestination: URL {
        if let t = testDirectory { return t }
        return FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0].appendingPathComponent("Scans", isDirectory: true)
    }
}
