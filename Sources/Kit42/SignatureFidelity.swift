import AppKit

/// How close copies learned from a person's samples are to that person's real signatures — measured, not judged by
/// eye, so every new sample set is a regression test for the whole learning pipeline (sheet splitting, tracing,
/// stroke joining, variation, rendering).
///
/// `PDF42 --signature-fidelity INPUT… OUT [--check]`: INPUT is a photo/scan of a sheet with signatures, several
/// pictures or a folder. Every third signature is held out; the hand is learned from the rest and as many copies are
/// generated as there are real signatures. Real and generated pictures then go through the same tracing and are
/// compared on:
/// - shape: chamfer distance between centre lines (height 1). `shape ratio` = distance copies→held-out ÷ distance
///   training→held-out: 1 means copies are as close to unseen real signatures as other real ones are;
/// - variation: mean distance between copies ÷ between real signatures (≈1; below 0.6 copies are clones, above 1.6
///   they wander off);
/// - proportions, slant, line width, path length and stroke count: difference of means in units of the real spread.
/// `--check` exits 1 outside the thresholds. Also writes `split/` (the cut signatures), `copies/`, a blind-test
/// `sheet.pdf` with `key.json`, and `report.md`.
public enum SignatureFidelity {
    struct Shape {
        var points: [CGPoint]          // 256 points along the centre lines, height 1, centred
        var aspect: Double, slant: Double, width: Double, length: Double, strokes: Double, cusps: Double, wobble: Double
        var inkHeight: Double = 0      // centre-line height in the traced picture's pixels
    }

    static func shape(_ image: CGImage) -> Shape? {
        guard let sample = SignatureLearning.trace(image) else { return nil }
        let strokes = sample.strokes.map { $0.map { CGPoint(x: $0[0], y: $0[1]) } }.filter { $0.count > 1 }
        let all = strokes.flatMap { $0 }
        guard let minX = all.map(\.x).min(), let maxX = all.map(\.x).max(), let minY = all.map(\.y).min(), let maxY = all.map(\.y).max(),
              maxY - minY > 1 else { return nil }
        let h = maxY - minY
        var segments: [(CGPoint, CGPoint)] = []
        for s in strokes { for i in 1..<s.count { segments.append((s[i - 1], s[i])) } }
        let total = segments.reduce(0) { $0 + hypot($1.1.x - $1.0.x, $1.1.y - $1.0.y) }
        // Slant: horizontal drift of the near-vertical parts of the line per unit of height.
        var shear: CGFloat = 0, rise: CGFloat = 0
        for (a, b) in segments where abs(b.y - a.y) > abs(b.x - a.x) * 1.5 {
            let dy = b.y - a.y
            shear += (b.x - a.x) * (dy > 0 ? 1 : -1); rise += abs(dy)
        }
        // Cusps: the line reverses (turn over 120°) within a short stretch (about 6 % of the height).
        var cusps = 0
        for s in strokes {
            var along: [CGPoint] = [s[0]]
            for p in s.dropFirst() where hypot(p.x - along.last!.x, p.y - along.last!.y) >= h * 0.03 { along.append(p) }
            var i = 1
            while i < along.count - 1 {
                let a = atan2(along[i].y - along[i - 1].y, along[i].x - along[i - 1].x)
                let b = atan2(along[i + 1].y - along[i].y, along[i + 1].x - along[i].x)
                var d = abs(b - a); if d > .pi { d = 2 * .pi - d }
                if d > 2.1 { cusps += 1; i += 2 } else { i += 1 }
            }
        }
        // Wobble: how far the centre line strays from itself smoothed over 8 % of the height (shaky or wavy lines).
        var stray: CGFloat = 0, counted: CGFloat = 0
        let k = max(2, Int(h * 0.04))
        for s in strokes where s.count > 2 * k + 1 {
            for i in k..<(s.count - k) {
                let r = (i - k)...(i + k)
                let mx = s[r].reduce(0) { $0 + $1.x } / CGFloat(r.count), my = s[r].reduce(0) { $0 + $1.y } / CGFloat(r.count)
                stray += hypot(s[i].x - mx, s[i].y - my); counted += 1
            }
        }
        // Even points along all strokes.
        var points: [CGPoint] = []
        let step = total / 256
        var carry: CGFloat = 0
        for (a, b) in segments {
            let len = hypot(b.x - a.x, b.y - a.y)
            var t = step - carry
            while t <= len { points.append(CGPoint(x: a.x + (b.x - a.x) * t / len, y: a.y + (b.y - a.y) * t / len)); t += step }
            carry = len - (t - step)
        }
        guard !points.isEmpty else { return nil }
        let cx = points.reduce(0) { $0 + $1.x } / CGFloat(points.count), cy = points.reduce(0) { $0 + $1.y } / CGFloat(points.count)
        // Compared within the signature's own box (proportions are a separate feature), so a longer run-out does not
        // shift every letter.
        let wd = max(1, maxX - minX)
        return Shape(points: points.map { CGPoint(x: ($0.x - cx) / wd, y: ($0.y - cy) / h) },
                     aspect: Double((maxX - minX) / h), slant: Double(rise > 0 ? shear / rise : 0),
                     width: sample.penWidth ?? 0, length: Double(total / h), strokes: Double(strokes.count), cusps: Double(cusps),
                     wobble: Double(counted > 0 ? stray / counted / h : 0), inkHeight: Double(h))
    }

