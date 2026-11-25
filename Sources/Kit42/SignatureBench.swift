import AppKit
import Vision

/// Measurement bench for generated signatures (`PDF42 --signature-bench DIR`).
///
/// Metrics from handwriting research, computed on the pen's actual path and timing:
/// - **Normalized jerk** (Teulings et al.): √(½·∫|j|²dt·T⁵/L²) per stroke — lower is smoother; slow, laboured
///   writing has much higher values than fluent, automated writing.
/// - **Velocity peaks per stroke (NIV)**: local speed maxima above 10 % of the stroke's peak — fluent writing has few
///   per movement; hesitant writing breaks movements into many.
/// - **Hesitation share**: time inside a stroke with speed under 8 % of its peak (pen almost stopped).
/// - **Line wobble**: RMS deviation (px) of the line from itself smoothed over 13 px — what reads as a shaky hand,
///   independent of timing (letter curvature changes more slowly and is not counted).
/// - **Variability**: dynamic-time-warping distance between normalised shapes — between copies of one design
///   (must be above zero: copies differ) and between designs (must be clearly larger: styles are distinct).
/// - **Legibility**: whether Vision's handwriting recognition reads the written name for legible forms.
///
/// There is no real-signature reference here (research datasets need licences), so numbers compare versions of the
/// generator with each other and with the thresholds in the report; they are not a human-likeness score.
public enum SignatureBench {
    public struct Fluency: Codable {
        public var normalizedJerk: Double
        public var peaksPerStroke: Double
        public var hesitation: Double
        public var tremor: Double
    }

    // MARK: Fluency

    static func fluency(_ strokes: [[PenSample]]) -> Fluency? {
        var jerks: [Double] = [], peaks: [Double] = [], hes: [Double] = [], trem: [Double] = []
        for s in strokes where s.count >= 24 {
            let dt = max(1e-4, (s.last!.t - s.first!.t) / Double(s.count - 1))
            // Light smoothing (5 samples) so measurement noise does not count as jerk.
            let p = s.indices.map { i -> CGPoint in
                let r = max(0, i - 2)...min(s.count - 1, i + 2)
                let n = CGFloat(r.count)
                return CGPoint(x: s[r].reduce(0) { $0 + $1.p.x } / n, y: s[r].reduce(0) { $0 + $1.p.y } / n)
            }
            var speed: [Double] = [Double](repeating: 0, count: p.count)
            for i in 1..<(p.count - 1) { speed[i] = Double(hypot(p[i + 1].x - p[i - 1].x, p[i + 1].y - p[i - 1].y)) / (2 * dt) }
            speed[0] = speed[1]; speed[p.count - 1] = speed[p.count - 2]
            var length: Double = 0
            for i in 1..<p.count { length += Double(hypot(p[i].x - p[i - 1].x, p[i].y - p[i - 1].y)) }
            guard length > 5 else { continue }
            var jsum: Double = 0
            for i in 2..<(p.count - 2) {
                let jx = Double(p[i + 2].x - 2 * p[i + 1].x + 2 * p[i - 1].x - p[i - 2].x) / (2 * pow(dt, 3))
                let jy = Double(p[i + 2].y - 2 * p[i + 1].y + 2 * p[i - 1].y - p[i - 2].y) / (2 * pow(dt, 3))
                jsum += (jx * jx + jy * jy) * dt
            }
            let T = dt * Double(p.count - 1)
            jerks.append((0.5 * jsum * pow(T, 5) / (length * length)).squareRoot())
            let top = speed.max() ?? 0
            var count = 0
            for i in 1..<(speed.count - 1) where speed[i] > speed[i - 1] && speed[i] >= speed[i + 1] && speed[i] > 0.1 * top { count += 1 }
            // Per 100 px of path: long strokes naturally hold more movements.
            peaks.append(Double(count) / (length / 100))
            let inner = Array(speed[(speed.count / 20)..<(speed.count - speed.count / 20)])
            hes.append(Double(inner.filter { $0 < 0.08 * top }.count) / Double(max(1, inner.count)))
            trem.append(tremor(p))
        }
        guard !jerks.isEmpty else { return nil }
        func median(_ a: [Double]) -> Double { let s = a.sorted(); return s[s.count / 2] }
        return Fluency(normalizedJerk: median(jerks), peaksPerStroke: peaks.reduce(0, +) / Double(peaks.count),
                       hesitation: hes.reduce(0, +) / Double(hes.count), tremor: median(trem))
    }

