import AppKit
import ImageIO
import Kit42
import UniformTypeIdentifiers

enum SaveFormat: Int, CaseIterable, Codable {
    case pdf, pdfPerPage, pdfA, jpeg, png, tiff, heic

    var title: String {
        switch self {
        case .pdf: return L("PDF (one file)")
        case .pdfPerPage: return L("PDF (one file per page)")
        case .pdfA: return L("PDF/A (for archiving)")
        case .jpeg: return "JPEG"
        case .png: return "PNG"
        case .tiff: return L("TIFF (multi-page)")
        case .heic: return "HEIC"
        }
    }

    var ext: String {
        switch self {
        case .pdf, .pdfPerPage, .pdfA: return "pdf"
        case .jpeg: return "jpg"
        case .png: return "png"
        case .tiff: return "tiff"
        case .heic: return "heic"
        }
    }

    var type: UTType {
        switch self {
        case .pdf, .pdfPerPage, .pdfA: return .pdf
        case .jpeg: return .jpeg
        case .png: return .png
        case .tiff: return .tiff
        case .heic: return .heic
        }
    }

    var isPDF: Bool { self == .pdf || self == .pdfPerPage || self == .pdfA }
    /// Formats that keep every page in one file.
    var isMultiPage: Bool { self == .pdf || self == .pdfA || self == .tiff }
    var isLossy: Bool { isPDF || self == .jpeg || self == .heic }
}

/// File size presets (the idea of PDF42's "Reduce File Size": downsample to a resolution, JPEG quality).
enum Compression: Int, CaseIterable, Codable {
    case original, high, balanced, smallest

    var title: String {
        switch self {
        case .original: return L("Best quality (lossless)")
        case .high: return L("High (300 dpi)")
        case .balanced: return L("Balanced (200 dpi)")
        case .smallest: return L("Smallest (150 dpi)")
        }
    }

    var maxDPI: Double? { [nil, 300, 200, 150][rawValue] }
    var quality: Double { [1, 0.82, 0.68, 0.5][rawValue] }

    static func named(_ s: String) -> Compression? {
        ["original": .original, "lossless": .original, "high": .high, "balanced": .balanced, "smallest": .smallest][s.lowercased()]
    }
}

enum OpenAfter: Int, CaseIterable, Codable {
    case none, finder, pdf42, img42, defaultApp

    var title: String {
        switch self {
        case .none: return L("Nothing")
        case .finder: return L("Show in Finder")
        case .pdf42: return "PDF42"
        case .img42: return "IMG42"
        case .defaultApp: return L("Default app")
        }
    }

    var bundleID: String? {
        switch self {
        case .pdf42: return "com.sfzorin.pdf42"
        case .img42: return "com.sfzorin.img42"
        default: return nil
        }
    }
}

struct SaveOptions: Codable, Equatable {
    var format: SaveFormat = .pdf
    var compression: Compression = .balanced
    /// JPEG/HEIC file quality (0…1).
    var quality = 0.85
    /// Searchable text layer in PDFs.
    var ocr = true
    var nameTemplate = SaveOptions.defaultTemplate
    var folder: String?
    var openAfter: OpenAfter = .finder
    var counter = 1

    static var defaultTemplate: String { L("Scan") + " YYYY-MM-DD HH-mm" }

    var folderURL: URL { folder.map { URL(fileURLWithPath: $0, isDirectory: true) } ?? Store.defaultDestination }
}

/// One page as it goes into a file.
struct ExportPage {
    var image: CGImage
    var dpi: Double
    var text: [TextLine]?
    var title: String?
}

enum Exporter {
    // MARK: - Names

