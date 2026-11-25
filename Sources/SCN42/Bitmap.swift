import CoreGraphics
import Foundation

/// 8-bit pixels (1 channel gray or 4 channels RGBX), row 0 at the top. Used for the measurements and the
/// document clean-up that Core Image has no ready filter for.
struct Bitmap {
    let width: Int
    let height: Int
    let channels: Int
    var data: [UInt8]

    static let sRGB = CGColorSpace(name: CGColorSpace.sRGB)!
    static let gray = CGColorSpaceCreateDeviceGray()

    /// Supports the standard paper sizes at 1200 dpi, while bounding decoded scratch buffers.
    static func validSize(width: Int, height: Int, channels: Int) -> Bool {
        guard width > 0, height > 0, width <= 32_768, height <= 32_768,
              channels == 1 || channels == 4 else { return false }
        return width <= 256_000_000 / height
    }

    var isValid: Bool { Self.validSize(width: width, height: height, channels: channels) && data.count == width * height * channels }

    init(width: Int, height: Int, channels: Int, fill: UInt8 = 0) {
        guard Self.validSize(width: width, height: height, channels: channels) else {
            self.width = 0; self.height = 0; self.channels = channels == 1 ? 1 : 4; self.data = []
            return
        }
        self.width = width
        self.height = height
        self.channels = channels
        data = [UInt8](repeating: fill, count: width * height * channels)
    }

    /// Draws `image` (scaled so its longer side is at most `maxSide`) into gray or RGBX pixels.
    init(_ image: CGImage, maxSide: Int? = nil, gray: Bool) {
        var w = image.width, h = image.height
        if let maxSide, maxSide > 0, max(w, h) > maxSide {
            let s = Double(maxSide) / Double(max(w, h))
            w = max(1, Int((Double(w) * s).rounded()))
            h = max(1, Int((Double(h) * s).rounded()))
        }
        self.init(width: w, height: h, channels: gray ? 1 : 4, fill: 255)
        guard isValid else { return }
        let space = gray ? Bitmap.gray : Bitmap.sRGB
        let info = gray ? CGImageAlphaInfo.none.rawValue : CGImageAlphaInfo.noneSkipLast.rawValue
        data.withUnsafeMutableBytes { raw in
            guard let ctx = CGContext(data: raw.baseAddress, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * channels,
                                      space: space, bitmapInfo: info) else { return }
            ctx.interpolationQuality = .high
            ctx.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
        }
    }

    func cgImage() -> CGImage? {
        guard isValid else { return nil }
        let space = channels == 1 ? Bitmap.gray : Bitmap.sRGB
        let info = channels == 1 ? CGImageAlphaInfo.none.rawValue : CGImageAlphaInfo.noneSkipLast.rawValue
        guard let provider = CGDataProvider(data: Data(data) as CFData) else { return nil }
        return CGImage(width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 8 * channels, bytesPerRow: width * channels,
                       space: space, bitmapInfo: CGBitmapInfo(rawValue: info), provider: provider, decode: nil, shouldInterpolate: true,
                       intent: .defaultIntent)
    }