    /// Lateral wobble: RMS distance (px) between the line and the same line smoothed over 13 px. Letter shapes change
    /// over tens of pixels and are kept by the smoothing; a shaky hand adds deviation at a few-pixel scale.
    static func tremor(_ path: [CGPoint]) -> Double {
        var pts: [CGPoint] = [path[0]]
        var carry: CGFloat = 0
        for i in 1..<path.count {
            let a = path[i - 1], b = path[i], len = hypot(b.x - a.x, b.y - a.y)
            guard len > 0 else { continue }
            var t = 1 - carry
            while t <= len { pts.append(CGPoint(x: a.x + (b.x - a.x) * t / len, y: a.y + (b.y - a.y) * t / len)); t += 1 }
            carry = len - (t - 1)
        }
        guard pts.count > 60 else { return 0 }
        var sum: Double = 0
        var n = 0
        for i in 6..<(pts.count - 6) {
            let r = (i - 6)...(i + 6)
            let mx = pts[r].reduce(0) { $0 + $1.x } / 13, my = pts[r].reduce(0) { $0 + $1.y } / 13
            let d = Double(hypot(pts[i].x - mx, pts[i].y - my))
            sum += d * d
            n += 1
        }
        return (sum / Double(max(1, n))).squareRoot()
    }

    // MARK: Shape distance

    /// The pen path as 200 points evenly spaced along all strokes in writing order, centred and scaled to unit height.
    static func shape(_ strokes: [[PenSample]]) -> [CGPoint] {
        let all = strokes.flatMap { $0.map(\.p) }
        guard all.count > 1, let minY = all.map(\.y).min(), let maxY = all.map(\.y).max() else { return [] }
        let h = max(1, maxY - minY)
        let cx = all.map(\.x).reduce(0, +) / CGFloat(all.count), cy = all.map(\.y).reduce(0, +) / CGFloat(all.count)
        var lengths: [CGFloat] = [0]
        for i in 1..<all.count { lengths.append(lengths[i - 1] + hypot(all[i].x - all[i - 1].x, all[i].y - all[i - 1].y)) }
        let total = max(1e-6, lengths.last!)
        var out: [CGPoint] = []
        var j = 0
        for k in 0..<200 {
            let target = total * CGFloat(k) / 199
            while j < all.count - 2, lengths[j + 1] < target { j += 1 }
            let seg = max(1e-6, lengths[j + 1] - lengths[j]), u = (target - lengths[j]) / seg
            let p = CGPoint(x: all[j].x + (all[j + 1].x - all[j].x) * u, y: all[j].y + (all[j + 1].y - all[j].y) * u)
            out.append(CGPoint(x: (p.x - cx) / h, y: (p.y - cy) / h))
        }
        return out
    }

