import AppKit
import ImageCaptureCore
import ImageIO
import PDFKit

/// Headless checks used while developing (not localised: developer output). Nothing here starts a real scan.
///
///     SCN42 --make-test-inputs dir             skewed page on glass, photos on glass, feeder pages (blank, skewed, upside down…)
///     SCN42 --process in.png out.png [--crop --deskew --rotate --enhance --photo --gray --bw --dpi n]
///     SCN42 --pipeline dir|files… out.pdf [--ocr --remove-blank --compress original|high|balanced|smallest --bw --gray --pdfa
///                                         --format pdf|tiff|jpeg|png|heic]
///     SCN42 --detect-photos glass.png [outdir]  photos on the glass → separate, straightened images
///     SCN42 --fake-scan dir out.pdf [--feeder --duplex --dpi n --photos]   the virtual scanner end to end
///     SCN42 --list-devices [seconds]           scanners Image Capture can see (read-only, default 3 s)
///     SCN42 --name "template"                  file name for a template now
enum CLI {
    static var status: Int32 = 0

    static func run(_ args: [String]) -> Bool {
        guard args.count > 1 else { return false }
        let rest = Array(args.dropFirst(2))
        setlinebuf(stdout)
        status = 0
        switch args[1] {
        case "--selftest":
            guard rest.isEmpty else { fail("--selftest takes no arguments"); return true }
            selfTest()
        case "--make-test-inputs":
            guard rest.count == 1, let dir = rest.first else { print("usage: --make-test-inputs dir"); status = 2; return true }
            do { print(try TestImages.write(to: URL(fileURLWithPath: dir)).joined(separator: "\n")) } catch { print("error: \(error)"); status = 1 }
        case "--process": process(rest)
        case "--pipeline": pipeline(rest)
        case "--detect-photos": detectPhotos(rest)
        case "--fake-scan": fakeScan(rest)
        case "--list-devices":
            guard rest.count <= 1, let seconds = rest.first.flatMap(Double.init) ?? (rest.isEmpty ? 3 : nil), seconds.isFinite, (0...30).contains(seconds) else { fail("--list-devices requires 0...30 seconds"); return true }
            listDevices(seconds)
        case "--name":
            guard rest.count <= 1 else { fail("--name takes one quoted template"); return true }
            let exampleTitle = "Invoice No 4217"
            print(Exporter.expand(rest.first ?? SaveOptions.defaultTemplate, counter: 7, title: exampleTitle))
        default:
            if args[1].hasPrefix("--"), !["--uitest", "--audit"].contains(args[1]) { fail("unknown command: " + args[1]); return true }
            return false
        }
        return true
    }


    static func fail(_ message: String, code: Int32 = 2) { print("error: " + message); status = code }

    /// Parse complete flag/value pairs before any processing or output writes.
    static func validFlags(_ flags: [String], switches: Set<String>, values: Set<String>) -> Bool {
        var seen = Set<String>(), i = 0
        while i < flags.count {
            let key = flags[i]
            guard seen.insert(key).inserted else { fail("duplicate option: " + key); return false }
            if switches.contains(key) { i += 1; continue }
            guard values.contains(key), i + 1 < flags.count, !flags[i + 1].hasPrefix("--") else { fail("invalid or incomplete option: " + key); return false }
            i += 2
        }
        if flags.contains("--bw") && flags.contains("--gray") { fail("--bw and --gray are mutually exclusive"); return false }
        if let value = value(flags, "--dpi"), !(Double(value).map { $0.isFinite && (1...2400).contains($0) } ?? false) { fail("--dpi requires a finite number from 1 to 2400"); return false }
        if let value = value(flags, "--compress"), Compression.named(value) == nil { fail("unknown compression: " + value); return false }
        if let value = value(flags, "--format"), format(value) == nil { fail("unknown output format: " + value); return false }
        if flags.contains("--text") && !flags.contains("--ocr") { fail("--text requires --ocr"); return false }
        if let path = value(flags, "--text"), !["txt", "rtf"].contains((path as NSString).pathExtension.lowercased()) { fail("text output must use .txt or .rtf"); return false }
        return true
    }

