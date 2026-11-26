import AppKit

/// Headless regressions for recorded signature input and work limits; no shared library files are modified.
public enum SignatureSelfTest {
    public static func run() -> Int32 {
        var checks = 0, failures = 0
        func check(_ value: Bool, _ message: String) {
            checks += 1
            if !value { failures += 1 }
            print(value ? "PASS" : "FAIL", "Signature:", message)
        }
        func sample(_ points: [[Double]]) -> SignatureSample { SignatureSample(strokes: [points]) }
        func render(_ sample: SignatureSample, height: CGFloat = 120) -> CGImage? {
            LearnedSignature(samples: [sample]).render(impression: 42, ink: .black, pen: .ballpoint, pixelHeight: height)
        }
        func hasInk(_ image: CGImage?) -> Bool {
            guard let image, image.width > 2, image.height > 2, image.width < 6_000, image.height < 4_000,
                  let context = InkDraw.context(image.width, image.height),
                  let data = context.data?.assumingMemoryBound(to: UInt8.self) else { return false }
            context.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
            return (0..<(image.width * image.height)).contains { data[$0 * 4 + 3] > 32 }
        }

        // A straight drag is valid input. Formerly height == 0 expanded this to billions of pixel samples.
        let horizontal = sample([[0, 0, 0, 0.5], [200, 0, 0.4, 0.5]])
        let normalized = SignatureLearning.normalized(horizontal).flatMap { $0 }
        check(normalized.count == 2 && normalized.allSatisfy { $0.p.x.isFinite && $0.p.y.isFinite && $0.p.x <= 32 },
              "horizontal recording has finite bounded geometry")
        check(hasInk(render(horizontal)), "horizontal two-point drag renders visible ink within bitmap limits")

        let nearlyFlat = sample([[0, 0, 0], [100, 0.000_001, 0.2], [200, 0, 0.4]])
        let flatPoints = SignatureLearning.normalized(nearlyFlat).flatMap { $0 }
        check(flatPoints.count == 3 && abs(flatPoints[1].p.y - 0.000_000_16) < 1e-12,
              "nearly horizontal input is uniformly scaled without changing its aspect ratio")
        check(hasInk(render(nearlyFlat)), "nearly horizontal recording renders without excessive resampling")

        let wide = sample([[0, 0, 0], [150, 10, 0.3], [300, 0, 0.6]])
        let widePoints = SignatureLearning.normalized(wide).flatMap { $0 }
        check(widePoints.count == 3 && widePoints[1].p.y == 1 && widePoints[2].p.x == 30,
              "ordinary wide signatures retain their original cap-height scale")
        check(hasInk(render(wide)), "a valid 30-to-1 wide signature still renders")

        let ordinary = sample([[0, 0, 0], [30, 80, 0.2], [60, 10, 0.4], [120, 60, 0.7], [200, 0, 1]])
        for pen in PenType.allCases {
            check(hasInk(LearnedSignature(samples: [ordinary]).render(impression: 7, ink: .black, pen: pen)),
                  "valid recorded signature renders with pen \(pen.rawValue)")
        }
        let data = try? JSONEncoder().encode(LearnedSignature(samples: [ordinary, wide]))
        let reopened = data.flatMap { try? JSONDecoder().decode(LearnedSignature.self, from: $0) }
        check(reopened.map { hasInk($0.render(impression: 11, ink: .black, pen: .gel)) } == true,
              "saved learned-signature recipe reopens and renders")

        var measured = ordinary; measured.penWidth = 0.04
        check(SignatureLearning.normalizedSample(measured)?.penWidth == 0.04,
              "normalization preserves measured pen width at the ordinary cap-height scale")
        let veryWide = SignatureSample(strokes: [[[0, 0, -1], [320, 10, -1], [640, 0, -1]]], penWidth: 0.04)
        check(SignatureLearning.normalizedSample(veryWide).map { abs(($0.penWidth ?? 0) - 0.02) < 1e-12 } == true,
              "a 64-to-1 recorded picture scales its measured pen width together with its geometry")
        var measuredFlat = horizontal; measuredFlat.penWidth = 0.03
        check(SignatureLearning.normalizedSample(measuredFlat).map { abs(($0.penWidth ?? 0) - 0.0048) < 1e-12 } == true
              && hasInk(render(measuredFlat)), "flat picture normalization preserves its finite measured-width scale and renders")
        func pixels(_ image: CGImage?) -> Data? {
            guard let data = image?.dataProvider?.data else { return nil }
            return data as Data
        }
        var fine = ordinary; fine.penWidth = 0.012
        check(hasInk(render(measured)) && pixels(render(measured)) != pixels(render(fine)),
              "measured pen widths change actual rendered pixels")
        let measuredData = try? JSONEncoder().encode(measured)
        let measuredReopened = measuredData.flatMap { try? JSONDecoder().decode(SignatureSample.self, from: $0) }
        check(measuredReopened?.penWidth == 0.04 && measuredReopened.map { hasInk(render($0)) } == true,
              "measured pen width survives the persisted sample round trip")
        let legacyData = Data("{\"strokes\":[[[0,0,0],[30,80,0.2],[200,0,1]]]}".utf8)
        let legacy = try? JSONDecoder().decode(SignatureSample.self, from: legacyData)
        check(legacy != nil && legacy?.penWidth == nil && legacy.map { hasInk(render($0)) } == true,
              "older samples without measured pen width remain readable and render")
        let invalidMeasured = SignatureSample(strokes: [[[]]], penWidth: 0.06)
        let mixed = LearnedSignature(samples: [invalidMeasured, measured])
            .render(impression: 42, ink: .black, pen: .ballpoint, pixelHeight: 120)
        check(hasInk(mixed) && pixels(mixed) == pixels(render(measured)),
              "discarded malformed samples cannot change the surviving sample's measured width")
        for width: Double in [0, -0.02, .nan, .infinity] {
            var invalidWidth = ordinary; invalidWidth.penWidth = width
            check(SignatureLearning.normalizedSample(invalidWidth) == nil && render(invalidWidth) == nil,
                  "invalid measured pen width is rejected with its sample")
        }

        let invalid: [SignatureSample] = [
            sample([[]]), sample([[0]]), sample([[0, .nan], [1, 1]]),
            sample([[0, 0, .infinity], [1, 1, 2]]),
            sample([[-Double.greatestFiniteMagnitude, 0], [Double.greatestFiniteMagnitude, 1]]),
            sample([[10, 10, 0], [10, 10, 1]]),
            sample(Array(repeating: [0, 0, -1], count: SignatureLearning.maximumSamplePoints + 1)),
        ]
        for (index, value) in invalid.enumerated() {
            check(SignatureLearning.normalized(value).isEmpty && render(value) == nil,
                  "malformed, degenerate or excessive recorded input is rejected: \(index)")
        }
        for height: CGFloat in [0, -1, .nan, .infinity, 10_000] {
            check(render(ordinary, height: height) == nil, "invalid or excessive requested render height is rejected")
        }
        check(LearnedSignature(samples: Array(repeating: ordinary, count: SignatureLearning.maximumSamples + 1))
            .render(impression: 1, ink: .black, pen: .ballpoint) == nil, "excessive learned sample count is rejected")
        check(render(sample([[0, 0, 0], [10, 10, 1_000_000], [20, 0, 2_000_000]])) == nil,
              "excessive recorded timing is rejected before integrating the speed profile")

        var random = SeededRandom(123)
        let hand = SigmaLognormal.Hand()
        let huge = SignatureGenerator.draw(strokes: [[.zero, CGPoint(x: 1e12, y: 1)]], hand: hand,
                                            slant: 0, penRatio: 0.024, pen: .ballpoint, ink: .black,
                                            pixelHeight: 120, copy: &random)
        check(huge == nil, "oversized spatial geometry is rejected before allocating per-pixel samples")
        let zigzag = (0..<128).map { CGPoint(x: $0.isMultiple(of: 2) ? 0 : 30, y: CGFloat($0) / 128) }
        let excessiveLength = SignatureGenerator.draw(strokes: [zigzag], hand: hand, slant: 0, penRatio: 0.024,
                                                       pen: .ballpoint, ink: .black, pixelHeight: 120, copy: &random)
        check(excessiveLength == nil, "a bounded box with excessive path length cannot exhaust the sampling budget")

        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("SignatureSelfTest-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: folder) }
        do {
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            let keyURL = folder.appendingPathComponent("key.json"), answersURL = folder.appendingPathComponent("answers.txt")
            let key = SignatureBlindTest.Key(cells: [true, false, true], sources: ["one", "one", "one"])
            try JSONEncoder().encode(key).write(to: keyURL)
            try " # Reviewer comment\n1, 3\n".write(to: answersURL, atomically: true, encoding: .utf8)
            let complete = try SignatureBlindTest.score(key: keyURL, answers: answersURL)
            check(complete.contains("3 of 3 correct (100 %)"), "blind-test scoring accepts valid cell numbers and comment lines")
            try "# All cells judged real\n".write(to: answersURL, atomically: true, encoding: .utf8)
            let unmarked = try SignatureBlindTest.score(key: keyURL, answers: answersURL)
            check(unmarked.contains("1 of 3 correct"), "an empty answer selection remains a valid all-real judgment")
            for answer in [String(Int.min), String(Int.max), "0", "4", "word", "1.5", "9999999999999999999999999", "1, word"] {
                try answer.write(to: answersURL, atomically: true, encoding: .utf8)
                do {
                    _ = try SignatureBlindTest.score(key: keyURL, answers: answersURL)
                    check(false, "blind-test scoring rejects an invalid answer before index subtraction: \(answer)")
                } catch {
                    let problem = error as NSError
                    check(problem.domain == "SignatureBlindTest" && problem.code == 3,
                          "blind-test scoring rejects an invalid answer before index subtraction: \(answer)")
                }
            }
        } catch { check(false, "blind-test answer fixture: \(error)") }
        // Which designs are offered (2026-11-06: people rejected crossed-out, oval-wrapped and tangled signatures).
        func design(_ a: SignatureGenerator.Archetype, flourish: Int, seed: UInt64) -> UInt64 {
            (seed << 16) | UInt64(SignatureGenerator.styleIndex(a, flourish: flourish, join: 0, capital: 0))
        }
        let names = ["Emma Clarke", "Анна Соколова", "Jonas Weber", "Дмитрий Орлов"]
        var plainWins = 0, comparisons = 0
        for name in names {
            for seed: UInt64 in [11, 23, 37, 51] {
                for a in [SignatureGenerator.Archetype.full, .initialSurname, .surname] {
                    let plain = SignatureGenerator.quality(name, design: design(a, flourish: 1, seed: seed))
                    for bad in [5, 4] {
                        comparisons += 1
                        if plain > SignatureGenerator.quality(name, design: design(a, flourish: bad, seed: seed)) { plainWins += 1 }
                    }
                }
                comparisons += 1
                if SignatureGenerator.quality(name, design: design(.initialSurname, flourish: 1, seed: seed))
                    > SignatureGenerator.quality(name, design: design(.enclosed, flourish: 0, seed: seed)) { plainWins += 1 }
            }
        }
        check(plainWins == comparisons, "a crossed-out or oval-wrapped design scores below the same plain design (\(plainWins)/\(comparisons))")
        let pool = (0..<24).map { i -> UInt64 in
            let archetypes = SignatureGenerator.Archetype.allCases
            return design(archetypes[i % archetypes.count], flourish: (i * 7) % SignatureGenerator.flourishes, seed: UInt64(101 + i))
        }
        var offeredBad = 0
        for name in names {
            let offered = SignatureGenerator.bestDesigns(name, count: 3, form: .any, candidates: pool)
            offeredBad += offered.filter { d in
                let s = SignatureGenerator.style(d)
                return [4, 5].contains(s.flourish) || s.archetype == .enclosed || SignatureGenerator.quality(name, design: d) < 0.6
            }.count
            check(offered.count == 3 && Set(offered).count == 3, "three different designs offered for \(name)")
        }
        check(offeredBad == 0, "best-of-pool never offers crossed-out, oval-wrapped or low-scoring designs (\(offeredBad) found)")
        let chosen = design(.initialSurname, flourish: 3, seed: 77)
        let near = SignatureGenerator.variations(of: chosen, count: 6)
        check(near.count == 6 && near.allSatisfy { $0 & 0xFFFF == chosen & 0xFFFF && $0 != chosen },
              "More Like This keeps the plan, flourish, capital and join and changes the hand")

        // Paraph: same hand (seed, capital, join), initial and short scribble, plain finish; saved designs unchanged.
        var shorter = 0
        for (i, name) in names.enumerated() {
            let d = pool[i * 3 % pool.count], p = SignatureGenerator.paraph(of: d)
            let sd = SignatureGenerator.style(d), sp = SignatureGenerator.style(p)
            check(p >> 16 == d >> 16 && sp.archetype == .initialsScribble && sp.capital == sd.capital && sp.join == sd.join
                  && [1, 2, 3].contains(sp.flourish) && sp.slant == sd.slant && sp.xh == sd.xh
                  && SignatureGenerator.isParaph(p) && !SignatureGenerator.isParaph(d) && SignatureGenerator.paraph(of: p) == p,
                  "paraph of \(name) keeps the hand and capital and writes the initial and a short scribble")
            if let full = SignatureGenerator.render(name, design: d, impression: 5, ink: .black),
               let short = SignatureGenerator.render(name, design: p, impression: 5, ink: .black),
               short.width < full.width || sd.archetype == .initialsScribble || sd.archetype == .capitalScribble || sd.archetype == .monogram {
                shorter += 1
            }
        }
        check(shorter == names.count, "paraphs are shorter than the full signatures (\(shorter)/\(names.count))")
        check(SignatureGenerator.designSeeds(200, form: .any).allSatisfy { !SignatureGenerator.isParaph($0) },
              "new signature designs never carry the paraph flag")
        // Every insertion is a new copy: different pixels, the same size within a few per cent.
        if let a = SignatureGenerator.render("Emma Clarke", design: chosen, impression: 1, ink: .black),
           let b = SignatureGenerator.render("Emma Clarke", design: chosen, impression: 2, ink: .black) {
            let same = a.dataProvider?.data as Data? == b.dataProvider?.data as Data?
            check(!same && abs(Double(a.width) / Double(b.width) - 1) < 0.2, "two insertions of one signature differ slightly")
        } else { check(false, "two insertions of one signature render") }

        // Names in scripts the pen alphabet cannot write.
        for name in ["Emma Clarke", "Анна Соколова", "Nguyễn Thu Hà", "Elif Yıldız", "Agnieszka Wójcik", "Kovács Eszter", "Łukasz Żak"] {
            check(SignatureGenerator.script(of: name) == .writable && SignatureGenerator.writableName(name) == name, "\(name) is written as typed")
        }
        check(SignatureGenerator.writableName("林佳穎") == "Lin Jiaying" && SignatureGenerator.writableName("王伟") == "Wang Wei",
              "Chinese names are written in pinyin, surname first and the given name as one word")
        check(SignatureGenerator.personalSeals(for: "林佳穎").first?.values["chars"] == "林佳穎印"
              && SignatureGenerator.personalSeals(for: "王伟").first?.values["chars"] == "王伟之印", "Chinese names get a personal seal")
        check(SignatureGenerator.script(of: "中村 美咲") == .japanese && SignatureGenerator.writableName("中村 美咲") == nil
              && SignatureGenerator.personalSeals(for: "中村 美咲").count == 3 && SignatureGenerator.script(of: "さくら") == .japanese,
              "Japanese names get hanko seals and no guessed romanisation")
        check(SignatureGenerator.writableName("김서연") == "Kim Seoyeon" && SignatureGenerator.writableName("박 지훈") == "Park Jihun"
              && SignatureGenerator.personalSeals(for: "김서연").count == 2, "Korean names: customary surname spelling and personal seals")
        check(SignatureGenerator.writableName("Ελένη Παππά") == "Eleni Pappa", "Greek names follow the UN/ELOT transliteration")
        check(SignatureGenerator.writableName("สมชาย ใจดี") == nil && SignatureGenerator.writableName("محمد أحمد") == nil
              && SignatureGenerator.personalSeals(for: "สมชาย ใจดี").isEmpty, "Thai and Arabic names are not guessed: the user types them in Latin")

        print("Signature regressions: \(checks - failures)/\(checks) passed")
        return failures == 0 ? 0 : 1
    }
}
