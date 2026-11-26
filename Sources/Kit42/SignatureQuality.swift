import AppKit

/// Which generated designs to show: a quality score, best-of-N choice, nearby variations, and names in scripts the pen
/// alphabet cannot write (transliteration, East Asian personal seals).
///
/// The score works on the pen plan (cap-height units, before rendering), so ranking a pool of designs is cheap.
/// It was calibrated on contact sheets reviewed by eye: the designs people rejected were the ones that cross the name
/// out, wrap it in an oval, or turn into a tangle of loops; the ones they kept were legible and calm.
extension SignatureGenerator {
    /// 0…1, higher is better. Deterministic for a name and design.
    public static func quality(_ name: String, design: UInt64) -> Double {
        let words = name.split(whereSeparator: { $0.isWhitespace || $0 == "." }).map(String.init)
        guard !words.isEmpty else { return 0 }
        let s = style(design)
        var copy = SeededRandom(design &+ 1)
        let strokes = plan(words, s, &copy)
        var score = 1.0
        // Flourishes that read as crossing the name out or boxing it in.
        let flourishPenalty: [Int: Double] = [5: 0.6, 99: 0.55, 4: 0.45, 9: 0.25, 6: 0.2, 7: 0.15]
        score -= flourishPenalty[s.archetype == .enclosed ? 99 : s.flourish] ?? 0
        // Scribble plans are legitimate but rarely what people want offered first.
        switch s.archetype {
        case .initialsScribble, .capitalScribble: score -= 0.3
        case .monogram: score -= 0.35
        case .enclosed: score -= 0.1
        // Fading plans write only the first letters and trail off: fine, but less legible than the whole word.
        case .fullFading, .initialSurnameFading, .surnameFading: score -= 0.08
        default: break
        }
        // Tangles: crossings of the pen path with itself, per written letter.
        let letters = max(1, words.joined().count)
        let crossings = Double(selfCrossings(strokes))
        score -= min(0.4, max(0, crossings / Double(letters) - 0.35) * 0.35)
        // Proportions: very short and tall or very long and flat signatures look odd.
        let all = strokes.flatMap { $0 }
        if let minX = all.map(\.x).min(), let maxX = all.map(\.x).max(), let minY = all.map(\.y).min(), let maxY = all.map(\.y).max(),
           maxY > minY {
            let aspect = Double((maxX - minX) / (maxY - minY))
            if aspect < 1.8 { score -= (1.8 - aspect) * 0.25 }
            if aspect > 9 { score -= min(0.3, (aspect - 9) * 0.05) }
        }
        // Small lowercase under a big capital stops being legible.
        if s.xh < 0.3 { score -= Double(0.3 - s.xh) * 2 }
        return max(0, min(1, score))
    }

    /// `count` designs for the form: the best of a pool of `pool` random designs, preferring different plans and
    /// flourishes so the choice is real. With `around`, the pool holds variations of that design instead.
    public static func bestDesigns(_ name: String, count: Int, form: SignatureForm, pool: Int = 18, around: UInt64? = nil,
                                   candidates given: [UInt64]? = nil) -> [UInt64] {
        let candidates = given ?? around.map { variations(of: $0, count: pool) } ?? designSeeds(pool, form: form)
        let ranked = candidates.map { ($0, quality(name, design: $0)) }.sorted { $0.1 > $1.1 }
        var chosen: [UInt64] = [], kinds = Set<Int>()
        // Any form: the best full name, initial and surname, and surname first (in that order), then the rest.
        if around == nil, form == .any {
            for group in [0, 1, 2] where chosen.count < count {
                if let (design, q) = ranked.first(where: { formGroup(style($0.0).archetype) == group }), q >= 0.6 {
                    chosen.append(design)
                    let s = style(design); kinds.insert(s.archetype.rawValue * 100 + s.flourish)
                }
            }
        }
        for (design, _) in ranked where chosen.count < count && !chosen.contains(design) {
            let s = style(design), kind = s.archetype.rawValue * 100 + s.flourish
            if around == nil, kinds.contains(kind) { continue }
            kinds.insert(kind)
            chosen.append(design)
        }
        for (design, _) in ranked where chosen.count < count && !chosen.contains(design) { chosen.append(design) }
        return chosen
    }

    private static func formGroup(_ a: Archetype) -> Int {
        switch a {
        case .full, .fullFading: return 0
        case .initialSurname, .initialSurnameFading, .enclosed: return 1
        case .surname, .surnameFading: return 2
        default: return 3
        }
    }

