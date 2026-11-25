import CoreGraphics
import Foundation

/// Four corners of a (possibly rotated) rectangle in pixel coordinates, origin top-left.
struct Quad: Codable, Equatable {
    var tl: CGPoint, tr: CGPoint, br: CGPoint, bl: CGPoint

    var points: [CGPoint] { [tl, tr, br, bl] }

    /// A usable perspective region is finite, nondegenerate and strictly convex.
    var isValid: Bool {
        guard points.allSatisfy({ $0.x.isFinite && $0.y.isFinite }), area > 1e-12 else { return false }
        var sign: CGFloat = 0
        for i in 0..<4 {
            let a = points[i], b = points[(i + 1) % 4], c = points[(i + 2) % 4]
            let cross = (b.x - a.x) * (c.y - b.y) - (b.y - a.y) * (c.x - b.x)
            guard cross.isFinite, abs(cross) > 1e-12 else { return false }
            if sign == 0 { sign = cross } else if cross * sign <= 0 { return false }
        }
        return true
    }

    var bounds: CGRect {
        let xs = points.map(\.x), ys = points.map(\.y)
        return CGRect(x: xs.min()!, y: ys.min()!, width: xs.max()! - xs.min()!, height: ys.max()! - ys.min()!)
    }

    /// Clockwise tilt of the top edge in degrees (y grows downwards).
    var angle: Double {
        let a = atan2(Double(tr.y - tl.y), Double(tr.x - tl.x))
        let b = atan2(Double(br.y - bl.y), Double(br.x - bl.x))
        return (a + b) / 2 * 180 / .pi
    }

    var area: Double {
        let p = points
        var s = 0.0
        for i in 0..<4 { let a = p[i], b = p[(i + 1) % 4]; s += Double(a.x * b.y - b.x * a.y) }
        return abs(s) / 2
    }

    var width: Double { (hypot(Double(tr.x - tl.x), Double(tr.y - tl.y)) + hypot(Double(br.x - bl.x), Double(br.y - bl.y))) / 2 }
    var height: Double { (hypot(Double(bl.x - tl.x), Double(bl.y - tl.y)) + hypot(Double(br.x - tr.x), Double(br.y - tr.y))) / 2 }

    func scaled(_ sx: CGFloat, _ sy: CGFloat) -> Quad {
        Quad(tl: CGPoint(x: tl.x * sx, y: tl.y * sy), tr: CGPoint(x: tr.x * sx, y: tr.y * sy),
             br: CGPoint(x: br.x * sx, y: br.y * sy), bl: CGPoint(x: bl.x * sx, y: bl.y * sy))
    }

    /// Moves every corner towards the centre by `d` pixels (trims a dark edge left by the glass).
    func inset(_ d: CGFloat) -> Quad {
        guard isValid, d.isFinite, d >= 0 else { return self }
        let d = min(d, CGFloat(min(width, height)) / 4)
        let c = CGPoint(x: points.map(\.x).reduce(0, +) / 4, y: points.map(\.y).reduce(0, +) / 4)
        func move(_ p: CGPoint) -> CGPoint {
            let dx = c.x - p.x, dy = c.y - p.y
            let len = max(1, hypot(dx, dy))
            return CGPoint(x: p.x + dx / len * d * 1.414, y: p.y + dy / len * d * 1.414)
        }
        return Quad(tl: move(tl), tr: move(tr), br: move(br), bl: move(bl))
    }

    /// Orders four arbitrary corners as top-left, top-right, bottom-right, bottom-left.
    static func ordered(_ pts: [CGPoint]) -> Quad {
        guard pts.count == 4, pts.allSatisfy({ $0.x.isFinite && $0.y.isFinite }) else {
            return Quad(tl: .zero, tr: .zero, br: .zero, bl: .zero)
        }
        let c = CGPoint(x: pts.map(\.x).reduce(0, +) / 4, y: pts.map(\.y).reduce(0, +) / 4)
        let sorted = pts.sorted { atan2($0.y - c.y, $0.x - c.x) < atan2($1.y - c.y, $1.x - c.x) }
        // atan2 order with y down: starts near "left", goes up (negative y) … ; pick tl as the one with min x+y.
        let start = sorted.indices.min { sorted[$0].x + sorted[$0].y < sorted[$1].x + sorted[$1].y }!
        let r = (0..<4).map { sorted[(start + $0) % 4] }
        // r is clockwise on screen (y down) from tl: tl, tr, br, bl.
        return Quad(tl: r[0], tr: r[1], br: r[2], bl: r[3])
    }

