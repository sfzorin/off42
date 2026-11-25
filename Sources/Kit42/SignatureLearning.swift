import AppKit

/// One signature written by the user: strokes of [x, y, time, pressure] (y up; time in seconds, −1 when unknown,
/// as for a scan or a sketch).
public struct SignatureSample: Codable {
    public var strokes: [[[Double]]]
    /// Line width as a share of the signature's height, measured on a picture (nil when drawn on the pad).
    public var penWidth: Double?
    /// The picture the sample was traced from (grey PNG, at most 800 px tall); copies of picture-learned signatures
    /// are made from its ink. Nil for samples drawn on the pad.
    public var picture: Data?
    public init(strokes: [[[Double]]], penWidth: Double? = nil, picture: Data? = nil) {
        self.strokes = strokes; self.penWidth = penWidth; self.picture = picture
    }

    /// A sample from a picture of one signature: traced, and the picture kept for copies made from its ink.
    public static func from(picture image: CGImage) -> SignatureSample? {
        guard var sample = SignatureLearning.trace(image) else { return nil }
        sample.picture = SignatureWarp.encode(image)
        return sample
    }
    public var isTimed: Bool { strokes.first?.first.map { $0.count > 2 && $0[2] >= 0 } ?? false }
}

/// A signature learned from the user's samples.
/// - Learned from pictures (scans, photos, a photographed sheet): every copy is made from the ink of one sample —
///   bent the way a hand varies (slant, width, a slight turn, a smooth bend) and inked again — so shapes stay as
///   written; tracing small photos into a centre line loses tips, loops and dense crossings. Larger, sharper samples
///   are used more often.
/// - Drawn on the pad (with timing): copies are re-synthesised with the sigma-lognormal model: via-points and their
///   real timing come from the samples, two samples are blended when they have the same structure, and each copy
///   perturbs the model.
public struct LearnedSignature: Codable {
    public var samples: [SignatureSample]
    public init(samples: [SignatureSample]) { self.samples = samples }

    public func render(impression: UInt64, ink: NSColor, pen: PenType, pixelHeight: CGFloat = 120,
                       capture: (([[PenSample]]) -> Void)? = nil) -> CGImage? {
        SignatureLearning.render(self, impression: impression, ink: ink, pen: pen, pixelHeight: pixelHeight, capture: capture)
    }
}

enum SignatureLearning {
    struct Point { var p: CGPoint; var t: Double; var band: Double = 0 }

    /// Sample in cap units: height 1, left edge and lowest point at 0.
    static func normalized(_ s: SignatureSample) -> [[Point]] {
        let all = s.strokes.flatMap { $0 }
        guard let minX = all.map({ $0[0] }).min(), let maxY = all.map({ $0[1] }).max(), let minY = all.map({ $0[1] }).min() else { return [] }
        let h = max(1e-6, maxY - minY)
        return s.strokes.map { stroke in
            // Index 4 (pictures only): ink thickness around the centre line, in the sample's units.
            stroke.map { Point(p: CGPoint(x: ($0[0] - minX) / h, y: ($0[1] - minY) / h), t: $0.count > 2 ? $0[2] : -1,
                               band: $0.count > 4 ? $0[4] / h : 0) }
        }.filter { $0.count > 1 }
    }

    /// Even spacing along a stroke (`n` points), carrying time.
    static func resample(_ s: [Point], count n: Int) -> [Point] {
        guard s.count > 1, n > 1 else { return s }
        var lengths: [CGFloat] = [0]
        for i in 1..<s.count { lengths.append(lengths[i - 1] + hypot(s[i].p.x - s[i - 1].p.x, s[i].p.y - s[i - 1].p.y)) }
        let total = max(1e-6, lengths.last!)
        var out: [Point] = []
        var j = 0
        for k in 0..<n {
            let target = total * CGFloat(k) / CGFloat(n - 1)
            while j < s.count - 2, lengths[j + 1] < target { j += 1 }
            let seg = max(1e-9, lengths[j + 1] - lengths[j]), u = (target - lengths[j]) / seg
            out.append(Point(p: CGPoint(x: s[j].p.x + (s[j + 1].p.x - s[j].p.x) * u, y: s[j].p.y + (s[j + 1].p.y - s[j].p.y) * u),
                             t: s[j].t + (s[j + 1].t - s[j].t) * Double(u), band: s[j].band + (s[j + 1].band - s[j].band) * Double(u)))
        }
        return out
    }

