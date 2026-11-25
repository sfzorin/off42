import AppKit
import Kit42
import ImageIO
import UniformTypeIdentifiers

/// EXIF orientation maths and image writing.
enum Orientation {
    /// Geometric operation on an image as shown.
    enum Op: Int { case rotateLeft, rotateRight, flipHorizontal, flipVertical
        /// The orientation that performs this operation on an upright image.
        var orientation: CGImagePropertyOrientation {
            switch self {
            case .rotateLeft: return .left
            case .rotateRight: return .right
            case .flipHorizontal: return .upMirrored
            case .flipVertical: return .downMirrored
            }
        }
    }

    /// Stored → displayed matrix in y-down image coordinates (a, b, c, d of x' = a x + b y, y' = c x + d y).
    static func matrix(_ o: CGImagePropertyOrientation) -> [Int] {
        switch o {
        case .up: return [1, 0, 0, 1]
        case .upMirrored: return [-1, 0, 0, 1]
        case .down: return [-1, 0, 0, -1]
        case .downMirrored: return [1, 0, 0, -1]
        case .right: return [0, -1, 1, 0]
        case .left: return [0, 1, -1, 0]
        case .leftMirrored: return [0, 1, 1, 0]
        case .rightMirrored: return [0, -1, -1, 0]
        @unknown default: return [1, 0, 0, 1]
        }
    }

    /// Orientation after applying `op` to an image stored with orientation `o`.
    static func compose(_ op: Op, after o: CGImagePropertyOrientation) -> CGImagePropertyOrientation {
        let a = matrix(op.orientation), b = matrix(o)
        let m = [a[0] * b[0] + a[1] * b[2], a[0] * b[1] + a[1] * b[3], a[2] * b[0] + a[3] * b[2], a[2] * b[1] + a[3] * b[3]]
        let all: [CGImagePropertyOrientation] = [.up, .upMirrored, .down, .downMirrored, .leftMirrored, .right, .rightMirrored, .left]
        return all.first { matrix($0) == m } ?? .up
    }

    static func swapsAxes(_ o: CGImagePropertyOrientation) -> Bool {
        [.left, .right, .leftMirrored, .rightMirrored].contains(o)
    }

    /// Renders `image` as it should be displayed for orientation `o`.
    static func apply(_ o: CGImagePropertyOrientation, to image: CGImage) -> CGImage {
        guard o != .up else { return image }
        let w = CGFloat(image.width), h = CGFloat(image.height)
        let size = swapsAxes(o) ? CGSize(width: h, height: w) : CGSize(width: w, height: h)
        // Same maths as `matrix`, in CoreGraphics' y-up space, moved back into the positive quadrant.
        let t: CGAffineTransform
        switch o {
        case .upMirrored: t = CGAffineTransform(a: -1, b: 0, c: 0, d: 1, tx: w, ty: 0)
        case .down: t = CGAffineTransform(a: -1, b: 0, c: 0, d: -1, tx: w, ty: h)
        case .downMirrored: t = CGAffineTransform(a: 1, b: 0, c: 0, d: -1, tx: 0, ty: h)
        case .right: t = CGAffineTransform(a: 0, b: -1, c: 1, d: 0, tx: 0, ty: w)
        case .left: t = CGAffineTransform(a: 0, b: 1, c: -1, d: 0, tx: h, ty: 0)
        case .leftMirrored: t = CGAffineTransform(a: 0, b: -1, c: -1, d: 0, tx: h, ty: w)
        case .rightMirrored: t = CGAffineTransform(a: 0, b: 1, c: 1, d: 0, tx: 0, ty: 0)
        default: t = .identity
        }
        let cs = image.colorSpace.flatMap { $0.model == .rgb ? $0 : nil }
        return ImageLoader.render(size: size, colorSpace: cs) { ctx in
            ctx.concatenate(t)
            ctx.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
        } ?? image
    }

    /// Point mapping for marks when the shown image of `size` undergoes `op` (y-down coordinates).
    static func transform(_ op: Op, size: CGSize) -> CGAffineTransform {
        switch op {
        case .rotateRight: return CGAffineTransform(a: 0, b: 1, c: -1, d: 0, tx: size.height, ty: 0)
        case .rotateLeft: return CGAffineTransform(a: 0, b: -1, c: 1, d: 0, tx: 0, ty: size.width)
        case .flipHorizontal: return CGAffineTransform(a: -1, b: 0, c: 0, d: 1, tx: size.width, ty: 0)
        case .flipVertical: return CGAffineTransform(a: 1, b: 0, c: 0, d: -1, tx: 0, ty: size.height)
        }
    }