    /// Dynamic time warping (Sakoe–Chiba band 20 %), mean point distance along the best alignment.
    static func dtw(_ a: [CGPoint], _ b: [CGPoint]) -> Double {
        let n = a.count, m = b.count
        guard n > 0, m > 0 else { return .infinity }
        let band = max(abs(n - m), n / 5)
        var prev = [Double](repeating: .infinity, count: m + 1), cur = prev
        prev[0] = 0
        var steps = [[Int]](repeating: [Int](repeating: 0, count: m + 1), count: 2)
        for i in 1...n {
            cur = [Double](repeating: .infinity, count: m + 1)
            for j in max(1, i - band)...min(m, i + band) {
                let d = Double(hypot(a[i - 1].x - b[j - 1].x, a[i - 1].y - b[j - 1].y))
                let options = [prev[j - 1], prev[j], cur[j - 1]]
                let best = options.enumerated().min { $0.element < $1.element }!
                cur[j] = d + best.element
                steps[i % 2][j] = (best.offset == 0 ? steps[(i - 1) % 2][j - 1] : best.offset == 1 ? steps[(i - 1) % 2][j] : steps[i % 2][j - 1]) + 1
            }
            prev = cur
        }
        return prev[m] / Double(max(1, steps[n % 2][m]))
    }

    // MARK: Legibility

    /// Best similarity (1 − edit distance / length) of Vision's handwriting reading to the expected text.
    static func legibility(_ image: CGImage, expected: String) -> Double {
        guard let ctx = InkDraw.context(image.width + 40, image.height + 40) else { return 0 }
        ctx.setFillColor(.white)
        ctx.fill(CGRect(x: 0, y: 0, width: image.width + 40, height: image.height + 40))
        ctx.draw(image, in: CGRect(x: 20, y: 20, width: image.width, height: image.height))
        guard let page = ctx.makeImage() else { return 0 }
        let request = VNRecognizeTextRequest()
        request.recognitionLevel = .accurate
        request.usesLanguageCorrection = false
        let supported = (try? request.supportedRecognitionLanguages()) ?? []
        request.recognitionLanguages = ["en-US", "ru-RU", "tr-TR"].filter(supported.contains)
        try? VNImageRequestHandler(cgImage: page).perform([request])
        let readings = (request.results ?? []).flatMap { $0.topCandidates(3).map(\.string) }
        let joined = readings.joined(separator: " ")
        func letters(_ s: String) -> [Character] { Array(s.lowercased().filter(\.isLetter)) }
        let want = letters(expected)
        guard !want.isEmpty else { return 0 }
        return ([joined] + readings).map { similarity(letters($0), want) }.max() ?? 0
    }

    private static func similarity(_ a: [Character], _ b: [Character]) -> Double {
        guard !a.isEmpty else { return 0 }
        var d = Array(0...b.count)
        for i in 1...a.count {
            var prev = d[0]; d[0] = i
            for j in 1...max(1, b.count) where b.count > 0 {
                let tmp = d[j]
                d[j] = min(d[j] + 1, d[j - 1] + 1, prev + (a[i - 1] == b[j - 1] ? 0 : 1))
                prev = tmp
            }
        }
        return max(0, 1 - Double(d[b.count]) / Double(max(a.count, b.count)))
    }

    // MARK: Run

    public struct Row: Codable {
        public var name: String
        public var form: String
        public var fluency: Fluency
        public var intra: Double          // mean DTW between copies of one design
        public var inter: Double          // mean DTW between first copies of different designs
        public var legibility: Double?    // share of designs read by Vision (similarity ≥ 0.6), legible forms only
    }

    /// Reference trajectories that give the numbers a scale: the same garland of loops written by a fluent hand,
    /// by a slow hand that nearly stops between movements, and by the slow hand with a 10 Hz tremor (at writing
    /// speed a tremor only shows as wobble when the hand is slow).
    static func calibration() -> [(String, Fluency)] {
        var via: [CGPoint] = [CGPoint(x: 0, y: 0)]
        for k in 0..<6 {
            let x = CGFloat(k) * 0.5
            via += [CGPoint(x: x + 0.15, y: 0.7), CGPoint(x: x + 0.05, y: 0.5), CGPoint(x: x + 0.35, y: 0.0)]
        }
        func trajectory(overlap: Double, tremor: Bool) -> [[PenSample]] {
            var rng = SeededRandom(42)
            var hand = SigmaLognormal.Hand()
            hand.overlap = overlap
            var path = SigmaLognormal.trajectory(SigmaLognormal.components(via: via, hand: hand, rng: &rng), start: via[0])
            path = path.map { var p = $0; p.p = CGPoint(x: p.p.x * 130, y: p.p.y * 130); return p }
            if tremor {
                path = path.map { var p = $0; let w = 2 * Double.pi * 10 * p.t
                    p.p.x += CGFloat(3 * sin(w)); p.p.y += CGFloat(3 * cos(w * 1.13)); return p }
            }
            return [path]
        }
        return [("fluent", fluency(trajectory(overlap: 0.7, tremor: false))!),
                ("slow, stops between movements", fluency(trajectory(overlap: 0.05, tremor: false))!),
                ("slow with 10 Hz tremor (3 px ≈ 0.2 mm): a laboured, shaky hand", fluency(trajectory(overlap: 0.05, tremor: true))!)]
    }