    /// Via-points of a stroke: Ramer–Douglas–Peucker simplification (keeps the shape), plus the time spent between
    /// them when the stroke was recorded. Sharp turns become corners (the pen nearly stops there).
    static func via(_ stroke: [Point], tolerance: CGFloat = 0.012) -> (points: [CGPoint], corners: Set<Int>, durations: [Double]?) {
        let pts = stroke.map(\.p)
        guard pts.count > 2 else { return (pts, [], nil) }
        var keep = [Bool](repeating: false, count: pts.count)
        keep[0] = true; keep[pts.count - 1] = true
        func rdp(_ a: Int, _ b: Int) {
            guard b > a + 1 else { return }
            let pa = pts[a], pb = pts[b]
            let dx = pb.x - pa.x, dy = pb.y - pa.y, len = max(1e-9, hypot(dx, dy))
            var worst = -1, dist: CGFloat = 0
            for i in (a + 1)..<b {
                let d = abs((pts[i].x - pa.x) * dy - (pts[i].y - pa.y) * dx) / len
                if d > dist { dist = d; worst = i }
            }
            if dist > tolerance, worst > 0 { keep[worst] = true; rdp(a, worst); rdp(worst, b) }
        }
        rdp(0, pts.count - 1)
        var idx = keep.indices.filter { keep[$0] }
        // A tip that tracing rounded: thinning turns a sharp reversal into a tight U, which simplification keeps as
        // two moderate turns around a short segment — neither reads as a corner, so the copy would be drawn round
        // and come out blunter than the original. A short segment (< 4 % of the height) between two turns the
        // same way that add up to more than 125° is collapsed into one point at the tip.
        func heading(_ a: Int, _ b: Int) -> CGFloat { atan2(pts[b].y - pts[a].y, pts[b].x - pts[a].x) }
        func signedTurn(_ a: CGFloat, _ b: CGFloat) -> CGFloat { var d = b - a; while d > .pi { d -= 2 * .pi }; while d < -.pi { d += 2 * .pi }; return d }
        var k = 1
        while k + 2 < idx.count {
            let p1 = pts[idx[k]], p2 = pts[idx[k + 1]]
            let short = hypot(p2.x - p1.x, p2.y - p1.y) < 0.04
            let t1 = signedTurn(heading(idx[k - 1], idx[k]), heading(idx[k], idx[k + 1]))
            let t2 = signedTurn(heading(idx[k], idx[k + 1]), heading(idx[k + 1], idx[k + 2]))
            // A tip, not a small loop: on a loop (о, е, the bowl of б) the line keeps turning the same way before
            // and after; at a tip the turning stops.
            let t0 = k >= 2 ? signedTurn(heading(idx[k - 2], idx[k - 1]), heading(idx[k - 1], idx[k])) : 0
            let t3 = k + 3 < idx.count ? signedTurn(heading(idx[k + 1], idx[k + 2]), heading(idx[k + 2], idx[k + 3])) : 0
            let curling = (t0 * t1 > 0 && abs(t0) > 0.5) || (t3 * t2 > 0 && abs(t3) > 0.5)
            if short, !curling, t1 * t2 > 0, abs(t1 + t2) > 2.2 {
                // The tip: the point of the U farthest along the incoming direction.
                let dir = heading(idx[k - 1], idx[k])
                let tip = (idx[k]...idx[k + 1]).max { a, b in
                    (pts[a].x * cos(dir) + pts[a].y * sin(dir)) < (pts[b].x * cos(dir) + pts[b].y * sin(dir))
                } ?? idx[k]
                idx.replaceSubrange(k...(k + 1), with: [tip])
            }
            k += 1
        }
        let points = idx.map { pts[$0] }
        let timed = stroke.first.map { $0.t >= 0 } ?? false
        var corners = Set<Int>()
        for k in 1..<max(1, points.count - 1) {
            let a = atan2(points[k].y - points[k - 1].y, points[k].x - points[k - 1].x)
            let b = atan2(points[k + 1].y - points[k].y, points[k + 1].x - points[k].x)
            var d = abs(b - a); if d > .pi { d = 2 * .pi - d }
            // Cusp: more than ~115°.
            if d > 2.0 { corners.insert(k) }
        }
        let durations: [Double]? = timed ? zip(idx, idx.dropFirst()).map { max(0.02, stroke[$1].t - stroke[$0].t) * 2.2 } : nil
        return (points, corners, durations)
    }

    static func render(_ learned: LearnedSignature, impression: UInt64, ink: NSColor, pen: PenType, pixelHeight: CGFloat,
                       capture: (([[PenSample]]) -> Void)? = nil) -> CGImage? {
        // All samples from pictures: copies from their ink.
        let pictures = learned.samples.compactMap { $0.picture.flatMap(SignatureWarp.decode) }
        if !pictures.isEmpty, pictures.count == learned.samples.count, capture == nil {
            var pick = SeededRandom(impression)
            let weights = pictures.map { pow(Double(min($0.height, 800)), 2) }
            var r = Double(pick.next() % 1_000_000) / 1_000_000 * weights.reduce(0, +)
            var base = pictures.count - 1
            for (k, w) in weights.enumerated() { if r < w { base = k; break }; r -= w }
            return SignatureWarp.copy(of: pictures[base], ink: ink, seed: pick.next(), height: pixelHeight * 2.5)
        }
        let usable = learned.samples.filter { !normalized($0).isEmpty }
        let samples = usable.map(normalized)
        guard !samples.isEmpty else { return nil }
        var copy = SeededRandom(impression)
        // Base sample, blended with another one of the same structure (same number of strokes, similar lengths).
        // The base of this copy: detailed samples (a large, sharp picture traces into more of the real movement) are
        // chosen more often than small blurred ones, which trace into a simplified shape and would make look-alike
        // copies. Detail = traced path length per height × picture height of the centre line (pad samples count fully).
        func detail(_ k: Int) -> Double {
            let s = samples[k]
            let length = s.reduce(0.0) { sum, stroke in sum + zip(stroke, stroke.dropFirst()).reduce(0.0) { $0 + Double(hypot($1.1.p.x - $1.0.p.x, $1.1.p.y - $1.0.p.y)) } }
            let ys = usable[k].strokes.flatMap { $0.map { $0[1] } }
            let pixels = usable[k].isTimed ? 400 : (ys.max() ?? 0) - (ys.min() ?? 0)
            return length * min(1, pixels / 300)
        }
        let weights = samples.indices.map { pow(detail($0), 2) }
        var pick = Double(copy.next() % 1_000_000) / 1_000_000 * max(1e-9, weights.reduce(0, +))
        var ai = samples.count - 1
        for (k, w) in weights.enumerated() { if pick < w { ai = k; break }; pick -= w }
        let a = samples[ai]
        var strokes = a
        let partners = samples.indices.filter { $0 != ai && samples[$0].count == a.count }.map { samples[$0] }
        if let b = partners.randomElement(using: &copy), similar(a, b) {
            // Close to one of the two: a half-and-half mix smears features that sit at slightly different places
            // (the notch of З, a cusp).
            let w = CGFloat(copy.range(0.05...0.3))
            strokes = zip(a, b).map { sa, sb in
                let n = max(sa.count, sb.count, 24)
                return zip(resample(sa, count: n), resample(sb, count: n)).map { pa, pb in
                    Point(p: CGPoint(x: pa.p.x * (1 - w) + pb.p.x * w, y: pa.p.y * (1 - w) + pb.p.y * w),
                          t: pa.t >= 0 && pb.t >= 0 ? pa.t * Double(1 - w) + pb.t * Double(w) : pa.t, band: pa.band)
                }
            }
        }
        var viaStrokes: [[CGPoint]] = []
        var durations: [[Double]] = []
        var anyTimed = false
        let ownWidth = usable[ai].penWidth ?? 0
        for s in strokes {
            let v = via(garland(s, pen: ownWidth, rng: &copy))
            // Corners are marked for the synthesiser by repeating the point.
            var pts: [CGPoint] = []
            for (k, p) in v.points.enumerated() { pts.append(p); if v.corners.contains(k) { pts.append(p) } }
            viaStrokes.append(pts)
            durations.append(v.durations ?? [])
            anyTimed = anyTimed || v.durations != nil
        }
        var hand = SigmaLognormal.Hand()
        hand.overlap = anyTimed ? 0.6 : 0.68
        hand.cornerOverlap = 0.3
        hand.sizeScaled = false
        hand.curvatureGain = 0.4
        // The line width of the user's own pen when it was measured on a picture: that of the sample this copy is
        // built on (people sign with different pens and pressure), else the average, else a usual ballpoint.
        let widths = usable.compactMap(\.penWidth)
        let own = usable[ai].penWidth ?? (widths.isEmpty ? nil : widths.reduce(0, +) / Double(widths.count))
        // The renderer's ink spreads a little beyond the nominal width (bleed, pressure on downstrokes): measured the
        // same way, a copy comes out about 25 % wider (calibrated on the synthetic sheet of SignatureFidelity, where
        // source and copies come from the same renderer), so the measured width is scaled back.
        let penRatio = own.map { CGFloat(min(0.06, max(0.012, $0 * 0.79))) } ?? 0.024
        // A learned hand varies less than an invented one: the samples already carry its natural spread.
        return SignatureGenerator.draw(strokes: viaStrokes, hand: hand, slant: 0, penRatio: penRatio, pen: pen, ink: ink,
                                       pixelHeight: pixelHeight, copy: &copy,
                                       // With several samples the spread comes from them; synthetic drift on top would
                                       // erase small features. One sample needs more invented variation.
                                       variation: samples.count > 2 ? 0.3 : samples.count > 1 ? 0.5 : 0.85,
                                       durations: anyTimed ? durations : nil,
                                       // A traced stroke already ends where the user's pen left the paper (or at a
                                       // junction): no extra run-out that would shoot past the line.
                                       runOut: anyTimed,
                                       // The shape is the user's own: smooth only the tracing's pixel steps, keep cusps.
                                       smoothing: anyTimed ? nil : 0.012, sharpCorners: true,
                                       // Small letters of a learned signature have via-points close together: merging
                                       // points within 1.2 % of the height (fine for invented letters) flattens them.
                                       merge: 0.002, layout: 1, strokeShift: samples.count > 2 ? 0.025 : 0.015,
                                       capture: capture)
    }