    /// Changes only the orientation tag (no re-encoding). Works for JPEG, HEIF, PNG and TIFF.
    static func rewriteLosslessly(_ url: URL, orientation: CGImagePropertyOrientation) -> Bool {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil), let type = CGImageSourceGetType(source),
              CGImageSourceGetCount(source) == 1 else { return false }
        let tmp = url.deletingLastPathComponent().appendingPathComponent(".\(UUID().uuidString).\(url.pathExtension)")
        guard let dest = CGImageDestinationCreateWithURL(tmp as CFURL, type, 1, nil) else { return false }
        let options: [CFString: Any] = [kCGImageDestinationOrientation: orientation.rawValue]
        var error: Unmanaged<CFError>?
        guard CGImageDestinationCopyImageSource(dest, source, options as CFDictionary, &error) else {
            try? FileManager.default.removeItem(at: tmp)
            return false
        }
        return replace(url, with: tmp)
    }

    /// Writes description and optional print resolution without re-encoding.
    static func writeMetadata(description text: String?, resolution: CGFloat? = nil, to url: URL) -> Bool {
        if let resolution, !resolution.isFinite || resolution <= 0 { return false }
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil), let type = CGImageSourceGetType(source) else { return false }
        let meta = CGImageMetadataCreateMutable()
        let value = (text ?? "") as CFString
        _ = CGImageMetadataSetValueMatchingImageProperty(meta, kCGImagePropertyIPTCDictionary, kCGImagePropertyIPTCCaptionAbstract, value)
        _ = CGImageMetadataSetValueMatchingImageProperty(meta, kCGImagePropertyTIFFDictionary, kCGImagePropertyTIFFImageDescription, value)
        if let resolution {
            let value = NSNumber(value: Double(resolution))
            _ = CGImageMetadataSetValueMatchingImageProperty(meta, kCGImagePropertyTIFFDictionary, kCGImagePropertyTIFFXResolution, value)
            _ = CGImageMetadataSetValueMatchingImageProperty(meta, kCGImagePropertyTIFFDictionary, kCGImagePropertyTIFFYResolution, value)
            _ = CGImageMetadataSetValueMatchingImageProperty(meta, kCGImagePropertyTIFFDictionary, kCGImagePropertyTIFFResolutionUnit, NSNumber(value: 2))
        }
        let tmp = url.deletingLastPathComponent().appendingPathComponent(".\(UUID().uuidString).\(url.pathExtension)")
        guard let dest = CGImageDestinationCreateWithURL(tmp as CFURL, type, CGImageSourceGetCount(source), nil) else { return false }
        let options: [CFString: Any] = [kCGImageDestinationMetadata: meta, kCGImageDestinationMergeMetadata: true]
        guard CGImageDestinationCopyImageSource(dest, source, options as CFDictionary, nil) else {
            try? FileManager.default.removeItem(at: tmp)
            return false
        }
        if let resolution {
            let actual = ImageLoader.properties(tmp)[kCGImagePropertyDPIWidth] as? CGFloat
            guard let actual, abs(actual - resolution) < 0.05 else {
                try? FileManager.default.removeItem(at: tmp)
                return false
            }
        }
        return replace(url, with: tmp)
    }

    static func replace(_ url: URL, with tmp: URL) -> Bool {
        guard (try? url.resourceValues(forKeys: [.isDirectoryKey]).isDirectory) != true else {
            try? FileManager.default.removeItem(at: tmp)
            return false
        }
        do {
            _ = try FileManager.default.replaceItemAt(url, withItemAt: tmp)
            return true
        } catch {
            try? FileManager.default.removeItem(at: tmp)
            return false
        }
    }
}

enum ImageWriter {
    static var writableTypes: [UTType] { (CGImageDestinationCopyTypeIdentifiers() as? [String] ?? []).compactMap(UTType.init) }

    static func canWrite(_ type: String?) -> Bool {
        guard let type, let t = UTType(type) else { return false }
        // Animated and multi-page images would lose frames when saved as one image.
        return writableTypes.contains(t) && t != .pdf && t != .gif
    }

    static func hasTransparency(_ image: CGImage) -> Bool {
        guard ![CGImageAlphaInfo.none, .noneSkipFirst, .noneSkipLast].contains(image.alphaInfo) else { return false }
        guard let context = CGContext(data: nil, width: image.width, height: image.height, bitsPerComponent: 8,
                                      bytesPerRow: image.width * 4, space: CGColorSpaceCreateDeviceRGB(),
                                      bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue), let data = context.data else { return true }
        context.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
        let bytes = data.bindMemory(to: UInt8.self, capacity: image.width * image.height * 4)
        return stride(from: 3, to: image.width * image.height * 4, by: 4).contains { bytes[$0] < 255 }
    }

