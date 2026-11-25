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
        print("Signature regressions: \(checks - failures)/\(checks) passed")
        return failures == 0 ? 0 : 1
    }
}
