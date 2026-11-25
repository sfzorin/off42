import AppKit
import CoreImage

/// Deterministic raster regressions; no window, file picker, network or Vision model is needed.
enum ImagePixelAudit {
    static func fixture(width: Int = 96, height: Int = 64) -> CGImage {
        var bytes = [UInt8](repeating: 255, count: width * height * 4)
        for y in 0..<height { for x in 0..<width {
            let i = (y * width + x) * 4
            bytes[i] = UInt8((x * 5 + y * 2) % 256)
            bytes[i + 1] = UInt8((y * 7 + x) % 256)
            bytes[i + 2] = UInt8((x / 7 + y / 5) % 2 == 0 ? 35 : 220)
        } }
        return image(bytes, width: width, height: height)
    }

    static func image(_ bytes: [UInt8], width: Int, height: Int) -> CGImage {
        CGImage(width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: width * 4,
                space: CGColorSpace(name: CGColorSpace.sRGB)!,
                bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue | CGBitmapInfo.byteOrder32Big.rawValue),
                provider: CGDataProvider(data: Data(bytes) as CFData)!, decode: nil, shouldInterpolate: false, intent: .defaultIntent)!
    }

    static func pixels(_ image: CGImage) -> [UInt8] {
        let context = CGContext(data: nil, width: image.width, height: image.height, bitsPerComponent: 8, bytesPerRow: image.width * 4,
                                space: CGColorSpace(name: CGColorSpace.sRGB)!,
                                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue | CGBitmapInfo.byteOrder32Big.rawValue)!
        context.interpolationQuality = .none
        context.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
        return Array(UnsafeBufferPointer(start: context.data!.assumingMemoryBound(to: UInt8.self), count: image.width * image.height * 4))
    }

    static func sample(_ image: CGImage, _ x: Int, _ y: Int) -> [UInt8] {
        let bytes = pixels(image), offset = (y * image.width + x) * 4
        return Array(bytes[offset..<(offset + 4)])
    }

    static func rectSelection(_ rect: CGRect, size: CGSize) -> PixelSelection {
        PixelSelection.from(path: CGPath(rect: rect, transform: nil), size: size, isRectangle: true)!
    }

    static func run() -> Int {
        var count = 0, failed = 0
        func check(_ value: Bool, _ name: String) { count += 1; if !value { failed += 1 }; print(value ? "PASS" : "FAIL", "IMG pixels:", name) }
        let source = fixture(), size = CGSize(width: source.width, height: source.height)
        check(sample(source, 0, 0) == [0, 0, 35, 255] && sample(source, 10, 12) == [74, 94, 220, 255], "fixture preserves explicit top-down RGBA coordinates")
        let rectangle = rectSelection(CGRect(x: 10, y: 12, width: 30, height: 20), size: size)
        if let cropped = PixelOps.extract(rectangle, from: source) {
            check(cropped.width == 30 && cropped.height == 20, "rectangular crop keeps exact selected dimensions")
            check(sample(cropped, 0, 0) == sample(source, 10, 12) && sample(cropped, 29, 19) == sample(source, 39, 31), "rectangular crop uses the selected top-down source pixels")
        } else { check(false, "rectangular crop produces output") }
        if let deleted = PixelOps.delete(rectangle, from: source) {
            check(sample(deleted, 20, 20)[3] == 0 && sample(deleted, 5, 5) == sample(source, 5, 5), "Delete clears selected pixels and preserves unselected pixels")
        } else { check(false, "Delete produces output") }
        let ellipsePath = CGPath(ellipseIn: CGRect(x: 10, y: 12, width: 30, height: 20), transform: nil)
        let ellipse = PixelSelection.from(path: ellipsePath, size: size)!
        if let cropped = PixelOps.extract(ellipse, from: source) {
            check(cropped.width == 30 && cropped.height == 20 && sample(cropped, 0, 0)[3] == 0 && sample(cropped, 15, 10)[3] == 255,
                  "elliptical crop retains a transparent outside and an opaque selected centre")
        } else { check(false, "elliptical crop produces output") }
        if let inverse = rectangle.inverted(), let deleted = PixelOps.delete(inverse, from: source) {
            check(inverse.bounds == CGRect(origin: .zero, size: size), "inverted selection spans the full image")
            check(sample(deleted, 20, 20) == sample(source, 20, 20) && sample(deleted, 5, 5)[3] == 0, "inverted Delete keeps the original selected rectangle")
        } else { check(false, "inverted selection produces output") }
        let triangle = CGMutablePath(); triangle.addLines(between: [CGPoint(x: 10, y: 10), CGPoint(x: 50, y: 10), CGPoint(x: 10, y: 50)]); triangle.closeSubpath()
        if let selected = PixelSelection.from(path: triangle, size: size), let cropped = PixelOps.extract(selected, from: source) {
            check(sample(cropped, 5, 5)[3] == 255 && sample(cropped, 35, 35)[3] == 0, "lasso crop respects the polygon mask")
        } else { check(false, "lasso selection produces output") }
        let otherSize = rectSelection(CGRect(x: 0, y: 0, width: 10, height: 10), size: CGSize(width: 20, height: 20))
        check(PixelOps.delete(otherSize, from: source) == nil && PixelOps.extract(otherSize, from: source) == nil, "a stale selection from another image cannot be scaled onto current pixels")
        for bad in [CGSize.zero, CGSize(width: CGFloat.nan, height: 10), CGSize(width: CGFloat.infinity, height: 10), CGSize(width: 20000, height: 10), CGSize(width: 12000, height: 12000)] {
            check(PixelSelection.grayContext(bad) == nil && PixelOps.rgbContext(bad, like: source) == nil, "invalid or oversized mask allocation is rejected")
        }
        var colors = [UInt8](repeating: 255, count: 40 * 20 * 4)
        for y in 0..<20 { for x in 0..<40 where x < 12 || x >= 28 {
            let i = (y * 40 + x) * 4; colors[i] = 10; colors[i + 1] = 20; colors[i + 2] = 30
        } }
        let islands = image(colors, width: 40, height: 20)
        if let selected = PixelSelection.similarColors(in: islands, seed: CGPoint(x: 3, y: 5), tolerance: 0), let deleted = PixelOps.delete(selected, from: islands) {
            check(selected.bounds == CGRect(x: 0, y: 0, width: 12, height: 20), "Instant Alpha follows only the connected same-colour component")
            check(sample(deleted, 3, 5)[3] == 0 && sample(deleted, 30, 5)[3] == 255, "Instant Alpha does not remove a disconnected colour island")
        } else { check(false, "Instant Alpha returns a selection") }
        check(PixelSelection.similarColors(in: islands, seed: CGPoint(x: -0.1, y: 2), tolerance: 0.2) == nil, "negative fractional Alpha seeds do not truncate into the image")
        check(PixelSelection.similarColors(in: islands, seed: CGPoint(x: CGFloat.nan, y: 2), tolerance: 0.2) == nil, "nonfinite Alpha seeds are rejected")
        check(PixelSelection.similarColors(in: islands, seed: CGPoint(x: 2, y: 2), tolerance: CGFloat.infinity) == nil, "nonfinite Alpha tolerance is rejected")
        for op in [Orientation.Op.rotateLeft, .rotateRight, .flipHorizontal, .flipVertical] {
            let output = Orientation.apply(op.orientation, to: source), transform = Orientation.transform(op, size: size)
            let swapped = Orientation.swapsAxes(op.orientation)
            check(output.width == (swapped ? source.height : source.width) && output.height == (swapped ? source.width : source.height), "pixel transform dimensions: \(op)")
            let old = CGPoint(x: 10.5, y: 12.5), mapped = old.applying(transform)
            check(sample(output, Int(mapped.x), Int(mapped.y)) == sample(source, 10, 12), "pixel transform matches annotation coordinates: \(op)")
        }
        for key in Adjustments.Key.allCases {
            var adjustments = Adjustments(); adjustments[key] = key == .sharpness || key == .vignette ? 100 : 65
            let result = adjustments.render(source)
            check(result.width == source.width && result.height == source.height && pixels(result) != pixels(source), "colour adjustment changes pixels without changing dimensions: " + key.rawValue)
        }
        for look in Adjustments.Look.allCases where look != .none {
            var adjustments = Adjustments(); adjustments.look = look
            check(pixels(adjustments.render(source)) != pixels(source), "look changes the rendered pixels: \(look)")
        }
        var highlights = Adjustments(); highlights[.highlights] = 65
        let sourceBytes = pixels(source), brightened = pixels(highlights.render(source))
        var brightOffsets: [Int] = []
        var originalHighlights: Int = 0
        var liftedHighlights: Int = 0
        for offset in stride(from: 0, to: sourceBytes.count, by: 4) {
            let red = Int(sourceBytes[offset])
            let green = Int(sourceBytes[offset + 1])
            let blue = Int(sourceBytes[offset + 2])
            let originalSum: Int = red + green + blue
            guard originalSum > 540 else { continue }
            brightOffsets.append(offset)
            originalHighlights += originalSum
            let liftedRed = Int(brightened[offset])
            let liftedGreen = Int(brightened[offset + 1])
            let liftedBlue = Int(brightened[offset + 2])
            liftedHighlights += liftedRed + liftedGreen + liftedBlue
        }
        check(!brightOffsets.isEmpty && liftedHighlights > originalHighlights, "positive Highlights brightens the high-luminance source pixels")
        highlights[.highlights] = -65
        check(pixels(highlights.render(source)) != sourceBytes, "negative Highlights retains its distinct darkening effect")
        var bounded = Adjustments(); bounded[.exposure] = .infinity; bounded[.contrast] = .nan
        check(bounded.isIdentity, "nonfinite adjustment values cannot reach Core Image")
        bounded[.saturation] = 900; bounded[.sharpness] = -100
        check(bounded[.saturation] == 100 && bounded[.sharpness] == 0, "adjustments remain within their control ranges")
        if let filter = CIFilter(name: "CIExposureAdjust") {
            let sentinel = CIImage(color: .red); filter.setValue(sentinel, forKey: kCIInputImageKey); filter.setValue(1, forKey: kCIInputEVKey)
            var automatic = Adjustments(); automatic.autoEnhance = true
            _ = automatic.apply(to: CIImage(cgImage: source), autoFilters: [filter])
            check(filter.value(forKey: kCIInputImageKey) as? CIImage === sentinel, "concurrent adjustment renders do not mutate shared automatic filter templates")
        } else { check(false, "automatic-filter fixture exists") }
        var circleBytes = [UInt8](repeating: 255, count: 128 * 128 * 4)
        for y in 0..<128 { for x in 0..<128 where hypot(Double(x) - 64, Double(y) - 64) <= 32 {
            let i = (y * 128 + x) * 4; circleBytes[i] = 20; circleBytes[i + 1] = 30; circleBytes[i + 2] = 170
        } }
        let circle = image(circleBytes, width: 128, height: 128)
        let stroke: [CGPoint] = (0..<48).map { i in
            let angle = Double(i) * Double.pi / 24.0
            return CGPoint(x: 64.0 + cos(angle) * 35.0, y: 64.0 + sin(angle) * 35.0)
        }
        if let path = SmartLasso.refine(stroke: stroke, radius: 10, image: circle, subject: nil, options: .init(useSubjectMask: false, useRegionFit: true)),
           let selection = PixelSelection.from(path: path, size: CGSize(width: 128, height: 128)),
           let selected = SmartLassoEval.binary(selection.mask, CGSize(width: 128, height: 128)) {
            var intersection = 0, union = 0
            for y in 0..<128 { for x in 0..<128 {
                let expected = hypot(Double(x) - 64, Double(y) - 64) <= 32, actual = selected[y * 128 + x]
                if expected && actual { intersection += 1 }; if expected || actual { union += 1 }
            } }
            check(Double(intersection) / Double(union) > 0.85, "Smart Lasso follows a high-contrast subject edge (IoU > 0.85)")
        } else { check(false, "Smart Lasso produces a closed selection") }
        check(SmartLasso.refine(stroke: stroke, radius: .infinity, image: circle, subject: nil) == nil, "Smart Lasso rejects nonfinite radius")
        check(SmartLasso.resampleClosed(stroke, spacing: 0).isEmpty, "Smart Lasso rejects zero sample spacing instead of looping")
        check(SmartLasso.resampleClosed([.zero, CGPoint(x: 1e30, y: 1), CGPoint(x: 2, y: 2)], spacing: 1).isEmpty, "Smart Lasso bounds pathological contour expansion")
        print("IMG42 pixel regressions: \(count - failed)/\(count) passed")
        return failed
    }
}