    /// Metadata of `source` for an image that is now upright: orientation 1, no stale pixel sizes or thumbnails.
    static func metadata(from source: URL?) -> [CFString: Any] {
        guard let source, var p = Optional(ImageLoader.properties(source)), !p.isEmpty else { return [:] }
        p[kCGImagePropertyOrientation] = 1
        if var tiff = p[kCGImagePropertyTIFFDictionary] as? [CFString: Any] {
            tiff[kCGImagePropertyTIFFOrientation] = 1
            p[kCGImagePropertyTIFFDictionary] = tiff
        }
        for key in [kCGImagePropertyPixelWidth, kCGImagePropertyPixelHeight] {
            p.removeValue(forKey: key)
        }
        if var exif = p[kCGImagePropertyExifDictionary] as? [CFString: Any] {
            exif.removeValue(forKey: kCGImagePropertyExifPixelXDimension)
            exif.removeValue(forKey: kCGImagePropertyExifPixelYDimension)
            p[kCGImagePropertyExifDictionary] = exif
        }
        return p
    }

    /// Writes `image` atomically. `quality` applies to lossy formats.
    static func write(_ image: CGImage, to url: URL, type: String, quality: Double = 0.92, metadataFrom source: URL? = nil,
                      description: String?? = .none, dpi: CGFloat? = nil) throws {
        guard writableTypes.contains(where: { $0.identifier == type }), quality.isFinite, (0...1).contains(quality),
              dpi.map({ $0.isFinite && $0 > 0 }) ?? true,
              (try? url.resourceValues(forKeys: [.isDirectoryKey]).isDirectory) != true else {
            throw NSError(domain: "IMG42", code: 1, userInfo: [NSLocalizedDescriptionKey: L("This format cannot be written.")])
        }
        var props = metadata(from: source)
        if let dpi, dpi.isFinite, dpi > 0 {
            props[kCGImagePropertyDPIWidth] = dpi
            props[kCGImagePropertyDPIHeight] = dpi
            var tiff = props[kCGImagePropertyTIFFDictionary] as? [CFString: Any] ?? [:]
            tiff[kCGImagePropertyTIFFXResolution] = dpi
            tiff[kCGImagePropertyTIFFYResolution] = dpi
            tiff[kCGImagePropertyTIFFResolutionUnit] = 2
            props[kCGImagePropertyTIFFDictionary] = tiff
        }
        if case .some(let text) = description {
            var iptc = props[kCGImagePropertyIPTCDictionary] as? [CFString: Any] ?? [:]
            var tiff = props[kCGImagePropertyTIFFDictionary] as? [CFString: Any] ?? [:]
            iptc[kCGImagePropertyIPTCCaptionAbstract] = text ?? ""
            tiff[kCGImagePropertyTIFFImageDescription] = text ?? ""
            props[kCGImagePropertyIPTCDictionary] = iptc
            props[kCGImagePropertyTIFFDictionary] = tiff
        }
        props[kCGImageDestinationLossyCompressionQuality] = quality
        let tmp = url.deletingLastPathComponent().appendingPathComponent(".\(UUID().uuidString).tmp")
        defer { try? FileManager.default.removeItem(at: tmp) }
        guard let dest = CGImageDestinationCreateWithURL(tmp as CFURL, type as CFString, 1, nil) else {
            throw NSError(domain: "IMG42", code: 1, userInfo: [NSLocalizedDescriptionKey: L("This format cannot be written.")])
        }
        // Formats without alpha (JPEG, BMP) get a white background instead of black.
        var out = image
        if let t = UTType(type), t.conforms(to: .jpeg) || t == .bmp, image.alphaInfo != .none, image.alphaInfo != .noneSkipLast {
            guard let opaque = ImageLoader.render(size: CGSize(width: image.width, height: image.height), colorSpace: image.colorSpace.flatMap { $0.model == .rgb ? $0 : nil }, { ctx in
                ctx.setFillColor(.white)
                ctx.fill(CGRect(x: 0, y: 0, width: image.width, height: image.height))
                ctx.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
            }) else { throw CocoaError(.fileWriteUnknown) }
            out = opaque
        }
        CGImageDestinationAddImage(dest, out, props as CFDictionary)
        guard CGImageDestinationFinalize(dest) else {
            try? FileManager.default.removeItem(at: tmp)
            throw NSError(domain: "IMG42", code: 2, userInfo: [NSLocalizedDescriptionKey: L("The image could not be saved.")])
        }
        if FileManager.default.fileExists(atPath: url.path) {
            guard Orientation.replace(url, with: tmp) else {
                throw NSError(domain: "IMG42", code: 3, userInfo: [NSLocalizedDescriptionKey: L("The image could not be saved.")])
            }
        } else {
            try FileManager.default.moveItem(at: tmp, to: url)
        }
    }
}
