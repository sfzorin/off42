import AppKit

/// How much of the name a signature shows.
public enum SignatureForm: Int, CaseIterable {
    case any, fullName, initialSurname, surname, initialsScribble, capitalScribble

    public var title: String {
        switch self {
        case .any: return L("Any form")
        case .fullName: return L("Full name")
        case .initialSurname: return L("Initial and surname")
        case .surname: return L("Surname")
        case .initialsScribble: return L("Initials and scribble")
        case .capitalScribble: return L("Capital and scribble")
        }
    }

    fileprivate var archetypes: [SignatureGenerator.Archetype] {
        switch self {
        case .any: return SignatureGenerator.Archetype.allCases
        case .fullName: return [.full, .fullFading]
        case .initialSurname: return [.initialSurname, .initialSurnameFading, .enclosed]
        case .surname: return [.surname, .surnameFading]
        case .initialsScribble: return [.initialsScribble, .monogram]
        case .capitalScribble: return [.capitalScribble]
        }
    }
}

/// Signatures written with a ballpoint pen by a kinematic model of the hand (no fonts).
///
/// Letters are hand-drawn centre lines (`PenAlphabet`): the pen moves through their via-points on a smooth spline.
/// Speed along the path sets the line: fast parts are slightly thinner and lighter, and every stroke starts and ends
/// thin and faint because the pen lands and lifts while moving — which is what makes a signature look confident.
///
/// A **style** (240 of them) combines a plan (what is written: full name, initial and surname, surname, initials and a
/// scribble, a monogram…), a flourish and how the initial joins the surname; the **design** seed also fixes slant,
/// proportions, garland or arcade, loops and rhythm. The **impression** seed varies each copy: via-points (±2.5 % of
/// the capital), size, slant, angle, pressure, ink shade, skips and touch-down blobs.
public enum SignatureGenerator {
    /// Bound work before spatial/timing resampling, not only when allocating the final bitmap.
    static let maximumRenderSamples = 100_000
    /// Largest capital height in pixels: 130 px (the preview) at up to 8× for insertion (`MarkInsertDialog.render`).
    static let maximumPixelHeight: CGFloat = 130 * 8
    enum Archetype: Int, CaseIterable {
        case full, fullFading, initialSurname, initialSurnameFading, surname, surnameFading,
             initialsScribble, capitalScribble, monogram, enclosed
    }

    static let flourishes = 11, joins = 3, capitalVariants = 4
    public static var styleCount: Int { Archetype.allCases.count * flourishes * joins * capitalVariants }

    static func styleIndex(_ a: Archetype, flourish: Int, join: Int, capital: Int) -> Int {
        ((a.rawValue * flourishes + flourish) * capitalVariants + capital) * joins + join
    }

    /// Design seeds for `count` styles of the form, with different plans where possible.
    public static func designSeeds(_ count: Int, form: SignatureForm) -> [UInt64] {
        var plans = form.archetypes.shuffled()
        while plans.count < count { plans += form.archetypes.shuffled() }
        return plans.prefix(count).map { a in
            let style = styleIndex(a, flourish: Int.random(in: 0..<flourishes), join: Int.random(in: 0..<joins),
                                   capital: Int.random(in: 0..<capitalVariants))
            return (UInt64.random(in: 1...(UInt64.max >> 16)) << 16) | UInt64(style)
        }
    }

    /// A fixed design seed (sample sheets): the `variant`-th style of the form.
    public static func design(form: SignatureForm, variant: Int, seed: UInt64) -> UInt64 {
        let plans = form.archetypes
        let style = styleIndex(plans[variant % plans.count], flourish: (variant * 7 + 3) % flourishes, join: variant % joins,
                               capital: (variant / 2) % capitalVariants)
        return (seed << 16) | UInt64(style)
    }

    /// Style number (1-based) of a design seed.
    public static func styleNumber(_ design: UInt64) -> Int { Int(design & styleMask) % styleCount + 1 }

    /// Low bits of a design: the style index (below `styleCount`) and, in the top bit, the paraph flag.
    static let styleMask: UInt64 = 0x7FFF, paraphFlag: UInt64 = 0x8000

    // MARK: - Plan

    struct Style {
        var archetype: Archetype
        var flourish: Int      // 0 none, 1 underline →, 2 return under ←, 3 rising tail, 4 loop around capital,
                               // 5 strike through capital, 6 double dash, 7 spiral end
        var join: Int          // 0 initial joined, 1 pen lifted, 2 initial with a dot
        var capital: Int       // 0 as drawn, 1 lead-in loop, 2 simplified (fast), 3 tall
        var slant: CGFloat, xh: CGFloat, initialScale: CGFloat, capW: CGFloat, letterW: CGFloat
        var decay: CGFloat, legibleDecay: CGFloat, garland: Bool, loops: Bool, rise: CGFloat
        var entry: Bool, exitLength: CGFloat, exitLift: CGFloat, pen: CGFloat, keep: Int, legible: Int
        var waveH: [CGFloat], waveW: [CGFloat]
        var allographs: UInt64 = 0   // picks the form of each lowercase letter
        var dots = 1                 // 0 none, 1 dot, 2 dash
        var bars = 1                 // 0 none, 1 short, 2 long (over the next letters)
    }