    /// Dense up-and-down strokes (a garland like «ини», «mm») merge into a solid band on a small photo, and its
    /// centre line is nearly straight. Where the traced ink is much thicker than the pen over a stretch, the pen
    /// writes a garland again: humps across the band as tall as the band, about one and a half band widths apart.
    static func garland(_ s: [Point], pen: Double, rng: inout SeededRandom) -> [Point] {
        guard pen > 0, s.count > 3, s.contains(where: { $0.band > pen * 2.2 }) else { return s }
        // Even steps, fine enough for the humps.
        var lengths: [CGFloat] = [0]
        for i in 1..<s.count { lengths.append(lengths[i - 1] + hypot(s[i].p.x - s[i - 1].p.x, s[i].p.y - s[i - 1].p.y)) }
        let thickest = s.map(\.band).max() ?? 0
        let step = CGFloat(max(pen, thickest) / 6)
        let dense = resample(s, count: max(s.count, Int(lengths.last! / max(1e-4, step)) + 1))
        // Runs of band, long enough to be a garland (a crossing is thick only briefly).
        var out = dense
        var i = 0
        while i < dense.count {
            guard dense[i].band > pen * 2.2 else { i += 1; continue }
            var j = i
            while j + 1 < dense.count, dense[j + 1].band > pen * 1.8 { j += 1 }
            let band = dense[i...j].map(\.band).reduce(0, +) / Double(j - i + 1)
            let wave = max(band * 1.5, pen * 4) * Double(rng.range(0.9...1.1))
            let run = Double(j - i) * Double(step)
            if run > wave * 1.5 {
                let amp = (band - pen) / 2 * 0.9
                for k in i...j {
                    let a = dense[max(i, k - 2)].p, b = dense[min(j, k + 2)].p
                    let len = max(1e-6, hypot(b.x - a.x, b.y - a.y))
                    let nx = -(b.y - a.y) / len, ny = (b.x - a.x) / len
                    let along = Double(k - i) * Double(step)
                    let taper = min(1, along / (wave / 2), (run - along) / (wave / 2))
                    let off = CGFloat(amp * max(0, taper) * sin(along / wave * 2 * .pi))
                    out[k].p = CGPoint(x: dense[k].p.x + nx * off, y: dense[k].p.y + ny * off)
                }
            }
            i = j + 1
        }
        return out
    }

    /// Same number of strokes with similar lengths: blending them gives a plausible in-between copy.
    private static func similar(_ a: [[Point]], _ b: [[Point]]) -> Bool {
        func len(_ s: [Point]) -> CGFloat { zip(s, s.dropFirst()).reduce(0) { $0 + hypot($1.1.p.x - $1.0.p.x, $1.1.p.y - $1.0.p.y) } }
        return zip(a, b).allSatisfy { sa, sb in
            let la = len(sa), lb = len(sb)
            return abs(la - lb) / max(1e-6, max(la, lb)) < 0.3 && hypot(sa[0].p.x - sb[0].p.x, sa[0].p.y - sb[0].p.y) < 0.35
        }
    }

    // MARK: - Pictures (scan, sketch from iPhone): ink → centre lines → strokes

