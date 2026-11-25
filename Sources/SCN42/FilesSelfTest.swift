import AppKit
import ImageIO
import PDFKit
import UniformTypeIdentifiers

/// File assertions inspect encoded data through ImageIO/PDFKit, independent of the save sheet.
enum ScannerFileAudit {
    static func fixture(width: Int = 96, height: Int = 64) -> CGImage {
        var bitmap = Bitmap(width: width, height: height, channels: 4, fill: 255)
        for y in 0..<height { for x in 0..<width {
            let p = (y * width + x) * 4
            bitmap.data[p] = x < width / 2 ? 230 : 20
            bitmap.data[p + 1] = y < height / 2 ? 210 : 30
            bitmap.data[p + 2] = x < width / 2 ? 20 : 220
        } }
        return bitmap.cgImage()!
    }

    static func page(_ image: CGImage? = nil, dpi: Double = 144, text: String = "ALPHA SEARCHABLE") -> ExportPage {
        ExportPage(image: image ?? fixture(), dpi: dpi,
                   text: [TextLine(text: text, box: CGRect(x: 0.1, y: 0.6, width: 0.8, height: 0.2))], title: text)
    }

    /// ImageIO exposes unpacked depth for one-bit TIFF, so read the encoded SHORT tags directly.
    static func tiffShortTags(_ data: Data) -> [Int: Int] {
        let bytes = [UInt8](data)
        guard bytes.count >= 8 else { return [:] }
        let little = bytes[0] == 0x49 && bytes[1] == 0x49
        guard little || (bytes[0] == 0x4d && bytes[1] == 0x4d) else { return [:] }
        func word(_ offset: Int, count: Int) -> Int? {
            guard offset >= 0, offset <= bytes.count - count else { return nil }
            var value = 0
            for i in 0..<count {
                let index = little ? offset + count - i - 1 : offset + i
                value = (value << 8) | Int(bytes[index])
            }
            return value
        }
        guard word(2, count: 2) == 42, let offset = word(4, count: 4), let count = word(offset, count: 2), count < 1024 else { return [:] }
        var result: [Int: Int] = [:]
        for i in 0..<count {
            let entry = offset + 2 + 12 * i
            if let tag = word(entry, count: 2), word(entry + 2, count: 2) == 3,
               word(entry + 4, count: 4) == 1, let value = word(entry + 8, count: 2) { result[tag] = value }
        }
        return result
    }