    static func style(_ design: UInt64) -> Style {
        let index = Int(design & styleMask) % styleCount
        var r = SeededRandom(design >> 16)
        let arche = Archetype(rawValue: index / (flourishes * joins * capitalVariants)) ?? .surname
        var s = Style(archetype: arche, flourish: (index / (joins * capitalVariants)) % flourishes, join: index % joins,
                      capital: (index / joins) % capitalVariants,
                      slant: r.range(0.12...0.6), xh: r.range(0.26...0.46), initialScale: r.range(0.7...1.0),
                      capW: r.range(0.6...1.0), letterW: r.range(0.95...1.4), decay: r.range(0.62...0.9),
                      legibleDecay: r.range(0.88...0.98), garland: r.chance(0.5), loops: r.chance(0.6),
                      rise: rising(r.range(0...1)), entry: r.chance(0.35), exitLength: r.range(0.15...0.6),
                      exitLift: r.range(-0.2...0.9), pen: r.range(0.026...0.034), keep: Int(r.range(3...7.9)),
                      legible: Int(r.range(1...3.9)),
                      waveH: (0..<12).map { _ in r.range(0.75...1.25) }, waveW: (0..<12).map { _ in r.range(0.8...1.25) })
        s.allographs = r.next()
        s.dots = r.weighted([0.8, 1.4, 1.0])
        s.bars = r.weighted([0.5, 1.4, 1.0])
        if design & paraphFlag != 0 {
            // A paraph: the initial, three humps of the surname and a short finish.
            s.keep = 3
            s.exitLength *= 0.6
        }
        if arche == .monogram, s.flourish == 0 { s.flourish = 4 }
        if arche == .enclosed { s.flourish = 99 }   // loop around the whole signature
        return s
    }

    /// Most signatures climb 2–8°; about one in eight runs level. One random draw, so the design seeds of saved
    /// styles keep all their other parameters.
    private static func rising(_ u: CGFloat) -> CGFloat {
        u < 0.12 ? -0.02 + u / 0.12 * 0.05 : 0.035 + (u - 0.12) / 0.88 * 0.105
    }