    /// Overlap of two quads (intersection over union), estimated on a grid.
    func iou(_ other: Quad) -> Double {
        guard isValid, other.isValid else { return 0 }
        let box = bounds.union(other.bounds)
        guard box.width > 0, box.height > 0 else { return 0 }
        let n = 80
        var inter = 0, uni = 0
        for j in 0..<n {
            for i in 0..<n {
                let p = CGPoint(x: box.minX + (CGFloat(i) + 0.5) / CGFloat(n) * box.width, y: box.minY + (CGFloat(j) + 0.5) / CGFloat(n) * box.height)
                let a = containsConvex(p), b = other.containsConvex(p)
                if a && b { inter += 1 }
                if a || b { uni += 1 }
            }
        }
        return uni == 0 ? 0 : Double(inter) / Double(uni)
    }

    func contains(_ p: CGPoint) -> Bool {
        guard isValid, p.x.isFinite, p.y.isFinite else { return false }
        return containsConvex(p)
    }

    private func containsConvex(_ p: CGPoint) -> Bool {
        let pts = points
        var sign = 0
        for i in 0..<4 {
            let a = pts[i], b = pts[(i + 1) % 4]
            let cross = (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x)
            let s = cross > 0 ? 1 : (cross < 0 ? -1 : 0)
            if s != 0 {
                if sign == 0 { sign = s } else if s != sign { return false }
            }
        }
        return true
    }

    var description: String {
        String(format: "(%.0f,%.0f)(%.0f,%.0f)(%.0f,%.0f)(%.0f,%.0f)", tl.x, tl.y, tr.x, tr.y, br.x, br.y, bl.x, bl.y)
    }
}

enum Hull {
    /// Convex hull (Andrew's monotone chain).
    static func convex(_ points: [CGPoint]) -> [CGPoint] {
        let p = points.filter { $0.x.isFinite && $0.y.isFinite }.sorted { $0.x == $1.x ? $0.y < $1.y : $0.x < $1.x }
        guard p.count > 2 else { return p }
        func cross(_ o: CGPoint, _ a: CGPoint, _ b: CGPoint) -> CGFloat { (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x) }
        var lower: [CGPoint] = [], upper: [CGPoint] = []
        for pt in p {
            while lower.count >= 2 && cross(lower[lower.count - 2], lower[lower.count - 1], pt) <= 0 { lower.removeLast() }
            lower.append(pt)
        }
        for pt in p.reversed() {
            while upper.count >= 2 && cross(upper[upper.count - 2], upper[upper.count - 1], pt) <= 0 { upper.removeLast() }
            upper.append(pt)
        }
        return Array(lower.dropLast() + upper.dropLast())
    }

    /// Minimum-area enclosing rectangle of a convex polygon (one side is collinear with a hull edge).
    static func minAreaRect(_ hull: [CGPoint]) -> Quad? {
        guard hull.count >= 3, hull.allSatisfy({ $0.x.isFinite && $0.y.isFinite }) else { return nil }
        var best: (area: CGFloat, quad: Quad)?
        for i in 0..<hull.count {
            let a = hull[i], b = hull[(i + 1) % hull.count]
            let len = hypot(b.x - a.x, b.y - a.y)
            guard len > 0 else { continue }
            let ux = (b.x - a.x) / len, uy = (b.y - a.y) / len   // edge direction
            let vx = -uy, vy = ux                                 // normal
            var minU = CGFloat.greatestFiniteMagnitude, maxU = -CGFloat.greatestFiniteMagnitude
            var minV = CGFloat.greatestFiniteMagnitude, maxV = -CGFloat.greatestFiniteMagnitude
            for p in hull {
                let u = (p.x - a.x) * ux + (p.y - a.y) * uy, v = (p.x - a.x) * vx + (p.y - a.y) * vy
                minU = min(minU, u); maxU = max(maxU, u); minV = min(minV, v); maxV = max(maxV, v)
            }
            let area = (maxU - minU) * (maxV - minV)
            if best == nil || area < best!.area {
                func pt(_ u: CGFloat, _ v: CGFloat) -> CGPoint { CGPoint(x: a.x + u * ux + v * vx, y: a.y + u * uy + v * vy) }
                best = (area, Quad.ordered([pt(minU, minV), pt(maxU, minV), pt(maxU, maxV), pt(minU, maxV)]))
            }
        }
        return best.flatMap { $0.quad.isValid ? $0.quad : nil }
    }
}