    /// `YYYY MM DD HH mm ss` → date parts, `{n}` / `###` → counter (001), `{title}` → first line of text.
    static func expand(_ template: String, date: Date = Date(), counter: Int, title: String?) -> String {
        let c = Calendar(identifier: .gregorian).dateComponents([.year, .month, .day, .hour, .minute, .second], from: date)
        func two(_ v: Int?) -> String { String(format: "%02d", v ?? 0) }
        var s = template
        s = s.replacingOccurrences(of: "YYYY", with: String(format: "%04d", c.year ?? 0))
        s = s.replacingOccurrences(of: "MM", with: two(c.month))
        s = s.replacingOccurrences(of: "DD", with: two(c.day))
        s = s.replacingOccurrences(of: "HH", with: two(c.hour))
        s = s.replacingOccurrences(of: "mm", with: two(c.minute))
        s = s.replacingOccurrences(of: "ss", with: two(c.second))
        s = s.replacingOccurrences(of: "{n}", with: String(format: "%03d", counter))
        s = s.replacingOccurrences(of: "###", with: String(format: "%03d", counter))
        s = s.replacingOccurrences(of: "{title}", with: title ?? "")
        s = s.components(separatedBy: CharacterSet(charactersIn: "/:\\").union(.controlCharacters)).joined(separator: "-")
        s = s.trimmingCharacters(in: .whitespaces.union(CharacterSet(charactersIn: "-_.")))
        while s.contains("  ") { s = s.replacingOccurrences(of: "  ", with: " ") }
        return s.isEmpty ? L("Scan") : s
    }

    /// `name.ext` in `folder`, with " 2", " 3"… when taken.
    static func uniqueURL(_ folder: URL, _ name: String, _ ext: String) -> URL {
        var url = folder.appendingPathComponent(name).appendingPathExtension(ext)
        var n = 2
        while FileManager.default.fileExists(atPath: url.path) {
            url = folder.appendingPathComponent("\(name) \(n)").appendingPathExtension(ext)
            n += 1
        }
        return url
    }

    // MARK: - Writing

    /// Writes the pages; returns the files made (one, or one per page for single-page formats).
    static func write(_ pages: [ExportPage], folder: URL, baseName: String, options: SaveOptions,
                      replaceExisting: Bool = false) throws -> [URL] {
        try validate(pages, options)
        guard !baseName.isEmpty, baseName != ".", baseName != "..", baseName.utf8.count <= 240,
              baseName.rangeOfCharacter(from: CharacterSet(charactersIn: "/:\\").union(.controlCharacters)) == nil else {
            throw CocoaError(.fileWriteInvalidFileName)
        }
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let f = options.format
        if f.isMultiPage || pages.count == 1 {
            let url = replaceExisting ? folder.appendingPathComponent(baseName).appendingPathExtension(f.ext)
                : uniqueURL(folder, baseName, f.ext)
            try data(pages, options).write(to: url, options: .atomic)
            return [url]
        }
        var urls: [URL] = []
        for (i, page) in pages.enumerated() {
            let name = "\(baseName) \(String(format: "%03d", i + 1))"
            let url = replaceExisting ? folder.appendingPathComponent(name).appendingPathExtension(f.ext) : uniqueURL(folder, name, f.ext)
            try data([page], options).write(to: url, options: .atomic)
            urls.append(url)
        }
        return urls
    }

    private static func validate(_ pages: [ExportPage], _ options: SaveOptions) throws {
        guard !pages.isEmpty, options.quality.isFinite, (0...1).contains(options.quality),
              pages.allSatisfy({ page in
                  page.dpi.isFinite && page.dpi > 0 && page.image.width > 0 && page.image.height > 0
                      && (Double(page.image.width) * 72 / page.dpi).isFinite
                      && (Double(page.image.height) * 72 / page.dpi).isFinite
              }) else { throw CocoaError(.fileWriteInvalidFileName) }
    }

    /// The file contents for `pages` in the chosen format. Single-page formats never silently drop pages.
    static func data(_ pages: [ExportPage], _ options: SaveOptions) throws -> Data {
        try validate(pages, options)
        guard options.format.isMultiPage || pages.count == 1 else { throw CocoaError(.fileWriteUnknown) }
        switch options.format {
        case .pdf, .pdfPerPage, .pdfA:
            return try pdfChecked(pages, compression: options.compression, ocr: options.ocr, archival: options.format == .pdfA)
        case .jpeg, .png, .tiff, .heic:
            return try imageFile(pages, options)
        }
    }

    /// Expected file size: the first pages encoded for real, the rest extrapolated.
    static func estimate(_ pages: [ExportPage], _ options: SaveOptions) -> Int {
        guard !pages.isEmpty else { return 0 }
        var o = options
        o.ocr = false
        let sample = Array(pages.prefix(3))
        let size = o.format.isMultiPage ? ((try? data(sample, o).count) ?? 0)
            : sample.reduce(0) { $0 + ((try? data([$1], o).count) ?? 0) }
        return Int(Double(size) * Double(pages.count) / Double(sample.count))
    }