    /// The pen paths (cap-height units) for a name in a style.
    /// `copy` varies this written copy: each letter shifts and scales a little as a whole, loops and flourishes
    /// change shape — the way one person's signatures differ from each other.
    static func plan(_ words: [String], _ s: Style, _ copy: inout SeededRandom) -> [[CGPoint]] {
        let surname = words.last!, given = words.dropLast()
        var strokes: [[CGPoint]] = []
        var current: [CGPoint] = []
        var x: CGFloat = 0
        var capitalBox = CGRect.null
        // What varies between one person's signatures is mostly the layout — how far the line climbs, how long the
        // run-out and flourish are — while letter shapes stay put.
        let rise = s.rise + copy.range(-0.025...0.025)
        func baseline(_ x: CGFloat) -> CGFloat { rise * x }

        func capital(_ c: Character, scale baseScale: CGFloat) {
            var parts = PenAlphabet.capital(c)
            switch s.capital {
            case 2:   // written fast: interior via-points thinned out
                parts = parts.map { p in p.count > 4 ? p.enumerated().filter { $0.offset == 0 || $0.offset == p.count - 1 || $0.offset % 2 == 1 }.map(\.element) : p }
            case 3:   // tall upper strokes
                parts = parts.map { $0.map { CGPoint(x: $0.x * 1.08, y: $0.y > 0.55 ? 0.55 + ($0.y - 0.55) * 1.35 : $0.y) } }
            default: break
            }
            let scale = baseScale * copy.range(0.96...1.04)
            let width = s.capW * scale * copy.range(0.96...1.04)
            let dx = copy.range(-0.03...0.03), dy = copy.range(-0.03...0.03)
            for (i, part) in parts.enumerated() {
                var pts = part.map { CGPoint(x: x + dx + $0.x * width, y: baseline(x) + dy + $0.y * scale) }
                if i == 0, s.capital == 1, let first = pts.first, current.isEmpty {
                    // Lead-in loop under the start of the capital.
                    pts.insert(contentsOf: [CGPoint(x: first.x - 0.22 * scale, y: first.y - 0.18 * scale),
                                            CGPoint(x: first.x - 0.02 * scale, y: first.y - 0.3 * scale),
                                            CGPoint(x: first.x + 0.08 * scale, y: first.y - 0.12 * scale)], at: 0)
                }
                if i == 0, s.entry, current.isEmpty { pts.insert(CGPoint(x: x - 0.25 * scale, y: baseline(x) - 0.12 * scale), at: 0) }
                for p in pts { capitalBox = capitalBox.union(CGRect(origin: p, size: .zero)) }
                if i == 0 { current += pts } else { flush(); strokes.append(pts) }
            }
            x += width * 0.95
        }
        func flush() { if current.count > 1 { strokes.append(current) }; current = [] }
        // Delayed strokes (i dots, t bars), written after the word as real writers do.
        var marks: [(x: CGFloat, y: CGFloat, kind: PenAlphabet.Mark)] = []
        var delayed: [[CGPoint]] = []   // added after everything else, so the flourish still follows the main stroke
        /// `fade` (0…1): how far into the word — later letters are written faster, smaller and simpler.
        func lower(_ c: Character, amp: CGFloat, fade: CGFloat = 0) {
            var h = s.allographs ^ UInt64(c.unicodeScalars.first?.value ?? 0) &* 0x9E37_79B9_7F4A_7C15
            h ^= h >> 29
            guard let (w, raw) = PenAlphabet.lower(c, variant: Int(h % 7)) else { wave(0, amp: amp); return }
            var pts = raw
            if fade > 0.45, pts.count > 4 {
                // Interior turns dropped: the letter becomes a quicker arc between its ends.
                pts = pts.enumerated().filter { $0.offset == 0 || $0.offset == pts.count - 1 || $0.offset % 2 == 0 }.map(\.element)
            }
            let flatten = 1 - 0.3 * fade
            let width = w * s.xh * s.letterW * (1 - 0.12 * fade) * copy.range(0.95...1.05)
            let a = amp * copy.range(0.95...1.05), dy = copy.range(-0.015...0.015)
            current += pts.map { CGPoint(x: x + $0.x * width, y: baseline(x) + dy + (0.5 + ($0.y - 0.5) * flatten) * s.xh * a) }
            let m = PenAlphabet.mark(c)
            switch m {
            case .none: break
            case .bar: marks.append((x + width * 0.45, baseline(x) + s.xh * a * 1.25, m))
            default: marks.append((x + width * 0.45, baseline(x) + s.xh * a * 1.5, m))
            }
            x += width
        }
        func addMarks() {
            for m in marks {
                // Dots, breves and diaeresis are placed carelessly (a little right of the letter, drifting with the
                // copy); some writers drop them altogether (dots == 0).
                let jx = copy.range(-0.03...0.06), jy = copy.range(-0.03...0.03)
                switch m.kind {
                case .dot where s.dots > 0:
                    let len: CGFloat = s.dots == 1 ? 0.03 : 0.1
                    delayed.append([CGPoint(x: m.x + jx, y: m.y + jy),
                                    CGPoint(x: m.x + jx + len + copy.range(0...0.03), y: m.y + jy + 0.015 + copy.range(-0.01...0.02))])
                case .twoDots where s.dots > 0:
                    if s.dots == 2 {   // written as one short dash
                        delayed.append([CGPoint(x: m.x - 0.05 + jx, y: m.y + jy), CGPoint(x: m.x + 0.1 + jx, y: m.y + jy + 0.02)])
                    } else {
                        for dx in [-0.045, 0.055] as [CGFloat] {
                            delayed.append([CGPoint(x: m.x + dx + jx, y: m.y + jy), CGPoint(x: m.x + dx + jx + 0.03, y: m.y + jy + 0.012)])
                        }
                    }
                case .breve where s.dots > 0:
                    let w = copy.range(0.11...0.15), d = copy.range(0.04...0.07)
                    delayed.append([CGPoint(x: m.x - w / 2 + jx, y: m.y + 0.05 + jy), CGPoint(x: m.x - w * 0.15 + jx, y: m.y - d + jy),
                                    CGPoint(x: m.x + w * 0.2 + jx, y: m.y - d * 0.8 + jy), CGPoint(x: m.x + w / 2 + jx, y: m.y + 0.06 + jy)])
                case .bar where s.bars > 0:
                    let len: CGFloat = s.bars == 1 ? 0.25 : 0.6
                    delayed.append([CGPoint(x: m.x - 0.12 + copy.range(-0.04...0.04), y: m.y + copy.range(-0.04...0.02)),
                                    CGPoint(x: m.x - 0.12 + len * copy.range(0.85...1.15), y: m.y + copy.range(0.0...0.08))])
                default: break
                }
            }
            marks = []
        }
        func wave(_ i: Int, amp: CGFloat, tall: Bool = false) {
            let width = s.xh * s.letterW * 0.9 * s.waveW[i % 12] * copy.range(0.92...1.08)
            let h = s.xh * amp * s.waveH[i % 12] * (tall && s.loops ? 2.0 : 1) * copy.range(0.9...1.1)
            let b = baseline(x)
            if s.garland {
                current += [CGPoint(x: x + width * 0.3, y: b), CGPoint(x: x + width * 0.75, y: b + h)]
                if tall, s.loops { current.append(CGPoint(x: x + width * 0.55, y: b + h * 0.8)) }
            } else {
                current += [CGPoint(x: x + width * 0.3, y: b + h)]
                if tall, s.loops { current.append(CGPoint(x: x + width * 0.12, y: b + h * 0.8)) }
                current.append(CGPoint(x: x + width * 0.75, y: b))
            }
            x += width
        }
        /// Word: capital, `legible` letters written out, the rest as a fading garland (or all legible).
        func word(_ w: String, legible: Int?, garlandCount: Int? = nil) {
            defer { addMarks() }
            capital(w.first!, scale: 1)
            let rest = Array(w.dropFirst())
            var amp: CGFloat = 1
            let n = legible ?? rest.count
            // Legibility falls along the word: the first letters are written out, the last ones run together.
            for (i, c) in rest.prefix(n).enumerated() {
                let fade = rest.count > 2 ? CGFloat(i) / CGFloat(rest.count - 1) : 0
                lower(c, amp: amp, fade: fade * fade)
                amp *= s.legibleDecay
            }
            let remaining = Array(rest.dropFirst(n))
            let garland = garlandCount ?? min(remaining.count, s.keep)
            for i in 0..<garland {
                let c = i < remaining.count ? remaining[i] : "n"
                wave(i, amp: amp, tall: PenAlphabet.isTall(c))
                amp *= s.decay
            }
        }
        func initials(_ letters: [Character], scale: CGFloat) {
            for c in letters {
                capital(c, scale: scale)
                if s.join == 1 { flush() }
                if s.join == 2 { flush(); strokes.append(dot(at: CGPoint(x: x + 0.04, y: baseline(x) + 0.03))); x += 0.12 }
                x += s.join == 0 ? 0.02 : 0.1
            }
        }
        func exit() {
            let length = s.exitLength * copy.range(0.6...1.4)
            current.append(CGPoint(x: x + length, y: baseline(x) + s.xh * (s.exitLift + copy.range(-0.3...0.3))))
            x += length
        }
        let givenInitials = given.compactMap(\.first)

        switch s.archetype {
        case .full:
            for g in given { word(g, legible: nil); flush(); x += 0.22 }
            word(surname, legible: nil); exit()
        case .fullFading:
            for g in given { word(g, legible: nil); if s.join == 1 { flush() }; x += 0.18 }
            word(surname, legible: s.legible + 1); exit()
        case .initialSurname:
            initials(Array(givenInitials.prefix(2)), scale: s.initialScale); word(surname, legible: nil); exit()
        case .initialSurnameFading, .enclosed:
            initials(Array(givenInitials.prefix(2)), scale: s.initialScale); word(surname, legible: s.legible); exit()
        case .surname:
            word(surname, legible: nil); exit()
        case .surnameFading:
            word(surname, legible: s.legible); exit()
        case .initialsScribble:
            initials(Array(givenInitials.prefix(1)), scale: s.initialScale)
            word(surname, legible: 0, garlandCount: max(3, s.keep)); exit()
        case .capitalScribble:
            word(surname, legible: 0, garlandCount: max(3, s.keep)); exit()
        case .monogram:
            for c in givenInitials.prefix(1) { capital(c, scale: 1.15); x -= s.capW * 0.45 }
            capital(surname.first!, scale: 1.05); exit()
        }
        flush()

        // Flourish, from the extent of what was written; its control points move more than the letters.
        func v(_ p: CGPoint) -> CGPoint { CGPoint(x: p.x + copy.range(-0.09...0.09), y: p.y + copy.range(-0.06...0.06)) }
        let flourishStart = strokes.count
        let mainEnd = strokes.last?.count ?? 0
        let all = strokes.flatMap { $0 }
        let left = all.map(\.x).min() ?? 0, right = all.map(\.x).max() ?? x
        let low = all.map(\.y).min() ?? 0, mid = (left + right) / 2
        let end = strokes.last?.last ?? CGPoint(x: right, y: 0)
        switch s.flourish {
        case 1:
            strokes.append([CGPoint(x: left - 0.15, y: low - 0.05), CGPoint(x: mid, y: low - 0.28), CGPoint(x: right + 0.35, y: low + 0.1)])
        case 2:
            strokes[strokes.count - 1] += [CGPoint(x: end.x + 0.25, y: low - 0.05), CGPoint(x: mid, y: low - 0.32), CGPoint(x: left - 0.3, y: low - 0.12)]
        case 3:
            strokes[strokes.count - 1] += [CGPoint(x: end.x + 0.3, y: end.y + 0.5), CGPoint(x: end.x + 0.4, y: end.y + 1.0)]
        case 4 where !capitalBox.isNull:
            let c = CGPoint(x: capitalBox.midX, y: capitalBox.midY), rx = capitalBox.width * 0.8 + 0.1, ry = capitalBox.height * 0.65
            strokes.append((0...14).map { i in
                let a = 0.9 + CGFloat(i) / 14 * 1.8 * .pi
                return CGPoint(x: c.x + rx * cos(a) + CGFloat(i) * 0.02, y: c.y + ry * sin(a))
            })
        case 5 where !capitalBox.isNull:
            // A slash across the foot of the capital only: through its middle it turns З into Б or F into E,
            // and across the whole name it reads as crossing the name out.
            let reach = min(right, capitalBox.maxX + 0.25 + capitalBox.width * 0.3)
            let y = capitalBox.minY + capitalBox.height * 0.22
            strokes.append([CGPoint(x: capitalBox.minX - 0.2, y: y - 0.06), CGPoint(x: capitalBox.midX, y: y),
                            CGPoint(x: reach, y: y + 0.1)])
        case 6:
            for k in 0..<2 {
                let y = low - 0.12 - CGFloat(k) * 0.12, x0 = mid + CGFloat(k) * 0.1
                strokes.append([CGPoint(x: x0, y: y), CGPoint(x: x0 + 0.35, y: y - 0.02), CGPoint(x: x0 + 0.7, y: y + 0.04)])
            }
        case 7:
            let r: CGFloat = 0.18
            strokes[strokes.count - 1] += (1...10).map { i in
                let a = CGFloat(i) / 10 * 2.2 * .pi
                return CGPoint(x: end.x + r + r * cos(.pi + a) * (1 - CGFloat(i) * 0.05), y: end.y + r * sin(.pi + a))
            }
        case 8:   // big swoop under everything, from below the capital, rising at the end
            strokes.append([CGPoint(x: left + 0.1, y: low + 0.25), CGPoint(x: left - 0.1, y: low - 0.15), CGPoint(x: mid, y: low - 0.32),
                            CGPoint(x: right, y: low - 0.1), CGPoint(x: right + 0.3, y: low + 0.45)])
        case 9:   // zigzag finish
            strokes[strokes.count - 1] += (1...4).map { k in
                CGPoint(x: end.x + CGFloat(k) * 0.13, y: end.y + (k % 2 == 0 ? 0.0 : 0.3) * (1 - CGFloat(k) * 0.15))
            }
        case 10:  // hook up and back over the name
            strokes[strokes.count - 1] += [CGPoint(x: end.x + 0.2, y: end.y + 0.55), CGPoint(x: end.x - 0.1, y: end.y + 0.95),
                                           CGPoint(x: mid, y: end.y + 0.9)]
        case 99:  // loop around everything, starting from the end
            let cx = mid, cy = (all.map(\.y).max()! + low) / 2, rx = (right - left) / 2 + 0.25, ry = (all.map(\.y).max()! - low) / 2 + 0.2
            strokes[strokes.count - 1] += (0...16).map { i in
                let a = -0.3 - CGFloat(i) / 16 * 1.9 * .pi
                return CGPoint(x: cx + rx * cos(a), y: cy + ry * sin(a))
            }
        default:
            break
        }
        // Copy variation of the flourish: new strokes and the points appended to the last written stroke.
        for i in strokes.indices where i >= flourishStart { strokes[i] = strokes[i].map(v) }
        if flourishStart > 0, strokes[flourishStart - 1].count > mainEnd {
            let k = flourishStart - 1
            strokes[k] = Array(strokes[k].prefix(mainEnd)) + strokes[k].dropFirst(mainEnd).map(v)
        }
        return strokes + delayed
    }

