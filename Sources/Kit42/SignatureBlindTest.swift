import AppKit

/// Blind test: can people tell real signatures from generated ones?
///
/// `PDF42 --signature-blindtest REAL_DIR OUT_DIR [copies]` reads scans or photos of real signatures (PNG/JPEG/HEIC/TIFF,
/// one signature per file, e.g. several of your own) and, for each, learns the hand from that picture and writes
/// `copies` generated signatures of it. Real and generated ones are shown the same way — the same ink, height and
/// grey paper — and shuffled into numbered cells of `sheet.pdf`; `key.json` holds which is which. Testers write the
/// numbers they think are generated, one per line or comma-separated, into a text file;
/// `PDF42 --signature-blindtest-score OUT_DIR/key.json answers.txt` prints accuracy, hit and false-alarm rates and d′.
/// Interpret accuracy against the generated/real balance; d′ near 0 indicates low measured separation in this test.
/// Use only signatures you may use: your own, or with the owner's consent.
public enum SignatureBlindTest {
    struct Key: Codable {
        var cells: [Bool]       // true = generated
        var sources: [String]   // real file each cell came from (or was learned from)
    }

    /// `pooled`: one hand learned from all pictures together (as "My Signature" does with several samples) instead of
    /// one hand per picture.
    public static func make(real folder: URL, to out: URL, copies: Int = 2, pooled: Bool = false) throws -> String {
        let files = (try FileManager.default.contentsOfDirectory(at: folder, includingPropertiesForKeys: nil))
            .filter { ["png", "jpg", "jpeg", "heic", "tif", "tiff"].contains($0.pathExtension.lowercased()) }
            .sorted { $0.lastPathComponent < $1.lastPathComponent }
        guard !files.isEmpty else { throw NSError(domain: "SignatureBlindTest", code: 1, userInfo: [NSLocalizedDescriptionKey: "no images in \(folder.path)"]) }
        var cells: [(image: CGImage, generated: Bool, source: String)] = []
        var seed: UInt64 = 0x5EED
        var all: [SignatureSample] = []
        for file in files {
            guard let picture = StampLibrary.rasterize(file) else { continue }
            cells.append((picture, false, file.lastPathComponent))
            guard let traced = SignatureSample.from(picture: picture) else { continue }
            all.append(traced)
            if pooled { continue }
            let learned = LearnedSignature(samples: [traced])
            for _ in 0..<max(1, copies) {
                seed = seed &* 6364136223846793005 &+ 1442695040888963407
                if let image = learned.render(impression: seed, ink: .black, pen: .ballpoint) { cells.append((image, true, file.lastPathComponent)) }
            }
        }
        // Generated pictures go through the same "camera" as the real ones (size and lens softness).
        let realPictures = cells.filter { !$0.generated }.map(\.image)
        if !pooled {
            let generated = cells.indices.filter { cells[$0].generated }
            let scanned = SignatureFidelity.scanned(generated.map { cells[$0].image }, like: realPictures)
            for (k, i) in generated.enumerated() { cells[i].image = scanned[k] }
        }
        if pooled, !all.isEmpty {
            let learned = LearnedSignature(samples: all)
            for _ in 0..<(max(1, copies) * all.count) {
                seed = seed &* 6364136223846793005 &+ 1442695040888963407
                if let image = learned.render(impression: seed, ink: .black, pen: .ballpoint),
                   let scan = SignatureFidelity.scanned([image], like: realPictures).first { cells.append((scan, true, "pooled")) }
            }
        }
        guard cells.contains(where: \.generated) else { throw NSError(domain: "SignatureBlindTest", code: 2, userInfo: [NSLocalizedDescriptionKey: "no signature could be traced"]) }
        var rng = SeededRandom(0xB11D)
        for i in stride(from: cells.count - 1, to: 0, by: -1) { cells.swapAt(i, Int(rng.next() % UInt64(i + 1))) }
        try FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
        let key = Key(cells: cells.map(\.generated), sources: cells.map(\.source))
        let encoder = JSONEncoder(); encoder.outputFormatting = [.prettyPrinted]
        try encoder.encode(key).write(to: out.appendingPathComponent("key.json"))
        try sheet(cells.map(\.image), to: out.appendingPathComponent("sheet.pdf"))
        // The traced strokes of every real picture, for checking what learning saw.
        try encoder.encode(all).write(to: out.appendingPathComponent("traced.json"))
        try "# Numbers of the signatures you think are generated, e.g.: 2, 5, 11\n"
            .write(to: out.appendingPathComponent("answers.txt"), atomically: true, encoding: .utf8)
        let generated = key.cells.filter { $0 }.count
        return "Blind test: \(cells.count) cells (\(cells.count - generated) real, \(generated) generated) in \(out.path)"
    }

