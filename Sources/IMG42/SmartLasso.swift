import AppKit
import CoreImage
import Vision

/// Smart Lasso (like Preview's): the user paints a wide band around an object; the selection follows the object's
/// edge inside that band.
///
/// The band is unrolled along the stroke: at every few pixels of the stroke's centre line we look across it (along
/// the normal) and score each offset as a boundary candidate by
///   * the colour edge across the band at that offset (directional gradient, slightly blurred image),
///   * a two-region fit of the profile (inner end = object colour, outer end = background, GrabCut-like in 1D),
///   * the Vision subject mask (`VNGenerateForegroundInstanceMaskRequest`) changing from subject to background there.
/// Dynamic programming (Viterbi, closed loop) then picks one offset per sample with a smoothness penalty — an
/// intelligent-scissors path constrained to the band.
enum SmartLasso {
    struct Options {
        var useSubjectMask = true
        var useRegionFit = true
    }

    /// Band half-width in screen points (the stroke is twice as wide).
    static let bandRadius: CGFloat = 12

    // MARK: Subject mask (cached per image)

    private static var subjectCache: (image: CGImage, mask: CGImage?)?
    private static let lock = NSLock()

    /// Union of Vision's foreground instances at image size (white = subject), or nil when Vision finds none.
    static func subjectMask(for image: CGImage) -> CGImage? {
        lock.lock()
        if let c = subjectCache, c.image === image { lock.unlock(); return c.mask }
        lock.unlock()
        var mask: CGImage?
        let request = VNGenerateForegroundInstanceMaskRequest()
        let handler = VNImageRequestHandler(cgImage: image)
        if (try? handler.perform([request])) != nil, let result = request.results?.first, !result.allInstances.isEmpty,
           let buffer = try? result.generateScaledMaskForImage(forInstances: result.allInstances, from: handler) {
            let ci = CIImage(cvPixelBuffer: buffer)
            mask = Adjustments.context.createCGImage(ci, from: ci.extent, format: .L8, colorSpace: CGColorSpaceCreateDeviceGray())
        }
        lock.lock()
        subjectCache = (image, mask)
        lock.unlock()
        return mask
    }

    /// Computes the subject mask in the background so the first stroke does not wait for it.
    static func prewarm(_ image: CGImage?) {
        guard let image else { return }
        DispatchQueue.global(qos: .utility).async { _ = subjectMask(for: image) }
    }

    // MARK: Refinement