    private static func dot(at p: CGPoint) -> [CGPoint] {
        [p, CGPoint(x: p.x + 0.02, y: p.y + 0.015), CGPoint(x: p.x + 0.035, y: p.y)]
    }

    // MARK: - Rendering

    /// Renders one copy. `pixelHeight` is the capital height in pixels.
    public static func render(_ name: String, design: UInt64, impression: UInt64, ink: NSColor, pen: PenType = .ballpoint,
                              pixelHeight: CGFloat = 130, capture: (([[PenSample]]) -> Void)? = nil) -> CGImage? {
        let words = name.split(whereSeparator: { $0.isWhitespace || $0 == "." }).map(String.init)
        guard !words.isEmpty else { return nil }
        let s = style(design)
        var copy = SeededRandom(impression)
        let raw = plan(words, s, &copy)
        // The hand of this design (fluency and rhythm) — fixed per design.
        var r = SeededRandom(design &* 0x9E37_79B9 &+ 17)
        var hand = SigmaLognormal.Hand()
        // Fluent timing (measured by SignatureBench against a fluent and a laboured reference): the line's shape comes
        // from the spline, so heavy overlap only removes near-stops between movements; cusps slow down, not stop.
        hand.overlap = Double(r.range(0.68...0.82))
        hand.cornerOverlap = 0.5
        hand.sizeScaled = false
        hand.sigma = Double(r.range(0.2...0.24))...Double(r.range(0.26...0.31))
        hand.curvatureGain = r.range(0.3...0.45)
        hand.tempo = Double(r.range(0.13...0.2))
        return draw(strokes: raw, hand: hand, slant: s.slant, penRatio: s.pen, pen: pen, ink: ink, pixelHeight: pixelHeight,
                    copy: &copy, capture: capture)
    }

