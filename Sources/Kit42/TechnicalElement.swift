import AppKit
import ImageIO
import JavaScriptCore
import UniformTypeIdentifiers

/// A portable recipe. PNG fallbacks carry this recipe so DOCX/XLSX can keep
/// editable codes while other applications continue to see an ordinary image.
public struct TechnicalElement: Codable, Equatable {
    public var symbolID: String?
    public var codeType: TechnicalCodeType
    public var value: String
    public var widthMM: Double
    public var caption: Bool

    public init(symbolID: String? = nil, codeType: TechnicalCodeType = .qr, value: String = "", widthMM: Double = 20, caption: Bool = true) {
        self.symbolID = symbolID; self.codeType = codeType; self.value = value
        self.widthMM = widthMM; self.caption = caption
    }

    private static let recipePrefix = "Suite42.TechnicalElement.v1:"
    public var encodedDescription: String {
        guard let data = try? JSONEncoder().encode(self) else { return "" }
        return Self.recipePrefix + data.base64EncodedString()
    }

    public static func fromDescription(_ string: String) -> TechnicalElement? {
        guard string.hasPrefix(recipePrefix), string.utf8.count < 24_000,
              let data = Data(base64Encoded: String(string.dropFirst(recipePrefix.count))),
              let element = try? JSONDecoder().decode(Self.self, from: data),
              (try? element.validate()) != nil else { return nil }
        return element
    }

    public static func fromPNG(_ data: Data) -> TechnicalElement? {
        guard data.count <= 32_000_000, let source = CGImageSourceCreateWithData(data as CFData, nil),
              let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [String: Any],
              let png = properties[kCGImagePropertyPNGDictionary as String] as? [String: Any],
              let description = png[kCGImagePropertyPNGDescription as String] as? String else { return nil }
        return fromDescription(description)
    }

