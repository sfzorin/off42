import AppKit
import CoreImage
import Vision

/// Explicit, model-dependent audit. Kept separate from the deterministic --selftest suite.
/// The fixture is generated locally; no downloaded source image or account is needed.
enum ImageVisionAudit {
    static func run(outputDirectory: URL? = nil) async -> Int {
        await Task.detached(priority: .userInitiated) { runChecks(outputDirectory: outputDirectory) }.value
    }

    private static func runChecks(outputDirectory: URL?) -> Int {
        var count = 0, failures = 0
        func check(_ passed: Bool, _ name: String) {
            count += 1
            if !passed { failures += 1 }
            print(passed ? "PASS" : "FAIL", "IMG Vision:", name)
            fflush(stdout)
        }
        let parent = outputDirectory
            ?? ProcessInfo.processInfo.environment["K42_UI_TEST_DIR"].map { URL(fileURLWithPath: $0).appendingPathComponent("vision") }
            ?? FileManager.default.temporaryDirectory.appendingPathComponent("IMG42-Vision-" + UUID().uuidString)
        do {
            try FileManager.default.createDirectory(at: parent, withIntermediateDirectories: true)
            let source = orangeFixture()
            let sourceURL = parent.appendingPathComponent("vision-source.png")
            try ImageWriter.write(source, to: sourceURL, type: "public.png")
            check(source.width == 768 && source.height == 768, "generated shaded subject fixture has the intended dimensions")
            print("IMG Vision fixture:", sourceURL.path)
            fflush(stdout)

            // This is the same production Vision request used by the Remove Background command.
            let removed: CGImage?
            do {
                removed = try PixelOps.removeBackground(source)
            } catch {
                let error = error as NSError
                check(false, "Remove Background backend failed: \(error.domain) / \(error.code): \(error.localizedDescription)")
                print("IMG42 Vision regressions: \(count - failures)/\(count) passed")
                return failures
            }
            check(removed != nil, "real Remove Background finds the generated foreground subject")
            if let removed {
                check(removed.width == source.width && removed.height == source.height, "Remove Background preserves the complete image dimensions")
                let rgba = ImagePixelAudit.pixels(removed)
                let alpha = stride(from: 3, to: rgba.count, by: 4).map { rgba[$0] }
                let transparent = alpha.filter { $0 <= 8 }.count
                let opaque = alpha.filter { $0 >= 240 }.count
                let partial = alpha.filter { $0 > 8 && $0 < 240 }.count
                print("IMG Vision alpha:", "transparent", transparent, "opaque", opaque, "partial", partial)
                check(transparent > alpha.count / 3 && opaque > alpha.count / 10 && partial > 0,
                      "removed image has transparent background, an opaque subject and a soft alpha edge")
                check(meanChannel(rgba, width: removed.width, rect: CGRect(x: 16, y: 16, width: 48, height: 48), channel: 3) < 8,
                      "the distant background corner becomes transparent")
                check(meanChannel(rgba, width: removed.width, rect: CGRect(x: 350, y: 300, width: 60, height: 60), channel: 3) > 235,
                      "the centre of the foreground subject remains opaque")
                let sourcePixels = ImagePixelAudit.pixels(source)
                let centre = CGRect(x: 350, y: 300, width: 60, height: 60)
                let channelErrors = (0..<3).map { channel in
                    abs(meanChannel(rgba, width: removed.width, rect: centre, channel: channel)
                        - meanChannel(sourcePixels, width: source.width, rect: centre, channel: channel))
                }
                check(channelErrors.allSatisfy { $0 < 20 }, "removing the background retains the subject's colour")
                let iou = sphereIoU(alpha.map { $0 > 127 }, width: removed.width, height: removed.height)
                print(String(format: "IMG Vision foreground IoU: %.4f", iou))
                check(iou > 0.80, "foreground alpha follows the generated subject silhouette (IoU > 0.80)")
                let resultURL = parent.appendingPathComponent("vision-background-removed.png")
                try ImageWriter.write(removed, to: resultURL, type: "public.png")
                let reopened = ImageLoader.load(resultURL).image
                check(reopened?.width == removed.width && reopened?.height == removed.height, "saved background-removal PNG reopens at its original size")
                if let reopened {
                    let reopenedPixels = ImagePixelAudit.pixels(reopened)
                    let reopenedAlpha = stride(from: 3, to: reopenedPixels.count, by: 4).map { reopenedPixels[$0] }
                    check(reopenedAlpha == alpha, "PNG output preserves the entire removed-background alpha channel")
                } else { check(false, "saved background-removal PNG contains readable pixels") }
                print("IMG Vision background removal:", resultURL.path)
            }

            // Exercise the actual production subject-mask cache and Vision model, not a hand-made mask.
            let subject = SmartLasso.subjectMask(for: source)
            check(subject != nil, "Smart Lasso obtains a real Vision foreground mask")
            if let subject {
                check(subject.width == source.width && subject.height == source.height, "Vision subject mask uses image-pixel dimensions")
                let subjectURL = parent.appendingPathComponent("vision-subject-mask.png")
                try ImageWriter.write(subject, to: subjectURL, type: "public.png")
                let size = CGSize(width: source.width, height: source.height)
                if let mask = SmartLassoEval.binary(subject, size) {
                    check(mask[330 * source.width + 380] && !mask[32 * source.width + 32], "Vision mask distinguishes the subject centre from the background")
                    let iou = sphereIoU(mask, width: source.width, height: source.height)
                    print(String(format: "IMG Vision mask IoU: %.4f", iou))
                    check(iou > 0.80, "real subject mask follows the known silhouette (IoU > 0.80)")
                } else { check(false, "Vision subject mask can be read") }
                let stroke: [CGPoint] = (0..<96).map { index in
                    let angle: Double = Double(index) * 2.0 * Double.pi / 96.0
                    return CGPoint(x: 384.0 + cos(angle) * 220.0, y: 334.0 + sin(angle) * 220.0)
                }
                if let outline = SmartLasso.refine(stroke: stroke, radius: 30, image: source, subject: subject),
                   let selection = PixelSelection.from(path: outline, size: size),
                   let selected = SmartLassoEval.binary(selection.mask, size) {
                    let iou = sphereIoU(selected, width: source.width, height: source.height)
                    print(String(format: "IMG Vision Smart Lasso IoU: %.4f", iou))
                    check(iou > 0.85, "Smart Lasso refines a stroke with the real Vision mask (IoU > 0.85)")
                    if let extracted = PixelOps.extract(selection, from: source) {
                        let extractedPixels = ImagePixelAudit.pixels(extracted)
                        let hasTransparent = stride(from: 3, to: extractedPixels.count, by: 4).contains { extractedPixels[$0] == 0 }
                        let hasOpaque = stride(from: 3, to: extractedPixels.count, by: 4).contains { extractedPixels[$0] == 255 }
                        check(extracted.width > 350 && extracted.height > 350 && hasTransparent && hasOpaque,
                              "Vision-assisted selection extracts the subject with a transparent outside")
                        try ImageWriter.write(extracted, to: parent.appendingPathComponent("vision-smart-lasso.png"), type: "public.png")
                    } else { check(false, "Vision-assisted selection can be extracted") }
                } else { check(false, "Smart Lasso produces an outline from the real Vision subject mask") }
                print("IMG Vision subject mask:", subjectURL.path)
            } else {
                // subjectMask's normal UI path deliberately returns nil for both no subject and Vision errors.
                // The audit reports the underlying error when available; neither outcome is counted as PASS.
                do {
                    let request = VNGenerateForegroundInstanceMaskRequest()
                    try VNImageRequestHandler(cgImage: source).perform([request])
                    print("IMG Vision diagnostic: foreground instance count", request.results?.first?.allInstances.count ?? 0)
                } catch {
                    let error = error as NSError
                    print("IMG Vision diagnostic:", error.domain, error.code, error.localizedDescription)
                }
            }
        } catch {
            let error = error as NSError
            check(false, "fixture/output failure: \(error.domain) / \(error.code): \(error.localizedDescription)")
        }
        print("IMG42 Vision regressions: \(count - failures)/\(count) passed")
        fflush(stdout)
        return failures
    }

