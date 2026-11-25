import CoreImage
import CoreImage.CIFilterBuiltins
import Foundation
import Vision

/// What to do with a page after it is scanned.
struct ProcessOptions: Equatable {
    var crop = true
    var deskew = true
    var autoRotate = true
    var detectBlank = false
    var enhancement: Enhancement = .document
    var colorMode: ColorMode = .color
    var brightness = 0.0
    var contrast = 0.0
    var threshold = 0.5
    var descreen = false
    /// Ink coverage below which a page counts as blank (fraction of the page area).
    var blankLimit = 0.0012
    /// Scan resolution: sizes of filters scale with it.
    var dpi = 300.0
    /// Outline already known (from the overview), normalised to the image (0…1, origin top-left).
    var priorQuad: Quad?

    var isValid: Bool {
        dpi.isFinite && dpi > 0 && dpi <= 9_600 && brightness.isFinite && (-1...1).contains(brightness)
        && contrast.isFinite && (-1...1).contains(contrast) && threshold.isFinite && (0...1).contains(threshold)
        && blankLimit.isFinite && (0...1).contains(blankLimit)
        && (priorQuad.map { $0.isValid && $0.points.allSatisfy { $0.x >= 0 && $0.x <= 1 && $0.y >= 0 && $0.y <= 1 } } ?? true)
    }
}

/// What processing found and did (shown in the CLI and kept with the page).
struct ProcessReport: Codable, Equatable {
    var cropMethod: String?
    var quad: Quad?
    var cropAngle = 0.0
    var deskewAngle = 0.0
    var rotation = 0
    var ink = 0.0
    var blank = false
    var tone = 0.0
    var inputSize = CGSize.zero
    var outputSize = CGSize.zero

    var summary: String {
        var parts = [String(format: "in=%.0fx%.0f out=%.0fx%.0f", inputSize.width, inputSize.height, outputSize.width, outputSize.height)]
        if let cropMethod, let quad { parts.append(String(format: "crop=%@ angle=%.2f° box=%@", cropMethod, cropAngle, quad.description)) }
        else { parts.append("crop=none") }
        parts.append(String(format: "deskew=%.2f°", deskewAngle))
        parts.append("rotate=\(rotation)°")
        parts.append(String(format: "ink=%.3f%% tone=%.1f", ink * 100, tone))
        if blank { parts.append("BLANK") }
        return parts.joined(separator: " ")
    }
}

enum Processor {
    static let context = CIContext(options: [.workingColorSpace: CGColorSpace(name: CGColorSpace.sRGB)!,
                                             .outputColorSpace: CGColorSpace(name: CGColorSpace.sRGB)!])

    static func render(_ image: CIImage) -> CGImage? {
        let extent = image.extent.integral
        guard extent.origin.x.isFinite, extent.origin.y.isFinite, extent.width.isFinite, extent.height.isFinite,
              extent.width > 0, extent.height > 0, extent.width <= 32_768, extent.height <= 32_768,
              Bitmap.validSize(width: Int(extent.width), height: Int(extent.height), channels: 4) else { return nil }
        return context.createCGImage(image, from: extent, format: .RGBA8, colorSpace: Bitmap.sRGB)
    }

    // MARK: - Pipeline