    static func sizeText(_ bytes: Int) -> String { ByteCountFormatter.string(fromByteCount: Int64(bytes), countStyle: .file) }

    // MARK: PDF

    /// Image pages with an invisible text layer. JPEG data is embedded as is (DCT), 1-bit pages stay 1-bit.
    static func pdf(_ pages: [ExportPage], compression: Compression, ocr: Bool, archival: Bool, title: String? = nil) -> Data {
        (try? pdfChecked(pages, compression: compression, ocr: ocr, archival: archival, title: title)) ?? Data()
    }

    static func pdfChecked(_ pages: [ExportPage], compression: Compression, ocr: Bool, archival: Bool, title: String? = nil) throws -> Data {
        try validate(pages, SaveOptions())
        let data = NSMutableData()
        var info: [CFString: Any] = [kCGPDFContextCreator: "SCN42"]
        let docTitle = title ?? pages.first?.title
        if let docTitle { info[kCGPDFContextTitle] = docTitle }
        if archival {
            // PDF/A-style output intent: everything is sRGB.
            info[kCGPDFContextOutputIntent] = [
                kCGPDFXOutputIntentSubtype: "GTS_PDFA1",
                kCGPDFXOutputConditionIdentifier: "sRGB IEC61966-2.1",
                kCGPDFXInfo: "sRGB IEC61966-2.1",
                kCGPDFXDestinationOutputProfile: CGColorSpace(name: CGColorSpace.sRGB)!,
            ] as [CFString: Any]
        }
        guard let consumer = CGDataConsumer(data: data), let ctx = CGContext(consumer: consumer, mediaBox: nil, info as CFDictionary) else { throw CocoaError(.fileWriteUnknown) }
        if archival { ctx.addDocumentMetadata(xmp(title: docTitle) as CFData) }
        for page in pages {
            let (image, _) = try encodedForPDF(page, compression)
            // Resampling rounds pixel counts; the scanned physical page size must remain exact.
            var box = CGRect(x: 0, y: 0, width: Double(page.image.width) * 72 / page.dpi, height: Double(page.image.height) * 72 / page.dpi)
            let boxData = Data(bytes: &box, count: MemoryLayout<CGRect>.size) as CFData
            ctx.beginPDFPage([kCGPDFContextMediaBox: boxData] as CFDictionary)
            ctx.interpolationQuality = .high
            ctx.draw(image, in: box)
            if ocr, let lines = page.text, !lines.isEmpty { OCR.drawInvisibleText(lines, box: box, in: ctx) }
            ctx.endPDFPage()
        }
        ctx.closePDF()
        guard data.length > 0 else { throw CocoaError(.fileWriteUnknown) }
        return data as Data
    }

    /// The page image ready for a PDF: downsampled to the preset's resolution and JPEG-compressed;
    /// 1-bit (B&W) pages are kept as they are (Flate-compressed bits are smaller than JPEG).
    static func encodedForPDF(_ page: ExportPage, _ c: Compression) throws -> (CGImage, Double) {
        try validate([page], SaveOptions())
        if page.image.bitsPerPixel == 1 || c == .original { return (page.image, page.dpi) }
        var image = page.image, dpi = page.dpi
        if let maxDPI = c.maxDPI, dpi > maxDPI + 1 {
            let s = maxDPI / dpi
            let gray = image.colorSpace?.model == .monochrome
            let bmp = Bitmap(image, gray: gray).resized(width: max(1, Int(Double(image.width) * s)), height: max(1, Int(Double(image.height) * s)))
            guard let resized = bmp.cgImage() else { throw CocoaError(.fileWriteUnknown) }
            image = resized; dpi = maxDPI
        }
        guard let jpeg = jpegData(image, quality: c.quality, dpi: dpi), let provider = CGDataProvider(data: jpeg as CFData),
              let decoded = CGImage(jpegDataProviderSource: provider, decode: nil, shouldInterpolate: true, intent: .defaultIntent) else {
            throw CocoaError(.fileWriteUnknown)
        }
        return (decoded, dpi)
    }