    private func validate() throws {
        guard widthMM.isFinite, (3...250).contains(widthMM) else {
            throw TechnicalElementError(L("Width must be between 3 and 250 mm."))
        }
        if let symbolID {
            guard TechnicalSymbols.all.contains(where: { $0.id == symbolID }) else {
                throw TechnicalElementError(L("Unknown technical symbol."))
            }
        } else {
            guard !value.isEmpty, value.utf8.count <= 1024,
                  !value.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) }) else {
                throw TechnicalElementError(L("Enter a value of 1–1024 bytes without control characters."))
            }
            if codeType == .code128, !value.unicodeScalars.allSatisfy({ (32...126).contains($0.value) }) {
                throw TechnicalElementError(L("Code 128 requires printable ASCII characters. Use QR for other text."))
            }
        }
    }

    public func render() throws -> TechnicalRender {
        try validate()
        let width = CGFloat(widthMM * 72 / 25.4)
        if let symbolID, let symbol = TechnicalSymbols.all.first(where: { $0.id == symbolID }) {
            guard let url = TechnicalResources.url(symbol.file),
                  let image = NSImage(contentsOf: url), image.size.width > 0, image.size.height > 0 else {
                throw TechnicalElementError(L("The technical symbol could not be loaded."))
            }
            let size = CGSize(width: width, height: width * image.size.height / image.size.width)
            return try makeRender(size: size, pixelsPerPoint: 600 / 72) { context in
                NSGraphicsContext.saveGraphicsState()
                NSGraphicsContext.current = NSGraphicsContext(cgContext: context, flipped: false)
                image.draw(in: CGRect(origin: .zero, size: size), from: .zero, operation: .sourceOver, fraction: 1)
                NSGraphicsContext.restoreGraphicsState()
            }
        }
        let raw = try TechnicalBarcodeEngine.shared.encode(type: codeType, value: value)
        let quiet = codeType.quietModules
        let columns = raw.columns + 2 * quiet
        let modulePoints = width / CGFloat(columns)
        let moduleMM = widthMM / Double(columns)
        guard moduleMM >= codeType.minimumModuleMM else {
            let minimum = ceil(Double(columns) * codeType.minimumModuleMM * 10) / 10
            throw TechnicalElementError(L("This value needs a width of at least %@ mm.", String(format: "%.1f", minimum)))
        }
        let font = NSFont.monospacedSystemFont(ofSize: 7, weight: .regular)
        let attributes: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: NSColor.black]
        let text = NSAttributedString(string: value, attributes: attributes)
        if caption, text.size().width + 2 > width {
            throw TechnicalElementError(L("The caption does not fit. Increase the width or turn off the caption."))
        }
        let captionUnits = caption ? Int(ceil(14 / modulePoints)) : 0
        let barRows = codeType == .code128 ? max(40, Int(ceil(28.35 / modulePoints))) : raw.rows
        let rows = barRows + 2 * quiet + captionUnits
        let size = CGSize(width: width, height: CGFloat(rows) * modulePoints)
        let pixelsPerModule = max(4, Int(ceil(moduleMM * 600 / 25.4)))
        let pixelsPerPoint = CGFloat(pixelsPerModule) / modulePoints
        return try makeRender(size: size, pixelsPerPoint: pixelsPerPoint) { context in
            context.setFillColor(NSColor.white.cgColor)
            context.fill(CGRect(origin: .zero, size: size))
            context.setShouldAntialias(false)
            context.setFillColor(NSColor.black.cgColor)
            let bottom = CGFloat(quiet + captionUnits) * modulePoints
            for rect in raw.rectangles {
                let y = codeType == .code128 ? bottom : bottom + CGFloat(barRows) * modulePoints - rect.maxY * modulePoints
                let height = codeType == .code128 ? CGFloat(barRows) * modulePoints : rect.height * modulePoints
                context.fill(CGRect(x: (rect.minX + CGFloat(quiet)) * modulePoints, y: y,
                                    width: rect.width * modulePoints, height: height))
            }
            if caption {
                context.setShouldAntialias(true)
                NSGraphicsContext.saveGraphicsState()
                NSGraphicsContext.current = NSGraphicsContext(cgContext: context, flipped: false)
                text.draw(at: CGPoint(x: (width - text.size().width) / 2, y: 1))
                NSGraphicsContext.restoreGraphicsState()
            }
        }
    }

    private func makeRender(size: CGSize, pixelsPerPoint: CGFloat, draw: (CGContext) -> Void) throws -> TechnicalRender {
        guard size.width.isFinite, size.height.isFinite, size.width > 0, size.height > 0 else {
            throw TechnicalElementError(L("Invalid technical element dimensions."))
        }
        let pixelWidth = Int(ceil(size.width * pixelsPerPoint - 0.000001))
        let pixelHeight = Int(ceil(size.height * pixelsPerPoint - 0.000001))
        guard pixelWidth > 0, pixelHeight > 0, pixelWidth <= 16_384, pixelHeight <= 16_384,
              pixelWidth <= 16_000_000 / pixelHeight else {
            throw TechnicalElementError(L("The technical element is too large to render."))
        }
        let pdf = NSMutableData()
        var box = CGRect(origin: .zero, size: size)
        guard let consumer = CGDataConsumer(data: pdf), let pdfContext = CGContext(consumer: consumer, mediaBox: &box, nil) else {
            throw TechnicalElementError(L("The technical element could not be rendered."))
        }
        pdfContext.beginPDFPage(nil); draw(pdfContext); pdfContext.endPDFPage(); pdfContext.closePDF()
        guard let bitmap = CGContext(data: nil, width: pixelWidth, height: pixelHeight, bitsPerComponent: 8,
                                     bytesPerRow: 0, space: CGColorSpace(name: CGColorSpace.sRGB)!,
                                     bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else {
            throw TechnicalElementError(L("The technical element could not be rendered."))
        }
        bitmap.scaleBy(x: pixelsPerPoint, y: pixelsPerPoint); draw(bitmap)
        guard let image = bitmap.makeImage() else { throw TechnicalElementError(L("The technical element could not be rendered.")) }
        let png = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(png, UTType.png.identifier as CFString, 1, nil) else {
            throw TechnicalElementError(L("The technical element could not be rendered."))
        }
        let properties: [CFString: Any] = [kCGImagePropertyDPIWidth: pixelsPerPoint * 72,
                                          kCGImagePropertyDPIHeight: pixelsPerPoint * 72,
                                          kCGImagePropertyPNGDictionary: [kCGImagePropertyPNGDescription: encodedDescription]]
        CGImageDestinationAddImage(destination, image, properties as CFDictionary)
        guard CGImageDestinationFinalize(destination) else { throw TechnicalElementError(L("The technical element could not be rendered.")) }
        return TechnicalRender(pngData: png as Data, pdfData: pdf as Data, sizePoints: size)
    }
}