    /// Traces a picture of a signature into strokes (no timing). Dark or opaque pixels are ink.
    static func trace(_ image: CGImage) -> SignatureSample? {
        // Work at about 360 px height: small letters inside a large signature keep a few pixels of their own. Small
        // pictures (one signature cut from a photographed sheet is often 60–100 px tall) are enlarged up to 4×, or
        // thinning merges small loops into blobs and their short pieces are dropped as spurs.
        let k = min(4, 360 / CGFloat(max(1, image.height)), 1600 / CGFloat(max(1, image.width)))
        let w = max(8, Int(CGFloat(image.width) * k)), h = max(8, Int(CGFloat(image.height) * k))
        guard let ctx = InkDraw.context(w, h), let data = ctx.data?.assumingMemoryBound(to: UInt8.self) else { return nil }
        ctx.setFillColor(.white)
        ctx.fill(CGRect(x: 0, y: 0, width: w, height: h))
        ctx.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
        var lum = [Double](repeating: 0, count: w * h)
        for i in 0..<(w * h) { lum[i] = (Double(data[i * 4]) * 3 + Double(data[i * 4 + 1]) * 6 + Double(data[i * 4 + 2])) / 10 }
        var m = inkMask(lum, w, h)
        // Line width from clearly inked pixels only: the faint fringe that hysteresis adds to keep fast strokes
        // connected would make every line measure wider than it is.
        let dark = darkness(lum, w, h), strong = inkThreshold(dark)
        let inkArea = dark.reduce(0) { $0 + ($1 > strong ? 1 : 0) }
        // Ink thickness around every pixel (distance to paper, both ways): kept with the centre line, so a garland
        // that merged into a band can be written again.
        var depth = m.map { $0 == 1 ? Double.infinity : 0 }
        for y in 0..<h { for x in 0..<w where depth[y * w + x] > 0 {
            var v = depth[y * w + x]
            if x > 0 { v = min(v, depth[y * w + x - 1] + 1) }; if y > 0 { v = min(v, depth[(y - 1) * w + x] + 1) }
            if x > 0, y > 0 { v = min(v, depth[(y - 1) * w + x - 1] + 1.414) }; if x < w - 1, y > 0 { v = min(v, depth[(y - 1) * w + x + 1] + 1.414) }
            depth[y * w + x] = v
        } }
        for y in stride(from: h - 1, through: 0, by: -1) { for x in stride(from: w - 1, through: 0, by: -1) where depth[y * w + x] > 0 {
            var v = depth[y * w + x]
            if x < w - 1 { v = min(v, depth[y * w + x + 1] + 1) }; if y < h - 1 { v = min(v, depth[(y + 1) * w + x] + 1) }
            if x < w - 1, y < h - 1 { v = min(v, depth[(y + 1) * w + x + 1] + 1.414) }; if x > 0, y < h - 1 { v = min(v, depth[(y + 1) * w + x - 1] + 1.414) }
            depth[y * w + x] = v
        } }
        let solid = m
        thin(&m, w, h)
        let centreLine = max(1, m.reduce(0) { $0 + Int($1) })
        // Short pieces are spurs where lines cross, not strokes (about a line width and a half).
        let width = Double(inkArea) / Double(centreLine)
        // Spurs: short pieces with a free end (a bump of the thinning), not short pieces between two crossings —
        // those are parts of small letters.
        let all = tracePaths(m, w, h)
        func attached(_ p: (Double, Double), to i: Int) -> Bool {
            all.indices.contains { j in j != i && all[j].contains { hypot($0.0 - p.0, $0.1 - p.1) < max(3, width * 1.2) } }
        }
        // A piece shorter than two line widths between two junctions is the crossing itself (thinning turns an X into
        // two forks joined by a short bridge): it is left out and the strokes are joined across the crossing.
        let paths = all.enumerated().filter { i, p in
            Double(p.count) >= max(8, width * 1.6) || (p.count >= 3 && attached(p[0], to: i) && attached(p[p.count - 1], to: i))
        }.map(\.element)
        guard !paths.isEmpty else { return nil }
        let ys = paths.flatMap { $0.map(\.1) }
        let height = max(1, (ys.max() ?? 1) - (ys.min() ?? 0))
        // Rows in memory run top-down: flip to y up. Order strokes left to right by their start.
        let assembled = detached(joined(paths, gap: max(4, width * 1.8), height: Double(height), width: width), height: Double(height))
        let strokes = sharpened(assembled, solid: solid, w: w, h: h, width: width).map { $0.map { p -> [Double] in
            let x = min(w - 1, max(0, Int(p.0.rounded()))), y = min(h - 1, max(0, Int(p.1.rounded())))
            return [p.0, Double(h - 1) - p.1, -1, -1, min(Double(h), 2 * depth[y * w + x])]
        } }
            .sorted { ($0.first?[0] ?? 0) < ($1.first?[0] ?? 0) }
        return SignatureSample(strokes: strokes, penWidth: width / height)
    }