    /// The refined closed outline (image coordinates, y down) for a stroke painted with half-width `radius` (image pixels).
    static func refine(stroke raw: [CGPoint], radius: CGFloat, image: CGImage, subject: CGImage?, options: Options = Options()) -> CGPath? {
        guard raw.count >= 3, raw.count <= 100_000, raw.allSatisfy({ $0.x.isFinite && $0.y.isFinite }),
              radius.isFinite, radius > 0, radius <= 32768 else { return nil }
        let full = CGRect(x: 0, y: 0, width: image.width, height: image.height)
        // Working region: the band's bounding box, downscaled so the band stays ≥ 8 px but the region ≤ ~900 px.
        var box = CGRect.null
        for p in raw { box = box.union(CGRect(x: p.x, y: p.y, width: 0, height: 0)) }
        let region = box.insetBy(dx: -radius * 1.6 - 4, dy: -radius * 1.6 - 4).intersection(full).integral
        guard region.width >= 4, region.height >= 4 else { return nil }
        let k = min(1, max(900 / max(region.width, region.height), 8 / radius), 2200 / max(region.width, region.height))
        let ww = max(4, Int((region.width * k).rounded())), wh = max(4, Int((region.height * k).rounded()))
        guard let rgb = RGBMap(image: image, region: region, width: ww, height: wh) else { return nil }
        let mask = options.useSubjectMask ? subject.flatMap { GrayMap(image: $0, region: region, width: ww, height: wh) } : nil
        let toWork: (CGPoint) -> CGPoint = { CGPoint(x: ($0.x - region.minX) * k, y: ($0.y - region.minY) * k) }

        // Centre line, resampled every `step` working pixels; normals smoothed over a few samples.
        let r = radius * k
        let step = max(1.5, min(3, r / 5))
        let centre = resampleClosed(raw.map(toWork), spacing: step)
        let n = centre.count
        guard n >= 8 else { return nil }
        let outwardSign: CGFloat = signedArea(centre) > 0 ? -1 : 1
        let span = max(2, Int((r / step).rounded()))
        var normals: [CGPoint] = []
        for i in 0..<n {
            let a = centre[(i - span + n * 4) % n], b = centre[(i + span) % n]
            var t = CGPoint(x: b.x - a.x, y: b.y - a.y)
            let len = max(0.0001, hypot(t.x, t.y))
            t = CGPoint(x: t.x / len, y: t.y / len)
            // Left normal (-ty, tx) points inside for a positive-area loop; flip to point outside.
            normals.append(CGPoint(x: -t.y * outwardSign, y: t.x * outwardSign))
        }

        // Candidate offsets across the band (outward positive), 1 working px apart (≤ 61).
        let kCount = min(61, max(9, Int((2 * r).rounded()) + 1))
        let offsets = (0..<kCount).map { -r + 2 * r * CGFloat($0) / CGFloat(kCount - 1) }
        let d = offsets[1] - offsets[0]

        // Profiles across the band (one extra sample beyond each end for the derivative).
        var edge = [Float](repeating: 0, count: n * kCount)
        var region2 = [Float](repeating: 0, count: n * kCount)
        var maskTerm = [Float](repeating: 0, count: n * kCount)
        var profiles = [[SIMD3<Float>]](repeating: [], count: n)
        var maskProfiles = [[Float]](repeating: [], count: n)
        for i in 0..<n {
            let c = centre[i], nv = normals[i]
            var prof: [SIMD3<Float>] = []
            var mprof: [Float] = []
            prof.reserveCapacity(kCount + 2)
            for j in -1...kCount {
                let o = -r + d * CGFloat(j)
                let q = CGPoint(x: c.x + nv.x * o, y: c.y + nv.y * o)
                prof.append(rgb.at(q))
                if let mask { mprof.append(mask.at(q)) }
            }
            profiles[i] = prof
            maskProfiles[i] = mprof
        }
        // Colour edge across the band.
        var all: [Float] = []
        all.reserveCapacity(n * kCount)
        for i in 0..<n {
            let p = profiles[i]
            for j in 0..<kCount {
                let e = length(p[j + 2] - p[j])
                edge[i * kCount + j] = e
                all.append(e)
            }
        }
        all.sort()
        let e95 = max(0.04, all[min(all.count - 1, Int(Float(all.count) * 0.95))])
        for x in edge.indices { edge[x] = min(1.2, edge[x] / e95) }

        // Two-region fit: inner end ≈ object, outer end ≈ background (models averaged over neighbouring samples).
        if options.useRegionFit {
            let ends = 3, window = 4
            var inner = [SIMD3<Float>](repeating: .zero, count: n), outer = inner
            for i in 0..<n {
                let p = profiles[i]
                inner[i] = p[1..<(1 + ends)].reduce(.zero, +) / Float(ends)
                outer[i] = p[(kCount + 1 - ends)..<(kCount + 1)].reduce(.zero, +) / Float(ends)
            }
            var contrastSum: Float = 0
            for i in 0..<n {
                var f = SIMD3<Float>.zero, b = SIMD3<Float>.zero
                for w in -window...window { f += inner[(i + w + n) % n]; b += outer[(i + w + n) % n] }
                f /= Float(2 * window + 1); b /= Float(2 * window + 1)
                let contrast = length(f - b)
                contrastSum += contrast
                let p = profiles[i]
                // D(j) = Σ_{o<j} |p−f| + Σ_{o≥j} |p−b|, via prefix sums.
                var toF = [Float](repeating: 0, count: kCount + 1), toB = toF
                for j in 0..<kCount {
                    toF[j + 1] = toF[j] + length(p[j + 1] - f)
                    toB[j + 1] = toB[j] + length(p[j + 1] - b)
                }
                let worst = max(toF[kCount], toB[kCount])
                for j in 0..<kCount {
                    let dj = toF[j] + (toB[kCount] - toB[j])
                    // 1 = perfect split here, 0 = no better than all-one-region; scaled by how distinct the regions are.
                    let gain = worst > 0 ? max(0, (worst - dj) / worst) : 0
                    region2[i * kCount + j] = gain * min(1, contrast / 0.12)
                }
            }
            _ = contrastSum
        }

        // Subject mask: inside (inward) high, outside (outward) low around this offset.
        if mask != nil {
            for i in 0..<n {
                let m = maskProfiles[i]
                for j in 0..<kCount {
                    let lo = max(0, j - 1), hi = min(kCount + 1, j + 3)
                    maskTerm[i * kCount + j] = max(0, m[lo] - m[hi])
                }
            }
        }

        // Cost per candidate; a slight pull to the band centre only breaks ties where nothing is found.
        var cost = [Float](repeating: 0, count: n * kCount)
        for i in 0..<n {
            for j in 0..<kCount {
                let x = i * kCount + j
                let centrePull = 0.04 * Float(abs(offsets[j]) / r)
                cost[x] = 1.4 - (1.0 * edge[x] + 0.6 * region2[x] + 1.1 * maskTerm[x]) + centrePull
            }
        }

        let path = viterbiClosed(cost: cost, n: n, k: kCount, maxStep: 2, smooth: 0.12)
        guard path.count == n else { return nil }
        var pts = (0..<n).map { i -> CGPoint in
            let c = centre[i], nv = normals[i], o = offsets[path[i]]
            return CGPoint(x: c.x + nv.x * o, y: c.y + nv.y * o)
        }
        // Light smoothing, then back to image coordinates.
        pts = (0..<n).map { i in
            let a = pts[(i - 1 + n) % n], b = pts[i], c = pts[(i + 1) % n]
            return CGPoint(x: (a.x + 2 * b.x + c.x) / 4, y: (a.y + 2 * b.y + c.y) / 4)
        }
        let out = CGMutablePath()
        out.addLines(between: pts.map { CGPoint(x: $0.x / k + region.minX, y: $0.y / k + region.minY) })
        out.closeSubpath()
        return out
    }