    /// Designs close to `design`: the same plan, flourish, capital and join (the skeleton of the signature), with a
    /// hand whose slant, proportions and rhythm are nearest to the original among many fresh draws.
    public static func variations(of design: UInt64, count: Int) -> [UInt64] {
        let styleBits = design & 0xFFFF, base = style(design)
        func distance(_ s: Style) -> CGFloat {
            abs(s.slant - base.slant) / 0.48 + abs(s.xh - base.xh) / 0.2 + abs(s.letterW - base.letterW) / 0.45
                + abs(s.capW - base.capW) / 0.4 + abs(s.rise - base.rise) / 0.15 + abs(s.initialScale - base.initialScale) / 0.3
                + (s.garland == base.garland ? 0 : 0.4) + (s.loops == base.loops ? 0 : 0.4)
        }
        let draws = (0..<count * 6).map { _ in (UInt64.random(in: 1...(UInt64.max >> 16)) << 16) | styleBits }
        return draws.map { ($0, distance(style($0))) }.sorted { $0.1 < $1.1 }.prefix(count).map(\.0)
    }

    /// Pen-path crossings with itself, ignoring neighbouring segments of the same stroke.
    static func selfCrossings(_ strokes: [[CGPoint]]) -> Int {
        var segments: [(CGPoint, CGPoint, Int, Int)] = []
        for (s, stroke) in strokes.enumerated() where stroke.count > 1 {
            for i in 1..<stroke.count { segments.append((stroke[i - 1], stroke[i], s, i)) }
        }
        func cross(_ o: CGPoint, _ a: CGPoint, _ b: CGPoint) -> CGFloat { (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x) }
        var count = 0
        for i in segments.indices {
            for j in (i + 1)..<segments.count {
                let a = segments[i], b = segments[j]
                if a.2 == b.2 && abs(a.3 - b.3) < 2 { continue }
                let d1 = cross(b.0, b.1, a.0), d2 = cross(b.0, b.1, a.1), d3 = cross(a.0, a.1, b.0), d4 = cross(a.0, a.1, b.1)
                if (d1 > 0) != (d2 > 0), (d3 > 0) != (d4 > 0), d1 != 0, d2 != 0, d3 != 0, d4 != 0 { count += 1 }
            }
        }
        return count
    }

    // MARK: - Scripts the pen alphabet cannot write

    /// How a name can be signed.
    public enum NameScript: Equatable {
        /// Latin, Cyrillic and their accented letters: written as typed.
        case writable
        /// Chinese: a personal seal, and the name in pinyin.
        case chinese
        /// Japanese: personal seals (hanko) only — the reading of kanji names cannot be derived from the characters.
        case japanese
        /// Korean: a personal seal, and the name romanised.
        case korean
        /// Greek, Armenian, Georgian: reliably transliterated.
        case transliterated
        /// Thai, Arabic, Hebrew, Indic and other scripts: automatic transliteration is too unreliable for a
        /// signature, so the user is asked to type the name in Latin letters.
        case typeInLatin
    }

    public static func script(of name: String) -> NameScript {
        if canWrite(name) { return .writable }
        let scalars = name.unicodeScalars.map(\.value)
        func any(_ ranges: [ClosedRange<UInt32>]) -> Bool { scalars.contains { v in ranges.contains { $0.contains(v) } } }
        if any([0xAC00...0xD7AF, 0x1100...0x11FF, 0x3130...0x318F]) { return .korean }
        if any([0x3040...0x30FF]) { return .japanese }
        if any([0x4E00...0x9FFF, 0x3400...0x4DBF]) {
            // Japanese names are written with a space between surname and given name; Chinese names without one.
            return name.trimmingCharacters(in: .whitespaces).contains(" ") ? .japanese : .chinese
        }
        let other = name.unicodeScalars.filter { $0.properties.isAlphabetic && !PenAlphabet.supports(Character($0)) }
        let reliable: [ClosedRange<UInt32>] = [0x0370...0x03FF, 0x1F00...0x1FFF, 0x0530...0x058F, 0x10A0...0x10FF]
        if !other.isEmpty, other.allSatisfy({ scalar in reliable.contains { $0.contains(scalar.value) } }) {
            return .transliterated
        }
        return .typeInLatin
    }

    /// Whether every letter of the name has a hand-drawn form.
    public static func canWrite(_ name: String) -> Bool { name.allSatisfy(PenAlphabet.supports) && name.contains(where: \.isLetter) }

