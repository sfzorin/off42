import AppKit
import Kit42
import ImageIO
import PDFKit
import UniformTypeIdentifiers

/// What IMG42 can open: everything ImageIO decodes on this Mac, plus SVG (NSImage) and PDF (PDFKit).
enum ImageFormats {
    static let imageIOTypes: [UTType] = (CGImageSourceCopyTypeIdentifiers() as? [String] ?? []).compactMap(UTType.init)
    static let svg = UTType("public.svg-image") ?? .svg
    static let extraTypes: [UTType] = [svg, .pdf]
    static var readableTypes: [UTType] { imageIOTypes + extraTypes }

    /// File extensions of every readable type, for folder scanning (faster than resolving each file's type).
    static let extensions: Set<String> = {
        var set = Set<String>()
        for t in imageIOTypes + extraTypes { t.tags[.filenameExtension]?.forEach { set.insert($0.lowercased()) } }
        // Camera RAW extensions that some systems map to the generic RAW type only.
        set.formUnion(["cr2", "cr3", "crw", "nef", "nrw", "arw", "sr2", "srf", "dng", "raf", "orf", "rw2", "pef", "srw", "x3f",
                       "3fr", "fff", "iiq", "mos", "mrw", "erf", "rwl", "jxl", "avif", "heic", "heif", "webp", "exr", "svg"])
        return set
    }()

    static func isSupported(_ url: URL) -> Bool {
        if extensions.contains(url.pathExtension.lowercased()) { return true }
        guard let type = UTType(filenameExtension: url.pathExtension) else { return false }
        return readableTypes.contains { type.conforms(to: $0) }
    }

    static func type(of url: URL) -> UTType? {
        (try? url.resourceValues(forKeys: [.contentTypeKey]).contentType) ?? UTType(filenameExtension: url.pathExtension)
    }

    static func isSVG(_ url: URL) -> Bool { type(of: url)?.conforms(to: svg) == true || url.pathExtension.lowercased() == "svg" }
    static func isPDF(_ url: URL) -> Bool { type(of: url)?.conforms(to: .pdf) == true }

    /// Images directly inside a folder (not recursive, no hidden files).
    static func images(in folder: URL) -> [URL] {
        let keys: [URLResourceKey] = [.isRegularFileKey, .isHiddenKey]
        let items = (try? FileManager.default.contentsOfDirectory(at: folder, includingPropertiesForKeys: keys, options: [.skipsHiddenFiles])) ?? []
        return items.filter { url in
            (try? url.resourceValues(forKeys: [.isRegularFileKey]).isRegularFile) == true && isSupported(url)
        }
    }

    /// Human-readable format name ("JPEG image", "HEIF image" ...).
    static func formatName(_ url: URL, typeIdentifier: String?) -> String {
        if let id = typeIdentifier, let t = UTType(id) { return t.localizedDescription ?? id }
        return type(of: url)?.localizedDescription ?? url.pathExtension.uppercased()
    }
}

/// A decoded image ready to show: pages (multi-page TIFF, PDF) or animation frames (GIF, APNG, WebP, HEICS).
struct LoadedImage {
    var url: URL
    var pages: [CGImage] = []
    var pageCount = 1
    var isAnimated = false
    var frameDelays: [Double] = []
    var typeIdentifier: String?
    /// Size of the stored image (before orientation), for the info panel.
    var pixelSize: CGSize = .zero
    var orientation: CGImagePropertyOrientation = .up
    var hasAlpha = false
    /// Display decoding may be bounded; never silently overwrite full-resolution source pixels with a proxy.
    var wasDownsampled = false
    var error: String?

    var image: CGImage? { pages.first }
}

enum ImageLoader {
    static let maxDisplayDimension = 16_384
    static let maxPixelCount: CGFloat = 100_000_000

    static func validPixelSize(_ size: CGSize) -> Bool {
        size.width.isFinite && size.height.isFinite && size.width >= 1 && size.height >= 1
            && size.width <= CGFloat(maxDisplayDimension) && size.height <= CGFloat(maxDisplayDimension)
            && size.width * size.height <= maxPixelCount
    }