    /// Symmetric mean nearest-point distance.
    static func distance(_ a: Shape, _ b: Shape) -> Double {
        func one(_ p: [CGPoint], _ q: [CGPoint]) -> Double {
            p.reduce(0) { sum, x in sum + Double(q.map { hypot($0.x - x.x, $0.y - x.y) }.min() ?? 0) } / Double(max(1, p.count))
        }
        return (one(a.points, b.points) + one(b.points, a.points)) / 2
    }

    public struct Result {
        public var report: String
        public var failures: [String]
    }

    public static func run(inputs: [URL], to out: URL) throws -> Result {
        var pictures: [URL] = []
        for url in inputs {
            var isDir: ObjCBool = false
            if FileManager.default.fileExists(atPath: url.path, isDirectory: &isDir), isDir.boolValue {
                pictures += try FileManager.default.contentsOfDirectory(at: url, includingPropertiesForKeys: nil)
                    .filter { ["png", "jpg", "jpeg", "heic", "tif", "tiff", "pdf"].contains($0.pathExtension.lowercased()) }
                    .sorted { $0.lastPathComponent < $1.lastPathComponent }
            } else { pictures.append(url) }
        }
        var real: [CGImage] = []
        for url in pictures {
            guard let image = StampLibrary.image(at: url) ?? StampLibrary.rasterize(url) else { continue }
            real += SignatureSheet.split(image).map(\.image)
        }
        try FileManager.default.createDirectory(at: out.appendingPathComponent("split"), withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: out.appendingPathComponent("copies"), withIntermediateDirectories: true)
        for (i, image) in real.enumerated() { try png(image, out.appendingPathComponent(String(format: "split/real-%02d.png", i + 1))) }
        guard real.count >= 4 else {
            return Result(report: "Signature fidelity: \(real.count) signatures found — at least 4 are needed", failures: ["too few signatures"])
        }
        // Hold out every third signature.
        let testIdx = real.indices.filter { $0 % 3 == 2 }, trainIdx = real.indices.filter { $0 % 3 != 2 }
        let train = trainIdx.compactMap { SignatureSample.from(picture: real[$0]) }
        let learned = LearnedSignature(samples: train)
        var copies: [CGImage] = []
        var seed: UInt64 = 0xF1DE
        for i in 0..<real.count {
            seed = seed &* 6364136223846793005 &+ 1442695040888963407
            if let image = learned.render(impression: seed, ink: InkColor.blue.pen, pen: .ballpoint) {
                copies.append(image)
                try png(image, out.appendingPathComponent(String(format: "copies/copy-%02d.png", i + 1)))
            }
        }
        // Copies go through the same "camera" as the real ones: scaled to the real pictures' typical size, so tracing
        // sees the same resolution (a sharp 120-px rendering would trace smoother than a phone photo of ink).
        let realShapes = real.map(shape)
        let scannedCopies = scanned(copies, like: real)
        let copyShapes = scannedCopies.compactMap(shape)
        let test = testIdx.compactMap { realShapes[$0] }, trainShapes = trainIdx.compactMap { realShapes[$0] }
        let allReal = realShapes.compactMap { $0 }
        func mean(_ v: [Double]) -> Double { v.isEmpty ? 0 : v.reduce(0, +) / Double(v.count) }
        func std(_ v: [Double]) -> Double { let m = mean(v); return sqrt(mean(v.map { ($0 - m) * ($0 - m) })) }
        func cross(_ a: [Shape], _ b: [Shape]) -> Double { mean(a.flatMap { x in b.map { distance(x, $0) } }) }
        func within(_ a: [Shape]) -> Double {
            var d: [Double] = []
            for i in a.indices { for j in a.indices where j > i { d.append(distance(a[i], a[j])) } }
            return mean(d)
        }
        let realToTest = cross(trainShapes, test), copyToTest = cross(copyShapes, test)
        let shapeRatio = copyToTest / max(1e-9, realToTest)
        let variation = within(copyShapes) / max(1e-9, within(allReal))
        // Outliers: copies farther from every learning signature than any held-out real one is.
        func nearest(_ s: Shape) -> Double { trainShapes.map { distance(s, $0) }.min() ?? 0 }
        let realNearest = test.map(nearest), limit = (realNearest.max() ?? 0) * 1.1
        let outliers = copyShapes.filter { nearest($0) > limit }.count
        let outlierShare = Double(outliers) / Double(max(1, copyShapes.count))
        var failures: [String] = []
        if outlierShare > 0.15 { failures.append(String(format: "%d of %d copies unlike any real signature", outliers, copyShapes.count)) }
        if shapeRatio > 1.35 { failures.append(String(format: "shape ratio %.2f > 1.35", shapeRatio)) }
        if variation < 0.6 || variation > 1.6 { failures.append(String(format: "variation %.2f outside 0.6…1.6", variation)) }
        var rows = ""
        let features: [(String, (Shape) -> Double)] = [("proportions (w/h)", \.aspect), ("slant", \.slant), ("line width / height", \.width),
                                                       ("path length / height", \.length), ("strokes", \.strokes),
                                                       ("sharp turns", \.cusps), ("wobble / height", \.wobble)]
        for (name, f) in features {
            let r = allReal.map(f), c = copyShapes.map(f)
            let spread = max(std(r), abs(mean(r)) * 0.08, 1e-6)
            let z = (mean(c) - mean(r)) / spread
            rows += String(format: "| %@ | %.3f ± %.3f | %.3f ± %.3f | %+.2f |\n", name, mean(r), std(r), mean(c), std(c), z)
            // Fine wobble of a photographed line is mostly the camera (JPEG, paper grain, pixel steps): reported only.
            if abs(z) > 1.5, name != "wobble / height" { failures.append(String(format: "%@ differs by %.1f real spreads", name, z)) }
        }
        // Blind-test sheet of all real and the copies, with the key.
        try SignatureBlindTest.write(real: real, generated: scannedCopies, to: out)
        var report = "# Signature fidelity\n\n"
        report += "Inputs: \(pictures.map(\.lastPathComponent).joined(separator: ", ")) → \(real.count) signatures "
        report += "(\(trainIdx.count) for learning, \(testIdx.count) held out), \(copies.count) copies.\n\n"
        report += String(format: "- Shape: copies → held-out %.4f, real → held-out %.4f, **ratio %.2f** (1 = as close as real ones)\n", copyToTest, realToTest, shapeRatio)
        report += String(format: "- Variation: between copies %.4f, between real %.4f, **ratio %.2f** (≈1; < 0.6 clones)\n", within(copyShapes), within(allReal), variation)
        report += String(format: "- Outliers: **%d of %d** copies farther from every learning signature than the farthest held-out real one (limit %.4f)\n\n", outliers, copyShapes.count, limit)
        report += "| Feature | Real | Copies | Difference (real spreads) |\n|---|---:|---:|---:|\n" + rows
        report += "\nWobble is reported only: on photos it is dominated by the camera (compression, paper grain, pixel steps).\n"
        report += "\n" + (failures.isEmpty ? "Checks: passed\n" : "Checks failed: " + failures.joined(separator: "; ") + "\n")
        try report.write(to: out.appendingPathComponent("report.md"), atomically: true, encoding: .utf8)
        return Result(report: report, failures: failures)
    }