    /// Pieces of one movement, cut apart where the centre line turns sharply or touches itself (the cusp of З, the
    /// top of a loop), joined back into one stroke with a corner: ends that meet are one pen path, not two strokes.
    /// Each stroke is then written left to right, as a hand moves along a line of writing.
    static func joined(_ paths: [[(Double, Double)]], gap: Double, height: Double, width: Double) -> [[(Double, Double)]] {
        typealias P = (Double, Double)
        var pieces = paths.filter { $0.count > 1 }
        func d(_ a: P, _ b: P) -> Double { hypot(a.0 - b.0, a.1 - b.1) }
        func length(_ s: [P]) -> Double { zip(s, s.dropFirst()).reduce(0) { $0 + d($1.0, $1.1) } }
        // T-junctions: a piece that ends on the middle of another (tracing carried the other straight through the
        // crossing). Split the other one there so the crossing becomes a node like any other.
        var split = true
        while split {
            split = false
            outer: for i in pieces.indices { for end in [pieces[i][0], pieces[i][pieces[i].count - 1]] {
                for j in pieces.indices where j != i && pieces[j].count > 8 {
                    let s = pieces[j]
                    guard let k = s.indices.dropFirst(4).dropLast(4).min(by: { d(s[$0], end) < d(s[$1], end) }),
                          d(s[k], end) < gap, d(s[0], end) > gap, d(s[s.count - 1], end) > gap else { continue }
                    pieces[j] = Array(s[...k]); pieces.append(Array(s[k...]))
                    split = true
                    break outer
                }
            } }
        }
        // Spurs: a short piece with one free end, hanging off another line (a bump of the thinning, the turn-back at
        // the top of З). A short piece free at both ends is a real stroke — a dot or a dash — and stays.
        func near(_ p: P, except i: Int) -> Bool { pieces.indices.contains { j in j != i && pieces[j].contains { d($0, p) < gap } } }
        pieces = pieces.enumerated().filter { i, s in
            guard length(s) < height * 0.2 else { return true }
            let a = near(s[0], except: i), b = near(s[s.count - 1], except: i)
            return a == b
        }.map(\.element)
        // Ends: piece k, side 0 (start) or 1 (end). Node = ends that meet (union–find).
        var parent = Array(0..<(pieces.count * 2))
        func find(_ x: Int) -> Int { var x = x; while parent[x] != x { parent[x] = parent[parent[x]]; x = parent[x] }; return x }
        func union(_ a: Int, _ b: Int) { parent[find(a)] = find(b) }
        func point(_ e: Int) -> P { let s = pieces[e / 2]; return e % 2 == 0 ? s[0] : s[s.count - 1] }
        for a in 0..<(pieces.count * 2) { for b in (a + 1)..<(pieces.count * 2) where a / 2 != b / 2 && d(point(a), point(b)) < gap { union(a, b) } }
        // A short piece between two nodes is the crossing itself (thinning splits an X into two forks joined by a
        // bridge): the two nodes become one and the bridge is left out. Decided per crossing, not by letter size.
        var removed = Set<Int>()
        for k in pieces.indices where length(pieces[k]) < gap * 1.6 {
            let a = find(k * 2), b = find(k * 2 + 1)
            let others = { (n: Int) in (0..<(pieces.count * 2)).contains { $0 / 2 != k && find($0) == n } }
            if a != b, others(a), others(b) { union(a, b); removed.insert(k) }
        }
        /// Direction the pen moves arriving at end `e` (from inside the piece out through that end).
        func arriving(_ e: Int) -> P {
            let s = pieces[e / 2], n = min(s.count - 1, max(2, Int(gap * 1.5)))
            let p = e % 2 == 0 ? s[0] : s[s.count - 1], q = e % 2 == 0 ? s[n] : s[s.count - 1 - n]
            let len = max(1e-9, d(p, q)); return ((p.0 - q.0) / len, (p.1 - q.1) / len)
        }
        let short = height * 0.25
        // At every node, pair the ends the way a pen passes: straight through (turn < 60°), back from a pointed tip
        // (> 150°), and a sharper turn only into a short piece (a stem the pen went back up). Best pairs first.
        var partner = [Int: Int]()
        var nodes = [Int: [Int]]()
        for e in 0..<(pieces.count * 2) where !removed.contains(e / 2) { nodes[find(e), default: []].append(e) }
        for (_, ends) in nodes where ends.count > 1 {
            var options: [(Double, Int, Int)] = []
            for x in ends.indices { for y in ends.indices where y > x && ends[x] / 2 != ends[y] / 2 {
                let a = arriving(ends[x]), b = arriving(ends[y])
                // Leaving through y means moving against y's arriving direction.
                let turn = acos(max(-1, min(1, a.0 * -b.0 + a.1 * -b.1)))
                let score: Double
                if turn < .pi / 3 { score = turn / .pi }
                else if turn > .pi * 5 / 6 { score = 0.5 }
                else if min(length(pieces[ends[x] / 2]), length(pieces[ends[y] / 2])) < short { score = 1.0 + turn / .pi }
                else { continue }
                options.append((score, ends[x], ends[y]))
            } }
            for (_, x, y) in options.sorted(by: { $0.0 < $1.0 }) where partner[x] == nil && partner[y] == nil {
                partner[x] = y; partner[y] = x
            }
        }
        // Loops that leave and return to one crossing (the bowl of б, a looped stem) are part of the stroke passing
        // through it: the pen arrives, goes round, and carries on. Drawn on their own, their ends stick out of the
        // crossing as ticks. Splice each into the passing pair (or onto a stroke that ends there), oriented for the
        // smoothest turns.
        func turn(_ x: Int, _ y: Int) -> Double {   // arriving at x, leaving through y
            let a = arriving(x), b = arriving(y)
            return acos(max(-1, min(1, a.0 * -b.0 + a.1 * -b.1)))
        }
        for k in pieces.indices where !removed.contains(k) && find(k * 2) == find(k * 2 + 1) && partner[k * 2] == nil && partner[k * 2 + 1] == nil {
            let node = find(k * 2)
            let here = (nodes[node] ?? []).filter { $0 / 2 != k }
            var best: (cost: Double, x: Int, y: Int?, a: Int, b: Int)?
            for x in here {
                let y = partner[x]
                for (a, b) in [(k * 2, k * 2 + 1), (k * 2 + 1, k * 2)] {
                    // x → loop (enter at a, leave at b) → y
                    let cost = turn(x, a) + (y.map { turn(b, $0) } ?? 0)
                    if y != nil || here.filter({ partner[$0] == nil }).contains(x), cost < (best?.cost ?? .infinity) { best = (cost, x, y, a, b) }
                }
            }
            if let b = best {
                partner[b.x] = b.a; partner[b.a] = b.x
                if let y = b.y { partner[b.b] = y; partner[y] = b.b }
            }
        }
        if ProcessInfo.processInfo.environment["K42_TRACE_DEBUG"] != nil {
            let multi = nodes.values.filter { $0.count > 1 }
            print("trace: \(pieces.count) pieces, gap \(Int(gap)), \(multi.count) junctions \(multi.map(\.count)), removed \(removed.count), paired \(partner.count / 2)")
            for k in pieces.indices { print("  piece \(k) len \(Int(length(pieces[k]))) start \(pieces[k][0]) end \(pieces[k][pieces[k].count - 1])") }
        }
        // Walk the pairings into pen strokes.
        var used = Set<Int>(removed)
        var strokes: [[P]] = []
        func walk(from start: Int) -> [P] {
            var path: [P] = [], e = start
            while !used.contains(e / 2) {
                used.insert(e / 2)
                let s = pieces[e / 2]
                path += e % 2 == 0 ? s : Array(s.reversed())
                let exit = e % 2 == 0 ? e + 1 : e - 1
                guard let next = partner[exit], !used.contains(next / 2) else { break }
                e = next
            }
            return path
        }
        // Strokes start at free ends (no partner), then any remaining closed loops.
        for e in 0..<(pieces.count * 2) where !used.contains(e / 2) && partner[e] == nil { strokes.append(walk(from: e)) }
        for k in pieces.indices where !used.contains(k) { strokes.append(walk(from: k * 2)) }
        strokes = strokes.filter { $0.count > 1 }
        // Fragments left over at crossings: short and lying on another stroke. Drawn as a pen stroke of their own
        // they become ticks across the line; a real short stroke (a dot, a bar) stands apart from the others.
        strokes = strokes.enumerated().filter { i, s in
            guard length(s) < height * 0.12 else { return true }
            let touching = s.contains { p in strokes.indices.contains { k in k != i && strokes[k].contains { d($0, p) < max(3, width * 1.5) } } }
            return !touching
        }.map(\.element)
        // An end that meets another line stops there: its last few pixels past the contact would draw a jab across
        // the line, which no pen makes.
        for i in strokes.indices {
            for atEnd in [false, true] {
                var s = atEnd ? strokes[i] : Array(strokes[i].reversed())
                let reach = min(s.count - 2, max(3, Int(gap * 1.5)))
                guard reach > 2 else { continue }
                let tail = (s.count - reach)..<s.count
                var cut: Int?
                for k in tail where strokes.indices.contains(where: { j in j != i && strokes[j].contains { d($0, s[k]) < max(2, width * 0.6) } }) {
                    cut = k; break
                }
                if let c = cut, c > 1 { s = Array(s[...c]); strokes[i] = atEnd ? s : Array(s.reversed()) }
            }
        }
        return strokes.map { s in (s.last!.0 < s.first!.0 - 1) ? Array(s.reversed()) : s }
    }