    static func format(_ name: String) -> SaveFormat? {
        ["pdf": .pdf, "pdfa": .pdfA, "pdf-per-page": .pdfPerPage, "tif": .tiff, "tiff": .tiff,
         "jpg": .jpeg, "jpeg": .jpeg, "png": .png, "heic": .heic][name.lowercased()]
    }

    static let processingFlags: Set<String> = ["--crop", "--deskew", "--rotate", "--enhance", "--photo", "--gray", "--bw", "--descreen", "--remove-blank"]

    /// Regression checks for save state and preservation of files when export fails.
    static func selfTest() {
        var failures = 0
        func check(_ condition: Bool, _ name: String) {
            print("\(condition ? "PASS" : "FAIL") \(name)")
            if !condition { failures += 1 }
        }
        check(ScannerStatus.afterWarmUp(selectingSource: true, acquiring: false) == .busy,
              "warm-up completion preserves a pending source-selection reservation")
        check(ScannerStatus.afterWarmUp(selectingSource: false, acquiring: true) == .scanning(0),
              "warm-up completion preserves an active acquisition reservation")
        check(ScannerStatus.afterWarmUp(selectingSource: false, acquiring: false) == .ready,
              "warm-up completion releases an idle device")
        guard let image = Bitmap(width: 8, height: 8, channels: 1, fill: 255).cgImage() else {
            print("FAIL create fixture image"); status = 1; return
        }
        func page() -> ScanPage {
            ScanPage(base: image, image: image, dpi: 300, options: ProcessOptions(), report: ProcessReport())
        }
        let doc = ScanDocument(), first = page()
        doc.append([first])
        doc.markSaved(revision: doc.revision)
        check(!doc.isDirty, "saved current snapshot is clean")
        let savedRevision = doc.revision
        doc.append([page()])
        doc.markSaved(revision: savedRevision)
        check(doc.isDirty, "page added during save remains dirty")
        doc.markSaved(revision: doc.revision)
        doc.setText(first, [TextLine(text: "Searchable text", box: .zero)])
        check(doc.isDirty, "OCR makes saved document dirty")
        let beforeRemoval = doc.revision
        doc.remove(ids: [first.id])
        doc.markSaved(revision: doc.revision)
        doc.setText(first, [])
        check(!doc.isDirty && doc.revision == beforeRemoval + 1, "late OCR for removed page is ignored")

        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("SCN42-selftest-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: folder) }
        do {
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            let target = folder.appendingPathComponent("existing.tiff")
            let original = Data("existing document".utf8)
            try original.write(to: target)
            var options = SaveOptions()
            options.format = .tiff
            do {
                _ = try Exporter.write([], folder: folder, baseName: "existing", options: options, replaceExisting: true)
                check(false, "empty TIFF export throws")
            } catch { check(true, "empty TIFF export throws") }
            check(try Data(contentsOf: target) == original, "failed export preserves existing file")
            let outputs = try Exporter.write([first.exportPage], folder: folder, baseName: "existing", options: options, replaceExisting: true)
            let replacement = try Data(contentsOf: target)
            check(outputs == [target] && replacement != original, "successful export replaces selected file")
        } catch {
            print("FAIL export regression: \(error)")
            failures += 1
        }
        failures += ScannerFileAudit.run()
        failures += ScanProcessingAudit.run()
        failures += ScanDocumentAudit.run()
        status = failures == 0 ? 0 : 1
        print("SCN42 selftest: \(failures) failures")
    }

    static func load(_ path: String, index: Int = 0) -> RawScan? {
        let url = URL(fileURLWithPath: path)
        guard let src = CGImageSourceCreateWithURL(url as CFURL, nil), index >= 0, index < CGImageSourceGetCount(src),
              let props = CGImageSourceCopyPropertiesAtIndex(src, index, nil) as? [CFString: Any],
              let width = props[kCGImagePropertyPixelWidth] as? Double, let height = props[kCGImagePropertyPixelHeight] as? Double,
              width.isFinite, height.isFinite, width > 0, height > 0, max(width, height) <= 32768, width * height <= 256_000_000 else { return nil }
        let options: [CFString: Any] = [kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true, kCGImageSourceThumbnailMaxPixelSize: max(width, height)]
        guard let image = CGImageSourceCreateThumbnailAtIndex(src, index, options as CFDictionary) else { return nil }
        let dpi = (props[kCGImagePropertyDPIWidth] as? Double).flatMap { $0.isFinite && $0 > 0 ? $0 : nil } ?? 300
        return RawScan(image: image, dpi: dpi)
    }

    static func loadPages(_ path: String) -> [RawScan]? {
        let url = URL(fileURLWithPath: path)
        if url.pathExtension.lowercased() == "pdf" {
            guard let document = PDFDocument(url: url), !document.isLocked, document.pageCount > 0 else { return nil }
            guard let pages = try? ContinuityImport.rasterizeChecked(document) else { return nil }
            return pages.count == document.pageCount ? pages : nil
        }
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil) else { return nil }
        let count = CGImageSourceGetCount(source)
        guard count > 0, count <= 2000 else { return nil }
        let pages = (0..<count).compactMap { load(path, index: $0) }
        return pages.count == count ? pages : nil
    }

