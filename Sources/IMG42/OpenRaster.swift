import AppKit
import ImageIO
import Kit42
import UniformTypeIdentifiers

/// OpenRaster is flattened on import and exported as a new single-layer archive.
enum OpenRaster {
    static let type = UTType(importedAs: "org.openraster.image", conformingTo: .image)
    static let mime = "image/openraster"
    private static let limit = 256 * 1024 * 1024

    // Inspect the central directory before ZipArchive inflates anything. Never extract paths.
    static func read(_ url: URL) throws -> CGImage {
        let attrs = try FileManager.default.attributesOfItem(atPath: url.path)
        guard let size = attrs[.size] as? NSNumber, size.int64Value >= 22, size.int64Value <= limit else { throw CocoaError(.fileReadCorruptFile) }
        let data = try Data(contentsOf: url)
        let b = [UInt8](data)
        func u16(_ i: Int) -> Int { Int(b[i]) | Int(b[i+1]) << 8 }
        func u32(_ i: Int) -> Int { u16(i) | u16(i+2) << 16 }
        var e = b.count - 22
        let first = max(0, e - 65535)
        while e >= first && !(u32(e) == 0x06054b50 && e + 22 + u16(e+20) == b.count) { e -= 1 }
        guard e >= first else { throw CocoaError(.fileReadCorruptFile) }
        let count = u16(e+10), start = u32(e+16), length = u32(e+12)
        guard count > 0, count <= 4096, start <= e, length <= e - start else { throw CocoaError(.fileReadCorruptFile) }
        var p = start, total = 0, names = Set<String>()
        for _ in 0..<count {
            guard p <= start + length - 46, u32(p) == 0x02014b50 else { throw CocoaError(.fileReadCorruptFile) }
            let n = u16(p+28), x = u16(p+30), c = u16(p+32), entrySize = u32(p+24)
            guard n > 0, n + x + c <= start + length - p - 46, entrySize <= limit - total else { throw CocoaError(.fileReadCorruptFile) }
            guard let name = String(bytes: b[(p+46)..<(p+46+n)], encoding: .utf8), !name.hasPrefix("/"), !name.contains("\\"), !name.split(separator: "/", omittingEmptySubsequences: false).contains(".."), names.insert(name).inserted else { throw CocoaError(.fileReadCorruptFile) }
            total += entrySize; p += 46 + n + x + c
        }
        guard p == start + length else { throw CocoaError(.fileReadCorruptFile) }
        let zip = try ZipArchive(data: data)
        guard zip.string("mimetype") == mime, let xml = zip["stack.xml"], xml.count <= 4 * 1024 * 1024,
              let merged = zip["mergedimage.png"], let source = CGImageSourceCreateWithData(merged as CFData, nil),
              CGImageSourceGetType(source) as String? == UTType.png.identifier,
              let props = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
              let w = props[kCGImagePropertyPixelWidth] as? Int, let h = props[kCGImagePropertyPixelHeight] as? Int,
              ImageLoader.validPixelSize(CGSize(width: w, height: h)) else { throw CocoaError(.fileReadCorruptFile) }
        guard let xmlText = String(data: xml, encoding: .utf8), !xmlText.uppercased().contains("<!DOCTYPE") else { throw CocoaError(.fileReadCorruptFile) }
        let parser = XMLParser(data: xml); let validator = StackValidator(); parser.delegate = validator
        guard parser.parse(), validator.valid, validator.hasStack, validator.width == w, validator.height == h,
              let image = CGImageSourceCreateImageAtIndex(source, 0, nil) else { throw CocoaError(.fileReadCorruptFile) }
        return image
    }

    static func write(_ image: CGImage, to url: URL) throws {
        let png = NSMutableData()
        guard let dest = CGImageDestinationCreateWithData(png, UTType.png.identifier as CFString, 1, nil) else { throw CocoaError(.fileWriteUnknown) }
        CGImageDestinationAddImage(dest, image, nil)
        guard CGImageDestinationFinalize(dest) else { throw CocoaError(.fileWriteUnknown) }
        var zip = ZipArchive(); zip["mimetype"] = Data(mime.utf8); zip.storedUncompressed.insert("mimetype")
        zip["stack.xml"] = Data("<image version=\"0.0.3\" w=\"\(image.width)\" h=\"\(image.height)\"><stack><layer name=\"Flattened\" src=\"data/layer0.png\" x=\"0\" y=\"0\" opacity=\"1.0\" visibility=\"visible\" composite-op=\"svg:src-over\"/></stack></image>".utf8)
        zip["data/layer0.png"] = png as Data; zip["mergedimage.png"] = png as Data
        try zip.data().write(to: url, options: .atomic)
    }

    private final class StackValidator: NSObject, XMLParserDelegate {
        var width = 0, height = 0, valid = false, hasStack = false, depth = 0
        func parser(_ parser: XMLParser, didStartElement elementName: String, namespaceURI: String?, qualifiedName qName: String?, attributes: [String: String]) {
            if depth == 0, elementName == "image", let w = attributes["w"].flatMap(Int.init), let h = attributes["h"].flatMap(Int.init), w > 0, h > 0 { width = w; height = h; valid = true }
            if depth == 1 && elementName == "stack" { hasStack = true }
            depth += 1
        }
        func parser(_ parser: XMLParser, didEndElement elementName: String, namespaceURI: String?, qualifiedName qName: String?) { depth -= 1 }
        func parser(_ parser: XMLParser, resolveExternalEntityName name: String, systemID: String?) -> Data? { nil }
    }
}