    /// The name as the pen writes it, or nil when it should not be guessed (Japanese, Thai, Arabic…).
    public static func writableName(_ name: String) -> String? {
        let trimmed = name.trimmingCharacters(in: .whitespaces)
        func capitalised(_ s: Substring) -> String { s.prefix(1).uppercased() + s.dropFirst().lowercased() }
        func latin(_ s: String) -> String {
            let t = s.applyingTransform(.toLatin, reverse: false) ?? s
            return t.applyingTransform(.stripDiacritics, reverse: false) ?? t
        }
        switch script(of: trimmed) {
        case .writable:
            return trimmed
        case .chinese:
            // Surname first, given name as one word: 林佳穎 → Lin Jiaying.
            let syllables = trimmed.map { latin(String($0)) }.filter { !$0.isEmpty }
            guard let first = syllables.first else { return nil }
            let given = syllables.dropFirst().joined()
            let result = capitalised(Substring(first)) + (given.isEmpty ? "" : " " + capitalised(Substring(given)))
            return canWrite(result) ? result : nil
        case .korean:
            // Surname (first syllable unless spaced) in its customary spelling, given name as one word.
            let parts = trimmed.split(separator: " ")
            let surname = parts.count > 1 ? String(parts[0]) : String(trimmed.prefix(1))
            let given = parts.count > 1 ? parts.dropFirst().joined() : String(trimmed.dropFirst())
            let customary = koreanSurnames[surname] ?? capitalised(Substring(latin(surname)))
            let result = customary + (given.isEmpty ? "" : " " + capitalised(Substring(latin(given).replacingOccurrences(of: " ", with: ""))))
            return canWrite(result) ? result : nil
        case .transliterated:
            // Modern Greek by the UN/ELOT rules (Ελένη → Eleni); Armenian and Georgian by ICU's default.
            func word(_ w: String) -> String {
                let t = w.applyingTransform(StringTransform("Greek-Latin/UNGEGN"), reverse: false) ?? latin(w)
                return t.applyingTransform(.stripDiacritics, reverse: false) ?? t
            }
            let result = trimmed.split(separator: " ").map { capitalised(Substring(word(String($0)))) }.joined(separator: " ")
            return canWrite(result) ? result : nil
        case .japanese, .typeInLatin:
            return nil
        }
    }

    /// Customary romanisation of the most common Korean surnames (the Revised Romanization gives Gim, I, Bak…).
    private static let koreanSurnames: [String: String] = [
        "김": "Kim", "이": "Lee", "박": "Park", "최": "Choi", "정": "Jung", "강": "Kang", "조": "Cho", "윤": "Yoon", "장": "Jang",
        "임": "Lim", "한": "Han", "오": "Oh", "서": "Seo", "신": "Shin", "권": "Kwon", "황": "Hwang", "안": "Ahn", "송": "Song",
        "류": "Ryu", "유": "Yoo", "홍": "Hong", "전": "Jeon", "고": "Ko", "문": "Moon", "양": "Yang", "손": "Son", "배": "Bae",
        "백": "Baek", "허": "Heo", "노": "Noh", "남": "Nam", "심": "Shim", "하": "Ha", "곽": "Kwak", "성": "Sung", "차": "Cha",
    ]

    /// Personal seals for Chinese, Japanese and Korean names (印章, 判子, 도장), most usual first.
    public static func personalSeals(for name: String) -> [SealSpec] {
        let trimmed = name.trimmingCharacters(in: .whitespaces), compact = trimmed.replacingOccurrences(of: " ", with: "")
        guard compact.count <= 6 else { return [] }
        func spec(_ id: String, _ values: [String: String]) -> SealSpec? {
            SealTemplate.named(id).map { SealSpec(template: $0, values: values, ink: $0.ink.stampPad, designSeed: 1) }
        }
        switch script(of: trimmed) {
        case .chinese:
            return [spec("cn.person", ["chars": compact.count >= 3 ? compact + "印" : compact + "之印"])].compactMap { $0 }
        case .japanese:
            let surname = String(trimmed.split(separator: " ").first ?? Substring(compact.prefix(2)))
            return [spec("jp.hanko", ["chars": surname]), spec("jp.hanko.oval", ["chars": surname]),
                    spec("jp.hanko", ["chars": compact])].compactMap { $0 }
        case .korean:
            // A round dojang with the name large in the middle, and a square one ending in 인 ("seal").
            return [spec("jp.hanko", ["chars": compact]), spec("cn.person", ["chars": compact.count >= 3 ? compact + "인" : compact + "의인"])].compactMap { $0 }
        default:
            return []
        }
    }
}

/// `PDF42 --signature-ranking "Name" OUT.png [pool]`: a pool of random designs sorted by quality (score under each), with
/// the three the generator would offer outlined — for reviewing the ranking by eye.
extension SignatureGenerator {
    /// The paraph of a signature design: the short mark for initialling each page. It is the same hand, because slant,
    /// proportions, rhythm, the capital and how it joins all come from the same design seed; only what is written
    /// changes, to the initial and a short scribble of the surname, with a plain finish (no strikes or loops).
    public static func paraph(of design: UInt64) -> UInt64 {
        let index = Int(design & styleMask) % styleCount
        let flourish = (index / (joins * capitalVariants)) % flourishes
        let plain = [1, 2, 3].contains(flourish) ? flourish : ((design >> 16) & 1 == 0 ? 3 : 1)
        let style = styleIndex(.initialsScribble, flourish: plain, join: index % joins, capital: (index / joins) % capitalVariants)
        return (design & ~UInt64(0xFFFF)) | paraphFlag | UInt64(style)
    }