    /// Crop + straighten → upright → blank check → clean-up, in that order.
    static func processChecked(_ input: CGImage, _ o: ProcessOptions) throws -> (CGImage, ProcessReport) {
        guard o.isValid, Bitmap.validSize(width: input.width, height: input.height, channels: 4) else { throw CocoaError(.fileReadCorruptFile) }
        var report = ProcessReport()
        report.inputSize = CGSize(width: input.width, height: input.height)
        var image = input

        if o.descreen {
            guard let d = descreen(image, dpi: o.dpi) else { throw CocoaError(.fileReadCorruptFile) }
            image = d
        }

        if o.crop, let found = cropQuad(image, o.priorQuad) {
            report.cropMethod = found.method
            report.quad = found.quad
            report.cropAngle = found.quad.angle
            guard let c = perspectiveCrop(image, found.quad.inset(CGFloat(max(2, o.dpi / 100)))) else { throw CocoaError(.fileReadCorruptFile) }
            image = c
        }
        // After a crop only a small correction is plausible; text skew beyond 8 degrees is more likely a picture's lines.
        // Photos are straightened by their outline only.
        if o.deskew, o.enhancement != .photo, let angle = skewAngle(image), abs(angle) >= 0.15, abs(angle) <= (report.cropMethod == nil ? 8 : 1.5) {
            report.deskewAngle = angle
            guard let r = rotate(image, degrees: -angle) else { throw CocoaError(.fileReadCorruptFile) }
            image = r
        }
        if o.autoRotate {
            let turn = uprightRotation(image)
            report.rotation = turn
            if turn != 0 {
                guard let r = rotate90(image, clockwise: turn) else { throw CocoaError(.fileReadCorruptFile) }
                image = r
            }
        }
        report.ink = inkCoverage(image)
        // Blank = almost no ink and no picture either (a photo has no "ink" but plenty of tone variation).
        report.tone = toneSpread(image)
        report.blank = report.ink < o.blankLimit && report.tone < 10

        guard let finished = finish(image, o) else { throw CocoaError(.fileReadCorruptFile) }
        image = finished
        report.outputSize = CGSize(width: image.width, height: image.height)
        return (image, report)
    }

    /// Enhancement, colour mode, brightness/contrast (the part re-run when only those settings change).
    static func finish(_ image: CGImage, _ o: ProcessOptions) -> CGImage? {
        guard o.isValid, Bitmap.validSize(width: image.width, height: image.height, channels: 4) else { return nil }
        switch o.enhancement {
        case .document:
            return enhanceDocument(image, o)
        case .photo:
            guard let p = enhancePhoto(image) else { return nil }
            return applyColor(p, o)
        case .none:
            return applyColor(image, o)
        }
    }

    // MARK: - Finding the document

    /// The outline to cut: what detection finds, unless a known outline (from the overview) disagrees with it.
    static func cropQuad(_ image: CGImage, _ prior: Quad?) -> (quad: Quad, method: String)? {
        let found = detectDocument(image)
        guard let prior, prior.isValid, prior.points.allSatisfy({ $0.x >= 0 && $0.x <= 1 && $0.y >= 0 && $0.y <= 1 }) else { return found }
        let known = prior.scaled(CGFloat(image.width), CGFloat(image.height))
        if let found, found.quad.iou(known) > 0.85 { return found }
        return (known, "overview")
    }

    /// The page lying on the glass: the scanner background around it is measured first (it is uniform),
    /// then Vision's document segmentation refines the corners when it agrees.
    static func detectDocument(_ image: CGImage) -> (quad: Quad, method: String)? {
        let glass = GlassSegmentation(image)
        let fromGlass = glass?.largest(minFraction: 0.08)
        let vision = visionDocument(image)
        let full = Double(image.width * image.height)
        if let g = fromGlass {
            // A quad that covers the whole frame means there is nothing to cut away.
            if g.area / full > 0.985 { return nil }
            if let v = vision, v.iou(g) > 0.93 { return (v, "vision") }
            return (g, "glass")
        }
        // No uniform glass around (white lid, feeder): trust Vision only for a clearly smaller page with a visible edge.
        if let v = vision, v.area / full > 0.2, v.area / full < 0.95, edgeContrast(image, v) > 18 { return (v, "vision") }
        return nil
    }

    static func visionDocument(_ image: CGImage) -> Quad? {
        let request = VNDetectDocumentSegmentationRequest()
        let handler = VNImageRequestHandler(cgImage: image, options: [:])
        guard (try? handler.perform([request])) != nil, let r = request.results?.first, r.confidence > 0.5 else { return nil }
        return quad(from: r, width: image.width, height: image.height)
    }

    static func quad(from r: VNRectangleObservation, width: Int, height: Int) -> Quad {
        let w = CGFloat(width), h = CGFloat(height)
        func p(_ v: CGPoint) -> CGPoint { CGPoint(x: v.x * w, y: (1 - v.y) * h) }
        return Quad.ordered([p(r.topLeft), p(r.topRight), p(r.bottomRight), p(r.bottomLeft)])
    }