    /// Tips and stroke ends taken back from the ink. Thinning shortens every free end by about half a line width
    /// and rounds every reversal into an arc inside the ink; the ink itself still reaches the real tip. So:
    /// - a free end is extended along its direction to the edge of the ink, less half the pen width (a ballpoint's
    ///   round end);
    /// - where the line reverses (more than 120° across about two pen widths), the apex is moved out along the
    ///   bisector to the edge of the ink, less half the pen width, and the arms run straight into it.
    static func sharpened(_ strokes: [[(Double, Double)]], solid: [UInt8], w: Int, h: Int, width: Double) -> [[(Double, Double)]] {
        typealias P = (Double, Double)
        func inside(_ p: P) -> Bool {
            let x = Int(p.0.rounded()), y = Int(p.1.rounded())
            return x >= 0 && y >= 0 && x < w && y < h && solid[y * w + x] == 1
        }
        /// From `p` along unit `u`: the last point still on ink, within `limit` pixels.
        func edge(_ p: P, _ u: P, limit: Double) -> Double {
            var t = 0.0
            while t < limit, inside((p.0 + u.0 * (t + 0.5), p.1 + u.1 * (t + 0.5))) { t += 0.5 }
            return t
        }
        func unit(_ a: P, _ b: P) -> P { let l = max(1e-9, hypot(b.0 - a.0, b.1 - a.1)); return ((b.0 - a.0) / l, (b.1 - a.1) / l) }
        let half = width / 2
        return strokes.map { stroke in
            var s = stroke
            guard s.count > 4 else { return s }
            // Free ends.
            let back = min(s.count - 1, max(2, Int(width * 1.5)))
            let endDir = unit(s[s.count - 1 - back], s[s.count - 1])
            let endReach = edge(s[s.count - 1], endDir, limit: width * 3) - half
            if endReach > 0.5 {
                let e = s[s.count - 1]
                for k in 1...Int(endReach.rounded(.up)) { let t = min(endReach, Double(k)); s.append((e.0 + endDir.0 * t, e.1 + endDir.1 * t)) }
            }
            let startDir = unit(s[back], s[0])
            let startReach = edge(s[0], startDir, limit: width * 3) - half
            if startReach > 0.5 {
                let b = s[0]
                let lead = (1...Int(startReach.rounded(.up))).reversed().map { k -> P in let t = min(startReach, Double(k)); return (b.0 + startDir.0 * t, b.1 + startDir.1 * t) }
                s = lead + s
            }
            // Reversals.
            let arm = max(3, Int(width * 2))
            var i = arm
            while i + arm < s.count {
                let a = s[i - arm], c = s[i + arm], p = s[i]
                let u1 = unit(a, p), u2 = unit(p, c)
                let turn = acos(max(-1, min(1, u1.0 * u2.0 + u1.1 * u2.1)))
                guard turn > 2.1 else { i += 1; continue }
                // The sharpest point of this bend: farthest from the chord between the arms.
                let mid = ((a.0 + c.0) / 2, (a.1 + c.1) / 2)
                let apexIndex = ((i - arm + 1)...(i + arm - 1)).max { hypot(s[$0].0 - mid.0, s[$0].1 - mid.1) < hypot(s[$1].0 - mid.0, s[$1].1 - mid.1) } ?? i
                let q = s[apexIndex]
                // Outward: away from the inside of the bend (bisector of the two arms seen from the apex).
                let ua = unit(q, a), uc = unit(q, c)
                let out = unit((ua.0 + uc.0, ua.1 + uc.1), (0, 0))
                let reach = edge(q, out, limit: width * 3) - half
                if reach > 0.5 {
                    let tip = (q.0 + out.0 * reach, q.1 + out.1 * reach)
                    let lo = max(0, apexIndex - arm), hi = min(s.count - 1, apexIndex + arm)
                    var run: [P] = []
                    let n1 = max(1, apexIndex - lo), n2 = max(1, hi - apexIndex)
                    for k in 0..<n1 { let t = Double(k) / Double(n1); run.append((s[lo].0 + (tip.0 - s[lo].0) * t, s[lo].1 + (tip.1 - s[lo].1) * t)) }
                    for k in 0...n2 { let t = Double(k) / Double(n2); run.append((tip.0 + (s[hi].0 - tip.0) * t, tip.1 + (s[hi].1 - tip.1) * t)) }
                    s.replaceSubrange(lo...hi, with: run)
                    i = lo + run.count + arm
                } else { i = apexIndex + arm }
            }
            return s
        }
    }