    /// A generated signature as if recorded from a tablet (positions with time) — test input for learning.
    static func recordedSample(_ name: String, design: UInt64, impression: UInt64) -> SignatureSample? {
        let words = name.split(whereSeparator: { $0.isWhitespace || $0 == "." }).map(String.init)
        guard !words.isEmpty else { return nil }
        let s = style(design)
        var copy = SeededRandom(impression)
        var hand = SigmaLognormal.Hand()
        hand.overlap = 0.55
        var strokes: [[[Double]]] = []
        var clock: Double = 0
        for stroke in plan(words, s, &copy) {
            var via: [CGPoint] = []
            var corners = Set<Int>()
            for p in stroke {
                if let last = via.last, hypot(last.x - p.x, last.y - p.y) < 0.012 { corners.insert(via.count - 1); continue }
                via.append(p)
            }
            guard via.count > 1 else { continue }
            let path = SigmaLognormal.trajectory(SigmaLognormal.components(via: via, corners: corners, hand: hand, rng: &copy), start: via[0])
            guard let t0 = path.first?.t else { continue }
            strokes.append(path.enumerated().compactMap { i, p in
                i % 4 == 0 ? [Double(p.p.x + p.p.y * s.slant), Double(p.p.y), clock + p.t - t0, 0.5] : nil
            })
            clock += (path.last?.t ?? t0) - t0 + 0.15
        }
        return SignatureSample(strokes: strokes)
    }