    /// Mean brightness difference just inside vs just outside the quad's edges.
    static func edgeContrast(_ image: CGImage, _ q: Quad) -> Double {
        guard q.isValid else { return 0 }
        let bmp = Bitmap(image, maxSide: 600, gray: true)
        let s = CGFloat(bmp.width) / CGFloat(image.width)
        let small = q.scaled(s, s)
        let c = CGPoint(x: small.points.map(\.x).reduce(0, +) / 4, y: small.points.map(\.y).reduce(0, +) / 4)
        var inside = 0.0, outside = 0.0, n = 0.0
        let pts = small.points
        for i in 0..<4 {
            let a = pts[i], b = pts[(i + 1) % 4]
            for t in stride(from: 0.1, through: 0.9, by: 0.05) {
                let e = CGPoint(x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t)
                let dx = c.x - e.x, dy = c.y - e.y, len = max(1, hypot(dx, dy))
                let pin = CGPoint(x: e.x + dx / len * 4, y: e.y + dy / len * 4), pout = CGPoint(x: e.x - dx / len * 4, y: e.y - dy / len * 4)
                func v(_ p: CGPoint) -> Double? {
                    let x = Int(p.x), y = Int(p.y)
                    guard x >= 0, y >= 0, x < bmp.width, y < bmp.height else { return nil }
                    return Double(bmp.data[y * bmp.width + x])
                }
                if let i1 = v(pin), let o1 = v(pout) { inside += i1; outside += o1; n += 1 }
            }
        }
        return n == 0 ? 0 : abs(inside - outside) / n
    }

    // MARK: - Several photos on the glass

    /// Photos lying on the glass, as quads in the image's pixels. The glass background gives the outlines;
    /// Vision's rectangle detector adds any it finds that the background test missed.
    static func detectPhotos(_ image: CGImage) -> (quads: [Quad], glassCount: Int, visionCount: Int) {
        let glassQuads = GlassSegmentation(image)?.components(minFraction: 0.015) ?? []
        let visionQuads = visionRectangles(image)
        var result = glassQuads
        for v in visionQuads {
            let overlaps = result.contains { r in r.iou(v) > 0.2 || r.contains(center(v)) }
            if !overlaps { result.append(v) }
        }
        // Reading order: top to bottom, then left to right.
        result.sort { a, b in
            let ra = a.bounds, rb = b.bounds
            if abs(ra.midY - rb.midY) < min(ra.height, rb.height) / 2 { return ra.midX < rb.midX }
            return ra.midY < rb.midY
        }
        return (result, glassQuads.count, visionQuads.count)
    }

    static func center(_ q: Quad) -> CGPoint {
        CGPoint(x: q.points.map(\.x).reduce(0, +) / 4, y: q.points.map(\.y).reduce(0, +) / 4)
    }

    static func visionRectangles(_ image: CGImage) -> [Quad] {
        let request = VNDetectRectanglesRequest()
        request.maximumObservations = 12
        request.minimumSize = 0.12
        request.minimumAspectRatio = 0.2
        request.quadratureTolerance = 20
        request.minimumConfidence = 0.6
        let handler = VNImageRequestHandler(cgImage: image, options: [:])
        guard (try? handler.perform([request])) != nil else { return [] }
        var quads = (request.results ?? []).map { quad(from: $0, width: image.width, height: image.height) }
        // Keep the outermost of nested rectangles (a photo's own frame or content).
        quads.sort { $0.area > $1.area }
        var kept: [Quad] = []
        for q in quads where !kept.contains(where: { $0.contains(center(q)) }) { kept.append(q) }
        return kept
    }

    // MARK: - Geometry

    /// Cuts out the quad and makes it a straight rectangle (perspective correction also removes the tilt).
    static func perspectiveCrop(_ image: CGImage, _ q: Quad) -> CGImage? {
        guard q.isValid, q.points.allSatisfy({ $0.x >= 0 && $0.x <= CGFloat(image.width) && $0.y >= 0 && $0.y <= CGFloat(image.height) }) else { return nil }
        let h = CGFloat(image.height)
        func ci(_ p: CGPoint) -> CIVector { CIVector(x: p.x, y: h - p.y) }
        let f = CIFilter.perspectiveCorrection()
        f.inputImage = CIImage(cgImage: image)
        f.topLeft = ci(q.tl).cgPointValue
        f.topRight = ci(q.tr).cgPointValue
        f.bottomRight = ci(q.br).cgPointValue
        f.bottomLeft = ci(q.bl).cgPointValue
        guard let out = f.outputImage else { return nil }
        return render(out)
    }