    static func options(_ a: [String], defaults all: Bool) -> ProcessOptions {
        var o = ProcessOptions()
        let explicit = a.contains { ["--crop", "--deskew", "--rotate", "--enhance", "--photo"].contains($0) }
        let on = !explicit && all
        o.crop = on || a.contains("--crop")
        o.deskew = on || a.contains("--deskew")
        o.autoRotate = on || a.contains("--rotate")
        o.enhancement = a.contains("--photo") ? .photo : ((on || a.contains("--enhance")) ? .document : .none)
        o.colorMode = a.contains("--bw") ? .bw : (a.contains("--gray") ? .gray : .color)
        o.detectBlank = a.contains("--remove-blank")
        o.descreen = a.contains("--descreen")
        return o
    }

    static func value(_ a: [String], _ key: String) -> String? {
        guard let i = a.firstIndex(of: key), i + 1 < a.count else { return nil }
        return a[i + 1]
    }

    static func writePNG(_ image: CGImage, _ path: String, dpi: Double) throws {
        let ext = (path as NSString).pathExtension.lowercased()
        guard ["png", "jpg", "jpeg"].contains(ext) else { throw CocoaError(.fileWriteInvalidFileName) }
        var options = SaveOptions(); options.format = ext == "png" ? .png : .jpeg
        let data = try Exporter.data([ExportPage(image: image, dpi: dpi)], options)
        try data.write(to: URL(fileURLWithPath: path), options: .atomic)
    }

    static func process(_ a: [String]) {
        guard a.count >= 2 else { fail("usage: --process in out.png|out.jpg [options]"); return }
        let flags = Array(a.dropFirst(2))
        guard validFlags(flags, switches: processingFlags, values: ["--dpi"]) else { return }
        guard ["png", "jpg", "jpeg"].contains((a[1] as NSString).pathExtension.lowercased()) else { fail("process output must use .png or .jpg"); return }
        guard let inputPages = loadPages(a[0]), inputPages.count == 1, let raw = inputPages.first else { fail("--process requires one readable image page; use --pipeline for documents", code: 1); return }
        var o = options(flags, defaults: false)
        o.dpi = value(flags, "--dpi").flatMap(Double.init) ?? raw.dpi
        let t = Date()
        do {
            let (image, report) = try Processor.processChecked(raw.image, o)
            print("\(a[0]): \(report.summary) time=\(String(format: "%.2f", Date().timeIntervalSince(t)))s")
            try writePNG(image, a[1], dpi: o.dpi)
        } catch { fail(error.localizedDescription, code: 1) }
    }

    static func inputs(_ paths: [String]) -> [String] {
        var out: [String] = []
        for p in paths {
            var isDir: ObjCBool = false
            if FileManager.default.fileExists(atPath: p, isDirectory: &isDir), isDir.boolValue {
                let files = (try? FileManager.default.contentsOfDirectory(atPath: p)) ?? []
                out += files.filter { ["png", "jpg", "jpeg", "tif", "tiff", "heic", "pdf"].contains(($0 as NSString).pathExtension.lowercased()) }
                    .sorted { $0.localizedStandardCompare($1) == .orderedAscending }.map { (p as NSString).appendingPathComponent($0) }
            } else { out.append(p) }
        }
        return out
    }