    /// Without specks: short pieces lying apart from the rest of the signature (a neighbour's ink, the paper's edge,
    /// a dot of dirt). A short piece close to other strokes (an i-dot, a bar) stays.
    static func detached(_ strokes: [[(Double, Double)]], height: Double) -> [[(Double, Double)]] {
        func length(_ s: [(Double, Double)]) -> Double { zip(s, s.dropFirst()).reduce(0) { $0 + hypot($1.1.0 - $1.0.0, $1.1.1 - $1.0.1) } }
        let longest = strokes.map(length).max() ?? 0
        return strokes.enumerated().filter { i, s in
            guard length(s) < longest * 0.2 else { return true }
            let gap = strokes.enumerated().filter { $0.offset != i }.flatMap(\.element).map { q in
                s.map { hypot($0.0 - q.0, $0.1 - q.1) }.min() ?? .infinity
            }.min() ?? .infinity
            return gap < height * 0.3
        }.map(\.element)
    }

    /// Ink pixels of a photo or scan: darkness against the local paper (a phone photo has grey, unevenly lit paper),
    /// a threshold chosen by Otsu's method between paper and ink, then a closing that joins a faint line broken by
    /// the paper's grain.
    static func inkMask(_ lum: [Double], _ w: Int, _ h: Int) -> [UInt8] {
        let dark = darkness(lum, w, h)
        let t = inkThreshold(dark)
        // Hysteresis: a ballpoint leaves less ink where it moves fast, so a stroke fades without ending. Pixels
        // above a lower threshold count as ink when they connect to clearly inked ones; lone grain does not.
        var m = dark.map { $0 > t ? UInt8(1) : 0 }
        // The insides of small loops (о, е, the bowl of б) as clear ink encloses them: hysteresis and closing must
        // not fill them, or the loop becomes a blot and the copy a dot.
        var holes = [Bool](repeating: false, count: w * h)
        do {
            var outside = [Bool](repeating: false, count: w * h)
            var stack: [Int] = []
            for x in 0..<w { for y in [0, h - 1] where m[y * w + x] == 0 { stack.append(y * w + x) } }
            for y in 0..<h { for x in [0, w - 1] where m[y * w + x] == 0 { stack.append(y * w + x) } }
            for i in stack { outside[i] = true }
            while let i = stack.popLast() {
                let x = i % w, y = i / w
                for (nx, ny) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] where nx >= 0 && ny >= 0 && nx < w && ny < h {
                    let j = ny * w + nx
                    if m[j] == 0, !outside[j] { outside[j] = true; stack.append(j) }
                }
            }
            for i in 0..<(w * h) where m[i] == 0 && !outside[i] { holes[i] = true }
        }
        let low = max(12, t * 0.65)
        var stack = m.indices.filter { m[$0] == 1 }
        while let i = stack.popLast() {
            let x = i % w, y = i / w
            for dy in -1...1 { for dx in -1...1 where dx != 0 || dy != 0 {
                let nx = x + dx, ny = y + dy
                guard nx >= 0, ny >= 0, nx < w, ny < h else { continue }
                let j = ny * w + nx
                if m[j] == 0, !holes[j], dark[j] > low { m[j] = 1; stack.append(j) }
            } }
        }
        func shift(_ src: [UInt8], grow: Bool) -> [UInt8] {
            var out = src
            for y in 0..<h { for x in 0..<w {
                var v = src[y * w + x]
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let nx = x + dx, ny = y + dy
                    guard nx >= 0, ny >= 0, nx < w, ny < h else { if !grow { v = 0 }; continue }
                    v = grow ? max(v, src[ny * w + nx]) : min(v, src[ny * w + nx])
                }
                out[y * w + x] = v
            } }
            return out
        }
        // Closing only for a faint, thin line (few ink pixels): on a normal line it would merge strokes that run close
        // together (the cusp of З, a small loop) into false crossings.
        let share = Double(m.reduce(0) { $0 + Int($1) }) / Double(max(1, w * h))
        if share < 0.012 { m = shift(shift(m, grow: true), grow: false) }
        for i in 0..<(w * h) where holes[i] { m[i] = 0 }
        return m
    }

    /// Otsu's threshold between paper and ink on darkness values, kept between 16 and 70.
    static func inkThreshold(_ dark: [Double]) -> Double {
        var histogram = [Double](repeating: 0, count: 256)
        for d in dark { histogram[min(255, max(0, Int(d)))] += 1 }
        let total = Double(dark.count)
        var sum = 0.0; for (i, c) in histogram.enumerated() { sum += Double(i) * c }
        var best = 0.0, threshold = 40, below = 0.0, sumBelow = 0.0
        for t in 0..<255 {
            below += histogram[t]; sumBelow += Double(t) * histogram[t]
            guard below > 0, below < total else { continue }
            let m0 = sumBelow / below, m1 = (sum - sumBelow) / (total - below)
            let between = below * (total - below) * (m0 - m1) * (m0 - m1)
            if between > best { best = between; threshold = t }
        }
        // Clean white paper gives Otsu a tiny threshold (noise); a very faint picture a huge one.
        return Double(min(70, max(16, threshold)))
    }

    /// How much darker each pixel is than the paper around it (0…255): the paper level is the brightest part of a
    /// wide neighbourhood, so ink, shadows and lighting gradients do not pull it down.
    static func darkness(_ lum: [Double], _ w: Int, _ h: Int) -> [Double] {
        let r = max(6, min(w, h) / 6)
        // Local paper = box mean of the brightest values: max over small cells, then a box blur over them.
        let cell = max(2, r / 3), cw = (w + cell - 1) / cell, chh = (h + cell - 1) / cell
        var peaks = [Double](repeating: 0, count: cw * chh)
        for y in 0..<h { for x in 0..<w { let i = (y / cell) * cw + x / cell; peaks[i] = max(peaks[i], lum[y * w + x]) } }
        var smooth = peaks
        let k = max(1, r / cell)
        for cy in 0..<chh { for cx in 0..<cw {
            var s = 0.0, n = 0.0
            for yy in max(0, cy - k)...min(chh - 1, cy + k) { for xx in max(0, cx - k)...min(cw - 1, cx + k) { s += peaks[yy * cw + xx]; n += 1 } }
            smooth[cy * cw + cx] = s / n
        } }
        var out = [Double](repeating: 0, count: w * h)
        for y in 0..<h { for x in 0..<w {
            let paper = smooth[(y / cell) * cw + x / cell]
            out[y * w + x] = max(0, paper - lum[y * w + x]) * 255 / max(60, paper)
        } }
        return out
    }

    /// Zhang–Suen thinning to one-pixel centre lines.
    private static func thin(_ m: inout [UInt8], _ w: Int, _ h: Int) {
        var changed = true
        var remove: [Int] = []
        while changed {
            changed = false
            for step in 0..<2 {
                remove.removeAll(keepingCapacity: true)
                for y in 1..<(h - 1) {
                    let row = y * w
                    for x in 1..<(w - 1) where m[row + x] == 1 {
                        let p2 = m[row - w + x], p3 = m[row - w + x + 1], p4 = m[row + x + 1], p5 = m[row + w + x + 1]
                        let p6 = m[row + w + x], p7 = m[row + w + x - 1], p8 = m[row + x - 1], p9 = m[row - w + x - 1]
                        let b = Int(p2) + Int(p3) + Int(p4) + Int(p5) + Int(p6) + Int(p7) + Int(p8) + Int(p9)
                        guard b >= 2, b <= 6 else { continue }
                        let seq = [p2, p3, p4, p5, p6, p7, p8, p9, p2]
                        var a = 0
                        for k in 0..<8 where seq[k] == 0 && seq[k + 1] == 1 { a += 1 }
                        guard a == 1 else { continue }
                        if step == 0 { guard p2 * p4 * p6 == 0, p4 * p6 * p8 == 0 else { continue } }
                        else { guard p2 * p4 * p8 == 0, p2 * p6 * p8 == 0 else { continue } }
                        remove.append(row + x)
                    }
                }
                if !remove.isEmpty { changed = true }
                for i in remove { m[i] = 0 }
            }
        }
    }

    /// Walks the centre-line graph into strokes: from the leftmost free end, always continuing in the straightest
    /// direction at crossings (good continuation), until every pixel is used; closed loops are picked up too.
    private static func tracePaths(_ m: [UInt8], _ w: Int, _ h: Int) -> [[(Double, Double)]] {
        var used = [Bool](repeating: false, count: w * h)
        let offsets = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)]
        func on(_ x: Int, _ y: Int) -> Bool { x >= 0 && y >= 0 && x < w && y < h && m[y * w + x] == 1 }
        func neighbours(_ x: Int, _ y: Int) -> [(Int, Int)] { offsets.compactMap { on(x + $0.0, y + $0.1) ? (x + $0.0, y + $0.1) : nil } }
        var ends: [(Int, Int)] = []
        for y in 0..<h { for x in 0..<w where m[y * w + x] == 1 && neighbours(x, y).count == 1 { ends.append((x, y)) } }
        ends.sort { $0.0 < $1.0 }
        var paths: [[(Int, Int)]] = []
        func walk(from start: (Int, Int)) {
            var path = [start]
            used[start.1 * w + start.0] = true
            var cur = start
            var dir: (Double, Double) = (1, 0)
            while true {
                let free = neighbours(cur.0, cur.1).filter { !used[$0.1 * w + $0.0] }
                guard !free.isEmpty else { break }
                // Prefer the step closest to the current direction (looking back a few pixels).
                let back = path[max(0, path.count - 6)]
                let dx = Double(cur.0 - back.0), dy = Double(cur.1 - back.1), dl = max(1e-6, (dx * dx + dy * dy).squareRoot())
                if path.count > 2 { dir = (dx / dl, dy / dl) }
                let next = free.max { a, b in
                    let ax = Double(a.0 - cur.0), ay = Double(a.1 - cur.1), bx = Double(b.0 - cur.0), by = Double(b.1 - cur.1)
                    return (ax * dir.0 + ay * dir.1) / (ax * ax + ay * ay).squareRoot() < (bx * dir.0 + by * dir.1) / (bx * bx + by * by).squareRoot()
                }!
                used[next.1 * w + next.0] = true
                path.append(next)
                cur = next
            }
            if path.count > 1 { paths.append(path) }
        }
        for e in ends where !used[e.1 * w + e.0] { walk(from: e) }
        // Closed loops (an "o" with no free end).
        for y in 0..<h { for x in 0..<w where m[y * w + x] == 1 && !used[y * w + x] { walk(from: (x, y)) } }
        // Smooth pixel steps a little (5-point average) so via-points follow the line, not the pixel grid.
        return paths.map { p in
            p.indices.map { i in
                let r = max(0, i - 2)...min(p.count - 1, i + 2)
                let n = Double(r.count)
                return (Double(p[r].reduce(0) { $0 + $1.0 }) / n, Double(p[r].reduce(0) { $0 + $1.1 }) / n)
            }
        }
    }
}