    static func run() -> Int {
        var count = 0, failures = 0
        func check(_ condition: Bool, _ name: String) { count += 1; if !condition { failures += 1 }; print(condition ? "PASS" : "FAIL", "SCN files:", name) }
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("SCN42-files-" + UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: folder) }
        do {
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            let first = page(), second = page(fixture(width: 60, height: 90), dpi: 72, text: "BRAVO SECOND")
            var options = SaveOptions(); options.openAfter = .none; options.compression = .original
            for format in [SaveFormat.pdf, .pdfA, .tiff] {
                options.format = format
                let data = try Exporter.data([first, second], options)
                if format.isPDF {
                    let document = PDFDocument(data: data)
                    check(document?.pageCount == 2, "multipage PDF retains every page: \(format)")
                    check(document?.page(at: 0)?.bounds(for: .mediaBox).size == CGSize(width: 48, height: 32)
                        && document?.page(at: 1)?.bounds(for: .mediaBox).size == CGSize(width: 60, height: 90), "PDF physical page dimensions use each page DPI: \(format)")
                    check(document?.string?.contains("ALPHA SEARCHABLE") == true && document?.string?.contains("BRAVO SECOND") == true,
                          "searchable PDF retains both recognised text layers: \(format)")
                } else {
                    guard let source = CGImageSourceCreateWithData(data as CFData, nil) else { throw CocoaError(.fileReadCorruptFile) }
                    check(CGImageSourceGetCount(source) == 2, "TIFF retains both pages")
                    let a = CGImageSourceCreateImageAtIndex(source, 0, nil), b = CGImageSourceCreateImageAtIndex(source, 1, nil)
                    check(a?.width == 96 && a?.height == 64 && b?.width == 60 && b?.height == 90, "TIFF keeps each page's original pixel dimensions")
                    let props = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any]
                    check(abs((props?[kCGImagePropertyDPIWidth] as? Double ?? 0) - 144) < 0.1, "TIFF keeps print resolution")
                }
            }
            options.format = .pdf; options.ocr = false
            check(PDFDocument(data: try Exporter.data([first], options))?.string?.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty != false,
                  "disabling searchable text omits the text layer")
            options.ocr = true
            for compression in Compression.allCases {
                options.compression = compression
                let document = PDFDocument(data: try Exporter.data([page(fixture(width: 600, height: 400), dpi: 600)], options))
                check(document?.pageCount == 1 && document?.page(at: 0)?.bounds(for: .mediaBox).size == CGSize(width: 72, height: 48),
                      "PDF compression preserves physical size: \(compression)")
            }
            for format in [SaveFormat.png, .jpeg, .heic] {
                if !(CGImageDestinationCopyTypeIdentifiers() as! [String]).contains(format.type.identifier) {
                    print("SKIP SCN files: encoder unavailable: \(format)"); continue
                }
                options.format = format
                let data = try Exporter.data([first], options)
                guard let source = CGImageSourceCreateWithData(data as CFData, nil), let decoded = CGImageSourceCreateImageAtIndex(source, 0, nil) else { throw CocoaError(.fileReadCorruptFile) }
                check(decoded.width == 96 && decoded.height == 64, "encoded image reopens with requested dimensions: \(format)")
                check(CGImageSourceGetType(source) as String? == format.type.identifier, "encoded image uses the requested container: \(format)")
                do { _ = try Exporter.data([first, second], options); check(false, "single-image data must reject extra pages") }
                catch { check(true, "single-image data rejects extra pages instead of discarding them: \(format)") }
            }
            options.format = .png
            let urls = try Exporter.write([first, second], folder: folder, baseName: "pages", options: options)
            check(urls.map(\.lastPathComponent) == ["pages 001.png", "pages 002.png"], "single-image export produces one numbered file per page")
            let again = try Exporter.write([first, second], folder: folder, baseName: "pages", options: options)
            check(again.map(\.lastPathComponent) == ["pages 001 2.png", "pages 002 2.png"], "ordinary Save keeps existing numbered files")
            let replaced = try Exporter.write([second, first], folder: folder, baseName: "pages", options: options, replaceExisting: true)
            check(replaced == urls && CGImageSourceCreateWithURL(urls[0] as CFURL, nil).flatMap { CGImageSourceCreateImageAtIndex($0, 0, nil) }?.width == 60,
                  "explicit numbered export replaces the intended destinations")
            options.format = .pdfPerPage
            let pdfs = try Exporter.write([first, second], folder: folder, baseName: "separate", options: options)
            check(pdfs.count == 2 && pdfs.allSatisfy { PDFDocument(url: $0)?.pageCount == 1 }, "PDF per page creates two independent one-page documents")
            let sentinel = folder.appendingPathComponent("keep.pdf"), bytes = Data("KEEP ORIGINAL".utf8)
            try bytes.write(to: sentinel)
            options.format = .pdf
            for badPages in [[], [page(dpi: 0)], [page(dpi: -.infinity)], [page(dpi: .nan)]] as [[ExportPage]] {
                do { _ = try Exporter.write(badPages, folder: folder, baseName: "keep", options: options, replaceExisting: true); check(false, "invalid pages must fail") }
                catch { check((try? Data(contentsOf: sentinel)) == bytes, "invalid/empty export preserves an existing destination") }
            }
            for quality in [Double.nan, .infinity, -0.1, 1.1] {
                options.quality = quality
                do { _ = try Exporter.write([first], folder: folder, baseName: "keep", options: options, replaceExisting: true); check(false, "invalid quality must fail") }
                catch { check((try? Data(contentsOf: sentinel)) == bytes, "invalid quality cannot replace an existing file") }
            }
            options.quality = 0.85
            for name in ["../escape", "bad/name", "", "..", "line\nbreak"] {
                do { _ = try Exporter.write([first], folder: folder, baseName: name, options: options); check(false, "invalid output name must fail") }
                catch { check(true, "invalid file name is rejected before output") }
            }
            let expanded = Exporter.expand("{title} {n} ###", counter: 7, title: "A/B:C\\D\nE")
            check(expanded == "A-B-C-D-E 007 007", "name template replaces path separators/control characters and formats counters")
            check(Exporter.expand(" /:. ", counter: 1, title: nil).isEmpty == false, "an empty sanitized name has a usable fallback")
            let lines = [[TextLine(text: "Alpha русский Türkçe", box: .zero)], [TextLine(text: "Bravo", box: .zero)]]
            check(String(data: Exporter.textData(lines, rtf: false)!, encoding: .utf8) == "Alpha русский Türkçe\n\nBravo", "plain text keeps Unicode and page separators")
            if let rtf = Exporter.textData(lines, rtf: true), let string = NSAttributedString(rtf: rtf, documentAttributes: nil)?.string {
                check(string == "Alpha русский Türkçe\n\nBravo", "RTF round trip keeps Unicode and page separators")
            } else { check(false, "RTF data is readable") }
            let bw = Bitmap(first.image, gray: true).oneBitImage(threshold: 128)!
            options.format = .tiff
            let binary = try Exporter.data([page(bw)], options)
            let source = CGImageSourceCreateWithData(binary as CFData, nil)!
            let decoded = CGImageSourceCreateImageAtIndex(source, 0, nil)
            let props = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any]
            let tiff = props?[kCGImagePropertyTIFFDictionary] as? [CFString: Any]
            let tags = tiffShortTags(binary)
            print("SCN TIFF diagnostic: encoded BitsPerSample=", tags[258] ?? -1, "Compression=", tags[259] ?? -1,
                  "ImageIO depth=", props?[kCGImagePropertyDepth] ?? "missing", "TIFF dictionary compression=", tiff?[kCGImagePropertyTIFFCompression] ?? "missing")
            check(decoded != nil && tags[258] == 1 && tags[259] == 4, "B&W TIFF remains one-bit CCITT Group 4")
            do { _ = try Exporter.encodedForPDF(page(dpi: .nan), .balanced); check(false, "invalid compression input must throw") }
            catch { check(true, "checked compression rejects invalid geometry rather than returning the original bitmap") }
            options.format = .png
            check(Exporter.estimate([first, second], options) > Exporter.estimate([first], options), "single-page-format estimate includes all pages")
        } catch { check(false, "file fixture failed: \(error)") }
        print("SCN42 file/output regressions: \(count - failures)/\(count) passed")
        return failures
    }
}