    /// Runs the bench and writes `report.md`, `results.json` and sample images into `folder`. Returns the report.
    public static func run(to folder: URL, designs: Int = 6, copies: Int = 5) throws -> String {
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let names = ["Sergey Zorin", "Иван Петров", "Ayşe Yılmaz", "John A. Smith"]
        let ink = InkColor.blue.pen
        var rows: [Row] = []
        for name in names {
            for form in SignatureForm.allCases.dropFirst() {
                var flu: [Fluency] = [], intra: [Double] = [], firsts: [[CGPoint]] = []
                var read = 0, readable = 0
                for d in 0..<designs {
                    let design = SignatureGenerator.design(form: form, variant: d, seed: UInt64(7000 + d * 31 + names.firstIndex(of: name)! * 7))
                    var shapes: [[CGPoint]] = []
                    for c in 0..<copies {
                        var traj: [[PenSample]] = []
                        let image = SignatureGenerator.render(name, design: design, impression: UInt64(c + 1) &* 104_729 &+ UInt64(d),
                                                              ink: ink, capture: { traj = $0 })
                        if let f = fluency(traj) { flu.append(f) }
                        shapes.append(shape(traj))
                        if c == 0, let image {
                            if d < 3, let data = NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:]) {
                                try data.write(to: folder.appendingPathComponent("\(names.firstIndex(of: name)!)-form\(form.rawValue)-design\(d).png"))
                            }
                            if [.fullName, .initialSurname, .surname].contains(form) {
                                let words = name.split(separator: " ").map(String.init)
                                let expected = form == .fullName ? words.joined(separator: " ") : words.last!
                                readable += 1
                                if legibility(image, expected: expected) >= 0.6 { read += 1 }
                            }
                        }
                    }
                    firsts.append(shapes[0])
                    for i in 0..<shapes.count { for j in (i + 1)..<shapes.count { intra.append(dtw(shapes[i], shapes[j])) } }
                }
                var inter: [Double] = []
                for i in 0..<firsts.count { for j in (i + 1)..<firsts.count { inter.append(dtw(firsts[i], firsts[j])) } }
                func mean(_ a: [Double]) -> Double { a.isEmpty ? 0 : a.reduce(0, +) / Double(a.count) }
                rows.append(Row(name: name, form: "\(form)",
                                fluency: Fluency(normalizedJerk: mean(flu.map(\.normalizedJerk)), peaksPerStroke: mean(flu.map(\.peaksPerStroke)),
                                                 hesitation: mean(flu.map(\.hesitation)), tremor: mean(flu.map(\.tremor))),
                                intra: mean(intra), inter: mean(inter), legibility: readable > 0 ? Double(read) / Double(readable) : nil))
            }
        }
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        try encoder.encode(rows).write(to: folder.appendingPathComponent("results.json"))
        var report = markdown(rows, designs: designs, copies: copies)
        report += "\n## Calibration (one garland of loops, 130 px high)\n\n| Reference | Norm. jerk | Peaks/100 px | Hesitation | Tremor px |\n|---|---:|---:|---:|---:|\n"
        for (label, f) in calibration() {
            report += "| \(label) | \(String(format: "%.1f", f.normalizedJerk)) | \(String(format: "%.2f", f.peaksPerStroke)) | "
            report += "\(String(format: "%.1f", f.hesitation * 100)) % | \(String(format: "%.2f", f.tremor)) |\n"
        }
        try report.write(to: folder.appendingPathComponent("report.md"), atomically: true, encoding: .utf8)
        return report
    }

    /// Regression thresholds (from the calibration references and the 2026-11-06 runs): fluent timing, smooth line,
    /// copies that differ but stay one style, styles that are distinct. Returns the failed checks.
    public static func check(folder: URL) -> [String] {
        guard let data = try? Data(contentsOf: folder.appendingPathComponent("results.json")),
              let rows = try? JSONDecoder().decode([Row].self, from: data), !rows.isEmpty else { return ["no results.json"] }
        func mean(_ a: [Double]) -> Double { a.reduce(0, +) / Double(max(1, a.count)) }
        var failed: [String] = []
        let hesitation = mean(rows.map(\.fluency.hesitation)), tremor = mean(rows.map(\.fluency.tremor))
        let intra = mean(rows.map(\.intra)), inter = mean(rows.map(\.inter))
        if hesitation > 0.15 { failed.append(String(format: "hesitation %.1f %% > 15 %% (fluent reference ≈ 5 %%, laboured ≈ 49 %%)", hesitation * 100)) }
        if tremor > 0.5 { failed.append(String(format: "line wobble %.2f px > 0.5 px (fluent ≈ 0.28, shaky ≈ 0.68)", tremor)) }
        if intra < 0.02 { failed.append(String(format: "copies too alike: intra %.3f < 0.02", intra)) }
        if inter / max(1e-6, intra) < 3 { failed.append(String(format: "styles too close: inter/intra %.1f < 3", inter / max(1e-6, intra))) }
        return failed
    }

    static func markdown(_ rows: [Row], designs: Int, copies: Int) -> String {
        func f(_ x: Double, _ d: Int = 2) -> String { String(format: "%.\(d)f", x) }
        var s = "# Signature bench\n\n\(designs) designs × \(copies) copies per name and form.\n\n"
        s += "| Name | Form | Norm. jerk ↓ | Peaks/100 px ↓ | Hesitation ↓ | Tremor px ↓ | Intra DTW | Inter DTW | Inter/intra ↑ | Read by Vision |\n"
        s += "|---|---|---:|---:|---:|---:|---:|---:|---:|---:|\n"
        for r in rows {
            s += "| \(r.name) | \(r.form) | \(f(r.fluency.normalizedJerk, 1)) | \(f(r.fluency.peaksPerStroke)) | \(f(r.fluency.hesitation * 100, 1)) % | "
            s += "\(f(r.fluency.tremor)) | \(f(r.intra, 3)) | \(f(r.inter, 3)) | \(f(r.inter / max(1e-6, r.intra), 1)) | "
            s += (r.legibility.map { "\(Int(($0 * 100).rounded())) %" } ?? "—") + " |\n"
        }
        func mean(_ a: [Double]) -> Double { a.isEmpty ? 0 : a.reduce(0, +) / Double(a.count) }
        let leg = rows.compactMap(\.legibility)
        s += "\n**Overall:** normalized jerk \(f(mean(rows.map(\.fluency.normalizedJerk)), 1)), "
        s += "peaks/100 px \(f(mean(rows.map(\.fluency.peaksPerStroke)))), hesitation \(f(mean(rows.map(\.fluency.hesitation)) * 100, 1)) %, "
        s += "tremor \(f(mean(rows.map(\.fluency.tremor)))) px, intra \(f(mean(rows.map(\.intra)), 3)), inter \(f(mean(rows.map(\.inter)), 3)), "
        s += "read by Vision \(Int((mean(leg) * 100).rounded())) % of legible-form designs.\n"
        return s
    }
}