    static func jpegData(_ image: CGImage, quality: Double, dpi: Double) -> Data? {
        guard quality.isFinite, (0...1).contains(quality), dpi.isFinite, dpi > 0 else { return nil }
        let out = NSMutableData()
        guard let dest = CGImageDestinationCreateWithData(out, UTType.jpeg.identifier as CFString, 1, nil) else { return nil }
        guard let image = Bitmap(image, gray: image.colorSpace?.model == .monochrome).cgImage() else { return nil }
        CGImageDestinationAddImage(dest, image, [kCGImageDestinationLossyCompressionQuality: quality,
                                                 kCGImagePropertyDPIWidth: dpi, kCGImagePropertyDPIHeight: dpi] as CFDictionary)
        return CGImageDestinationFinalize(dest) ? out as Data : nil
    }

    /// XMP packet declaring PDF/A-2B, with title and creator tool.
    static func xmp(title: String?) -> Data {
        let date = ISO8601DateFormatter().string(from: Date())
        let t = (title ?? L("Scan")).replacingOccurrences(of: "&", with: "&amp;").replacingOccurrences(of: "<", with: "&lt;")
        let s = """
        <?xpacket begin="\u{FEFF}" id="W5M0MpCehiHzreSzNTczkc9d"?>
        <x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
        <rdf:Description rdf:about="" xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/" xmlns:dc="http://purl.org/dc/elements/1.1/"
          xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmlns:pdf="http://ns.adobe.com/pdf/1.3/">
        <pdfaid:part>2</pdfaid:part><pdfaid:conformance>B</pdfaid:conformance>
        <dc:title><rdf:Alt><rdf:li xml:lang="x-default">\(t)</rdf:li></rdf:Alt></dc:title>
        <xmp:CreatorTool>SCN42</xmp:CreatorTool><xmp:CreateDate>\(date)</xmp:CreateDate><pdf:Producer>SCN42</pdf:Producer>
        </rdf:Description></rdf:RDF></x:xmpmeta>
        <?xpacket end="w"?>
        """
        return Data(s.utf8)
    }

    // MARK: Images

    static func imageFile(_ pages: [ExportPage], _ o: SaveOptions) throws -> Data {
        try validate(pages, o)
        guard o.format.isMultiPage || pages.count == 1 else { throw CocoaError(.fileWriteUnknown) }
        let out = NSMutableData()
        let count = o.format == .tiff ? pages.count : 1
        guard let dest = CGImageDestinationCreateWithData(out, o.format.type.identifier as CFString, count, nil) else {
            throw NSError(domain: "SCN42", code: 1, userInfo: [NSLocalizedDescriptionKey: L("This format cannot be written on this Mac.")])
        }
        for page in pages.prefix(count) {
            var props: [CFString: Any] = [kCGImagePropertyDPIWidth: page.dpi, kCGImagePropertyDPIHeight: page.dpi]
            var image = page.image
            switch o.format {
            case .jpeg, .heic:
                props[kCGImageDestinationLossyCompressionQuality] = o.quality
                // Opaque output uses white for transparent paper; B&W expands to gray without losing content.
                guard let opaque = Bitmap(image, gray: image.bitsPerPixel == 1 || image.colorSpace?.model == .monochrome).cgImage() else {
                    throw CocoaError(.fileWriteUnknown)
                }
                image = opaque
            case .tiff:
                props[kCGImagePropertyTIFFDictionary] = [kCGImagePropertyTIFFCompression: image.bitsPerPixel == 1 ? 4 : 5]
            default: break
            }
            if let title = page.title { props[kCGImagePropertyTIFFDictionary] = (props[kCGImagePropertyTIFFDictionary] as? [CFString: Any] ?? [:]).merging([kCGImagePropertyTIFFImageDescription: title]) { a, _ in a } }
            CGImageDestinationAddImage(dest, image, props as CFDictionary)
        }
        guard CGImageDestinationFinalize(dest) else {
            throw NSError(domain: "SCN42", code: 2, userInfo: [NSLocalizedDescriptionKey: L("The file could not be written.")])
        }
        return out as Data
    }

    // MARK: Text

    static func textData(_ pages: [[TextLine]], rtf: Bool) -> Data? {
        let text = pages.map(OCR.text).joined(separator: "\n\n")
        guard rtf else { return Data(text.utf8) }
        let s = NSAttributedString(string: text, attributes: [.font: NSFont.systemFont(ofSize: 12)])
        return s.rtf(from: NSRange(location: 0, length: s.length), documentAttributes: [:])
    }
}