    private static func meanChannel(_ rgba: [UInt8], width: Int, rect: CGRect, channel: Int) -> Double {
        var sum = 0.0, count = 0
        for y in Int(rect.minY)..<Int(rect.maxY) { for x in Int(rect.minX)..<Int(rect.maxX) {
            sum += Double(rgba[(y * width + x) * 4 + channel]); count += 1
        } }
        return sum / Double(max(1, count))
    }

    private static func sphereIoU(_ actual: [Bool], width: Int, height: Int) -> Double {
        var intersection = 0, union = 0
        for y in 0..<height { for x in 0..<width {
            let expected = hypot(Double(x) - 384.0, Double(y) - 334.0) <= 210.0
            let selected = actual[y * width + x]
            if expected && selected { intersection += 1 }
            if expected || selected { union += 1 }
        } }
        return union == 0 ? 0 : Double(intersection) / Double(union)
    }

    /// A softly lit, textured orange with a small leaf and contact shadow on a neutral background.
    /// Pixel generation keeps the fixture deterministic while providing a recognisable photographic subject.
    private static func orangeFixture() -> CGImage {
        let width = 768, height = 768
        var bytes = [UInt8](repeating: 255, count: width * height * 4)
        for y in 0..<height { for x in 0..<width {
            let dx = (Double(x) - 384.0) / 210.0
            let dy = (Double(y) - 334.0) / 210.0
            let distanceSquared = dx * dx + dy * dy
            let shadowX = (Double(x) - 405.0) / 225.0
            let shadowY = (Double(y) - 551.0) / 43.0
            let shadow = exp(-(shadowX * shadowX + shadowY * shadowY) * 2.0) * 0.26
            let background = 246.0 - Double(y) / 768.0 * 12.0
            var red = background * (1.0 - shadow)
            var green = (background - 1.0) * (1.0 - shadow)
            var blue = (background - 5.0) * (1.0 - shadow)
            if distanceSquared <= 1.0 {
                let dz = sqrt(max(0.0, 1.0 - distanceSquared))
                let diffuse = max(0.0, -0.38 * dx - 0.50 * dy + 0.78 * dz)
                let illumination = 0.40 + diffuse * 0.60
                let highlight = pow(max(0.0, -0.20 * dx - 0.28 * dy + 0.94 * dz), 48.0) * 42.0
                let hash = (x &* 73856093) ^ (y &* 19349663)
                let texture = Double(hash & 255) / 255.0 * 8.0 - 4.0
                let pores = sin(Double(x) * 2.4) * sin(Double(y) * 2.1) * 2.0
                red = 244.0 * illumination + highlight + texture + pores
                green = 139.0 * illumination + highlight * 0.85 + texture + pores
                blue = 24.0 * illumination + highlight * 0.6 + texture * 0.5
            }
            // A small green leaf and a stem help separate the fruit from a synthetic colour disk.
            let leafX = (Double(x) - 416.0) * 0.88 + (Double(y) - 132.0) * 0.47
            let leafY = -(Double(x) - 416.0) * 0.47 + (Double(y) - 132.0) * 0.88
            if (leafX * leafX) / 2500.0 + (leafY * leafY) / 256.0 <= 1.0 {
                let light = 0.80 + 0.20 * (1.0 - abs(leafY) / 16.0)
                red = 43.0 * light; green = 95.0 * light; blue = 30.0 * light
                if abs(leafY) < 1.5 { red += 22; green += 20 }
            }
            if x >= 379 && x <= 386 && y >= 112 && y <= 131 { red = 92; green = 64; blue = 33 }
            let offset = (y * width + x) * 4
            bytes[offset] = UInt8(max(0, min(255, red.rounded())))
            bytes[offset + 1] = UInt8(max(0, min(255, green.rounded())))
            bytes[offset + 2] = UInt8(max(0, min(255, blue.rounded())))
        } }
        return ImagePixelAudit.image(bytes, width: width, height: height)
    }
}
