import CoreGraphics
import CoreImage
import Foundation

/// Deterministic pixel/geometry checks. Vision model-dependent behavior has its own explicit audit.
enum ScanProcessingAudit {
    static func run() -> Int {
        var count = 0, failures = 0
        func check(_ passed: Bool, _ name: String) {
            count += 1; if !passed { failures += 1 }
            print(passed ? "PASS" : "FAIL", "SCN processing:", name)
        }
        func plainOptions() -> ProcessOptions {
            var options = ProcessOptions()
            options.crop = false; options.deskew = false; options.autoRotate = false; options.enhancement = .none
            return options
        }
        func pixel(_ image: CGImage, _ x: Int, _ y: Int) -> [UInt8] {
            let bitmap = Bitmap(image, gray: false), offset = (y * bitmap.width + x) * 4
            return Array(bitmap.data[offset..<(offset + 3)])
        }
        func mean(_ image: CGImage) -> Double {
            let bytes = Bitmap(image, gray: true).data
            return bytes.reduce(0.0) { $0 + Double($1) } / Double(max(1, bytes.count))
        }
        for dimensions in [(-1, 3, 1), (4, 0, 4), (4, 4, 3), (Int.max, 2, 4), (32_769, 1, 1), (20_000, 20_000, 1)] {
            let invalid = Bitmap(width: dimensions.0, height: dimensions.1, channels: dimensions.2)
            check(!invalid.isValid && invalid.data.isEmpty && invalid.cgImage() == nil, "invalid or excessive bitmap allocation is rejected")
        }
        check(Bitmap.validSize(width: 10_200, height: 16_800, channels: 4), "Legal at 1200 dpi remains within the supported allocation limit")
        var source = Bitmap(width: 96, height: 64, channels: 4, fill: 255)
        for y in 0..<source.height { for x in 0..<source.width {
            let offset = (y * source.width + x) * 4
            source.data[offset] = UInt8(x * 2)
            source.data[offset + 1] = UInt8(y * 3)
            source.data[offset + 2] = x < 48 ? 50 : 210
        } }
        guard let image = source.cgImage() else { check(false, "pixel fixture exists"); return failures }
        check(pixel(image, 13, 17) == [26, 51, 50], "bitmap round trip preserves top-down RGB coordinates")
        check(Bitmap(image, maxSide: 24, gray: false).width == 24 && Bitmap(image, maxSide: 24, gray: false).height == 16,
              "bounded thumbnail decoding preserves the aspect ratio")
        check(!source.resized(width: -1, height: 4).isValid && source.resized(width: -1, height: 4).cgImage() == nil,
              "invalid resize cannot return the original as a successful resize")
        let luminance = source.grayscale()
        let expectedLuminance = (26 * 77 + 51 * 150 + 50 * 29) >> 8
        check(luminance.channels == 1 && Int(luminance.data[17 * source.width + 13]) == expectedLuminance,
              "RGB luminance uses the declared channel order")
        check(source.oneBitImage(threshold: 128) == nil, "one-bit conversion rejects RGB input without trapping")

        var striped = Bitmap(width: 13, height: 5, channels: 1)
        for y in 0..<striped.height { for x in 0..<striped.width { striped.data[y * striped.width + x] = (x + y) % 3 == 0 ? 0 : 255 } }
        if let packed = striped.oneBitImage(threshold: 128) {
            check(packed.bitsPerPixel == 1 && packed.bytesPerRow == 2, "one-bit rows correctly pad a width not divisible by eight")
            check(Bitmap(packed, gray: true).data == striped.data, "one-bit packing preserves every row and pixel")
        } else { check(false, "one-bit output exists") }
        let pair = Bitmap(width: 2, height: 1, channels: 1, fill: 80)
        check(pair.boxBlur(radius: Int.max).data == [80, 80] && pair.maxFilter(radius: Int.max).data == [80, 80],
              "extreme filter radii cannot overflow pixel bounds")

        var blackWhite = Bitmap(width: 100, height: 60, channels: 1, fill: 255)
        for y in 10..<50 { for x in 15..<45 { blackWhite.data[y * 100 + x] = 0 } }
        let bwSource = blackWhite.cgImage()!
        var bwOptions = plainOptions(); bwOptions.colorMode = .bw
        if let bw = Processor.finish(bwSource, bwOptions) {
            let output = Bitmap(bw, gray: true)
            check(bw.bitsPerPixel == 1 && output.data[20 * 100 + 20] == 0 && output.data[20 * 100 + 80] == 255,
                  "automatic B&W retains both black ink and white paper when Otsu chooses zero")
            check(output.data == blackWhite.data, "automatic B&W does not erase a binary page")
        } else { check(false, "automatic B&W output exists") }
        var threshold = plainOptions(); threshold.colorMode = .bw; threshold.threshold = 0.25
        let low = Processor.finish(image, threshold)
        threshold.threshold = 0.8
        let high = Processor.finish(image, threshold)
        check(low != nil && high != nil && mean(high!) < mean(low!), "manual B&W threshold changes the actual black-pixel coverage")
        var grey = plainOptions(); grey.colorMode = .gray
        let grayImage = Processor.finish(image, grey)
        check(grayImage?.bitsPerPixel == 8 && grayImage?.colorSpace?.model == .monochrome, "Gray produces an actual monochrome image")
        check(Processor.finish(image, plainOptions()) === image, "None plus Colour preserves unchanged source pixels")
        var brightness = plainOptions(); brightness.brightness = 0.7
        let brighter = Processor.finish(image, brightness)
        check(brighter != nil && mean(brighter!) > mean(image) + 15, "Brightness changes rendered luminance")
        var contrast = plainOptions(); contrast.contrast = 0.8
        let contrasted = Processor.finish(image, contrast)
        check(contrasted != nil && Bitmap(contrasted!, gray: false).data != Bitmap(image, gray: false).data,
              "Contrast changes actual pixels without requiring document enhancement")
        for key in 0..<5 {
            var invalid = plainOptions()
            switch key { case 0: invalid.dpi = .nan; case 1: invalid.brightness = .infinity; case 2: invalid.contrast = 2
            case 3: invalid.threshold = .nan; default: invalid.blankLimit = -1 }
            check(!invalid.isValid && Processor.finish(image, invalid) == nil, "invalid processing values fail before rendering")
            do { _ = try Processor.processChecked(image, invalid); check(false, "invalid pipeline must throw") }
            catch { check(true, "invalid pipeline fails instead of silently returning unprocessed pixels") }
        }
        check(Processor.render(CIImage(color: .white)) == nil && Processor.rotate(image, degrees: .nan) == nil,
              "infinite image extent and nonfinite rotation are rejected")

        for degrees in [90, 180, 270] {
            guard let rotated = Processor.rotate90(image, clockwise: degrees) else { check(false, "quarter-turn output exists"); continue }
            let swaps = degrees != 180
            check(rotated.width == (swaps ? image.height : image.width) && rotated.height == (swaps ? image.width : image.height),
                  "quarter turn has the expected dimensions: \(degrees)")
            let x = 13, y = 17
            let dest: (Int, Int)
            switch degrees { case 90: dest = (image.height - y - 1, x); case 180: dest = (image.width - x - 1, image.height - y - 1)
            default: dest = (y, image.width - x - 1) }
            check(pixel(rotated, dest.0, dest.1) == pixel(image, x, y), "quarter turn maps the correct source pixel: \(degrees)")
        }
        let black = Bitmap(width: 100, height: 80, channels: 1).cgImage()!
        if let turned = Processor.rotate(black, degrees: 7) {
            check(turned.width == 100 && turned.height == 80 && mean(turned) > 0 && pixel(turned, 0, 0) == [255, 255, 255],
                  "deskew rotation keeps dimensions and fills exposed paper corners white")
        } else { check(false, "fine rotation output exists") }

        let rectangle = Quad(tl: CGPoint(x: 10, y: 10), tr: CGPoint(x: 80, y: 10), br: CGPoint(x: 80, y: 50), bl: CGPoint(x: 10, y: 50))
        check(rectangle.isValid && abs(rectangle.area - 2800) < 0.001 && rectangle.iou(rectangle) == 1,
              "quad area and overlap describe the real convex region")
        check(Quad.ordered([rectangle.br, rectangle.tl, rectangle.bl, rectangle.tr]) == rectangle,
              "arbitrary corners are reordered consistently")
        let empty = Quad.ordered([])
        let crossing = Quad(tl: rectangle.tl, tr: rectangle.br, br: rectangle.tr, bl: rectangle.bl)
        let nonfinite = Quad(tl: CGPoint(x: CGFloat.nan, y: 0), tr: rectangle.tr, br: rectangle.br, bl: rectangle.bl)
        check(!empty.isValid && !empty.contains(.zero) && !crossing.isValid && !nonfinite.isValid,
              "empty, crossing and nonfinite quadrilaterals are rejected")
        check(Processor.perspectiveCrop(image, empty) == nil && Processor.perspectiveCrop(image, nonfinite) == nil,
              "invalid perspective requests cannot reach Core Image")
        let outside = rectangle.scaled(10, 10)
        check(Processor.perspectiveCrop(image, outside) == nil, "perspective crop rejects corners outside the source")
        if let cropped = Processor.perspectiveCrop(image, rectangle) {
            check(abs(cropped.width - 70) <= 1 && abs(cropped.height - 40) <= 1, "perspective crop uses the selected region dimensions")
            let sample = pixel(cropped, cropped.width / 2, cropped.height / 2)
            check(abs(Int(sample[0]) - 90) < 5 && abs(Int(sample[1]) - 90) < 5, "perspective crop samples the selected centre instead of another source region")
        } else { check(false, "valid perspective output exists") }
        check(rectangle.inset(1000).isValid && rectangle.inset(1000).area < rectangle.area,
              "excessive crop inset cannot reverse or collapse the region")

        for side in [1, 2, 3] {
            let tiny = Bitmap(width: side, height: side, channels: 1, fill: 255).cgImage()!
            check(Processor.inkCoverage(tiny) == 0 && Processor.toneSpread(tiny) == 0, "tiny blank page measurements do not trap: \(side)")
        }
        check(Processor.inkCoverage(bwSource) > 0.005 && Processor.toneSpread(bwSource) > 10,
              "blank detection retains a page with a sizeable ink region")
        let white = Bitmap(width: 120, height: 160, channels: 1, fill: 255).cgImage()!
        do {
            let (_, blankReport) = try Processor.processChecked(white, plainOptions())
            let (_, textReport) = try Processor.processChecked(bwSource, plainOptions())
            check(blankReport.blank && !textReport.blank, "pipeline blank classification separates white paper and marked paper")
            check(blankReport.inputSize == CGSize(width: 120, height: 160) && blankReport.outputSize == blankReport.inputSize,
                  "pipeline report matches the real image geometry")
        } catch { check(false, "valid pipeline completes: \(error)") }

        var yellow = Bitmap(width: 400, height: 300, channels: 4, fill: 255)
        for y in 0..<300 { for x in 0..<400 {
            let i = (y * 400 + x) * 4
            let shade = UInt8(160 + x * 50 / 399)
            yellow.data[i] = shade; yellow.data[i + 1] = shade - 10; yellow.data[i + 2] = shade - 35
            if (100..<300).contains(x), (145..<155).contains(y) { yellow.data[i] = 15; yellow.data[i + 1] = 15; yellow.data[i + 2] = 15 }
        } }
        let yellowImage = yellow.cgImage()!
        var documentOptions = plainOptions(); documentOptions.enhancement = .document
        if let clean = Processor.finish(yellowImage, documentOptions) {
            check(pixel(clean, 70, 100).allSatisfy { $0 > 235 } && pixel(clean, 200, 150).allSatisfy { $0 < 80 },
                  "document clean-up whitens yellow/shaded paper while retaining ink")
            check(clean.width == 400 && clean.height == 300, "document clean-up preserves dimensions")
        } else { check(false, "document clean-up produces output") }
        var dots = Bitmap(width: 128, height: 128, channels: 1)
        for y in 0..<128 { for x in 0..<128 { dots.data[y * 128 + x] = (x + y) % 2 == 0 ? 0 : 255 } }
        let dotted = dots.cgImage()!
        if let descreened = Processor.descreen(dotted, dpi: 300) {
            check(Processor.toneSpread(descreened) < 2 && Bitmap(descreened, gray: true).data != dots.data,
                  "Descreen removes a high-frequency dot pattern")
        } else { check(false, "Descreen output exists") }
        check(Processor.descreen(image, dpi: .infinity) == nil, "nonfinite scan resolution cannot reach descreen filters")

        var textRows = Bitmap(width: 600, height: 400, channels: 1, fill: 255)
        for row in stride(from: 45, through: 345, by: 25) { for y in row..<(row + 4) {
            for x in 40..<560 where x % 47 < 31 { textRows.data[y * 600 + x] = 20 }
        } }
        if let tilted = Processor.rotate(textRows.cgImage()!, degrees: 3), let angle = Processor.skewAngle(tilted) {
            check(abs(angle - 3) < 0.4, "text-line deskew measures the known clockwise tilt")
            var deskew = plainOptions(); deskew.deskew = true
            do {
                let (straight, report) = try Processor.processChecked(tilted, deskew)
                check(abs(report.deskewAngle - 3) < 0.4 && abs(Processor.skewAngle(straight) ?? 99) < 0.4,
                      "deskew pipeline removes the measured tilt")
            } catch { check(false, "deskew pipeline completes: \(error)") }
        } else { check(false, "known text-line tilt is measurable") }

        let lines = [TextLine(text: "Bottom", box: CGRect(x: 0.1, y: 0.1, width: 0.5, height: 0.1)),
                     TextLine(text: "Right", box: CGRect(x: 0.6, y: 0.8, width: 0.3, height: 0.1)),
                     TextLine(text: "Left", box: CGRect(x: 0.1, y: 0.8, width: 0.3, height: 0.1))]
        check(OCR.text(lines) == "Left\nRight\nBottom", "OCR text exports in top-to-bottom and left-to-right order")
        check(OCR.title(lines) == "Left", "automatic title comes from the first readable line")
        check(OCR.title([TextLine(text: "////", box: .zero)]) == nil, "punctuation-only OCR cannot become a filename title")
        print("SCN42 processing regressions: \(count - failures)/\(count) passed")
        return failures
    }
}