    /// Copies as if scanned or photographed like the real pictures: scaled so their ink is as tall as the real
    /// signatures' ink (real crops carry margins, copies are trimmed close), on white paper, with the softness of a
    /// lens (Gaussian blur of about 0.7 px at that size), sensor noise and JPEG. Used for measuring and for the blind-test sheet, so neither
    /// compares a sharp rendering with a photo.
    public static func scanned(_ copies: [CGImage], like real: [CGImage]) -> [CGImage] {
        let inkHeights = real.compactMap { shape($0)?.inkHeight }.sorted()
        guard !inkHeights.isEmpty else { return copies }
        let typicalInk = inkHeights[inkHeights.count / 2]
        var rng = SeededRandom(0x5CA4)
        return copies.map { copy in
            guard let own = shape(copy), own.inkHeight > 0 else { return copy }
            let small = resized(copy, height: Int((Double(copy.height) * typicalInk / own.inkHeight).rounded()))
            let soft = blurred(small, radius: 0.7) ?? small
            return sensor(soft, rng: &rng) ?? soft
        }
    }

    /// Sensor noise (±4 levels) and JPEG at quality 0.8, as a phone or a scanner's default settings give.
    static func sensor(_ image: CGImage, rng: inout SeededRandom) -> CGImage? {
        let w = image.width, h = image.height
        guard let ctx = InkDraw.context(w, h), let data = ctx.data?.assumingMemoryBound(to: UInt8.self) else { return nil }
        ctx.setFillColor(.white); ctx.fill(CGRect(x: 0, y: 0, width: w, height: h))
        ctx.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
        for i in 0..<(w * h) {
            let n = Int(rng.range(-4...4))
            for c in 0..<3 { data[i * 4 + c] = UInt8(max(0, min(255, Int(data[i * 4 + c]) + n))) }
        }
        guard let noisy = ctx.makeImage(), let jpeg = NSBitmapImageRep(cgImage: noisy).representation(using: .jpeg, properties: [.compressionFactor: 0.8]),
              let rep = NSBitmapImageRep(data: jpeg) else { return nil }
        return rep.cgImage
    }