public enum TechnicalCodeType: String, Codable, CaseIterable {
    case qr, dataMatrix, code128, aztec
    public var title: String {
        switch self { case .qr: return "QR"; case .dataMatrix: return "Data Matrix"; case .code128: return "Code 128"; case .aztec: return "Aztec" }
    }
    fileprivate var encoder: String {
        switch self { case .qr: return "qrcode"; case .dataMatrix: return "datamatrix"; case .code128: return "code128"; case .aztec: return "azteccode" }
    }
    fileprivate var quietModules: Int { self == .qr ? 4 : self == .code128 ? 10 : 1 }
    fileprivate var minimumModuleMM: Double { self == .dataMatrix ? 0.19 : 0.25 }
}

public struct TechnicalRender {
    public let pngData: Data
    public let pdfData: Data
    public let sizePoints: CGSize
    public init(pngData: Data, pdfData: Data, sizePoints: CGSize) {
        self.pngData = pngData; self.pdfData = pdfData; self.sizePoints = sizePoints
    }
}

/// A product mark from the bundled catalogue (`Technical/marks.json`, imported from Lably): conformity,
/// electrical safety, environmental, hazard, handling and interface marks. Artwork only: inserting a mark
/// neither certifies a product nor grants the right to use a certification mark or trademark.
public struct TechnicalSymbol {
    public let id: String
    public let file: String
    public let category: String
    public let name: String
    public let categoryName: String
    public let region: String
    public let summary: String
    public let warning: String?
    /// Registration or licence number required next to the mark (BIS, BSMI…).
    public let needsNumber: Bool
    /// Artwork Lably withheld from insertion (unverified drawing, licensing or provenance doubts).
    public let unverified: Bool
    public let legal: String

    /// The guidance shown under the preview, most important first.
    public var notice: String {
        var parts: [String] = []
        if unverified { parts.append(L("Unverified artwork: compare it with the official mark before use.")) }
        if legal == "trademark" { parts.append(L("Trademark or certification mark: use only with the owner's authorization for this product.")) }
        if let warning { parts.append(warning) }
        if needsNumber { parts.append(L("Add the required registration or licence number next to the mark.")) }
        return parts.joined(separator: " ")
    }
}

public enum TechnicalSymbols {
    public static let categories = ["certification", "safety", "environmental", "hazardous", "handling", "wireless", "medical", "quality"]

    private struct Entry: Decodable {
        struct Copy: Decodable { let name: String; let description: String?; let warning: String?; let category: String?; let region: String? }
        let id: String; let file: String; let category: String; let legal: String
        let registrationNumber: Bool; let unverified: Bool; let copy: [String: Copy]
    }

    private static let entries: [Entry] = {
        guard let url = TechnicalResources.url("marks.json"), let data = try? Data(contentsOf: url),
              let list = try? JSONDecoder().decode([Entry].self, from: data) else { return [] }
        return list
    }()

    /// Catalogue in the interface language (Lably copy exists for 11 languages; others use English).
    public static var all: [TechnicalSymbol] { cache }
    private static let cache: [TechnicalSymbol] = entries.map { e in
        let english = e.copy["en"]
        let local = e.copy[Language.current.rawValue] ?? english
        return TechnicalSymbol(id: e.id, file: e.file, category: e.category, name: local?.name ?? english?.name ?? e.id,
                               categoryName: local?.category ?? english?.category ?? e.category, region: local?.region ?? english?.region ?? "",
                               summary: local?.description ?? english?.description ?? "", warning: local?.warning ?? english?.warning,
                               needsNumber: e.registrationNumber, unverified: e.unverified, legal: e.legal)
    }

    /// Search by name, id, region, category or description; exact name/id matches first.
    public static func search(_ query: String) -> [TechnicalSymbol] {
        let q = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !q.isEmpty else { return all }
        func exact(_ s: TechnicalSymbol) -> Bool { s.id.caseInsensitiveCompare(q) == .orderedSame || s.name.caseInsensitiveCompare(q) == .orderedSame }
        let hits = all.filter { s in
            [s.name, s.id, s.region, s.categoryName, s.summary].contains { $0.localizedCaseInsensitiveContains(q) }
        }
        return hits.filter(exact) + hits.filter { !exact($0) }
    }
}

public struct TechnicalElementError: LocalizedError {
    private let message: String
    public init(_ message: String) { self.message = message }
    public var errorDescription: String? { message }
}

enum TechnicalResources {
    static func url(_ file: String) -> URL? {
        let ns = file as NSString
        return url(ns.deletingPathExtension, extension: ns.pathExtension)
    }

