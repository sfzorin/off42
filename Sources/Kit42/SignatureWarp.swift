import AppKit

/// Copies of a signature made from the ink of a real sample rather than from a traced centre line: the sample's ink
/// (darkness against the local paper) is bent the way one hand varies between signatures — slant, width, a slight
/// turn and a smooth, low-frequency bend — and inked again in the chosen colour with the pen's texture. Shapes
/// that tracing gets wrong on small photos (dense crossings, loops, tips) stay exactly as written.
enum SignatureWarp {
    /// A sample picture as stored with the learned signature: grey PNG, at most 800 px tall (enough for a 600-dpi scan
    /// of a signature; copies are drawn from these pixels).
    static func encode(_ image: CGImage) -> Data? {
        let k = min(1, 800 / CGFloat(max(1, image.height)))
        let w = max(1, Int(CGFloat(image.width) * k)), h = max(1, Int(CGFloat(image.height) * k))
        guard let ctx = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0, space: CGColorSpaceCreateDeviceGray(),
                                  bitmapInfo: CGImageAlphaInfo.none.rawValue) else { return nil }
        ctx.setFillColor(gray: 1, alpha: 1); ctx.fill(CGRect(x: 0, y: 0, width: w, height: h))
        ctx.interpolationQuality = .high
        ctx.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
        return ctx.makeImage().flatMap { NSBitmapImageRep(cgImage: $0).representation(using: .png, properties: [:]) }
    }

    static func decode(_ data: Data) -> CGImage? { NSBitmapImageRep(data: data)?.cgImage }

    static func copy(of picture: CGImage, ink: NSColor, seed: UInt64, height target: CGFloat = 300) -> CGImage? {
        // Work at a usable size (a signature cut from a sheet photo is often under 100 px tall).
        let scale = max(1, target / 300)   // pixels per base pixel of the usual 300-px copy
        let k = max(1, min(4 * scale, target / CGFloat(max(1, picture.height))))
        let w = Int(CGFloat(picture.width) * k), h = Int(CGFloat(picture.height) * k)
        guard w > 8, h > 8, let src = InkDraw.context(w, h), let data = src.data?.assumingMemoryBound(to: UInt8.self) else { return nil }
        src.setFillColor(.white); src.fill(CGRect(x: 0, y: 0, width: w, height: h))
        src.interpolationQuality = .high
        src.draw(picture, in: CGRect(x: 0, y: 0, width: w, height: h))
        let lum = (0..<(w * h)).map { i in (Double(data[i * 4]) * 3 + Double(data[i * 4 + 1]) * 6 + Double(data[i * 4 + 2])) / 10 }
        let dark = SignatureLearning.darkness(lum, w, h)
        let t = SignatureLearning.inkThreshold(dark)
        // Ink coverage 0…1: paper below half the threshold, full ink at twice it.
        let cover = dark.map { min(1, max(0, ($0 - t * 0.5) / (t * 1.5))) }
        // This copy's hand: slant, width, height, a slight turn, and a smooth bend.
        var r = SeededRandom(seed)
        let shear = Double(r.range(-0.07...0.07)), sx = Double(r.range(0.94...1.06)), sy = Double(r.range(0.96...1.04))
        let turn = Double(r.range(-0.035...0.035)), ct = cos(turn), st = sin(turn)
        let bends = (0..<3).map { _ in (a: Double(r.range(0.006...0.014)) * Double(h), fx: Double(r.range(0.6...1.6)) / Double(w),
                                        fy: Double(r.range(0.4...1.2)) / Double(h), p: Double(r.range(0...6.28)), q: Double(r.range(0...6.28))) }
        // Output canvas with room for the bend.
        let pad = Int(Double(h) * 0.12)
        let ow = Int(Double(w) * 1.12) + 2 * pad, oh = h + 2 * pad
        guard let out = InkDraw.context(ow, oh), let od = out.data?.assumingMemoryBound(to: UInt8.self) else { return nil }
        let cx = Double(ow) / 2, cy = Double(oh) / 2, scx = Double(w) / 2, scy = Double(h) / 2
        let c = (ink.usingColorSpace(.sRGB) ?? ink)
        let ir = Double(c.redComponent), ig = Double(c.greenComponent), ib = Double(c.blueComponent)
        func sample(_ x: Double, _ y: Double) -> Double {
            guard x >= 0, y >= 0, x < Double(w - 1), y < Double(h - 1) else { return 0 }
            let x0 = Int(x), y0 = Int(y), fx = x - Double(x0), fy = y - Double(y0)
            let a = cover[y0 * w + x0], b = cover[y0 * w + x0 + 1], c2 = cover[(y0 + 1) * w + x0], d = cover[(y0 + 1) * w + x0 + 1]
            return (a + (b - a) * fx) * (1 - fy) + (c2 + (d - c2) * fx) * fy
        }
        for oy in 0..<oh { for ox in 0..<ow {
            // Inverse map: output → source (undo turn, scale, shear; then the bend).
            var x = Double(ox) - cx, y = Double(oy) - cy
            (x, y) = (x * ct + y * st, -x * st + y * ct)
            x /= sx; y /= sy
            x -= shear * y
            var bx = 0.0, by = 0.0
            for b in bends { bx += b.a * sin(Double(oy) * b.fy * 2 * .pi + b.p); by += b.a * sin(Double(ox) * b.fx * 2 * .pi + b.q) }
            let v = sample(x + scx + bx, y + scy + by)
            guard v > 0.01 else { continue }
            let i = (oy * ow + ox) * 4
            // Premultiplied ink.
            od[i] = UInt8(ir * v * 255); od[i + 1] = UInt8(ig * v * 255); od[i + 2] = UInt8(ib * v * 255); od[i + 3] = UInt8(v * 255)
        } }
        guard let image = out.makeImage() else { return nil }
        var texture = InkTexture.Strength(pressure: Double(r.range(0.05...0.1)), gaps: Double(r.range(0.004...0.012)), bleed: 0.3,
                                          alongX: Double(r.range(0.03...0.07)))
        texture.paper = r.range(0.04...0.08)
        return InkDraw.trim(InkTexture.apply(image, seed: r.next(), strength: texture, scale: scale), pad: Int((6 * scale).rounded()))
    }
}