    /// 1-bit image (black where the gray value is below `threshold`): small PDFs and TIFFs for B&W pages.
    func oneBitImage(threshold: UInt8) -> CGImage? {
        guard isValid, channels == 1 else { return nil }
        let rowBytes = (width + 7) / 8
        var bits = [UInt8](repeating: 0, count: rowBytes * height)
        data.withUnsafeBufferPointer { src in
            bits.withUnsafeMutableBufferPointer { dst in
                for y in 0..<height {
                    let s = y * width, d = y * rowBytes
                    for x in 0..<width where src[s + x] >= threshold {
                        dst[d + (x >> 3)] |= 0x80 >> UInt8(x & 7)   // 1 = white in DeviceGray
                    }
                }
            }
        }
        guard let provider = CGDataProvider(data: Data(bits) as CFData) else { return nil }
        return CGImage(width: width, height: height, bitsPerComponent: 1, bitsPerPixel: 1, bytesPerRow: rowBytes, space: Bitmap.gray,
                       bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.none.rawValue), provider: provider, decode: nil,
                       shouldInterpolate: false, intent: .defaultIntent)
    }

    /// Luminance of a 4-channel bitmap.
    func grayscale() -> Bitmap {
        guard isValid, channels == 4 else { return self }
        var out = Bitmap(width: width, height: height, channels: 1)
        data.withUnsafeBufferPointer { s in
            out.data.withUnsafeMutableBufferPointer { d in
                for i in 0..<(width * height) {
                    let r = Int(s[i * 4]), g = Int(s[i * 4 + 1]), b = Int(s[i * 4 + 2])
                    d[i] = UInt8((r * 77 + g * 150 + b * 29) >> 8)
                }
            }
        }
        return out
    }

    /// Box blur with an integral image (any radius, O(1) per pixel), per channel.
    func boxBlur(radius r: Int) -> Bitmap {
        guard isValid, r > 0 else { return self }
        let r = min(r, max(width, height))
        var out = self
        let w = width, h = height, c = channels
        var integral = [Int](repeating: 0, count: (w + 1) * (h + 1))
        for ch in 0..<c {
            data.withUnsafeBufferPointer { s in
                integral.withUnsafeMutableBufferPointer { ii in
                    for y in 0..<h {
                        var row = 0
                        for x in 0..<w {
                            row += Int(s[(y * w + x) * c + ch])
                            ii[(y + 1) * (w + 1) + x + 1] = ii[y * (w + 1) + x + 1] + row
                        }
                    }
                }
            }
            integral.withUnsafeBufferPointer { ii in
                out.data.withUnsafeMutableBufferPointer { d in
                    for y in 0..<h {
                        let y0 = max(0, y - r), y1 = min(h, y + r + 1)
                        for x in 0..<w {
                            let x0 = max(0, x - r), x1 = min(w, x + r + 1)
                            let sum = ii[y1 * (w + 1) + x1] - ii[y0 * (w + 1) + x1] - ii[y1 * (w + 1) + x0] + ii[y0 * (w + 1) + x0]
                            d[(y * w + x) * c + ch] = UInt8(sum / ((y1 - y0) * (x1 - x0)))
                        }
                    }
                }
            }
        }
        return out
    }

    /// Separable max filter (grey dilation): removes dark text strokes from a background estimate.
    func maxFilter(radius r: Int) -> Bitmap {
        guard isValid, r > 0 else { return self }
        let r = min(r, max(width, height))
        let w = width, h = height, c = channels
        var tmp = self
        data.withUnsafeBufferPointer { s in
            tmp.data.withUnsafeMutableBufferPointer { d in
                for y in 0..<h {
                    for x in 0..<w {
                        let x0 = max(0, x - r), x1 = min(w - 1, x + r)
                        for ch in 0..<c {
                            var m: UInt8 = 0
                            for xx in x0...x1 { m = max(m, s[(y * w + xx) * c + ch]) }
                            d[(y * w + x) * c + ch] = m
                        }
                    }
                }
            }
        }
        var out = tmp
        tmp.data.withUnsafeBufferPointer { s in
            out.data.withUnsafeMutableBufferPointer { d in
                for y in 0..<h {
                    let y0 = max(0, y - r), y1 = min(h - 1, y + r)
                    for x in 0..<w {
                        for ch in 0..<c {
                            var m: UInt8 = 0
                            for yy in y0...y1 { m = max(m, s[(yy * w + x) * c + ch]) }
                            d[(y * w + x) * c + ch] = m
                        }
                    }
                }
            }
        }
        return out
    }

    /// Scales to an exact size (Core Graphics, high quality).
    func resized(width w: Int, height h: Int) -> Bitmap {
        guard Self.validSize(width: w, height: h, channels: channels), let image = cgImage() else {
            return Bitmap(width: 0, height: 0, channels: channels)
        }
        var out = Bitmap(width: w, height: h, channels: channels, fill: 255)
        let space = channels == 1 ? Bitmap.gray : Bitmap.sRGB
        let info = channels == 1 ? CGImageAlphaInfo.none.rawValue : CGImageAlphaInfo.noneSkipLast.rawValue
        out.data.withUnsafeMutableBytes { raw in
            guard let ctx = CGContext(data: raw.baseAddress, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * channels,
                                      space: space, bitmapInfo: info) else { return }
            ctx.interpolationQuality = .high
            ctx.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
        }
        return out
    }

    /// Otsu threshold of a gray bitmap.
    func otsu() -> UInt8 {
        var hist = [Int](repeating: 0, count: 256)
        data.withUnsafeBufferPointer { s in for v in s { hist[Int(v)] += 1 } }
        let total = Double(data.count)
        var sum = 0.0
        for i in 0..<256 { sum += Double(i * hist[i]) }
        var sumB = 0.0, wB = 0.0, best = 0.0, threshold = 128
        for t in 0..<256 {
            wB += Double(hist[t])
            if wB == 0 { continue }
            let wF = total - wB
            if wF == 0 { break }
            sumB += Double(t * hist[t])
            let mB = sumB / wB, mF = (sum - sumB) / wF
            let between = wB * wF * (mB - mF) * (mB - mF)
            if between > best { best = between; threshold = t }
        }
        return UInt8(threshold)
    }
}