    /// Rotates by `degrees` clockwise around the centre; uncovered corners become white (paper).
    static func rotate(_ image: CGImage, degrees: Double) -> CGImage? {
        guard degrees.isFinite else { return nil }
        let ci = CIImage(cgImage: image)
        let r = -degrees * .pi / 180   // CI rotates counter-clockwise with y up
        let cx = ci.extent.midX, cy = ci.extent.midY
        let t = CGAffineTransform(translationX: cx, y: cy).rotated(by: r).translatedBy(x: -cx, y: -cy)
        let rotated = ci.transformed(by: t)
        let white = CIImage(color: .white).cropped(to: ci.extent)
        return render(rotated.composited(over: white).cropped(to: ci.extent))
    }

    /// Quarter turns clockwise (90, 180, 270).
    static func rotate90(_ image: CGImage, clockwise degrees: Int) -> CGImage? {
        let o: CGImagePropertyOrientation
        switch ((degrees % 360) + 360) % 360 {
        case 90: o = .right
        case 180: o = .down
        case 270: o = .left
        default: return image
        }
        let ci = CIImage(cgImage: image).oriented(o)
        return render(ci.transformed(by: CGAffineTransform(translationX: -ci.extent.minX, y: -ci.extent.minY)))
    }

    /// Text skew in degrees (clockwise positive) from the projection profile of the dark pixels:
    /// lines of text give the sharpest row histogram when they are horizontal. nil when there's too little text.
    static func skewAngle(_ image: CGImage) -> Double? {
        let g = Bitmap(image, maxSide: 900, gray: true)
        let bg = g.boxBlur(radius: 12)
        var xs: [Float] = [], ys: [Float] = []
        let w = g.width, h = g.height
        let mx = w / 25, my = h / 25
        g.data.withUnsafeBufferPointer { s in
            bg.data.withUnsafeBufferPointer { b in
                for y in my..<(h - my) {
                    for x in mx..<(w - mx) {
                        let v = Int(s[y * w + x]), m = Int(b[y * w + x])
                        if v < m - 40 && v < 150 { xs.append(Float(x)); ys.append(Float(y)) }
                    }
                }
            }
        }
        guard xs.count > 400 else { return nil }
        // Subsample large sets.
        let step = max(1, xs.count / 60000)
        func score(_ deg: Double) -> Double {
            let a = deg * .pi / 180
            let sn = Float(sin(a)), cs = Float(cos(a))
            let bins = h * 2 + w
            var hist = [Int32](repeating: 0, count: bins)
            var i = 0
            while i < xs.count {
                // Row of the point after rotating the page by −deg.
                let r = Int(ys[i] * cs - xs[i] * sn) + w
                if r >= 0 && r < bins { hist[r] += 1 }
                i += step
            }
            var s = 0.0
            for v in hist { s += Double(v) * Double(v) }
            return s
        }
        var best = 0.0, bestScore = -1.0
        for k in -40...40 {
            let d = Double(k) * 0.25
            let sc = score(d)
            if sc > bestScore { bestScore = sc; best = d }
        }
        var fine = best
        for k in -10...10 {
            let d = best + Double(k) * 0.025
            let sc = score(d)
            if sc > bestScore { bestScore = sc; fine = d }
        }
        // Flat profile at 0 vs best: no text structure (photo) → no answer.
        let zero = score(0)
        if fine != 0 && bestScore < zero * 1.02 { return 0 }
        return fine
    }