    /// Whether a design is a paraph.
    public static func isParaph(_ design: UInt64) -> Bool { design & paraphFlag != 0 }
}

public enum SignatureRankingSheet {
    /// Review sheet: each offered signature followed by three copies of its paraph.
    public static func writeParaphs(name: String, to url: URL, count: Int = 6) throws {
        let designs = SignatureGenerator.bestDesigns(name, count: count, form: .any)
        let cols = 4, cellW = 300, cellH = 120, W = cols * cellW, H = designs.count * cellH
        guard !designs.isEmpty, let ctx = CGContext(data: nil, width: W, height: H, bitsPerComponent: 8, bytesPerRow: 0,
                                  space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue) else { return }
        ctx.setFillColor(.white); ctx.fill(CGRect(x: 0, y: 0, width: W, height: H))
        for (row, design) in designs.enumerated() {
            for col in 0..<cols {
                let x = CGFloat(col * cellW), y = CGFloat(H - (row + 1) * cellH)
                let d = col == 0 ? design : SignatureGenerator.paraph(of: design)
                // Paraphs are drawn smaller, as on a page margin.
                let height: CGFloat = col == 0 ? 130 : 100
                guard let image = SignatureGenerator.render(name, design: d, impression: d &+ UInt64(col * 977 + 1), ink: InkColor.blue.pen,
                                                            pen: .ballpoint, pixelHeight: height) else { continue }
                let k = min(CGFloat(cellW - 20) / CGFloat(image.width), CGFloat(cellH - 12) / CGFloat(image.height), col == 0 ? 1 : 0.77)
                let w = CGFloat(image.width) * k, h = CGFloat(image.height) * k
                ctx.draw(image, in: CGRect(x: x + (CGFloat(cellW) - w) / 2, y: y + (CGFloat(cellH) - h) / 2, width: w, height: h))
            }
        }
        try NSBitmapImageRep(cgImage: ctx.makeImage()!).representation(using: .png, properties: [:])!.write(to: url)
    }

    public static func write(name: String, to url: URL, pool: Int = 30) throws {
        let designs = SignatureGenerator.designSeeds(pool, form: .any)
        let ranked = designs.map { ($0, SignatureGenerator.quality(name, design: $0)) }.sorted { $0.1 > $1.1 }
        let offered = Set(SignatureGenerator.bestDesigns(name, count: 3, form: .any, pool: pool, around: nil, candidates: designs))
        let cols = 6, cellW = 300, cellH = 120, rows = (ranked.count + cols - 1) / cols
        let W = cols * cellW, H = rows * cellH
        guard let ctx = CGContext(data: nil, width: W, height: H, bitsPerComponent: 8, bytesPerRow: 0,
                                  space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue) else { return }
        ctx.setFillColor(.white); ctx.fill(CGRect(x: 0, y: 0, width: W, height: H))
        for (i, (design, q)) in ranked.enumerated() {
            let x = CGFloat(i % cols * cellW), y = CGFloat(H - (i / cols + 1) * cellH)
            let cell = CGRect(x: x, y: y, width: CGFloat(cellW), height: CGFloat(cellH))
            if offered.contains(design) {
                ctx.setStrokeColor(NSColor.systemBlue.cgColor); ctx.setLineWidth(4); ctx.stroke(cell.insetBy(dx: 2, dy: 2))
            } else {
                ctx.setStrokeColor(NSColor(white: 0.85, alpha: 1).cgColor); ctx.setLineWidth(1); ctx.stroke(cell)
            }
            let label = String(format: "#%d  %.2f", i + 1, q)
            let line = CTLineCreateWithAttributedString(NSAttributedString(string: label, attributes: [.font: NSFont.systemFont(ofSize: 11), .foregroundColor: NSColor.darkGray]))
            ctx.textPosition = CGPoint(x: x + 6, y: y + 6); CTLineDraw(line, ctx)
            if let image = SignatureGenerator.render(name, design: design, impression: design &+ 1, ink: InkColor.blue.pen, pen: .ballpoint) {
                let k = min(CGFloat(cellW - 20) / CGFloat(image.width), CGFloat(cellH - 28) / CGFloat(image.height))
                let w = CGFloat(image.width) * k, h = CGFloat(image.height) * k
                ctx.draw(image, in: CGRect(x: x + (CGFloat(cellW) - w) / 2, y: y + 20 + (CGFloat(cellH - 20) - h) / 2, width: w, height: h))
            }
        }
        try NSBitmapImageRep(cgImage: ctx.makeImage()!).representation(using: .png, properties: [:])!.write(to: url)
    }
}