    /// Shared by generated and learned signatures. Geometry and time are separate, as in handwriting models:
    /// - the line is a smooth centripetal spline through the via-points (corners swept without stops), low-pass
    ///   filtered like a fast hand, with this copy's smooth drift, size, slant and angle;
    /// - the pen's speed along that line comes from the sigma-lognormal model (overlapping lognormal impulses per
    ///   movement, real durations when recorded), perturbed for this copy; speed drives width and ink.
    static func draw(strokes raw: [[CGPoint]], hand: SigmaLognormal.Hand, slant baseSlant: CGFloat, penRatio: CGFloat,
                     pen: PenType, ink: NSColor, pixelHeight: CGFloat, copy: inout SeededRandom, variation: Double = 1,
                     durations: [[Double]]? = nil, runOut: Bool = true, smoothing: CGFloat? = nil, sharpCorners: Bool = false,
                     merge: CGFloat = 0.012, layout: Double? = nil, strokeShift: CGFloat = 0,
                     capture: (([[PenSample]]) -> Void)? = nil) -> CGImage? {
        guard pixelHeight.isFinite, pixelHeight > 0, pixelHeight <= maximumPixelHeight,
              baseSlant.isFinite, penRatio.isFinite, penRatio > 0, penRatio <= 1,
              variation.isFinite, (0...2).contains(variation), raw.count <= 1_024 else { return nil }
        var rawCount = 0
        for stroke in raw {
            guard stroke.count <= SignatureLearning.maximumSamplePoints - rawCount,
                  stroke.allSatisfy({ $0.x.isFinite && $0.y.isFinite }) else { return nil }
            rawCount += stroke.count
        }
        let vary = CGFloat(variation)
        let jitter: CGFloat = 0.006 * vary
        let driftA = copy.range(0.02...0.045) * vary, driftX = copy.range(1.1...2.2), driftY = copy.range(0.9...1.8)
        let driftP1 = copy.range(0...6.28), driftP2 = copy.range(0...6.28)
        let size = pixelHeight * copy.range(0.93...1.07)
        // Layout (slant, angle) varies on its own scale: a learned hand keeps its letter shapes (small `variation`)
        // but its copies still lean and climb differently.
        let lay = CGFloat(layout ?? variation)
        let slant = baseSlant + copy.range(-0.08...0.08) * lay, angle = copy.range(-0.06...0.06) * lay
        let xStretch = copy.range(0.94...1.06)
        let ca = cos(angle), sa = sin(angle)
        let width = max(2, size * penRatio * copy.range(0.95...1.05))
        func toPixels(_ p: CGPoint) -> CGPoint {
            let q = CGPoint(x: (p.x + p.y * slant) * size * xStretch, y: p.y * size)
            return CGPoint(x: q.x * ca - q.y * sa, y: q.x * sa + q.y * ca)
        }
        var trajectories: [[PenSample]] = []
        // Above the usual capital height (130 px) the same pen is drawn at a higher resolution, not a finer pen:
        // spatial sampling (about one pixel) and bitmap limits grow with it; timing (400 Hz) does not.
        let scale = max(1, pixelHeight / 130)
        var spatialBudget = Int(CGFloat(maximumRenderSamples) * scale), timedBudget = maximumRenderSamples
        var timingWorkBudget: Double = 5_000_000
        for (index, stroke) in raw.enumerated() {
            // After a pen lift the next part lands a little elsewhere each time.
            let shift = CGPoint(x: copy.range(-1...1) * strokeShift, y: copy.range(-1...1) * strokeShift)
            var via: [CGPoint] = []
            var corners = Set<Int>()
            for p in stroke {
                if let last = via.last, hypot(last.x - p.x, last.y - p.y) < merge { corners.insert(via.count - 1); continue }
                via.append(p)
            }
            via = via.map { p in
                CGPoint(x: p.x + shift.x + driftA * sin(p.y / driftY * 2 * .pi + driftP1) + copy.range(-jitter...jitter),
                        y: p.y + shift.y + driftA * sin(p.x / driftX * 2 * .pi + driftP2) + copy.range(-jitter...jitter))
            }
            guard via.count > 1 else {
                if let p = via.first { trajectories.append([PenSample(p: toPixels(p), t: 0, speed: 0, direction: 0)]) }
                continue
            }
            // Geometry: smooth line in pixels, about one pixel between samples.
            // Sharp corners (learned signatures): the spline is broken at cusps, which a real hand makes by reversing.
            var path: [CGPoint] = []
            if sharpCorners, !corners.isEmpty {
                var start = 0
                for c in corners.sorted() + [via.count - 1] where c > start {
                    let piece = splineThrough(Array(via[start...c]))
                    path += path.isEmpty ? piece : Array(piece.dropFirst())
                    start = c
                }
            } else { path = splineThrough(via) }
            guard let line = fastHand(path.map(toPixels), smoothing: smoothing.map { size * $0 } ?? max(width * 3, size * 0.03),
                                      runOut: runOut, pointBudget: spatialBudget, scale: scale) else { return nil }
            spatialBudget -= line.count
            guard line.count > 2 else { continue }
            var lengths: [CGFloat] = [0]
            for i in 1..<line.count { lengths.append(lengths[i - 1] + hypot(line[i].x - line[i - 1].x, line[i].y - line[i - 1].y)) }
            let total = max(1e-6, lengths.last!)
            // Time: lognormal impulses for the movements between via-points (this copy's timing perturbed).
            var comps = SigmaLognormal.components(via: via, corners: corners, durations: durations?[safe: index], hand: hand, rng: &copy)
            comps = SigmaLognormal.perturb(comps, amount: variation, rng: &copy)
            // speedProfile integrates at 400 Hz and visits every component at each step. Reject invalid or
            // excessive recorded timing before entering that loop, including corrupt persisted recipes.
            guard let first = comps.first,
                  comps.allSatisfy({ $0.D.isFinite && $0.t0.isFinite && $0.mu.isFinite && $0.sigma.isFinite && $0.sigma > 0 }),
                  let end = comps.map({ $0.t0 + exp($0.mu + 3.2 * $0.sigma) }).max() else { return nil }
            let estimatedSteps = ceil((end - first.t0) * 400) + 1
            let timingWork = estimatedSteps * Double(comps.count)
            guard estimatedSteps.isFinite, estimatedSteps > 0, estimatedSteps <= Double(timedBudget),
                  timingWork <= timingWorkBudget else { return nil }
            timingWorkBudget -= timingWork
            let profile = SigmaLognormal.speedProfile(comps)
            guard profile.count <= timedBudget else { return nil }
            timedBudget -= profile.count
            let travel = profile.reduce(0) { $0 + $1.1 }
            guard travel > 0 else { continue }
            // Walk the line at the model's speed: distance travelled so far → position on the line.
            var samples: [PenSample] = []
            var done: CGFloat = 0
            var j = 0
            for (t, v) in profile {
                done += v
                let s = total * min(1, done / travel)
                while j < line.count - 2, lengths[j + 1] < s { j += 1 }
                let seg = max(1e-6, lengths[j + 1] - lengths[j]), u = (s - lengths[j]) / seg
                let p = CGPoint(x: line[j].x + (line[j + 1].x - line[j].x) * u, y: line[j].y + (line[j + 1].y - line[j].y) * u)
                let dir = atan2(line[j + 1].y - line[j].y, line[j + 1].x - line[j].x)
                samples.append(PenSample(p: p, t: t, speed: v / travel * total, direction: dir))
            }
            if samples.count > 1 { trajectories.append(samples) }
        }
        guard !trajectories.isEmpty else { return nil }
        let points = trajectories.flatMap { $0.map(\.p) }
        guard points.allSatisfy({ $0.x.isFinite && $0.y.isFinite }),
              let minX = points.map(\.x).min(), let maxX = points.map(\.x).max(),
              let minY = points.map(\.y).min(), let maxY = points.map(\.y).max(),
              maxX - minX + width * 8 < 6_000 * scale, maxY - minY + width * 8 < 4_000 * scale else { return nil }
        capture?(trajectories)   // measurement (SignatureBench): the pen's path and timing in pixels
        return PenRenderer.draw(trajectories, pen: pen, width: width, ink: shift(ink, &copy), rng: &copy, scale: scale)
    }