    /// Clockwise quarter turn (0, 90, 180, 270) that makes the text read upright. Vision's accurate recogniser
    /// reads rotated text too and returns each line's quad in reading direction: the direction of the baselines
    /// (top-left → top-right corner) tells how the page lies. Sideways pages it can't read are tried turned.
    static func uprightRotation(_ image: CGImage) -> Int {
        guard let small = Bitmap(image, maxSide: 1300, gray: true).cgImage() else { return 0 }
        func votes(_ o: CGImagePropertyOrientation) -> (turns: [Int: Double], letters: Double) {
            let request = VNRecognizeTextRequest()
            request.recognitionLevel = .accurate
            request.usesLanguageCorrection = false
            request.recognitionLanguages = OCR.languages
            try? VNImageRequestHandler(cgImage: small, orientation: o, options: [:]).perform([request])
            var turns: [Int: Double] = [:], total = 0.0
            let w = Double(small.width), h = Double(small.height)
            for obs in request.results ?? [] {
                guard let c = obs.topCandidates(1).first, c.confidence > 0.4 else { continue }
                let letters = Double(c.string.filter(\.isLetter).count)
                guard letters >= 3 else { continue }
                // Baseline direction in pixels, y up.
                let dx = Double(obs.topRight.x - obs.topLeft.x) * w, dy = Double(obs.topRight.y - obs.topLeft.y) * h
                let deg = atan2(dy, dx) * 180 / .pi
                // Text running right = upright; left = upside down; up = page turned left (needs 90° clockwise); down = 270°.
                let turn: Int
                switch deg {
                case -45..<45: turn = 0
                case 45..<135: turn = 90
                case -135 ..< -45: turn = 270
                default: turn = 180
                }
                turns[turn, default: 0] += letters * Double(c.confidence)
                total += letters
            }
            return (turns, total)
        }
        let up = votes(.up)
        if up.letters >= 25, let best = up.turns.max(by: { $0.value < $1.value }) {
            let rest = up.turns.values.reduce(0, +) - best.value
            return best.value > rest * 2 ? best.key : 0
        }
        // Little readable text as it lies: try the page turned a quarter either way.
        var bestTurn = 0, bestScore = up.letters
        for (o, turn) in [(CGImagePropertyOrientation.right, 90), (.left, 270)] {
            let v = votes(o)
            if v.letters > max(25, bestScore * 1.5) { bestScore = v.letters; bestTurn = turn }
        }
        return bestTurn
    }

    // MARK: - Blank pages

    /// Fraction of the page covered by ink: dark marks against the local paper colour, ignoring single specks
    /// of noise and the outer 5 % (feeder shadows, punched holes).
    static func inkCoverage(_ image: CGImage) -> Double {
        let g = Bitmap(image, maxSide: 1000, gray: true)
        let bg = g.maxFilter(radius: 3).boxBlur(radius: 10)
        let w = g.width, h = g.height
        guard w >= 3, h >= 3 else { return 0 }
        var dark = [Bool](repeating: false, count: w * h)
        g.data.withUnsafeBufferPointer { s in
            bg.data.withUnsafeBufferPointer { b in
                for i in 0..<(w * h) {
                    let v = Int(s[i]), m = Int(b[i])
                    dark[i] = v < m - 45 && v * 10 < m * 7
                }
            }
        }
        let mx = w / 20, my = h / 20
        var count = 0
        for y in max(1, my)..<min(h - 1, h - my) {
            for x in max(1, mx)..<min(w - 1, w - mx) where dark[y * w + x] {
                var n = 0
                for dy in -1...1 { for dx in -1...1 where dark[(y + dy) * w + x + dx] { n += 1 } }
                if n >= 4 { count += 1 }
            }
        }
        let area = max(1, (w - 2 * mx) * (h - 2 * my))
        return Double(count) / Double(area)
    }

    /// Standard deviation of the smoothed brightness inside the page (sensor noise averaged out).
    static func toneSpread(_ image: CGImage) -> Double {
        let g = Bitmap(image, maxSide: 300, gray: true).boxBlur(radius: 2)
        let w = g.width, h = g.height, mx = w / 20, my = h / 20
        var sum = 0.0, sq = 0.0, n = 0.0
        for y in my..<(h - my) {
            for x in mx..<(w - mx) {
                let v = Double(g.data[y * w + x])
                sum += v; sq += v * v; n += 1
            }
        }
        guard n > 0 else { return 0 }
        let mean = sum / n
        return sqrt(max(0, sq / n - mean * mean))
    }

    // MARK: - Clean-up