    /// Decodes the first page (all frames for animations). Runs on any thread.
    static func load(_ url: URL, page: Int = 0) -> LoadedImage {
        var result = LoadedImage(url: url)
        guard page >= 0 else { result.error = L("The image is damaged and cannot be shown."); return result }
        if ImageFormats.isPDF(url) { return loadPDF(url, page: page) }
        if ImageFormats.isSVG(url) { return loadSVG(url) }
        guard let source = CGImageSourceCreateWithURL(url as CFURL, [kCGImageSourceShouldCache: false] as CFDictionary) else {
            result.error = L("The file cannot be read.")
            return result
        }
        result.typeIdentifier = CGImageSourceGetType(source) as String?
        let count = CGImageSourceGetCount(source)
        guard count > 0 else {
            result.error = CGImageSourceGetStatus(source) == .statusUnknownType || result.typeIdentifier == nil
                ? L("This file is not an image or its format is not supported.") : L("The image is damaged and cannot be shown.")
            return result
        }
        let props = CGImageSourceCopyPropertiesAtIndex(source, min(page, count - 1), nil) as? [CFString: Any] ?? [:]
        let w = props[kCGImagePropertyPixelWidth] as? Int ?? 0, h = props[kCGImagePropertyPixelHeight] as? Int ?? 0
        result.pixelSize = CGSize(width: w, height: h)
        result.hasAlpha = props[kCGImagePropertyHasAlpha] as? Bool ?? false
        if let raw = props[kCGImagePropertyOrientation] as? UInt32, let o = CGImagePropertyOrientation(rawValue: raw) { result.orientation = o }
        result.pageCount = count

        let type = result.typeIdentifier.flatMap(UTType.init)
        let animatable = [UTType.gif, .png, .webP, UTType("public.heics"), UTType("public.avis")].compactMap { $0 }
        if count > 1, let type, animatable.contains(where: { type.conforms(to: $0) }) {
            result.isAnimated = true
            for i in 0..<min(count, 2000) {
                guard let frame = decode(source, index: i, orientation: .up) else { continue }
                result.pages.append(frame)
                result.frameDelays.append(delay(source, index: i))
            }
            result.pageCount = 1
        } else if let image = decode(source, index: min(page, count - 1), orientation: result.orientation) {
            result.pages = [image]
        }
        if let image = result.image, w > 0, h > 0 {
            let swapsAxes = !result.isAnimated && Orientation.swapsAxes(result.orientation)
            let storedSize = swapsAxes ? CGSize(width: h, height: w) : CGSize(width: w, height: h)
            result.wasDownsampled = image.width != Int(storedSize.width) || image.height != Int(storedSize.height)
        }
        if result.pages.isEmpty { result.error = L("The image is damaged and cannot be shown.") }
        return result
    }

    /// Full-resolution decode with EXIF orientation applied (huge images are limited to what the GPU can show).
    static func decode(_ source: CGImageSource, index: Int, orientation: CGImagePropertyOrientation) -> CGImage? {
        guard index >= 0, index < CGImageSourceGetCount(source) else { return nil }
        let properties = CGImageSourceCopyPropertiesAtIndex(source, index, nil) as? [CFString: Any] ?? [:]
        let width = (properties[kCGImagePropertyPixelWidth] as? NSNumber)?.doubleValue ?? 0
        let height = (properties[kCGImagePropertyPixelHeight] as? NSNumber)?.doubleValue ?? 0
        if width.isFinite, height.isFinite, width > 0, height > 0,
           !validPixelSize(CGSize(width: width, height: height)) {
            let scale = min(Double(maxDisplayDimension) / max(width, height), sqrt(Double(maxPixelCount) / width / height))
            let maximum = max(1, Int((max(width, height) * scale).rounded(.down)))
            let options: [CFString: Any] = [kCGImageSourceCreateThumbnailFromImageAlways: true,
                kCGImageSourceCreateThumbnailWithTransform: true, kCGImageSourceThumbnailMaxPixelSize: maximum]
            return CGImageSourceCreateThumbnailAtIndex(source, index, options as CFDictionary)
        }
        guard let image = CGImageSourceCreateImageAtIndex(source, index, [kCGImageSourceShouldCacheImmediately: true] as CFDictionary) else {
            return nil
        }
        var out = Orientation.apply(orientation, to: image)
        if max(out.width, out.height) > maxDisplayDimension {
            let opts: [CFString: Any] = [kCGImageSourceCreateThumbnailFromImageAlways: true, kCGImageSourceCreateThumbnailWithTransform: true,
                                         kCGImageSourceThumbnailMaxPixelSize: maxDisplayDimension]
            if let small = CGImageSourceCreateThumbnailAtIndex(source, index, opts as CFDictionary) { out = small }
        }
        return out
    }

