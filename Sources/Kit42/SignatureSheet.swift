import AppKit

/// A photo or scan of a sheet with several signatures (the usual way to give samples: sign a page ten or thirty
/// times and photograph it) → one picture per signature.
///
/// Ink is found against the local paper, only inside the paper (a table or floor around the sheet is coloured and
/// textured). Pieces of ink are grouped into signatures by closeness — much more along the line than across it, as
/// a signature is written left to right — and groups that are clearly not one signature are dropped: fragments
/// (much smaller than the typical one), two signatures run together (much taller or wider) and ones cut by the edge
/// of the paper or picture.
public enum SignatureSheet {
    public struct Piece {
        public var image: CGImage
        /// Position in the analysed picture (pixels, origin top left), for review.
        public var box: CGRect
    }

    /// Signatures on the sheet in reading order. A picture with one signature returns that signature.
    public static func split(_ image: CGImage) -> [Piece] {
        let k = min(1, 1600 / CGFloat(max(image.width, image.height)))
        let w = max(16, Int(CGFloat(image.width) * k)), h = max(16, Int(CGFloat(image.height) * k))
        guard let ctx = InkDraw.context(w, h), let data = ctx.data?.assumingMemoryBound(to: UInt8.self) else { return [] }
        ctx.setFillColor(.white); ctx.fill(CGRect(x: 0, y: 0, width: w, height: h))
        ctx.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
        var lum = [Double](repeating: 0, count: w * h), sat = [Double](repeating: 0, count: w * h)
        for i in 0..<(w * h) {
            let r = Double(data[i * 4]), g = Double(data[i * 4 + 1]), b = Double(data[i * 4 + 2])
            lum[i] = 0.3 * r + 0.59 * g + 0.11 * b
            let mx = max(r, g, b), mn = min(r, g, b)
            sat[i] = mx > 0 ? (mx - mn) / mx : 0
        }
        // Paper: bright and nearly grey over a wide neighbourhood (ink is too thin to change that).
        let cell = max(4, min(w, h) / 40)
        let cw = (w + cell - 1) / cell, chh = (h + cell - 1) / cell
        var cellLum = [Double](repeating: 0, count: cw * chh), cellSat = cellLum, cellN = cellLum
        for y in 0..<h { for x in 0..<w {
            let c = (y / cell) * cw + x / cell
            cellLum[c] += lum[y * w + x]; cellSat[c] += sat[y * w + x]; cellN[c] += 1
        } }
        let brightest = (0..<(cw * chh)).map { cellLum[$0] / max(1, cellN[$0]) }.sorted().last ?? 255
        let paperCell = (0..<(cw * chh)).map { c in
            cellLum[c] / max(1, cellN[c]) > brightest * 0.62 && cellSat[c] / max(1, cellN[c]) < 0.22
        }
        func onPaper(_ x: Int, _ y: Int) -> Bool { paperCell[(y / cell) * cw + x / cell] }
        let dark = SignatureLearning.darkness(lum, w, h)
        let threshold = SignatureLearning.inkThreshold(dark.enumerated().filter { onPaper($0.offset % w, $0.offset / w) }.map(\.element))
        var ink = [UInt8](repeating: 0, count: w * h)
        for y in 0..<h { for x in 0..<w where onPaper(x, y) && dark[y * w + x] > threshold {
            // Not next to the paper's edge (shadows and the table show up as dark there).
            let nearEdge = [(-2, 0), (2, 0), (0, -2), (0, 2)].contains { dx, dy in
                let cx = x / cell + dx, cy = y / cell + dy
                return cx < 0 || cy < 0 || cx >= cw || cy >= chh || !paperCell[cy * cw + cx]
            }
            if !nearEdge { ink[y * w + x] = 1 }
        } }
        // Group: grow ink 3.5 % of the width along the line and 0.4 % of the height across it.
        let gx = max(3, Int(Double(w) * 0.035)), gy = max(1, Int(Double(h) * 0.004))
        var grown = ink
        for y in 0..<h {
            var last = -10_000
            for x in 0..<w { if ink[y * w + x] == 1 { last = x }; if x - last <= gx { grown[y * w + x] = 1 } }
            last = 10_000
            for x in stride(from: w - 1, through: 0, by: -1) { if ink[y * w + x] == 1 { last = x }; if last - x <= gx { grown[y * w + x] = 1 } }
        }
        var tall = grown
        for x in 0..<w { for y in 0..<h where grown[y * w + x] == 1 {
            for dy in -gy...gy where y + dy >= 0 && y + dy < h { tall[(y + dy) * w + x] = 1 }
        } }
        // Connected groups.
        var label = [Int32](repeating: 0, count: w * h)
        var groups: [(box: CGRect, ink: Int)] = []
        for start in 0..<(w * h) where tall[start] == 1 && label[start] == 0 {
            let id = Int32(groups.count + 1)
            var stack = [start]; label[start] = id
            var x0 = w, y0 = h, x1 = 0, y1 = 0, count = 0
            while let i = stack.popLast() {
                let x = i % w, y = i / w
                x0 = min(x0, x); x1 = max(x1, x); y0 = min(y0, y); y1 = max(y1, y)
                count += Int(ink[i])
                for j in [i - 1, i + 1, i - w, i + w] where j >= 0 && j < w * h && tall[j] == 1 && label[j] == 0 {
                    if (j == i - 1 && x == 0) || (j == i + 1 && x == w - 1) { continue }
                    label[j] = id; stack.append(j)
                }
            }
            groups.append((CGRect(x: x0, y: y0, width: x1 - x0 + 1, height: y1 - y0 + 1), count))
        }
        let substantial = groups.filter { $0.ink > 30 }
        guard substantial.count > 1 else {
            return [Piece(image: image, box: CGRect(x: 0, y: 0, width: w, height: h))]
        }
        func median(_ v: [CGFloat]) -> CGFloat { let s = v.sorted(); return s[s.count / 2] }
        // The typical signature: median of the bigger groups (fragments would pull it down).
        let big = substantial.sorted { $0.ink > $1.ink }.prefix(max(1, substantial.count * 2 / 3))
        let mw = median(big.map(\.box.width)), mh = median(big.map(\.box.height)), mi = median(big.map { CGFloat($0.ink) })
        /// Pieces of ink inside a box, grouped more tightly (half the reach along the line, none across it).
        func pieces(in box: CGRect) -> [(box: CGRect, ink: Int)] {
            let bx0 = Int(box.minX), by0 = Int(box.minY), bw = Int(box.width), bh = Int(box.height)
            var near = [UInt8](repeating: 0, count: bw * bh)
            let reach = max(2, gx / 2)
            for y in 0..<bh {
                var last = -10_000
                for x in 0..<bw { if ink[(by0 + y) * w + bx0 + x] == 1 { last = x }; if x - last <= reach { near[y * bw + x] = 1 } }
                last = 10_000
                for x in stride(from: bw - 1, through: 0, by: -1) { if ink[(by0 + y) * w + bx0 + x] == 1 { last = x }; if last - x <= reach { near[y * bw + x] = 1 } }
            }
            var seen = [Bool](repeating: false, count: bw * bh), out: [(CGRect, Int)] = []
            for s in 0..<(bw * bh) where near[s] == 1 && !seen[s] {
                var stack = [s]; seen[s] = true
                var x0 = bw, y0 = bh, x1 = 0, y1 = 0, n = 0
                while let i = stack.popLast() {
                    let x = i % bw, y = i / bw
                    x0 = min(x0, x); x1 = max(x1, x); y0 = min(y0, y); y1 = max(y1, y)
                    n += Int(ink[(by0 + y) * w + bx0 + x])
                    for (nx, ny) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] where nx >= 0 && ny >= 0 && nx < bw && ny < bh {
                        let j = ny * bw + nx
                        if near[j] == 1 && !seen[j] { seen[j] = true; stack.append(j) }
                    }
                }
                out.append((CGRect(x: bx0 + x0, y: by0 + y0, width: x1 - x0 + 1, height: y1 - y0 + 1), n))
            }
            return out
        }
        let kept = substantial.filter { g in
            guard g.box.width > mw * 0.6, g.box.width < mw * 1.6, g.box.height < mh * 1.7, CGFloat(g.ink) > mi * 0.45,
                  g.box.minX > 1, g.box.minY > 1, g.box.maxX < CGFloat(w - 1), g.box.maxY < CGFloat(h - 1),
                  // Two signatures written over each other carry about twice the ink.
                  CGFloat(g.ink) < mi * 1.75 else { return false }
            // Bigger than usual: two signatures written into each other (offset, so the box stays moderate) fall
            // apart into two signature-sized pieces when grouped more tightly.
            if g.box.height > mh * 1.25 || g.box.width > mw * 1.2 || CGFloat(g.ink) > mi * 1.3 {
                let parts = pieces(in: g.box).filter { CGFloat($0.ink) > mi * 0.3 && $0.box.width > mw * 0.4 }
                if parts.count >= 2 { return false }
                // Written over each other: much taller with more ink. Losing one sample is safer than learning a pair.
                if g.box.height > mh * 1.4, CGFloat(g.ink) > mi * 1.1 { return false }
            }
            return true
        }
        if ProcessInfo.processInfo.environment["K42_SHEET_DEBUG"] != nil {
            print("typical w \(Int(mw)) h \(Int(mh)) ink \(Int(mi))")
            for g in substantial { print("group", Int(g.box.minX), Int(g.box.minY), Int(g.box.width), Int(g.box.height), g.ink, kept.contains { $0.box == g.box } ? "kept" : "dropped") }
        }
        // Reading order: rows (by centre, a row is half a signature high), then left to right.
        let ordered = kept.sorted { a, b in
            let ra = Int(a.box.midY / max(1, mh * 0.8)), rb = Int(b.box.midY / max(1, mh * 0.8))
            return ra != rb ? ra < rb : a.box.minX < b.box.minX
        }
        let scale = 1 / k
        return ordered.compactMap { g in
            let pad = max(mh * 0.12, 4)
            let r = g.box.insetBy(dx: -pad, dy: -pad).intersection(CGRect(x: 0, y: 0, width: w, height: h))
            let crop = CGRect(x: r.minX * scale, y: r.minY * scale, width: r.width * scale, height: r.height * scale).integral
            guard let piece = image.cropping(to: crop) else { return nil }
            let own = label[Int(g.box.midY) * w + Int(g.box.midX)] != 0 ? label[Int(g.box.midY) * w + Int(g.box.midX)]
                : label[(Int(g.box.minY)..<Int(g.box.maxY)).flatMap { y in (Int(g.box.minX)..<Int(g.box.maxX)).map { y * w + $0 } }.first { label[$0] != 0 } ?? 0]
            return Piece(image: isolated(piece, origin: crop.origin, scale: k, label: label, own: own, w: w, h: h) ?? piece, box: r)
        }
    }

    /// The crop with ink of neighbouring signatures (other groups reaching into the margin) painted over with the
    /// crop's own paper colour.
    private static func isolated(_ piece: CGImage, origin: CGPoint, scale k: CGFloat, label: [Int32], own: Int32, w: Int, h: Int) -> CGImage? {
        let pw = piece.width, ph = piece.height
        guard let ctx = InkDraw.context(pw, ph), let data = ctx.data?.assumingMemoryBound(to: UInt8.self) else { return nil }
        ctx.draw(piece, in: CGRect(x: 0, y: 0, width: pw, height: ph))
        var lums: [(Int, Int)] = []
        for i in stride(from: 0, to: pw * ph, by: 7) { lums.append(((Int(data[i * 4]) * 3 + Int(data[i * 4 + 1]) * 6 + Int(data[i * 4 + 2])) / 10, i)) }
        lums.sort { $0.0 < $1.0 }
        let paper = lums[lums.count * 3 / 4].1
        let pr = data[paper * 4], pg = data[paper * 4 + 1], pb = data[paper * 4 + 2]
        var changed = false
        for y in 0..<ph { for x in 0..<pw {
            // Memory rows run top down, as the label map does.
            let ax = Int((origin.x + CGFloat(x)) * k), ay = Int((origin.y + CGFloat(y)) * k)
            guard ax >= 0, ay >= 0, ax < w, ay < h else { continue }
            let l = label[ay * w + ax]
            if l != 0, l != own {
                let i = (y * pw + x) * 4
                data[i] = pr; data[i + 1] = pg; data[i + 2] = pb; data[i + 3] = 255
                changed = true
            }
        } }
        return changed ? ctx.makeImage() : piece
    }

    /// Learning samples from one picture: every signature on it, traced.
    public static func samples(from image: CGImage) -> [SignatureSample] {
        split(image).compactMap { SignatureSample.from(picture: $0.image) }
    }
}
