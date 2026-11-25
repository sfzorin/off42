import AppKit
import Vision

/// `IMG42 --smart-lasso-eval`: measures the Smart Lasso against a reference mask (developer check, not localised).
///
/// The reference is `--gt mask.png` or Vision's subject mask. A "user" stroke is simulated from the reference outline:
/// offset along the normal by smooth random wobble of ±`noise`·radius plus jitter, sampled like mouse events. Prints IoU
/// of the plain lasso (the stroke polygon) and of the Smart Lasso with and without the subject mask term.
enum SmartLassoEval {
    static func run(_ a: [String]) {
        guard let file = a.first, let image = ImageLoader.load(URL(fileURLWithPath: file)).image else {
            print("usage: --smart-lasso-eval in [--gt mask.png] [--radius r] [--noise a] [--seed n] [--vis out.png]"); CLI.status = 2; return
        }
        func opt(_ name: String) -> String? { a.firstIndex(of: name).flatMap { $0 + 1 < a.count ? a[$0 + 1] : nil } }
        let radius = CGFloat(Double(opt("--radius") ?? "") ?? 14)
        let noise = CGFloat(Double(opt("--noise") ?? "") ?? 0.6)
        guard radius.isFinite, radius > 0, radius <= 32768, noise.isFinite, noise >= 0 else {
            print("invalid Smart Lasso radius or noise"); CLI.status = 2; return
        }
        var rng = SeededRandom(seed: UInt64(opt("--seed") ?? "") ?? 1)
        let size = CGSize(width: image.width, height: image.height)
        let subject = SmartLasso.subjectMask(for: image)
        guard let gtImage = opt("--gt").flatMap({ ImageLoader.load(URL(fileURLWithPath: $0)).image }) ?? subject,
              let gt = binary(gtImage, size), let outline = largestContour(gtImage) else { print("no reference mask"); CLI.status = 1; return }

        // Simulated stroke.
        let base = SmartLasso.resampleClosed(outline, spacing: 2)
        let n = base.count
        guard n >= 4 else { print("invalid or excessive reference contour"); CLI.status = 2; return }
        let phases = (0..<4).map { _ in CGFloat(rng.next()) * 2 * .pi }
        let freqs: [CGFloat] = [1, 2.3, 4.1, 7.7]
        let outward: CGFloat = SmartLasso.signedArea(base) > 0 ? -1 : 1
        var stroke: [CGPoint] = []
        for i in Swift.stride(from: 0, to: n, by: 4) {
            let a = base[(i - 3 + n) % n], b = base[(i + 3) % n]
            let len = max(0.001, hypot(b.x - a.x, b.y - a.y))
            let nv = CGPoint(x: -(b.y - a.y) / len * outward, y: (b.x - a.x) / len * outward)
            let t = CGFloat(i) / CGFloat(n) * 2 * .pi
            var w: CGFloat = 0
            for (f, ph) in zip(freqs, phases) { w += sin(t * f + ph) / CGFloat(freqs.count).squareRoot() }
            let o = max(-0.95, min(0.95, noise * w)) * radius + (CGFloat(rng.next()) - 0.5) * 2
            stroke.append(CGPoint(x: base[i].x + nv.x * o, y: base[i].y + nv.y * o))
        }

        if let path = opt("--stroke-out") {
            try? stroke.map { String(format: "%.0f,%.0f", $0.x, $0.y) }.joined(separator: ";").write(toFile: path, atomically: true, encoding: .utf8)
        }
        let plainPath = CGMutablePath()
        plainPath.addLines(between: stroke)
        plainPath.closeSubpath()
        func iou(_ path: CGPath?) -> Double {
            guard let path, let sel = PixelSelection.from(path: path, size: size), let m = binary(sel.mask, size) else { return 0 }
            var inter = 0, uni = 0
            for i in m.indices { if m[i] && gt[i] { inter += 1 }; if m[i] || gt[i] { uni += 1 } }
            return uni > 0 ? Double(inter) / Double(uni) : 0
        }
        let t0 = Date()
        let smart = SmartLasso.refine(stroke: stroke, radius: radius, image: image, subject: subject)
        let dt = Date().timeIntervalSince(t0)
        let edgesOnly = SmartLasso.refine(stroke: stroke, radius: radius, image: image, subject: nil)
        let edgeOnlyNoRegion = SmartLasso.refine(stroke: stroke, radius: radius, image: image, subject: nil,
                                                  options: .init(useSubjectMask: false, useRegionFit: false))
        print(String(format: "%@\tr=%.0f noise=%.2f\tplain IoU=%.3f\tsmart IoU=%.3f\tsmart(no subject) IoU=%.3f\tsmart(edges only) IoU=%.3f\trefine=%.3fs",
                     (file as NSString).lastPathComponent, radius, noise, iou(plainPath), iou(smart), iou(edgesOnly), iou(edgeOnlyNoRegion), dt))

        if let vis = opt("--vis"), let ctx = PixelOps.rgbContext(size, like: image) {
            ctx.draw(image, in: CGRect(origin: .zero, size: size))
            ctx.translateBy(x: 0, y: size.height)
            ctx.scaleBy(x: 1, y: -1)
            ctx.setLineJoin(.round); ctx.setLineCap(.round)
            ctx.addPath(plainPath); ctx.setStrokeColor(CGColor(red: 1, green: 1, blue: 0, alpha: 0.35)); ctx.setLineWidth(radius * 2); ctx.strokePath()
            let ref = CGMutablePath(); ref.addLines(between: outline); ref.closeSubpath()
            ctx.addPath(ref); ctx.setStrokeColor(CGColor(red: 0, green: 0.9, blue: 0.2, alpha: 1)); ctx.setLineWidth(2); ctx.strokePath()
            if let edgesOnly { ctx.addPath(edgesOnly); ctx.setStrokeColor(CGColor(red: 0.2, green: 0.4, blue: 1, alpha: 1)); ctx.setLineWidth(1.5); ctx.strokePath() }
            if let smart { ctx.addPath(smart); ctx.setStrokeColor(CGColor(red: 1, green: 0, blue: 0.3, alpha: 1)); ctx.setLineWidth(1.5); ctx.strokePath() }
            if let out = ctx.makeImage() { try? ImageWriter.write(out, to: URL(fileURLWithPath: vis), type: "public.png") }
        }
    }