    /// The hand at speed: even spacing along the path (about one pixel), a short straight run-out where the pen
    /// lifts, and a low-pass filter over a few pen widths — fast movements carry no small wobbles.
    private static func fastHand(_ path: [CGPoint], smoothing: CGFloat, runOut: Bool = true, pointBudget: Int,
                                 scale: CGFloat = 1) -> [CGPoint]? {
        guard path.allSatisfy({ $0.x.isFinite && $0.y.isFinite }), smoothing.isFinite,
              smoothing >= 0, smoothing <= 1_024 * scale, pointBudget >= 2 else { return nil }
        guard path.count >= 2 else { return path }
        var p = path
        let a = p[p.count - 2], b = p[p.count - 1], d = max(0.001, hypot(b.x - a.x, b.y - a.y))
        if runOut { p.append(CGPoint(x: b.x + (b.x - a.x) / d * smoothing * 3, y: b.y + (b.y - a.y) / d * smoothing * 3)) }
        guard let minX = p.map(\.x).min(), let maxX = p.map(\.x).max(),
              let minY = p.map(\.y).min(), let maxY = p.map(\.y).max(),
              maxX - minX < 6_000 * scale, maxY - minY < 4_000 * scale else { return nil }
        var length: CGFloat = 0
        for i in 1..<p.count {
            length += hypot(p[i].x - p[i - 1].x, p[i].y - p[i - 1].y)
            guard length.isFinite, length <= CGFloat(pointBudget - 2) else { return nil }
        }
        var even: [CGPoint] = [p[0]]
        var carry: CGFloat = 0
        for i in 1..<p.count {
            let s = p[i - 1], e = p[i], len = hypot(e.x - s.x, e.y - s.y)
            var t = 1 - carry
            while t <= len {
                guard even.count < pointBudget - 1 else { return nil }
                even.append(CGPoint(x: s.x + (e.x - s.x) * t / len, y: s.y + (e.y - s.y) * t / len))
                t += 1
            }
            carry = len - (t - 1)
        }
        even.append(p.last!)
        let k = max(1, Int(smoothing))
        for _ in 0..<2 {
            let src = even
            even = src.indices.map { i in
                let r = max(0, i - k)...min(src.count - 1, i + k)
                let w = CGFloat(r.count)
                return CGPoint(x: src[r].reduce(0) { $0 + $1.x } / w, y: src[r].reduce(0) { $0 + $1.y } / w)
            }
        }
        return even
    }