    /// Cells of the same size on A4; each signature drawn as grey ink at the same height, so colour, scale or scan
    /// noise does not give away which is which.
    /// Shuffled sheet and key from pictures already at hand (used by the fidelity check).
    static func write(real: [CGImage], generated: [CGImage], to out: URL) throws {
        var cells = real.map { ($0, false) } + generated.map { ($0, true) }
        var rng = SeededRandom(0xB11D)
        for i in stride(from: cells.count - 1, to: 0, by: -1) { cells.swapAt(i, Int(rng.next() % UInt64(i + 1))) }
        let encoder = JSONEncoder(); encoder.outputFormatting = [.prettyPrinted]
        try encoder.encode(Key(cells: cells.map(\.1), sources: cells.map { $0.1 ? "generated" : "real" }))
            .write(to: out.appendingPathComponent("key.json"))
        try sheet(cells.map(\.0), to: out.appendingPathComponent("sheet.pdf"))
        try "# Numbers of the signatures you think are generated, e.g.: 2, 5, 11\n"
            .write(to: out.appendingPathComponent("answers.txt"), atomically: true, encoding: .utf8)
    }

    private static func sheet(_ images: [CGImage], to url: URL) throws {
        var box = CGRect(x: 0, y: 0, width: 595, height: 842)
        guard let ctx = CGContext(url as CFURL, mediaBox: &box, nil) else { throw CocoaError(.fileWriteUnknown) }
        let columns = 3, rows = 7, cellW: CGFloat = 165, cellH: CGFloat = 100, left: CGFloat = 50, top: CGFloat = 790
        for (i, image) in images.enumerated() {
            let slot = i % (columns * rows)
            if slot == 0 { if i > 0 { ctx.endPDFPage() }; ctx.beginPDFPage(nil) }
            let cell = CGRect(x: left + CGFloat(slot % columns) * (cellW + 7), y: top - CGFloat(slot / columns + 1) * (cellH + 7), width: cellW, height: cellH)
            ctx.setStrokeColor(NSColor(white: 0.8, alpha: 1).cgColor); ctx.stroke(cell)
            let label = NSAttributedString(string: "\(i + 1)", attributes: [.font: NSFont.systemFont(ofSize: 8), .foregroundColor: NSColor.gray])
            let line = CTLineCreateWithAttributedString(label)
            ctx.textPosition = CGPoint(x: cell.minX + 4, y: cell.maxY - 11); CTLineDraw(line, ctx)
            // Cropped to the ink: a scan's margins would make it look smaller and lighter than a generated copy.
            guard let full = monochrome(image) else { continue }
            let mono = InkDraw.trim(full)
            let area = cell.insetBy(dx: 12, dy: 16)
            let k = min(area.width / CGFloat(mono.width), area.height / CGFloat(mono.height))
            let size = CGSize(width: CGFloat(mono.width) * k, height: CGFloat(mono.height) * k)
            ctx.saveGState(); ctx.setBlendMode(.multiply)
            ctx.draw(mono, in: CGRect(x: area.midX - size.width / 2, y: area.midY - size.height / 2, width: size.width, height: size.height))
            ctx.restoreGState()
        }
        ctx.endPDFPage(); ctx.closePDF()
    }