    static func delay(_ source: CGImageSource, index: Int) -> Double {
        let p = CGImageSourceCopyPropertiesAtIndex(source, index, nil) as? [CFString: Any] ?? [:]
        let dicts: [(CFString, CFString, CFString)] = [
            (kCGImagePropertyGIFDictionary, kCGImagePropertyGIFUnclampedDelayTime, kCGImagePropertyGIFDelayTime),
            (kCGImagePropertyPNGDictionary, kCGImagePropertyAPNGUnclampedDelayTime, kCGImagePropertyAPNGDelayTime),
            (kCGImagePropertyWebPDictionary, kCGImagePropertyWebPUnclampedDelayTime, kCGImagePropertyWebPDelayTime),
            (kCGImagePropertyHEICSDictionary, kCGImagePropertyHEICSUnclampedDelayTime, kCGImagePropertyHEICSDelayTime),
        ]
        for (dict, unclamped, clamped) in dicts {
            if let d = p[dict] as? [CFString: Any] {
                let v = (d[unclamped] as? Double).flatMap { $0 > 0 ? $0 : nil } ?? d[clamped] as? Double ?? 0.1
                return v < 0.02 ? 0.1 : v
            }
        }
        return 0.1
    }

    static func loadPDF(_ url: URL, page: Int) -> LoadedImage {
        var result = LoadedImage(url: url)
        result.typeIdentifier = UTType.pdf.identifier
        guard page >= 0, let doc = PDFDocument(url: url), !doc.isLocked, doc.pageCount > 0, let p = doc.page(at: min(page, doc.pageCount - 1)) else {
            result.error = L("The image is damaged and cannot be shown.")
            return result
        }
        result.pageCount = doc.pageCount
        let box = p.bounds(for: .cropBox).size
        let rotated = p.rotation % 180 == 0 ? box : CGSize(width: box.height, height: box.width)
        let scale = min(3, 4000 / max(rotated.width, rotated.height, 1))
        let size = CGSize(width: (rotated.width * scale).rounded(), height: (rotated.height * scale).rounded())
        result.pixelSize = size
        result.pages = [render(size: size) { ctx in
            ctx.setFillColor(.white)
            ctx.fill(CGRect(origin: .zero, size: size))
            ctx.scaleBy(x: scale, y: scale)
            p.draw(with: .cropBox, to: ctx)
        }].compactMap { $0 }
        if result.pages.isEmpty { result.error = L("The image is damaged and cannot be shown.") }
        return result
    }