    static func binary(_ image: CGImage, _ size: CGSize) -> [Bool]? {
        guard let ctx = PixelSelection.grayContext(size), let data = ctx.data else { return nil }
        ctx.draw(image, in: CGRect(origin: .zero, size: size))
        let p = data.bindMemory(to: UInt8.self, capacity: ctx.bytesPerRow * Int(size.height))
        var out = [Bool](repeating: false, count: Int(size.width) * Int(size.height))
        for y in 0..<Int(size.height) { for x in 0..<Int(size.width) { out[y * Int(size.width) + x] = p[y * ctx.bytesPerRow + x] > 127 } }
        return out
    }

    /// The longest outer contour of a mask, in image coordinates (y down).
    static func largestContour(_ mask: CGImage) -> [CGPoint]? {
        let request = VNDetectContoursRequest()
        request.detectsDarkOnLight = false
        request.contrastAdjustment = 1
        request.maximumImageDimension = 1024
        try? VNImageRequestHandler(cgImage: mask).perform([request])
        guard let obs = request.results?.first else { return nil }
        let best = obs.topLevelContours.max { $0.pointCount < $1.pointCount }
        guard let c = best else { return nil }
        let w = CGFloat(mask.width), h = CGFloat(mask.height)
        return c.normalizedPoints.map { CGPoint(x: CGFloat($0.x) * w, y: (1 - CGFloat($0.y)) * h) }
    }
}

struct SeededRandom {
    var state: UInt64
    init(seed: UInt64) { state = seed &* 0x9E3779B97F4A7C15 | 1 }
    mutating func next() -> Double {
        state ^= state << 13; state ^= state >> 7; state ^= state << 17
        return Double(state % 1_000_000) / 1_000_000
    }
}
