import CoreGraphics
import Foundation

/// A pen sample with time: position (cap-height units), speed (units per second) and direction (radians).
public struct PenSample {
    public var p: CGPoint
    public var t: Double
    public var speed: CGFloat
    public var direction: CGFloat
}

/// Sigma-lognormal model of rapid handwriting (Plamondon's kinematic theory).
///
/// A stroke is a sum of overlapping movement components. Each component moves the pen a distance `D` along a
/// circular arc whose direction turns from `θs` to `θe`, with a lognormal speed profile (fast rise, slower decay).
/// Summing the overlapping velocities gives the trajectory: corners are rounded the way a fast hand rounds them,
/// speed rises and falls by itself, and pen-down/pen-up happen while moving. Copies of one signature are made by
/// perturbing the component parameters, which is how synthetic signature duplicates are produced in the literature.
public enum SigmaLognormal {
    public struct Component {
        public var D: CGFloat          // distance along the arc
        public var t0: Double          // onset (s)
        public var mu: Double          // log-time delay
        public var sigma: Double       // log-response time
        public var thetaStart: CGFloat
        public var thetaEnd: CGFloat
    }

    /// How a stroke is written: how much consecutive components overlap (fluency) and their timing spread.
    public struct Hand {
        public var overlap: Double = 0.55         // share of the previous component's duration before the next starts
        public var cornerOverlap: Double = 0.22   // at marked corners the pen nearly stops
        public var sigma: ClosedRange<Double> = 0.22...0.32
        public var tempo: Double = 0.16           // seconds for a movement one cap high
        public var curvatureGain: CGFloat = 0.45  // how much of the turn at via-points becomes arc curvature
        /// Smaller overlap for short movements (keeps letter shapes when the model draws the line itself);
        /// off when the line comes from elsewhere and the model only times it.
        public var sizeScaled = true
        public init() {}
    }

    /// Components through via-points. `corners[i]` marks a sharp turn at via-point i (the pen nearly stops).
    /// `durations` (optional, one per segment) come from a recorded hand; otherwise isochrony sets them:
    /// bigger movements are faster, so time grows only slowly with size.
    public static func components(via p: [CGPoint], corners: Set<Int> = [], durations: [Double]? = nil,
                                  hand: Hand, rng: inout SeededRandom) -> [Component] {
        guard p.count > 1 else { return [] }
        func angle(_ a: CGPoint, _ b: CGPoint) -> CGFloat { atan2(b.y - a.y, b.x - a.x) }
        func wrap(_ a: CGFloat) -> CGFloat { var x = a; while x > .pi { x -= 2 * .pi }; while x < -.pi { x += 2 * .pi }; return x }
        let n = p.count - 1
        var chords: [CGFloat] = [], alphas: [CGFloat] = []
        for k in 0..<n { chords.append(hypot(p[k + 1].x - p[k].x, p[k + 1].y - p[k].y)); alphas.append(angle(p[k], p[k + 1])) }
        // Turning angle at each interior via-point.
        var turn = [CGFloat](repeating: 0, count: n + 1)
        for k in 1..<n { turn[k] = wrap(alphas[k] - alphas[k - 1]) }
        var out: [Component] = []
        var t0: Double = 0
        var previousDuration: Double = 0
        for k in 0..<n {
            guard chords[k] > 1e-4 else { continue }
            // Arc between via-points: curvature from the turns at both ends (none across marked corners).
            let a = corners.contains(k) ? 0 : turn[k], b = corners.contains(k + 1) ? 0 : turn[k + 1]
            let delta = max(-2.2, min(2.2, hand.curvatureGain * (a + b)))
            let D = abs(delta) > 1e-3 ? chords[k] * (delta / 2) / sin(delta / 2) : chords[k]
            let duration = durations?[safe: k] ?? hand.tempo * pow(Double(max(0.02, chords[k])), 0.35)
            let sigma = Double.random(in: hand.sigma, using: &rng)
            // The lognormal spreads over about exp(mu ± 3σ); its bulk lasts ~`duration`.
            let mu = log(max(0.02, duration) / 2.2)
            if k > 0 {
                // Small movements (inside letters) overlap less, so letter shapes survive; long sweeps flow together.
                let sizeFactor = hand.sizeScaled ? Double(min(1, max(0.45, chords[k] / 0.45))) : 1
                let ov = corners.contains(k) ? hand.cornerOverlap : hand.overlap * sizeFactor
                t0 += previousDuration * (1 - ov)
            }
            out.append(Component(D: D, t0: t0, mu: mu, sigma: sigma,
                                 thetaStart: alphas[k] - delta / 2, thetaEnd: alphas[k] + delta / 2))
            previousDuration = duration
        }
        return out
    }