    static func url(_ name: String, extension ext: String) -> URL? {
        let bundle = Bundle.main.resourceURL.flatMap { Bundle(url: $0.appendingPathComponent("Suite42_Kit42.bundle")) } ?? Bundle.module
        return bundle.url(forResource: name, withExtension: ext, subdirectory: "Technical")
    }
}

private struct TechnicalBarcodeGeometry {
    let columns: Int
    let rows: Int
    let rectangles: [CGRect]
}

/// All calls use immutable bundled code and pass values as JS arguments, never
/// source interpolation. The context stays behind a lock and values do not escape.
private final class TechnicalBarcodeEngine {
    static let shared = TechnicalBarcodeEngine()
    private let lock = NSLock()
    private var context: JSContext?

    func encode(type: TechnicalCodeType, value: String) throws -> TechnicalBarcodeGeometry {
        lock.lock(); defer { lock.unlock() }
        if context == nil {
            guard let url = TechnicalResources.url("bwip-js-min", extension: "js"),
                  let script = try? String(contentsOf: url, encoding: .utf8), let js = JSContext() else {
                throw TechnicalElementError(L("The barcode encoder could not be loaded."))
            }
            js.evaluateScript("""
            function atob(str) { var chars='ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/',bits=0,acc=0,out='';
                for(var i=0;i<str.length;i++){var v=chars.indexOf(str[i]);if(v<0)continue;acc=(acc<<6)|v;bits+=6;
                if(bits>=8){bits-=8;out+=String.fromCharCode((acc>>bits)&255);}}return out; }
            """)
            js.evaluateScript(script)
            guard js.exception == nil else { throw TechnicalElementError(L("The barcode encoder could not be loaded.")) }
            context = js
        }
        guard let js = context else { throw TechnicalElementError(L("The barcode encoder could not be loaded.")) }
        js.exception = nil
        var options: [String: Any] = ["bcid": type.encoder, "text": value, "includetext": false]
        if value.unicodeScalars.contains(where: { $0.value > 127 }) {
            // ECI 26 explicitly identifies UTF-8. Escape every byte so literal
            // carets in user input can never become BWIPP function controls.
            options["text"] = "^ECI000026" + value.utf8.map { $0 == 94 ? "^^" : String(format: "^%03d", $0) }.joined()
            options["parse"] = true
            options["parsefnc"] = true
        }
        let result = js.objectForKeyedSubscript("bwipjs")?.invokeMethod("raw", withArguments: [options])
        if let exception = js.exception {
            let message = exception.toString() ?? L("Invalid barcode value.")
            js.exception = nil
            throw TechnicalElementError(message)
        }
        guard let stack = result?.toArray() as? [[String: Any]], stack.count == 1, let raw = stack.first else {
            throw TechnicalElementError(L("The barcode encoder returned unsupported geometry."))
        }
        var rectangles: [CGRect] = []
        if type == .code128 {
            guard let widths = raw["sbs"] as? [NSNumber], !widths.isEmpty, widths.count <= 16_384 else {
                throw TechnicalElementError(L("The barcode encoder returned unsupported geometry."))
            }
            var x = 0
            for (index, width) in widths.enumerated() {
                let n = width.intValue
                guard n > 0, n <= 16, width.doubleValue == Double(n) else { throw TechnicalElementError(L("Invalid barcode geometry.")) }
                if index.isMultiple(of: 2) { rectangles.append(CGRect(x: x, y: 0, width: n, height: 1)) }
                x += n
            }
            return TechnicalBarcodeGeometry(columns: x, rows: 1, rectangles: rectangles)
        }
        guard let cols = raw["pixx"] as? NSNumber, let rows = raw["pixy"] as? NSNumber,
              let pixels = raw["pixs"] as? [NSNumber], (1...512).contains(cols.intValue), (1...512).contains(rows.intValue),
              pixels.count == cols.intValue * rows.intValue else { throw TechnicalElementError(L("Invalid barcode geometry.")) }
        for y in 0..<rows.intValue {
            var x = 0
            while x < cols.intValue {
                if pixels[y * cols.intValue + x].intValue == 0 { x += 1; continue }
                let first = x
                while x < cols.intValue && pixels[y * cols.intValue + x].intValue != 0 { x += 1 }
                rectangles.append(CGRect(x: first, y: y, width: x - first, height: 1))
            }
        }
        return TechnicalBarcodeGeometry(columns: cols.intValue, rows: rows.intValue, rectangles: rectangles)
    }
}