    /// Paper becomes white (shadows, yellowing and uneven light divided out), text dark and sharp;
    /// gray and B&W from the cleaned page (adaptive: the threshold works against the local background).
    static func enhanceDocument(_ image: CGImage, _ o: ProcessOptions) -> CGImage? {
        guard o.isValid, Bitmap.validSize(width: image.width, height: image.height, channels: 4) else { return nil }
        let gray = o.colorMode != .color
        var src = Bitmap(image, gray: gray)
        guard src.isValid else { return nil }
        let w = src.width, h = src.height, c = src.channels
        // Background (paper) estimate at low resolution: dilate away the text, smooth.
        let scale = max(1, max(w, h) / 320)
        let small = src.resized(width: max(1, w / scale), height: max(1, h / scale))
        let paper = small.maxFilter(radius: 3).boxBlur(radius: 4).resized(width: w, height: h)
        // Levels after dividing by the paper: background → 255, ink stretched darker.
        let lo = 38.0, hi = 232.0
        let b = o.brightness * 60, k = 1 + o.contrast * 0.8
        var lut = [UInt8](repeating: 0, count: 256)
        for i in 0..<256 {
            var v = (Double(i) - lo) / (hi - lo)
            v = pow(max(0, min(1, v)), 1.25) * 255
            v = (v - 128) * k + 128 + b
            lut[i] = UInt8(max(0, min(255, v.rounded())))
        }
        // B&W thresholds the divided page itself (levels would thin the strokes); brightness moves the threshold.
        let bw = o.colorMode == .bw
        src.data.withUnsafeMutableBufferPointer { s in
            paper.data.withUnsafeBufferPointer { p in
                for i in 0..<(w * h * c) {
                    if c == 4 && i & 3 == 3 { continue }
                    let bgv = max(40, Int(p[i]))
                    let v = min(255, Int(s[i]) * 255 / bgv)
                    s[i] = bw ? UInt8(v) : lut[v]
                }
            }
        }
        if bw {
            var t = abs(o.threshold - 0.5) < 0.01 ? max(120, min(215, Int(src.otsu()) + 45)) : Int(max(20, min(235, o.threshold * 255)))
            t = max(10, min(245, t - Int(o.brightness * 60)))
            cleanBorders(&src, below: UInt8(t))
            return src.oneBitImage(threshold: UInt8(t))
        }
        cleanBorders(&src, below: 200)
        guard let cleaned = src.cgImage() else { return nil }
        // Crisper text.
        let f = CIFilter.unsharpMask()
        f.inputImage = CIImage(cgImage: cleaned)
        f.radius = Float(max(0.8, o.dpi / 300 * 1.4))
        f.intensity = 0.55
        guard let sharp = f.outputImage?.cropped(to: CGRect(x: 0, y: 0, width: w, height: h)) else { return cleaned }
        guard let rgb = render(sharp) else { return cleaned }
        return gray ? Bitmap(rgb, gray: true).cgImage() : rgb
    }