    /// Integrates the components into a timed trajectory starting at `start` (dt seconds per sample).
    public static func trajectory(_ components: [Component], start: CGPoint, dt: Double = 1.0 / 400) -> [PenSample] {
        guard let first = components.first else { return [] }
        let end = components.map { $0.t0 + exp($0.mu + 3.2 * $0.sigma) }.max() ?? first.t0
        var out: [PenSample] = []
        var p = start
        var t = first.t0 + 1e-4
        while t <= end {
            var vx: CGFloat = 0, vy: CGFloat = 0
            for c in components where t > c.t0 {
                let x = log(t - c.t0)
                let z = (x - c.mu) / c.sigma
                let speed = Double(c.D) / (c.sigma * sqrt(2 * .pi) * (t - c.t0)) * exp(-0.5 * z * z)
                guard speed > 1e-9 else { continue }
                let cdf = 0.5 * (1 + erf(z / sqrt(2)))
                let theta = c.thetaStart + (c.thetaEnd - c.thetaStart) * CGFloat(cdf)
                vx += CGFloat(speed) * cos(theta)
                vy += CGFloat(speed) * sin(theta)
            }
            let s = hypot(vx, vy)
            out.append(PenSample(p: p, t: t, speed: s, direction: atan2(vy, vx)))
            p.x += vx * CGFloat(dt)
            p.y += vy * CGFloat(dt)
            t += dt
        }
        // Drop the silent tails before the pen lands and after it stops (speed under 2 % of the peak).
        let peak = out.map(\.speed).max() ?? 0
        guard peak > 0 else { return out }
        let first2 = out.firstIndex { $0.speed > peak * 0.02 } ?? 0
        let last = out.lastIndex { $0.speed > peak * 0.02 } ?? out.count - 1
        return Array(out[first2...max(first2, last)])
    }

    /// Scalar speed of the summed components over time: (time, distance moved in this step). Used to move along
    /// a given smooth line — geometry from the line, rhythm from the model.
    public static func speedProfile(_ components: [Component], dt: Double = 1.0 / 400) -> [(Double, CGFloat)] {
        guard let first = components.first else { return [] }
        let end = components.map { $0.t0 + exp($0.mu + 3.2 * $0.sigma) }.max() ?? first.t0
        var out: [(Double, CGFloat)] = []
        var t = first.t0 + 1e-4
        while t <= end {
            var v: Double = 0
            for c in components where t > c.t0 {
                let z = (log(t - c.t0) - c.mu) / c.sigma
                v += Double(c.D) / (c.sigma * sqrt(2 * .pi) * (t - c.t0)) * exp(-0.5 * z * z)
            }
            out.append((t, CGFloat(v * dt)))
            t += dt
        }
        // Trim the silent tails (under 2 % of the peak step).
        let peak = out.map(\.1).max() ?? 0
        let a = out.firstIndex { $0.1 > peak * 0.02 } ?? 0, b = out.lastIndex { $0.1 > peak * 0.02 } ?? out.count - 1
        return Array(out[a...max(a, b)])
    }

    /// Copy variation in the model's own terms: each component's size, direction, timing and spread change a
    /// little, so a copy differs the way a person's repetitions differ — smoothly and everywhere at once.
    public static func perturb(_ components: [Component], amount: Double, rng: inout SeededRandom) -> [Component] {
        components.map { c in
            var c = c
            // Direction errors accumulate along a stroke, so they stay small; size and timing vary more.
            c.D *= CGFloat(1 + Double.random(in: -0.06...0.06, using: &rng) * amount)
            let dAngle = CGFloat(Double.random(in: -0.045...0.045, using: &rng) * amount)
            c.thetaStart += dAngle + CGFloat(Double.random(in: -0.03...0.03, using: &rng) * amount)
            c.thetaEnd += dAngle + CGFloat(Double.random(in: -0.03...0.03, using: &rng) * amount)
            c.t0 *= 1 + Double.random(in: -0.035...0.035, using: &rng) * amount
            c.mu += Double.random(in: -0.05...0.05, using: &rng) * amount
            c.sigma *= 1 + Double.random(in: -0.06...0.06, using: &rng) * amount
            return c
        }
    }
}

extension SeededRandom: RandomNumberGenerator {}

extension Array {
    subscript(safe i: Int) -> Element? { indices.contains(i) ? self[i] : nil }
}