    /// Minimum-cost closed path choosing one offset per sample; consecutive offsets differ by ≤ `maxStep`.
    static func viterbiClosed(cost: [Float], n: Int, k: Int, maxStep: Int, smooth: Float) -> [Int] {
        guard n > 0, n <= 20_000, k > 0, k <= 256, cost.count == n * k,
              maxStep >= 0, maxStep <= k, smooth.isFinite, cost.allSatisfy(\.isFinite) else { return [] }
        func run(start: Int, fixed: Int?) -> [Int] {
            var dp = [Float](repeating: .infinity, count: k)
            var back = [Int32](repeating: 0, count: n * k)
            for j in 0..<k where fixed == nil || fixed == j { dp[j] = cost[start * k + j] }
            for s in 1..<n {
                let i = (start + s) % n
                var next = [Float](repeating: .infinity, count: k)
                for j in 0..<k {
                    var best = Float.infinity, arg = j
                    for pj in max(0, j - maxStep)...min(k - 1, j + maxStep) {
                        let v = dp[pj] + smooth * Float(abs(pj - j))
                        if v < best { best = v; arg = pj }
                    }
                    next[j] = best + cost[i * k + j]
                    back[s * k + j] = Int32(arg)
                }
                dp = next
            }
            var end = 0
            var best = Float.infinity
            for j in 0..<k {
                var v = dp[j]
                if let fixed {
                    let gap = abs(j - fixed)
                    v += gap <= maxStep ? smooth * Float(gap) : smooth * Float(gap) * 4
                }
                if v < best { best = v; end = j }
            }
            var path = [Int](repeating: 0, count: n)
            var j = end
            for s in stride(from: n - 1, through: 0, by: -1) {
                path[(start + s) % n] = j
                if s > 0 { j = Int(back[s * k + j]) }
            }
            return path
        }
        // Open pass, then a pass anchored at the open path's middle (closes the loop consistently).
        let first = run(start: 0, fixed: nil)
        let mid = n / 2
        return run(start: mid, fixed: first[mid])
    }

    static func resampleClosed(_ pts: [CGPoint], spacing: CGFloat) -> [CGPoint] {
        guard spacing.isFinite, spacing > 0, pts.count <= 100_000,
              pts.allSatisfy({ $0.x.isFinite && $0.y.isFinite }) else { return [] }
        var src = pts
        if let f = src.first { src.append(f) }
        let perimeter = zip(src, src.dropFirst()).reduce(CGFloat.zero) { $0 + hypot($1.1.x - $1.0.x, $1.1.y - $1.0.y) }
        guard perimeter.isFinite, perimeter / spacing <= 20_000 else { return [] }
        var out: [CGPoint] = []
        var carry: CGFloat = 0
        for (a, b) in zip(src, src.dropFirst()) {
            let len = hypot(b.x - a.x, b.y - a.y)
            guard len > 0 else { continue }
            var t = carry
            while t < len {
                out.append(CGPoint(x: a.x + (b.x - a.x) * t / len, y: a.y + (b.y - a.y) * t / len))
                t += spacing
            }
            carry = t - len
        }
        return out
    }

    static func signedArea(_ pts: [CGPoint]) -> CGFloat {
        var s: CGFloat = 0
        for i in pts.indices {
            let a = pts[i], b = pts[(i + 1) % pts.count]
            s += a.x * b.y - b.x * a.y
        }
        return s / 2
    }