    static func pipeline(_ a: [String]) {
        let positional = a.prefix { !$0.hasPrefix("--") }
        guard positional.count >= 2 else { print("usage: --pipeline dir|files… out.pdf [--ocr --remove-blank --compress balanced]"); status = 2; return }
        let flags = Array(a.dropFirst(positional.count))
        guard validFlags(flags, switches: processingFlags.union(["--ocr", "--pdfa"]), values: ["--dpi", "--text", "--compress", "--format"]) else { return }
        let out = positional.last!
        guard let outputFormat = value(flags, "--format").flatMap(format) ?? format((out as NSString).pathExtension) else { fail("unsupported output extension"); return }
        guard !flags.contains("--pdfa") || outputFormat.isPDF else { fail("--pdfa requires PDF output"); return }
        if let textPath = value(flags, "--text"), URL(fileURLWithPath: textPath).standardizedFileURL == URL(fileURLWithPath: out).standardizedFileURL { fail("text and image outputs must be different files"); return }
        var o = options(flags, defaults: true)
        o.detectBlank = true
        let removeBlank = flags.contains("--remove-blank")
        var pages: [ScanPage] = []
        var removed = 0
        let start = Date()
        for path in inputs(Array(positional.dropLast())) {
            guard let inputPages = loadPages(path) else { fail("cannot read: " + path, code: 1); return }
            for var raw in inputPages {
            if let dpi = value(flags, "--dpi").flatMap(Double.init) { raw.dpi = dpi }
            let t = Date()
            let page: ScanPage
            do { page = try ScanPage.make(raw, options: o) } catch { fail(error.localizedDescription, code: 1); return }
            print("\((path as NSString).lastPathComponent): \(page.report.summary) time=\(String(format: "%.2f", Date().timeIntervalSince(t)))s")
            if removeBlank && page.report.blank { removed += 1; continue }
            pages.append(page)
            }
        }
        guard !pages.isEmpty else { fail("no pages to export", code: 1); return }
        if flags.contains("--ocr") {
            for (i, p) in pages.enumerated() {
                do { p.text = try OCR.recognizeChecked(p.image) } catch { fail(error.localizedDescription, code: 1); return }
                let sample = OCR.text(p.text ?? []).replacingOccurrences(of: "\n", with: " | ")
                print("ocr page \(i + 1): \(p.text?.count ?? 0) lines: \(sample.prefix(160))")
            }
            if let title = pages.first?.text.flatMap(OCR.title) { print("title: \(title)") }
            // --text out.txt|out.rtf: the recognised text as a file.
            if let t = value(flags, "--text"), let data = Exporter.textData(pages.compactMap(\.text), rtf: t.lowercased().hasSuffix(".rtf")) {
                do { try data.write(to: URL(fileURLWithPath: t), options: .atomic) } catch { fail(error.localizedDescription, code: 1); return }
                print("text: \(t) \(data.count) bytes")
            }
        }
        var save = SaveOptions()
        save.ocr = flags.contains("--ocr")
        save.compression = value(flags, "--compress").flatMap(Compression.named) ?? .balanced
        save.format = flags.contains("--pdfa") ? .pdfA : outputFormat
        let url = URL(fileURLWithPath: out)
        do {
            let estimate = Exporter.estimate(pages.map(\.exportPage), save)
            let urls = try Exporter.write(pages.map(\.exportPage), folder: url.deletingLastPathComponent(), baseName: url.deletingPathExtension().lastPathComponent, options: save, replaceExisting: true)
            for u in urls {
                let size = (try? u.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0
                print("wrote \(u.path): \(Exporter.sizeText(size)) (estimate \(Exporter.sizeText(estimate)))")
                if save.format.isPDF, let doc = PDFDocument(url: u) {
                    let text = (doc.string ?? "").replacingOccurrences(of: "\n", with: " ")
                    print("pdf: \(doc.pageCount) pages, searchable text \(text.count) chars: \(text.prefix(100))")
                }
            }
            print("pages: \(pages.count), blank removed: \(removed), compression: \(save.compression), total time \(String(format: "%.1f", Date().timeIntervalSince(start)))s")
        } catch { print("error: \(error.localizedDescription)"); status = 1 }
    }

    static func detectPhotos(_ a: [String]) {
        guard (1...2).contains(a.count), let path = a.first, let raw = load(path) else { print("usage: --detect-photos glass.png [outdir]"); status = 2; return }
        let found = Processor.detectPhotos(raw.image)
        print("photos: \(found.quads.count) (glass outlines \(found.glassCount), vision rectangles \(found.visionCount))")
        for (i, q) in found.quads.enumerated() {
            print(String(format: "photo %d: angle %.2f° size %.0fx%.0f box %@", i + 1, q.angle, q.width, q.height, q.description))
            if a.count > 1, let crop = Processor.perspectiveCrop(raw.image, q.inset(4)) {
                do {
                    try FileManager.default.createDirectory(atPath: a[1], withIntermediateDirectories: true)
                    try writePNG(crop, (a[1] as NSString).appendingPathComponent("photo-\(i + 1).png"), dpi: raw.dpi)
                } catch { fail(error.localizedDescription, code: 1); return }
            }
        }
    }

    /// Drives the virtual scanner like the window does (overview → areas → scan, or the feeder).
    static func fakeScan(_ a: [String]) {
        guard a.count >= 2 else { print("usage: --fake-scan dir out.pdf [--feeder --duplex --dpi n --photos]"); status = 2; return }
        let flags = Array(a.dropFirst(2))
        guard validFlags(flags, switches: ["--feeder", "--duplex", "--photos"], values: ["--dpi"]),
              (a[1] as NSString).pathExtension.lowercased() == "pdf" else { if status == 0 { fail("fake scan output must use .pdf") }; return }
        guard !flags.contains("--duplex") || flags.contains("--feeder") else { fail("--duplex requires --feeder"); return }
        guard !(flags.contains("--photos") && flags.contains("--feeder")) else { fail("--photos requires the flatbed"); return }
        if let dpi = value(flags, "--dpi"), Int(dpi) == nil { fail("scanner DPI must be a whole number"); return }
        let scanner = FakeScanner(folder: URL(fileURLWithPath: a[0]))
        scanner.onStatus = { print("status: \($0.text)") }
        var settings = ScanSettings()
        settings.source = a.contains("--feeder") ? .feeder : .flatbed
        settings.duplex = a.contains("--duplex")
        settings.resolution = Int(value(a, "--dpi") ?? "") ?? 200
        settings.removeBlank = true
        settings.detectPhotos = a.contains("--photos")
        if settings.detectPhotos { settings.enhancement = .photo; settings.autoRotate = false }
        var pages: [ScanPage] = []
        var finished = false
        scanner.open()
        var priors: [Quad] = []
        func scanAreas(_ areas: [CGRect]) {
            var queue = Array(areas.enumerated())
            func next() {
                guard !queue.isEmpty else { finished = true; return }
                let (index, area) = queue.removeFirst()
                var options = settings.processing
                if priors.indices.contains(index) { options.priorQuad = priors[index] }
                let req = ScanRequest(source: settings.source, resolution: settings.resolution, colorMode: settings.colorMode, bitDepth: 8,
                                      area: area, paper: nil, duplex: settings.duplex, lastInJob: queue.isEmpty)
                let asked = Date()
                scanner.scan(req, page: { raw in
                    let t = Date()
                    let p: ScanPage
                    do { p = try ScanPage.make(raw, options: options) } catch { fail(error.localizedDescription, code: 1); finished = true; scanner.cancel(); return }
                    print(String(format: "page %d: %dx%d@%d scanned in %.1fs, processed in %.1fs: ", pages.count + 1, raw.image.width, raw.image.height,
                                 Int(raw.dpi), t.timeIntervalSince(asked), Date().timeIntervalSince(t)) + p.report.summary)
                    if !(settings.removeBlank && p.report.blank) { pages.append(p) } else { print("  blank page removed") }
                }, done: { error in
                    if let error { fail("scan error: " + error.localizedDescription, code: 1); finished = true; return }
                    if status == 0 { next() }
                })
            }
            next()
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.4) {
            if settings.detectPhotos {
                scanner.overview { result in
                    guard case .success(let overview) = result else { fail("overview failed", code: 1); finished = true; return }
                    let found = Processor.detectPhotos(overview).quads
                    let areas = found.map { Self.area(for: $0, in: overview) }
                    priors = zip(found, areas).map { q, a in
                        let n = q.scaled(1 / CGFloat(overview.width), 1 / CGFloat(overview.height))
                        func rel(_ p: CGPoint) -> CGPoint { CGPoint(x: (p.x - a.minX) / a.width, y: (p.y - a.minY) / a.height) }
                        return Quad(tl: rel(n.tl), tr: rel(n.tr), br: rel(n.br), bl: rel(n.bl))
                    }
                    print("overview \(overview.width)x\(overview.height): \(areas.count) photos")
                    scanAreas(areas)
                }
            } else {
                scanAreas([CGRect(x: 0, y: 0, width: 1, height: 1)])
            }
        }
        let deadline = Date().addingTimeInterval(120)
        while !finished && Date() < deadline { RunLoop.main.run(until: Date().addingTimeInterval(0.05)) }
        guard finished, status == 0, !pages.isEmpty else {
            if status == 0 { fail(finished ? "no pages scanned" : "scan timed out", code: 1) }
            scanner.cancel(); scanner.close(); return
        }
        scanner.close()
        var save = SaveOptions()
        save.compression = .balanced
        save.ocr = false
        let url = URL(fileURLWithPath: a[1])
        do {
            let urls = try Exporter.write(pages.map(\.exportPage), folder: url.deletingLastPathComponent(), baseName: url.deletingPathExtension().lastPathComponent, options: save, replaceExisting: true)
            for u in urls { print("wrote \(u.path) \(Exporter.sizeText((try? u.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0)), \(pages.count) pages") }
        } catch { fail(error.localizedDescription, code: 1) }
    }

    /// Normalised scan area (origin top-left) around a quad found on an overview, with a small margin.
    static func area(for q: Quad, in image: CGImage) -> CGRect {
        let b = q.bounds.insetBy(dx: -CGFloat(image.width) * 0.012, dy: -CGFloat(image.width) * 0.012)
        let r = CGRect(x: b.minX / CGFloat(image.width), y: b.minY / CGFloat(image.height), width: b.width / CGFloat(image.width), height: b.height / CGFloat(image.height))
        return r.intersection(CGRect(x: 0, y: 0, width: 1, height: 1))
    }

    /// Read-only discovery: lists what Image Capture sees; no session is opened.
    static func listDevices(_ seconds: Double) {
        final class Lister: NSObject, ICDeviceBrowserDelegate {
            var found: [ICDevice] = []
            func deviceBrowser(_ browser: ICDeviceBrowser, didAdd device: ICDevice, moreComing: Bool) { found.append(device) }
            func deviceBrowser(_ browser: ICDeviceBrowser, didRemove device: ICDevice, moreGoing: Bool) {}
        }
        let lister = Lister()
        let browser = ICDeviceBrowser()
        browser.delegate = lister
        let mask = ICDeviceTypeMask.scanner.rawValue | ICDeviceLocationTypeMask.local.rawValue | ICDeviceLocationTypeMask.shared.rawValue
            | ICDeviceLocationTypeMask.bonjour.rawValue | ICDeviceLocationTypeMask.bluetooth.rawValue | ICDeviceLocationTypeMask.remote.rawValue
        browser.browsedDeviceTypeMask = ICDeviceTypeMask(rawValue: mask) ?? .scanner
        browser.start()
        RunLoop.main.run(until: Date().addingTimeInterval(seconds))
        browser.stop()
        if lister.found.isEmpty { print("no scanners found in \(Int(seconds)) s") }
        for d in lister.found {
            let s = d as? ICScannerDevice
            print("\(d.name ?? "?") — \(d.productKind ?? "") transport=\(d.transportType ?? "?") location=\(d.locationDescription ?? "?") uuid=\(d.uuidString ?? "?") units=\(s?.availableFunctionalUnitTypes.map(\.intValue) ?? [])")
        }
    }
}
