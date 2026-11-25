import AppKit
import CoreImage
import Vision

/// A pixel selection (selection tools, Instant Alpha): a grey mask of the image size, white = selected.
struct PixelSelection {
    var mask: CGImage
    /// Outline in image coordinates (y down) for path selections; nil for Instant Alpha masks.
    var outline: CGPath?
    var bounds: CGRect
    var isRectangle = false

    var size: CGSize { CGSize(width: mask.width, height: mask.height) }

    static func grayContext(_ size: CGSize) -> CGContext? {
        guard ImageLoader.validPixelSize(size) else { return nil }
        return CGContext(data: nil, width: Int(size.width), height: Int(size.height), bitsPerComponent: 8, bytesPerRow: 0,
                  space: CGColorSpaceCreateDeviceGray(), bitmapInfo: CGImageAlphaInfo.none.rawValue)
    }

    /// Selection from a path in image coordinates (y down).
    static func from(path: CGPath, size: CGSize, isRectangle: Bool = false) -> PixelSelection? {
        guard let ctx = grayContext(size) else { return nil }
        ctx.setFillColor(gray: 0, alpha: 1)
        ctx.fill(CGRect(origin: .zero, size: size))
        ctx.translateBy(x: 0, y: size.height)
        ctx.scaleBy(x: 1, y: -1)
        ctx.addPath(path)
        ctx.setFillColor(gray: 1, alpha: 1)
        ctx.fillPath(using: .evenOdd)
        guard let mask = ctx.makeImage() else { return nil }
        let b = path.boundingBoxOfPath.intersection(CGRect(origin: .zero, size: size))
        guard !b.isNull, b.width >= 1, b.height >= 1 else { return nil }
        return PixelSelection(mask: mask, outline: path, bounds: b, isRectangle: isRectangle)
    }

    func inverted() -> PixelSelection? {
        guard let ctx = Self.grayContext(size) else { return nil }
        let full = CGRect(origin: .zero, size: size)
        ctx.draw(mask, in: full)
        ctx.setBlendMode(.difference)
        ctx.setFillColor(gray: 1, alpha: 1)
        ctx.fill(full)
        guard let m = ctx.makeImage() else { return nil }
        var outlinePath: CGPath?
        if let outline {
            let p = CGMutablePath()
            p.addRect(full)
            p.addPath(outline)
            outlinePath = p
        }
        return PixelSelection(mask: m, outline: outlinePath, bounds: full)
    }

    /// Instant Alpha: the contiguous region around `seed` whose colour is within `tolerance` (0…1).
    static func similarColors(in image: CGImage, seed: CGPoint, tolerance: CGFloat) -> PixelSelection? {
        let w = image.width, h = image.height
        guard ImageLoader.validPixelSize(CGSize(width: w, height: h)), seed.x.isFinite, seed.y.isFinite,
              seed.x >= 0, seed.y >= 0, seed.x < CGFloat(w), seed.y < CGFloat(h), tolerance.isFinite,
              (0...1).contains(tolerance) else { return nil }
        let x0 = Int(seed.x), y0 = Int(seed.y)
        guard x0 >= 0, y0 >= 0, x0 < w, y0 < h,
              let ctx = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
                                  space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue),
              let data = ctx.data else { return nil }
        ctx.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
        let px = data.bindMemory(to: UInt8.self, capacity: w * h * 4)   // row 0 = top
        var mask = [UInt8](repeating: 0, count: w * h)
        let s = (y0 * w + x0) * 4
        let sr = Int(px[s]), sg = Int(px[s + 1]), sb = Int(px[s + 2]), sa = Int(px[s + 3])
        let tol = Int(tolerance * 255)
        var stack = [y0 * w + x0]
        mask[y0 * w + x0] = 255
        var minX = x0, maxX = x0, minY = y0, maxY = y0
        while let i = stack.popLast() {
            let x = i % w, y = i / w
            minX = min(minX, x); maxX = max(maxX, x); minY = min(minY, y); maxY = max(maxY, y)
            for n in [x > 0 ? i - 1 : -1, x < w - 1 ? i + 1 : -1, y > 0 ? i - w : -1, y < h - 1 ? i + w : -1] where n >= 0 && mask[n] == 0 {
                let o = n * 4
                if abs(Int(px[o]) - sr) <= tol && abs(Int(px[o + 1]) - sg) <= tol && abs(Int(px[o + 2]) - sb) <= tol && abs(Int(px[o + 3]) - sa) <= tol {
                    mask[n] = 255
                    stack.append(n)
                }
            }
        }
        let provider = CGDataProvider(data: Data(mask) as CFData)!
        guard let m = CGImage(width: w, height: h, bitsPerComponent: 8, bitsPerPixel: 8, bytesPerRow: w, space: CGColorSpaceCreateDeviceGray(),
                              bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.none.rawValue), provider: provider, decode: nil,
                              shouldInterpolate: false, intent: .defaultIntent) else { return nil }
        return PixelSelection(mask: m, outline: nil, bounds: CGRect(x: minX, y: minY, width: maxX - minX + 1, height: maxY - minY + 1))
    }
}

