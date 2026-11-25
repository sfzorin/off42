import AppKit
import Foundation

/// Explicit real-Vision checks on generated fixtures. This does not exercise scanner hardware or Continuity Camera.
enum ScanVisionAudit {
    @MainActor static func run(outputDirectory: URL? = nil) async -> Int {
        guard let document = TestImages.feederPage(1, skew: 0, dpi: 100),
              let glass = TestImages.documentOnGlass(angle: 5), let photos = TestImages.photosOnGlass(),
              let blank = TestImages.blankPage() else {
            print("FAIL SCN Vision: fixture creation")
            return 1
        }
        let folder = outputDirectory
            ?? ProcessInfo.processInfo.environment["K42_UI_TEST_DIR"].map { URL(fileURLWithPath: $0).appendingPathComponent("vision") }
            ?? FileManager.default.temporaryDirectory.appendingPathComponent("SCN42-Vision-" + UUID().uuidString)
        return await Task.detached(priority: .userInitiated) {
            runChecks(document: document, glass: glass, photos: photos, blank: blank, folder: folder)
        }.value
    }

    private static func runChecks(document: CGImage, glass: CGImage, photos: CGImage, blank: CGImage, folder: URL) -> Int {
        var count = 0, failures = 0
        func check(_ value: Bool, _ name: String) {
            count += 1; if !value { failures += 1 }
            print(value ? "PASS" : "FAIL", "SCN Vision:", name); fflush(stdout)
        }
        func save(_ image: CGImage, _ name: String) throws { try TestImages.png(image, dpi: 100).write(to: folder.appendingPathComponent(name)) }
        func knownQuad(center: CGPoint, size: CGSize, degrees: Double) -> Quad {
            let angle = degrees * .pi / 180
            let cx = Double(center.x), cy = Double(center.y), width = Double(size.width), height = Double(size.height)
            func point(_ x: Double, _ y: Double) -> CGPoint {
                CGPoint(x: cx + cos(angle) * x - sin(angle) * y, y: cy + sin(angle) * x + cos(angle) * y)
            }
            return Quad(tl: point(-width / 2, -height / 2), tr: point(width / 2, -height / 2),
                        br: point(width / 2, height / 2), bl: point(-width / 2, height / 2))
        }
        do {
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            try save(document, "document-upright.png"); try save(glass, "glass-document.png"); try save(photos, "glass-photos.png")
            let lines = try OCR.recognizeChecked(document)
            let recognized = OCR.text(lines)
            print("SCN Vision OCR:", recognized.prefix(400))
            check(lines.count >= 8 && recognized.lowercased().contains("invoice") && recognized.contains("4217"),
                  "real OCR reads the generated invoice and multiple text lines")
            check(lines.allSatisfy { $0.box.minX >= 0 && $0.box.minY >= 0 && $0.box.maxX <= 1.001 && $0.box.maxY <= 1.001 && $0.box.width > 0 && $0.box.height > 0 },
                  "real OCR returns usable normalized text geometry")
            check(OCR.title(lines)?.lowercased().contains("invoice") == true, "real OCR supplies the expected automatic filename title")
            check(try OCR.recognizeChecked(blank).isEmpty, "real OCR does not invent text on a noisy blank page")

            for originalTurn in [0, 90, 180, 270] {
                guard let rotated = Processor.rotate90(document, clockwise: originalTurn) else { check(false, "rotated text fixture exists"); continue }
                let required = (360 - originalTurn) % 360
                let turn = Processor.uprightRotation(rotated)
                print("SCN Vision orientation: source", originalTurn, "required", required, "detected", turn)
                check(turn == required, "auto-orientation chooses the needed clockwise correction: source \(originalTurn)")
                var options = ProcessOptions(); options.crop = false; options.deskew = false; options.enhancement = .none; options.dpi = 100
                let (corrected, report) = try Processor.processChecked(rotated, options)
                try save(corrected, "upright-from-\(originalTurn).png")
                let correctedText = OCR.text(try OCR.recognizeChecked(corrected))
                check(report.rotation == required && corrected.width == document.width && corrected.height == document.height
                      && correctedText.lowercased().contains("invoice") && correctedText.contains("4217"),
                      "auto-orientation pipeline outputs readable portrait pixels: source \(originalTurn)")
            }

            let expectedDocument = knownQuad(center: CGPoint(x: Double(glass.width) / 2 + 10, y: Double(glass.height) / 2 + 20),
                                             size: CGSize(width: 8.27 * 200, height: 11.69 * 200), degrees: 5)
            if let found = Processor.detectDocument(glass) {
                let overlap = found.quad.iou(expectedDocument)
                print("SCN Vision document:", found.method, "IoU", overlap, "angle", found.quad.angle)
                check(found.quad.isValid && overlap > 0.9 && abs(found.quad.angle - 5) < 1,
                      "document detection locates the known tilted paper on glass")
                if let crop = Processor.perspectiveCrop(glass, found.quad) {
                    try save(crop, "glass-document-cropped.png")
                    check(abs(Double(crop.width) / Double(crop.height) - 8.27 / 11.69) < 0.03,
                          "perspective correction restores the original paper aspect ratio")
                    let text = OCR.text(try OCR.recognizeChecked(crop))
                    check(text.lowercased().contains("invoice") && text.contains("4217"), "cropped and straightened output retains readable original text")
                } else { check(false, "detected paper can be perspective-cropped") }
            } else { check(false, "document detector finds the known paper") }

            let detected = Processor.detectPhotos(photos)
            print("SCN Vision photos: glass", detected.glassCount, "Vision", detected.visionCount, "combined", detected.quads.count)
            check(detected.quads.count == 3, "photo detection returns exactly the three separate originals")
            let positions: [(Double, Double, Double, Double, Double)] = [
                (0.28, 0.78, 4, 3, -4), (0.72, 0.72, 3, 4.2, 3), (0.42, 0.28, 5, 3.4, 7)
            ]
            for (index, p) in positions.enumerated() {
                let expected = knownQuad(center: CGPoint(x: p.0 * Double(photos.width), y: (1 - p.1) * Double(photos.height)),
                                         size: CGSize(width: p.2 * 200, height: p.3 * 200), degrees: p.4)
                let best = detected.quads.max { $0.iou(expected) < $1.iou(expected) }
                let overlap = best?.iou(expected) ?? 0
                print("SCN Vision photo", index + 1, "IoU", overlap)
                check(overlap > 0.85, "detected photo matches its independent known placement: \(index + 1)")
                if let best, let crop = Processor.perspectiveCrop(photos, best) {
                    try save(crop, "photo-\(index + 1).png")
                    check(abs(Double(crop.width) / Double(crop.height) - p.2 / p.3) < 0.05,
                          "separate photo extraction restores its expected aspect ratio: \(index + 1)")
                } else { check(false, "separate photo can be extracted: \(index + 1)") }
            }
            var photoOptions = ProcessOptions(); photoOptions.enhancement = .photo
            if let enhanced = Processor.finish(photos, photoOptions) {
                try save(enhanced, "photos-enhanced.png")
                check(enhanced.width == photos.width && enhanced.height == photos.height,
                      "real automatic photo enhancement retains complete image dimensions")
                check(Bitmap(enhanced, maxSide: 200, gray: false).data != Bitmap(photos, maxSide: 200, gray: false).data,
                      "photo auto-enhancement changes rendered colour/tone pixels")
            } else { check(false, "photo enhancement returns output") }
        } catch {
            let error = error as NSError
            check(false, "backend/output error: \(error.domain) / \(error.code): \(error.localizedDescription)")
        }
        print("SCN42 Vision regressions: \(count - failures)/\(count) passed"); fflush(stdout)
        print("SCN42 Vision artifacts:", folder.path)
        return failures
    }
}