    /// Border clean-up: dark marks that touch the page edge (glass, feeder shadow, the corners left by
    /// straightening) become paper, within a band of 4 % of the page; a thin outer margin is always whitened.
    static func cleanBorders(_ bmp: inout Bitmap, below t: UInt8) {
        guard bmp.isValid else { return }
        let w = bmp.width, h = bmp.height, c = bmp.channels
        let band = max(4, min(w, h) / 25), margin = max(2, min(w, h) / 300)
        func lum(_ i: Int, _ d: UnsafeMutableBufferPointer<UInt8>) -> UInt8 {
            c == 1 ? d[i] : UInt8((Int(d[i * 4]) * 77 + Int(d[i * 4 + 1]) * 150 + Int(d[i * 4 + 2]) * 29) >> 8)
        }
        func inBand(_ x: Int, _ y: Int) -> Bool { x < band || y < band || x >= w - band || y >= h - band }
        bmp.data.withUnsafeMutableBufferPointer { d in
            func whiten(_ i: Int) { for k in 0..<c where !(c == 4 && k == 3) { d[i * c + k] = 255 } }
            var stack: [Int] = []
            var seen = [Bool](repeating: false, count: w * h)
            for y in 0..<h {
                for x in 0..<w where x < margin || y < margin || x >= w - margin || y >= h - margin {
                    let i = y * w + x
                    whiten(i)
                    seen[i] = true
                    stack.append(i)
                }
            }
            while let i = stack.popLast() {
                let x = i % w, y = i / w
                for (nx, ny) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] where nx >= 0 && ny >= 0 && nx < w && ny < h && inBand(nx, ny) {
                    let j = ny * w + nx
                    guard !seen[j] else { continue }
                    seen[j] = true
                    if lum(j, d) < t { whiten(j); stack.append(j) }
                }
            }
            // Light streaks along the edges (paper meeting the scanner's backing) become paper too; ink stays.
            if c == 4 || t >= 200 {
                for y in 0..<h {
                    for x in 0..<w where inBand(x, y) {
                        let i = y * w + x
                        if lum(i, d) > 175 { whiten(i) }
                    }
                }
            }
        }
    }

    /// Core Image's automatic adjustments (exposure, contrast, colour balance, red eye off).
    static func enhancePhoto(_ image: CGImage) -> CGImage? {
        var ci = CIImage(cgImage: image)
        for f in ci.autoAdjustmentFilters(options: [.redEye: false]) {
            f.setValue(ci, forKey: kCIInputImageKey)
            if let out = f.outputImage { ci = out }
        }
        return render(ci.cropped(to: CGRect(x: 0, y: 0, width: image.width, height: image.height)))
    }

    /// Colour mode and brightness/contrast without the document clean-up.
    static func applyColor(_ image: CGImage, _ o: ProcessOptions) -> CGImage? {
        guard o.isValid, Bitmap.validSize(width: image.width, height: image.height, channels: 4) else { return nil }
        var img = image
        if o.brightness != 0 || o.contrast != 0 {
            let f = CIFilter.colorControls()
            f.inputImage = CIImage(cgImage: img)
            f.brightness = Float(o.brightness * 0.25)
            f.contrast = Float(1 + o.contrast * 0.6)
            f.saturation = 1
            guard let out = f.outputImage, let r = render(out) else { return nil }
            img = r
        }
        switch o.colorMode {
        case .color: return img
        case .gray: return Bitmap(img, gray: true).cgImage()
        case .bw:
            let g = Bitmap(img, gray: true)
            // Otsu returns the highest value in the dark class; oneBitImage treats equality as white.
            let t: UInt8 = abs(o.threshold - 0.5) < 0.01 ? UInt8(min(255, Int(g.otsu()) + 1)) : UInt8(max(20, min(235, o.threshold * 255)))
            return g.oneBitImage(threshold: t)
        }
    }

    /// Softens the dot pattern of printed pictures (magazines, newspapers), then restores edges.
    static func descreen(_ image: CGImage, dpi: Double) -> CGImage? {
        guard dpi.isFinite, dpi > 0, dpi <= 9_600 else { return nil }
        let ci = CIImage(cgImage: image)
        let blur = CIFilter.gaussianBlur()
        blur.inputImage = ci.clampedToExtent()
        blur.radius = Float(max(0.8, dpi / 300 * 1.3))
        let sharp = CIFilter.unsharpMask()
        sharp.inputImage = blur.outputImage
        sharp.radius = Float(max(1.5, dpi / 300 * 2.5))
        sharp.intensity = 0.5
        guard let out = sharp.outputImage?.cropped(to: ci.extent) else { return nil }
        return render(out)
    }
}

/// Foreground/background split for things lying on a scanner glass: the glass is the colour around the
/// border; anything brighter or clearly coloured is an original. Shadows (darker, grey) count as glass.
struct GlassSegmentation {
    let width: Int, height: Int
    let scale: CGFloat
    private let labels: [Int32]
    private let counts: [Int]