    static func loadSVG(_ url: URL) -> LoadedImage {
        var result = LoadedImage(url: url)
        result.typeIdentifier = ImageFormats.svg.identifier
        guard let image = NSImage(contentsOf: url), image.size.width > 0, image.size.height > 0 else {
            result.error = L("The image is damaged and cannot be shown.")
            return result
        }
        // Vector: rasterise sharp enough for zooming.
        let scale = min(4, 4096 / max(image.size.width, image.size.height))
        let size = CGSize(width: max(1, (image.size.width * scale).rounded()), height: max(1, (image.size.height * scale).rounded()))
        result.pixelSize = image.size
        result.hasAlpha = true
        result.pages = [render(size: size) { ctx in
            NSGraphicsContext.saveGraphicsState()
            NSGraphicsContext.current = NSGraphicsContext(cgContext: ctx, flipped: false)
            image.draw(in: CGRect(origin: .zero, size: size))
            NSGraphicsContext.restoreGraphicsState()
        }].compactMap { $0 }
        if result.pages.isEmpty { result.error = L("The image is damaged and cannot be shown.") }
        return result
    }

    static func render(size: CGSize, colorSpace: CGColorSpace? = nil, _ body: (CGContext) -> Void) -> CGImage? {
        guard validPixelSize(size) else { return nil }
        let cs = colorSpace ?? CGColorSpace(name: CGColorSpace.sRGB)!
        guard let ctx = CGContext(data: nil, width: Int(size.width), height: Int(size.height), bitsPerComponent: 8, bytesPerRow: 0,
                                  space: cs, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return nil }
        ctx.interpolationQuality = .high
        body(ctx)
        return ctx.makeImage()
    }

    // MARK: - Thumbnails

    static let thumbnailCache = NSCache<NSURL, CGImage>()

    static func thumbnail(_ url: URL, maxPixels: Int = 220) -> CGImage? {
        if let cached = thumbnailCache.object(forKey: url as NSURL) { return cached }
        var image: CGImage?
        if ImageFormats.isPDF(url) || ImageFormats.isSVG(url) {
            if let full = load(url).image { image = scaled(full, maxPixels: maxPixels) }
        } else if let source = CGImageSourceCreateWithURL(url as CFURL, nil) {
            let opts: [CFString: Any] = [kCGImageSourceCreateThumbnailFromImageIfAbsent: true, kCGImageSourceCreateThumbnailWithTransform: true,
                                         kCGImageSourceThumbnailMaxPixelSize: maxPixels, kCGImageSourceShouldCacheImmediately: true]
            image = CGImageSourceCreateThumbnailAtIndex(source, 0, opts as CFDictionary)
            // Embedded EXIF thumbnails can be tiny; rebuild from the image then.
            if let i = image, max(i.width, i.height) < maxPixels / 2 {
                var o = opts
                o[kCGImageSourceCreateThumbnailFromImageAlways] = true
                image = CGImageSourceCreateThumbnailAtIndex(source, 0, o as CFDictionary) ?? i
            }
        }
        if let image { thumbnailCache.setObject(image, forKey: url as NSURL) }
        return image
    }

    static func scaled(_ image: CGImage, maxPixels: Int) -> CGImage? {
        guard maxPixels > 0 else { return nil }
        let s = min(1, CGFloat(maxPixels) / CGFloat(max(image.width, image.height)))
        let size = CGSize(width: max(1, (CGFloat(image.width) * s).rounded()), height: max(1, (CGFloat(image.height) * s).rounded()))
        return render(size: size) { $0.draw(image, in: CGRect(origin: .zero, size: size)) }
    }

    // MARK: - Metadata

    static func properties(_ url: URL) -> [CFString: Any] {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil) else { return [:] }
        return CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any] ?? [:]
    }

    /// EXIF DateTimeOriginal ("2024:05:01 12:00:00").
    static func dateTaken(_ url: URL) -> Date? {
        let p = properties(url)
        let exif = p[kCGImagePropertyExifDictionary] as? [CFString: Any]
        let tiff = p[kCGImagePropertyTIFFDictionary] as? [CFString: Any]
        guard let s = exif?[kCGImagePropertyExifDateTimeOriginal] as? String ?? tiff?[kCGImagePropertyTIFFDateTime] as? String else { return nil }
        return exifDateFormatter.date(from: s)
    }

    static let exifDateFormatter: DateFormatter = {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.dateFormat = "yyyy:MM:dd HH:mm:ss"
        return f
    }()
}