    /// Dark grey ink with the original's darkness as coverage (white and light paper become transparent). Coverage is
    /// normalised so the darkest 10 % of the line is full ink: a faint scan and a dark rendering look equally dark.
    private static func monochrome(_ image: CGImage) -> CGImage? {
        let w = image.width, h = image.height
        guard let ctx = InkDraw.context(w, h), let data = ctx.data?.assumingMemoryBound(to: UInt8.self) else { return nil }
        ctx.setFillColor(.white); ctx.fill(CGRect(x: 0, y: 0, width: w, height: h))
        ctx.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
        // Darkness against the local paper, so a grey photographed sheet becomes white like a scan.
        let lum = (0..<(w * h)).map { i in 0.3 * Double(data[i * 4]) + 0.59 * Double(data[i * 4 + 1]) + 0.11 * Double(data[i * 4 + 2]) }
        let dark = SignatureLearning.darkness(lum, w, h)
        func coverage(_ i: Int) -> Double { min(1, max(0, (dark[i] - 12) / 120)) }
        let inked = (0..<(w * h)).map(coverage).filter { $0 > 0.05 }.sorted()
        let full = inked.isEmpty ? 1 : max(0.2, inked[Int(Double(inked.count - 1) * 0.9)])
        for i in 0..<(w * h) {
            let cover = min(1, coverage(i) / full)
            let a = UInt8(cover * 255), ink = UInt8(Double(a) * 0.18)
            data[i * 4] = ink; data[i * 4 + 1] = ink; data[i * 4 + 2] = ink; data[i * 4 + 3] = a
        }
        return ctx.makeImage()
    }

    /// Scores answers (numbers of cells marked as generated) against the key.
    public static func score(key url: URL, answers: URL) throws -> String {
        let key = try JSONDecoder().decode(Key.self, from: Data(contentsOf: url))
        let text = try String(contentsOf: answers, encoding: .utf8)
        let n = key.cells.count, generated = key.cells.filter { $0 }.count, real = n - generated
        let tokens = text.split(separator: "\n").map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.hasPrefix("#") }.joined(separator: ",")
            .split(whereSeparator: { ",; \t".contains($0) })
        var marked = Set<Int>()
        for token in tokens {
            guard let number = Int(token), number >= 1, number <= n else {
                throw NSError(domain: "SignatureBlindTest", code: 3,
                              userInfo: [NSLocalizedDescriptionKey: "Invalid answer '\(token)': expected a cell number from 1 to \(n)"])
            }
            marked.insert(number - 1)
        }
        let hits = key.cells.indices.filter { key.cells[$0] && marked.contains($0) }.count
        let falseAlarms = key.cells.indices.filter { !key.cells[$0] && marked.contains($0) }.count
        let correct = hits + (real - falseAlarms)
        // Rates clamped away from 0 and 1 (log-linear correction) for d′.
        let h = (Double(hits) + 0.5) / Double(generated + 1), f = (Double(falseAlarms) + 0.5) / Double(real + 1)
        let dPrime = probit(h) - probit(f)
        return String(format: "Blind test: %d of %d correct (%.0f %%); generated found %d of %d, real marked as generated %d of %d; d′ = %.2f (0 = no measured separation)",
                      correct, n, 100 * Double(correct) / Double(max(1, n)), hits, generated, falseAlarms, real, dPrime)
    }

    /// Inverse of the standard normal distribution (Acklam's approximation).
    static func probit(_ p: Double) -> Double {
        let a = [-39.69683028665376, 220.9460984245205, -275.9285104469687, 138.3577518672690, -30.66479806614716, 2.506628277459239]
        let b = [-54.47609879822406, 161.5858368580409, -155.6989798598866, 66.80131188771972, -13.28068155211841]
        let c = [-0.007784894002430293, -0.3223964580411365, -2.400758277161838, -2.549732539343734, 4.374664141464968, 2.938163982698783]
        let d = [0.007784695709041462, 0.3224671290700398, 2.445134137142996, 3.754408661907416]
        let q = min(max(p, 1e-9), 1 - 1e-9)
        if q < 0.02425 {
            let t = sqrt(-2 * log(q))
            return (((((c[0] * t + c[1]) * t + c[2]) * t + c[3]) * t + c[4]) * t + c[5]) / ((((d[0] * t + d[1]) * t + d[2]) * t + d[3]) * t + 1)
        }
        if q > 1 - 0.02425 { return -probit(1 - q) }
        let t = q - 0.5, r = t * t
        return (((((a[0] * r + a[1]) * r + a[2]) * r + a[3]) * r + a[4]) * r + a[5]) * t / (((((b[0] * r + b[1]) * r + b[2]) * r + b[3]) * r + b[4]) * r + 1)
    }
}