    init?(_ image: CGImage) {
        let bmp = Bitmap(image, maxSide: 700, gray: false)
        width = bmp.width
        height = bmp.height
        scale = CGFloat(image.width) / CGFloat(bmp.width)
        let w = width, h = height
        // Glass colour: median of a thin border ring.
        var rs: [UInt8] = [], gs: [UInt8] = [], bs: [UInt8] = []
        let m = max(2, min(w, h) / 80)
        for y in 0..<h {
            for x in 0..<w where x < m || y < m || x >= w - m || y >= h - m {
                let i = (y * w + x) * 4
                rs.append(bmp.data[i]); gs.append(bmp.data[i + 1]); bs.append(bmp.data[i + 2])
            }
        }
        func median(_ a: [UInt8]) -> Int { Int(a.sorted()[a.count / 2]) }
        let br = median(rs), bgc = median(gs), bb = median(bs)
        let bl = (br * 77 + bgc * 150 + bb * 29) >> 8
        // Border spread: a busy border means no uniform glass (the page fills the frame).
        var dev = 0
        for i in 0..<rs.count { dev += abs(Int(rs[i]) - br) + abs(Int(gs[i]) - bgc) + abs(Int(bs[i]) - bb) }
        let spread = Double(dev) / Double(max(1, rs.count) * 3)
        guard spread < 18, bl < 236 else { return nil }

        var mask = [Bool](repeating: false, count: w * h)
        for i in 0..<(w * h) {
            let r = Int(bmp.data[i * 4]), g = Int(bmp.data[i * 4 + 1]), b = Int(bmp.data[i * 4 + 2])
            let l = (r * 77 + g * 150 + b * 29) >> 8
            let chroma = abs((r - br) - (g - bgc)) + abs((g - bgc) - (b - bb))
            mask[i] = l > bl + 26 || chroma > 46 || l < bl - 70
        }
        // Close small gaps (text, dark photo parts), then fill holes by labelling the background from the border.
        mask = GlassSegmentation.dilate(mask, w, h, 3)
        mask = GlassSegmentation.erode(mask, w, h, 3)
        var labels = [Int32](repeating: 0, count: w * h)
        var counts = [0]
        var stack: [Int] = []
        var next: Int32 = 1
        for start in 0..<(w * h) where mask[start] && labels[start] == 0 {
            var n = 0
            stack.append(start)
            labels[start] = next
            while let p = stack.popLast() {
                n += 1
                let x = p % w, y = p / w
                for q in [x > 0 ? p - 1 : -1, x < w - 1 ? p + 1 : -1, y > 0 ? p - w : -1, y < h - 1 ? p + w : -1] where q >= 0 {
                    if mask[q] && labels[q] == 0 { labels[q] = next; stack.append(q) }
                }
            }
            counts.append(n)
            next += 1
        }
        self.labels = labels
        self.counts = counts
    }

    static func dilate(_ m: [Bool], _ w: Int, _ h: Int, _ r: Int) -> [Bool] {
        var tmp = m, out = m
        for y in 0..<h { for x in 0..<w where !m[y * w + x] {
            for xx in max(0, x - r)...min(w - 1, x + r) where m[y * w + xx] { tmp[y * w + x] = true; break }
        } }
        for y in 0..<h { for x in 0..<w where !tmp[y * w + x] {
            for yy in max(0, y - r)...min(h - 1, y + r) where tmp[yy * w + x] { out[y * w + x] = true; break }
        } }
        return out
    }

    static func erode(_ m: [Bool], _ w: Int, _ h: Int, _ r: Int) -> [Bool] {
        dilate(m.map { !$0 }, w, h, r).map { !$0 }
    }

    /// Outlines (min-area rectangles of the convex hull) of every component larger than `minFraction` of the image.
    func components(minFraction: Double) -> [Quad] {
        let total = Double(width * height)
        var quads: [Quad] = []
        for label in 1..<counts.count where Double(counts[label]) / total >= minFraction {
            if let q = outline(Int32(label)) { quads.append(q.scaled(scale, scale)) }
        }
        return quads
    }

    func largest(minFraction: Double) -> Quad? {
        guard counts.count > 1 else { return nil }
        let label = (1..<counts.count).max { counts[$0] < counts[$1] }!
        guard Double(counts[label]) / Double(width * height) >= minFraction else { return nil }
        return outline(Int32(label))?.scaled(scale, scale)
    }

    private func outline(_ label: Int32) -> Quad? {
        // Leftmost and rightmost pixel of each row are enough for the convex hull.
        var pts: [CGPoint] = []
        for y in 0..<height {
            var first = -1, last = -1
            for x in 0..<width where labels[y * width + x] == label {
                if first < 0 { first = x }
                last = x
            }
            if first >= 0 {
                pts.append(CGPoint(x: first, y: y)); pts.append(CGPoint(x: last + 1, y: y))
                pts.append(CGPoint(x: first, y: y + 1)); pts.append(CGPoint(x: last + 1, y: y + 1))
            }
        }
        return Hull.minAreaRect(Hull.convex(pts))
    }
}