    /// Centripetal Catmull-Rom spline through the via-points (no overshoot loops between uneven points).
    private static func splineThrough(_ p: [CGPoint], steps: Int = 22) -> [CGPoint] {
        guard p.count > 2 else { return p }
        var out: [CGPoint] = []
        for i in 0..<(p.count - 1) {
            let p0 = p[max(0, i - 1)], p1 = p[i], p2 = p[i + 1], p3 = p[min(p.count - 1, i + 2)]
            if p1 == p2 { continue }
            func knot(_ a: CGPoint, _ b: CGPoint) -> CGFloat { max(1e-4, pow(hypot(b.x - a.x, b.y - a.y), 0.5)) }
            let t1 = knot(p0, p1), t2 = t1 + knot(p1, p2), t3 = t2 + knot(p2, p3)
            for k in 0..<steps {
                let t = t1 + (t2 - t1) * CGFloat(k) / CGFloat(steps)
                func lerp(_ a: CGPoint, _ b: CGPoint, _ ta: CGFloat, _ tb: CGFloat) -> CGPoint {
                    let u = tb - ta < 1e-6 ? 0 : (t - ta) / (tb - ta)
                    return CGPoint(x: a.x + (b.x - a.x) * u, y: a.y + (b.y - a.y) * u)
                }
                let a1 = lerp(p0, p1, 0, t1), a2 = lerp(p1, p2, t1, t2), a3 = lerp(p2, p3, t2, t3)
                let b1 = lerp(a1, a2, 0, t2), b2 = lerp(a2, a3, t1, t3)
                out.append(lerp(b1, b2, t1, t2))
            }
        }
        out.append(p.last!)
        return out
    }

    /// The ink colour, nudged in brightness, saturation and hue for this copy.
    private static func shift(_ color: NSColor, _ copy: inout SeededRandom) -> NSColor {
        let c = color.usingColorSpace(.sRGB) ?? color
        var h: CGFloat = 0, s: CGFloat = 0, b: CGFloat = 0, a: CGFloat = 0
        c.getHue(&h, saturation: &s, brightness: &b, alpha: &a)
        return NSColor(hue: (h + copy.range(-0.01...0.01) + 1).truncatingRemainder(dividingBy: 1),
                       saturation: min(1, max(0, s * copy.range(0.92...1.08))),
                       brightness: min(1, max(0, b + copy.range(-0.05...0.02))), alpha: a)
    }
}