    static func blurred(_ image: CGImage, radius: Double) -> CGImage? {
        let input = CIImage(cgImage: image)
        guard let filter = CIFilter(name: "CIGaussianBlur", parameters: [kCIInputImageKey: input.clampedToExtent(), kCIInputRadiusKey: radius]),
              let output = filter.outputImage?.cropped(to: input.extent) else { return nil }
        return CIContext(options: [.useSoftwareRenderer: false]).createCGImage(output, from: input.extent)
    }

    /// Downscaled copy on paper-white (tracing treats transparency as paper anyway), with a light blur like a lens.
    static func resized(_ image: CGImage, height: Int) -> CGImage {
        guard image.height > height, height > 8 else { return image }
        let w = max(8, image.width * height / image.height)
        guard let ctx = InkDraw.context(w, height) else { return image }
        ctx.setFillColor(.white); ctx.fill(CGRect(x: 0, y: 0, width: w, height: height))
        ctx.interpolationQuality = .high
        ctx.draw(image, in: CGRect(x: 0, y: 0, width: w, height: height))
        return ctx.makeImage() ?? image
    }

    /// A synthetic sample sheet for regression without anyone's real signature: 15 copies of one generated
    /// signature (blue and black ink) on grey, unevenly lit, noisy paper, plus traps — two signatures written into
    /// each other and a fragment cut by the edge — that splitting must reject. Returns the picture and how many
    /// signatures splitting should find.
    public static func syntheticSheet() -> (image: CGImage, expected: Int)? {
        let w = 1400, h = 1900
        guard let ctx = InkDraw.context(w, h) else { return nil }
        // Paper: grey with a lighting gradient (a phone photo indoors).
        let space = CGColorSpace(name: CGColorSpace.sRGB)!
        let gradient = CGGradient(colorsSpace: space, colors: [CGColor(gray: 0.86, alpha: 1), CGColor(gray: 0.7, alpha: 1)] as CFArray, locations: [0, 1])!
        ctx.drawLinearGradient(gradient, start: CGPoint(x: 0, y: h), end: CGPoint(x: w, y: 0), options: [])
        let design = SignatureGenerator.design(form: .initialSurname, variant: 3, seed: 4242)
        var rng = SeededRandom(77)
        func put(_ seed: UInt64, at p: CGPoint, ink: NSColor, width: CGFloat = 300) {
            guard let sig = SignatureGenerator.render("Sergey Zorin", design: design, impression: seed, ink: ink, pen: .ballpoint) else { return }
            let hgt = width * CGFloat(sig.height) / CGFloat(sig.width)
            ctx.saveGState(); ctx.setBlendMode(.multiply)
            ctx.draw(sig, in: CGRect(x: p.x, y: p.y, width: width, height: hgt)); ctx.restoreGState()
        }
        var expected = 0
        for row in 0..<5 { for col in 0..<3 {
            let x = 90 + CGFloat(col) * 430 + rng.range(-30...30), y = CGFloat(h) - 300 - CGFloat(row) * 330 + rng.range(-30...30)
            put(UInt64(row * 3 + col + 1) &* 7919, at: CGPoint(x: x, y: y), ink: (row + col) % 3 == 0 ? InkColor.black.pen : InkColor.blue.pen)
            expected += 1
        } }
        // Trap: two signatures written into each other.
        put(901, at: CGPoint(x: 300, y: 120), ink: InkColor.blue.pen); put(902, at: CGPoint(x: 360, y: 150), ink: InkColor.black.pen)
        // Trap: a fragment cut by the right edge.
        put(903, at: CGPoint(x: CGFloat(w) - 120, y: 140), ink: InkColor.blue.pen)
        // A lens: the same softness copies get in `scanned`, at the sheet's scale (a signature here is ~3× larger).
        guard let sharp = ctx.makeImage(), let clean = blurred(sharp, radius: 1.8) else { return nil }
        // Sensor noise and JPEG, as from a phone.
        guard let noisy = InkDraw.context(w, h), let data = noisy.data?.assumingMemoryBound(to: UInt8.self) else { return nil }
        noisy.draw(clean, in: CGRect(x: 0, y: 0, width: w, height: h))
        for i in 0..<(w * h) {
            let n = Int(rng.range(-6...6))
            for c in 0..<3 { data[i * 4 + c] = UInt8(max(0, min(255, Int(data[i * 4 + c]) + n))) }
        }
        guard let image = noisy.makeImage(), let jpeg = NSBitmapImageRep(cgImage: image).representation(using: .jpeg, properties: [.compressionFactor: 0.8]),
              let rep = NSBitmapImageRep(data: jpeg), let final = rep.cgImage else { return nil }
        return (final, expected)
    }

    /// `--signature-fidelity-synthetic OUT`: the synthetic sheet through the whole pipeline; fails when splitting
    /// does not find exactly the expected signatures or the fidelity checks fail.
    public static func runSynthetic(to out: URL) throws -> Result {
        guard let (sheet, expected) = syntheticSheet() else { return Result(report: "no synthetic sheet", failures: ["sheet"]) }
        try FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
        let url = out.appendingPathComponent("synthetic-sheet.png")
        try png(sheet, url)
        let found = SignatureSheet.split(sheet).count
        var result = try run(inputs: [url], to: out)
        if found != expected { result.failures.append("split found \(found) signatures, expected \(expected)") }
        result.report += "\nSynthetic sheet: split found \(found) of \(expected) signatures (traps: a pair written together, a fragment at the edge).\n"
        result.report += result.failures.isEmpty ? "Synthetic checks: passed\n" : "Synthetic checks failed: " + result.failures.joined(separator: "; ") + "\n"
        return result
    }

    private static func png(_ image: CGImage, _ url: URL) throws {
        guard let data = NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:]) else { return }
        try data.write(to: url)
    }
}
