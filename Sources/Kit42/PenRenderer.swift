import AppKit

/// Writing instruments for signatures.
public enum PenType: Int, CaseIterable, Codable {
    case ballpoint, gel, fountain, felt

    public var title: String {
        switch self {
        case .ballpoint: return L("Ballpoint pen")
        case .gel: return L("Gel pen")
        case .fountain: return L("Fountain pen")
        case .felt: return L("Felt-tip pen")
        }
    }
}

/// Draws timed pen trajectories as ink on paper. Every effect comes from the pen's real speed and direction:
/// - ballpoint: near-constant width, more ink on downstrokes (more pressure), blobs where the pen is slow, skips
///   where it is fast, fine striations left by the ball along the stroke;
/// - gel: wider, even and saturated, few skips;
/// - fountain: width follows the angle between stroke and nib, ink pools at slow ends;
/// - felt-tip: broad soft line that dries out (lightens) at speed;
/// - paper: a fibre texture under every pen.
enum PenRenderer {
    /// `strokes` in pixels (y up), `pen` = nominal line width in pixels.
    /// `scale`: pixels per base pixel (high-resolution output keeps the same look; see `InkTexture.apply`).
    static func draw(_ strokes: [[PenSample]], pen type: PenType, width pen: CGFloat, ink: NSColor, rng: inout SeededRandom,
                     scale: CGFloat = 1) -> CGImage? {
        let paths = strokes.map { resample($0, step: scale) }.filter { $0.count > 2 }
        guard !paths.isEmpty else { return nil }
        let reach = pen * (type == .fountain ? 3 : 4)
        let all = paths.flatMap { $0 }
        let minX = all.map(\.p.x).min()! - reach, maxX = all.map(\.p.x).max()! + reach
        let minY = all.map(\.p.y).min()! - reach, maxY = all.map(\.p.y).max()! + reach
        let w = Int((maxX - minX).rounded(.up)), h = Int((maxY - minY).rounded(.up))
        guard w > 0, h > 0, CGFloat(w) < 6000 * scale, CGFloat(h) < 4000 * scale, let out = InkDraw.context(w, h) else { return nil }
        // Speeds normalised over the whole signature (robust to outliers).
        let speeds = all.map(\.speed).sorted()
        let lo = speeds[Int(Double(speeds.count - 1) * 0.08)], hi = max(lo + 1e-6, speeds[Int(Double(speeds.count - 1) * 0.92)])
        let nib = rng.range(0.55...0.85)   // fountain nib angle (≈ 30–50°)
        let skipNoise = SeededRandom(rng.next())

        for path in paths {
            let pts = path.map { CGPoint(x: $0.p.x - minX, y: $0.p.y - minY) }
            let n = pts.count
            var width: [CGFloat] = [], alpha: [CGFloat] = []
            var length: CGFloat = 0
            var lengths: [CGFloat] = [0]
            for i in 1..<n { length += hypot(pts[i].x - pts[i - 1].x, pts[i].y - pts[i - 1].y); lengths.append(length) }
            for i in 0..<n {
                let s = path[i]
                let v = min(1, max(0, (s.speed - lo) / (hi - lo)))
                let t = lengths[i] / max(1, length)
                // Pen lands and lifts while moving: thin, faint ends (about one pen width per 1 % of the stroke).
                let land = min(1, t / 0.04), lift = min(1, (1 - t) / 0.08)
                let touch = min(land, lift)
                let down = max(0, -sin(s.direction))            // moving down the page: more pressure
                var wd: CGFloat, a: CGFloat
                switch type {
                // Downstrokes are pressed harder (wider, darker); fast upstrokes and joins are lighter.
                case .ballpoint:
                    wd = pen * (1.0 - 0.16 * v + 0.2 * down)
                    // Tone follows the movement: fast joins lighter, slow turns and downstrokes darker (about 0.7–1).
                    a = 0.95 - 0.25 * v + 0.12 * down
                case .gel:
                    wd = pen * 1.25 * (1.0 - 0.08 * v + 0.1 * down)
                    a = 1.0 - 0.06 * v
                case .fountain:
                    let across = abs(sin(s.direction - nib))
                    wd = pen * (0.25 + 2.6 * across)
                    a = 0.96 - 0.12 * v + 0.08 * (1 - v)
                case .felt:
                    wd = pen * 3.0
                    a = 0.82 - 0.35 * v
                }
                // Touch-down: the pen lands pressing, so the first few pixels are full width (with the landing blob);
                // the lift tapers.
                let taper = t < 0.5 ? 0.75 + 0.25 * land : 0.3 + 0.7 * lift
                width.append(wd * taper)
                alpha.append(min(1, max(0.15, a - 0.3 * (1 - touch))))
            }
            let ribbon = InkDraw.taperedStroke(pts) { t in width[min(n - 1, Int((t * CGFloat(n - 1)).rounded()))] }
            guard let layer = InkDraw.context(w, h) else { continue }
            layer.addPath(ribbon)
            layer.clip()
            // Density along the stroke without darker overlaps ("copy": the last segment over a pixel wins).
            layer.setBlendMode(.copy)
            layer.setLineCap(.round)
            layer.setLineWidth(pen * (type == .fountain ? 5 : 3))
            var skip = skipNoise
            var skipLeft: CGFloat = 0
            for i in 1..<n {
                var a = alpha[i]
                if type == .ballpoint || type == .felt {
                    let v = min(1, max(0, (path[i].speed - lo) / (hi - lo)))
                    // Skips: short dropouts on the fastest parts.
                    if skipLeft <= 0, v > 0.85, skip.chance(type == .ballpoint ? 0.002 : 0.003) { skipLeft = skip.range(1...2.5) * scale }
                    if skipLeft > 0 { a *= type == .ballpoint ? 0.4 : 0.6; skipLeft -= lengths[i] - lengths[i - 1] }
                }
                layer.setStrokeColor(ink.withAlphaComponent(a).cgColor)
                layer.move(to: pts[i - 1])
                layer.addLine(to: pts[i])
                layer.strokePath()
            }
            if type == .ballpoint {
                // Striations: the ball leaves fine lighter grooves along the stroke.
                layer.setBlendMode(.destinationOut)
                for offset in [-0.28, 0.3] as [CGFloat] {
                    let groove = CGMutablePath()
                    for i in 0..<n {
                        let a = pts[max(0, i - 1)], b = pts[min(n - 1, i + 1)]
                        let d = max(0.001, hypot(b.x - a.x, b.y - a.y))
                        let q = CGPoint(x: pts[i].x - (b.y - a.y) / d * width[i] * offset, y: pts[i].y + (b.x - a.x) / d * width[i] * offset)
                        if i == 0 { groove.move(to: q) } else { groove.addLine(to: q) }
                    }
                    layer.addPath(groove)
                    layer.setLineWidth(max(0.45 * scale, pen * 0.16))
                    layer.setStrokeColor(NSColor(white: 0, alpha: 0.1).cgColor)
                    layer.strokePath()
                }
            }
            if let image = layer.makeImage() { out.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h)) }
            // Blobs where the pen moved slowly (ink gathers on the ball / nib), mostly at landings and turns.
            out.setFillColor(ink.withAlphaComponent(0.9).cgColor)
            for i in stride(from: 0, to: n, by: max(1, Int(pen * 2 / scale))) {
                let v = min(1, max(0, (path[i].speed - lo) / (hi - lo)))
                let chance: CGFloat = type == .gel ? 0 : (v < 0.06 ? 0.08 : 0)
                if rng.chance(chance) || (i == 0 && rng.chance(type == .fountain ? 0.5 : 0.18)) {
                    let r = width[i] * rng.range(0.55...0.8)
                    out.fillEllipse(in: CGRect(x: pts[i].x - r, y: pts[i].y - r, width: 2 * r, height: 2 * r))
                }
            }
        }
        guard let image = out.makeImage() else { return nil }
        var texture: InkTexture.Strength
        switch type {
        // Ballpoint ink is dense: grain and fibres show only at the edges of the line, not as faded stretches.
        case .ballpoint: texture = .init(pressure: rng.range(0.05...0.1), gaps: rng.range(0.004...0.012), bleed: 0.3, alongX: rng.range(0.03...0.07))
        case .gel: texture = .init(pressure: 0.04, gaps: 0.004, bleed: 0.35, alongX: 0.03)
        case .fountain: texture = .init(pressure: 0.08, gaps: 0.006, bleed: 0.55, alongX: 0.05)
        case .felt: texture = .init(pressure: 0.12, gaps: 0.02, bleed: 0.9, alongX: 0.08)
        }
        texture.paper = type == .ballpoint || type == .gel ? rng.range(0.04...0.08) : rng.range(0.1...0.18)
        return InkDraw.trim(InkTexture.apply(image, seed: rng.next(), strength: texture, scale: scale), pad: Int((6 * scale).rounded()))
    }

    /// Even spacing along the path (about one base pixel, `step` pixels), interpolating speed and direction.
    private static func resample(_ s: [PenSample], step: CGFloat = 1) -> [PenSample] {
        guard s.count > 2 else { return s }
        var out: [PenSample] = [s[0]]
        var carry: CGFloat = 0
        for i in 1..<s.count {
            let a = s[i - 1], b = s[i]
            let len = hypot(b.p.x - a.p.x, b.p.y - a.p.y)
            guard len > 0 else { continue }
            var t = step - carry
            while t <= len {
                let k = t / len
                out.append(PenSample(p: CGPoint(x: a.p.x + (b.p.x - a.p.x) * k, y: a.p.y + (b.p.y - a.p.y) * k),
                                     t: a.t + (b.t - a.t) * Double(k), speed: a.speed + (b.speed - a.speed) * k,
                                     direction: atan2(b.p.y - a.p.y, b.p.x - a.p.x)))
                t += step
            }
            carry = len - (t - step)
        }
        out.append(s[s.count - 1])
        return out
    }
}
