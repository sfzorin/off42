import AppKit
import PDFKit
import UniformTypeIdentifiers

/// Continuity Camera and file import: what comes from an iPhone ("Take Photo" gives an image,
/// "Scan Documents" a PDF already cropped by the phone) or from Finder becomes raw pages.
enum ContinuityImport {
    static let imageTypes: [NSPasteboard.PasteboardType] = [.tiff, .png, NSPasteboard.PasteboardType(UTType.jpeg.identifier), NSPasteboard.PasteboardType(UTType.heic.identifier)]
    static let returnTypes: [NSPasteboard.PasteboardType] = [.pdf] + imageTypes

    static func accepts(_ type: NSPasteboard.PasteboardType) -> Bool {
        returnTypes.contains(type) || (UTType(type.rawValue)?.conforms(to: .image) ?? false)
    }

    struct Imported {
        var pages: [RawScan]
        /// A PDF from "Scan Documents": the phone already cropped and straightened it.
        var isDocumentScan: Bool
    }

    static func read(_ pboard: NSPasteboard) -> Imported? {
        if let data = pboard.data(forType: .pdf), let doc = PDFDocument(data: data) {
            return Imported(pages: rasterize(doc), isDocumentScan: true)
        }
        for type in imageTypes + (pboard.types ?? []) where accepts(type) {
            if let data = pboard.data(forType: type), let raw = image(data) { return Imported(pages: [raw], isDocumentScan: false) }
        }
        return nil
    }

    static func image(_ data: Data) -> RawScan? {
        try? imagePages(data).first
    }

    /// UI import is bounded to 6000 pixels per side; preserve physical size when reducing a large page.
    static func imagePages(_ data: Data) throws -> [RawScan] {
        guard let source = CGImageSourceCreateWithData(data as CFData, nil) else { throw CocoaError(.fileReadCorruptFile) }
        let type = CGImageSourceGetType(source) as String?
        let count = type == UTType.tiff.identifier ? CGImageSourceGetCount(source) : min(1, CGImageSourceGetCount(source))
        guard (1...2000).contains(count) else { throw CocoaError(.fileReadCorruptFile) }
        return try (0..<count).map { index in
            let props = CGImageSourceCopyPropertiesAtIndex(source, index, nil) as? [CFString: Any]
            let width = (props?[kCGImagePropertyPixelWidth] as? NSNumber)?.doubleValue ?? 0
            let height = (props?[kCGImagePropertyPixelHeight] as? NSNumber)?.doubleValue ?? 0
            guard width.isFinite, height.isFinite, width > 0, height > 0 else { throw CocoaError(.fileReadCorruptFile) }
            let opts: [CFString: Any] = [kCGImageSourceCreateThumbnailFromImageAlways: true,
                kCGImageSourceCreateThumbnailWithTransform: true, kCGImageSourceThumbnailMaxPixelSize: 6000]
            guard let image = CGImageSourceCreateThumbnailAtIndex(source, index, opts as CFDictionary) else { throw CocoaError(.fileReadCorruptFile) }
            let metadataDPI = (props?[kCGImagePropertyDPIWidth] as? NSNumber)?.doubleValue
            let scale = min(1, Double(max(image.width, image.height)) / max(width, height))
            let dpi = metadataDPI.flatMap { $0.isFinite && $0 > 72.5 && $0 <= 100_000 ? $0 * scale : nil } ?? estimatedDPI(image)
            return RawScan(image: image, dpi: dpi)
        }
    }

    /// Photos have no real resolution: assume the long side is an A4 page's height.
    static func estimatedDPI(_ image: CGImage) -> Double {
        max(72, min(600, Double(max(image.width, image.height)) / 11.69))
    }

    static func rasterize(_ doc: PDFDocument, dpi: Double = 250) -> [RawScan] {
        (try? rasterizeChecked(doc, dpi: dpi)) ?? []
    }

    static func rasterizeChecked(_ doc: PDFDocument, dpi: Double = 250) throws -> [RawScan] {
        guard !doc.isLocked, (1...2000).contains(doc.pageCount), dpi.isFinite, dpi > 0, dpi <= 4800 else { throw CocoaError(.fileReadCorruptFile) }
        return try (0..<doc.pageCount).map { i in
            guard let page = doc.page(at: i) else { throw CocoaError(.fileReadCorruptFile) }
            let box = page.bounds(for: .mediaBox)
            guard box.width.isFinite, box.height.isFinite, box.width > 0, box.height > 0 else { throw CocoaError(.fileReadCorruptFile) }
            let rotated = abs(page.rotation % 180) == 90
            let points = rotated ? CGSize(width: box.height, height: box.width) : box.size
            let requested = CGSize(width: points.width * dpi / 72, height: points.height * dpi / 72)
            guard requested.width.isFinite, requested.height.isFinite else { throw CocoaError(.fileReadCorruptFile) }
            let scale = min(1, 6000 / max(requested.width, requested.height))
            let size = CGSize(width: max(1, requested.width * scale), height: max(1, requested.height * scale))
            let ns = page.thumbnail(of: size, for: .mediaBox)
            var r = CGRect(origin: .zero, size: ns.size)
            guard let cg = ns.cgImage(forProposedRect: &r, context: nil, hints: nil) else { throw CocoaError(.fileReadCorruptFile) }
            return RawScan(image: cg, dpi: Double(cg.width) / (points.width / 72))
        }
    }

    /// Images and PDF pages from files (File › Import, drops).
    static func load(_ urls: [URL]) -> [(RawScan, Bool)] {
        (try? loadChecked(urls)) ?? []
    }

    static func loadChecked(_ urls: [URL]) throws -> [(RawScan, Bool)] {
        var out: [(RawScan, Bool)] = []
        for url in urls {
            if UTType(filenameExtension: url.pathExtension)?.conforms(to: .pdf) == true {
                guard let doc = PDFDocument(url: url) else { throw CocoaError(.fileReadCorruptFile, userInfo: [NSFilePathErrorKey: url.path]) }
                out += try rasterizeChecked(doc).map { ($0, true) }
            } else {
                out += try imagePages(Data(contentsOf: url)).map { ($0, false) }
            }
        }
        return out
    }
}