    private static func length(_ v: SIMD3<Float>) -> Float { (v * v).sum().squareRoot() }
}

/// A slightly blurred RGB copy of an image region (0…1), sampled bilinearly in working coordinates.
struct RGBMap {
    let w: Int, h: Int
    var px: [SIMD3<Float>]

    init?(image: CGImage, region: CGRect, width: Int, height: Int) {
        guard let crop = image.cropping(to: region),
              let ctx = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4,
                                  space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue),
              let data = ctx.data else { return nil }
        // Transparent areas read as white (the checkerboard's light squares), so a cut-out's edge is an edge.
        ctx.setFillColor(CGColor(red: 1, green: 1, blue: 1, alpha: 1))
        ctx.fill(CGRect(x: 0, y: 0, width: width, height: height))
        ctx.interpolationQuality = .high
        ctx.draw(crop, in: CGRect(x: 0, y: 0, width: width, height: height))
        let p = data.bindMemory(to: UInt8.self, capacity: width * height * 4)
        w = width; h = height
        var v = [SIMD3<Float>](repeating: .zero, count: width * height)
        for i in 0..<(width * height) {
            v[i] = SIMD3(Float(p[i * 4]), Float(p[i * 4 + 1]), Float(p[i * 4 + 2])) / 255
        }
        // Two separable box blurs (radius 1) ≈ Gaussian σ≈1: texture noise must not beat real edges.
        for _ in 0..<2 { v = RGBMap.boxBlur(v, width, height) }
        px = v
    }

    static func boxBlur(_ v: [SIMD3<Float>], _ w: Int, _ h: Int) -> [SIMD3<Float>] {
        var t = v
        for y in 0..<h {
            for x in 0..<w {
                let a = v[y * w + max(0, x - 1)], b = v[y * w + x], c = v[y * w + min(w - 1, x + 1)]
                t[y * w + x] = (a + b + c) / 3
            }
        }
        var out = t
        for y in 0..<h {
            for x in 0..<w {
                let a = t[max(0, y - 1) * w + x], b = t[y * w + x], c = t[min(h - 1, y + 1) * w + x]
                out[y * w + x] = (a + b + c) / 3
            }
        }
        return out
    }

    func at(_ q: CGPoint) -> SIMD3<Float> {
        let x = min(max(Float(q.x) - 0.5, 0), Float(w - 1)), y = min(max(Float(q.y) - 0.5, 0), Float(h - 1))
        let x0 = Int(x), y0 = Int(y), x1 = min(w - 1, x0 + 1), y1 = min(h - 1, y0 + 1)
        let fx = x - Float(x0), fy = y - Float(y0)
        let top = px[y0 * w + x0] * (1 - fx) + px[y0 * w + x1] * fx
        let bottom = px[y1 * w + x0] * (1 - fx) + px[y1 * w + x1] * fx
        return top * (1 - fy) + bottom * fy
    }
}

/// A grey mask region (0…1), sampled bilinearly in working coordinates.
struct GrayMap {
    let w: Int, h: Int
    var v: [Float]

    init?(image: CGImage, region: CGRect, width: Int, height: Int) {
        guard let crop = image.cropping(to: region), let ctx = PixelSelection.grayContext(CGSize(width: width, height: height)),
              let data = ctx.data else { return nil }
        ctx.interpolationQuality = .high
        ctx.draw(crop, in: CGRect(x: 0, y: 0, width: width, height: height))
        let p = data.bindMemory(to: UInt8.self, capacity: ctx.bytesPerRow * height)
        w = width; h = height
        v = [Float](repeating: 0, count: width * height)
        for y in 0..<height { for x in 0..<width { v[y * width + x] = Float(p[y * ctx.bytesPerRow + x]) / 255 } }
    }

    func at(_ q: CGPoint) -> Float {
        let x = min(max(Float(q.x) - 0.5, 0), Float(w - 1)), y = min(max(Float(q.y) - 0.5, 0), Float(h - 1))
        let x0 = Int(x), y0 = Int(y), x1 = min(w - 1, x0 + 1), y1 = min(h - 1, y0 + 1)
        let fx = x - Float(x0), fy = y - Float(y0)
        let top = v[y0 * w + x0] * (1 - fx) + v[y0 * w + x1] * fx
        let bottom = v[y1 * w + x0] * (1 - fx) + v[y1 * w + x1] * fx
        return top * (1 - fy) + bottom * fy
    }
}