enum PixelOps {
    static func rgbContext(_ size: CGSize, like image: CGImage) -> CGContext? {
        guard ImageLoader.validPixelSize(size) else { return nil }
        return CGContext(data: nil, width: Int(size.width), height: Int(size.height), bitsPerComponent: 8, bytesPerRow: 0,
                  space: image.colorSpace.flatMap { $0.model == .rgb ? $0 : nil } ?? CGColorSpace(name: CGColorSpace.sRGB)!,
                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
    }

    /// Makes the selected pixels transparent.
    static func delete(_ sel: PixelSelection, from image: CGImage) -> CGImage? {
        let size = CGSize(width: image.width, height: image.height)
        guard sel.size == size, let ctx = rgbContext(size, like: image) else { return nil }
        let full = CGRect(origin: .zero, size: size)
        ctx.draw(image, in: full)
        ctx.clip(to: full, mask: sel.mask)
        ctx.clear(full)
        return ctx.makeImage()
    }

    /// The selected part, cropped to its bounds; outside a non-rectangular selection is transparent.
    static func extract(_ sel: PixelSelection, from image: CGImage) -> CGImage? {
        let size = CGSize(width: image.width, height: image.height)
        let bounds = sel.bounds
        guard sel.size == size, bounds.origin.x.isFinite, bounds.origin.y.isFinite,
              bounds.width.isFinite, bounds.height.isFinite, bounds.width > 0, bounds.height > 0 else { return nil }
        let r = bounds.integral.intersection(CGRect(origin: .zero, size: size))
        guard !r.isNull, r.width >= 1, r.height >= 1 else { return nil }
        guard let crop = image.cropping(to: r) else { return nil }
        if sel.isRectangle { return crop }
        guard let maskCrop = sel.mask.cropping(to: r), let ctx = rgbContext(r.size, like: image) else { return nil }
        let full = CGRect(origin: .zero, size: r.size)
        ctx.clip(to: full, mask: maskCrop)
        ctx.draw(crop, in: full)
        return ctx.makeImage()
    }

    /// Removes the background with Vision's subject lifting (macOS 14+).
    static func removeBackground(_ image: CGImage) throws -> CGImage? {
        let request = VNGenerateForegroundInstanceMaskRequest()
        let handler = VNImageRequestHandler(cgImage: image)
        try handler.perform([request])
        guard let result = request.results?.first, !result.allInstances.isEmpty else { return nil }
        let buffer = try result.generateMaskedImage(ofInstances: result.allInstances, from: handler, croppedToInstancesExtent: false)
        let ci = CIImage(cvPixelBuffer: buffer)
        return Adjustments.context.createCGImage(ci, from: ci.extent, format: .RGBA8,
                                                 colorSpace: image.colorSpace.flatMap { $0.model == .rgb ? $0 : nil } ?? CGColorSpace(name: CGColorSpace.sRGB)!)
    }

    /// Recognised text (Live Text): lines with their boxes in image coordinates (y down).
    struct TextLine {
        let text: String
        let observation: VNRecognizedText
        let box: CGRect
    }

    static func recognizeText(_ image: CGImage) -> [TextLine] {
        let request = VNRecognizeTextRequest()
        request.recognitionLevel = .accurate
        request.usesLanguageCorrection = true
        request.automaticallyDetectsLanguage = true
        // Russian, English and Turkish first (with automatic detection for the rest).
        let supported = (try? request.supportedRecognitionLanguages()) ?? []
        request.recognitionLanguages = ["ru-RU", "en-US", "tr-TR"].filter(supported.contains)
        let handler = VNImageRequestHandler(cgImage: image)
        try? handler.perform([request])
        let w = CGFloat(image.width), h = CGFloat(image.height)
        return (request.results ?? []).compactMap { obs in
            guard let top = obs.topCandidates(1).first else { return nil }
            return TextLine(text: top.string, observation: top, box: imageRect(obs.boundingBox, w, h))
        }
    }

    static func imageRect(_ normalized: CGRect, _ w: CGFloat, _ h: CGFloat) -> CGRect {
        CGRect(x: normalized.minX * w, y: (1 - normalized.maxY) * h, width: normalized.width * w, height: normalized.height * h)
    }

    /// Boxes of every occurrence of `query` (case- and diacritic-insensitive).
    static func find(_ query: String, in lines: [TextLine], size: CGSize) -> [CGRect] {
        guard !query.isEmpty else { return [] }
        var boxes: [CGRect] = []
        for line in lines {
            var range = line.text.startIndex..<line.text.endIndex
            while let found = line.text.range(of: query, options: [.caseInsensitive, .diacriticInsensitive], range: range) {
                if let obs = try? line.observation.boundingBox(for: found) {
                    boxes.append(imageRect(obs.boundingBox, size.width, size.height))
                } else {
                    boxes.append(line.box)
                }
                range = found.upperBound..<line.text.endIndex
            }
        }
        return boxes
    }
}
