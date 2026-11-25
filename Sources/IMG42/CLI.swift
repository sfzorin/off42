import AppKit
import ImageIO
import UniformTypeIdentifiers

/// Headless checks used while developing (not localised: developer output).
///
///     IMG42 --info a.jpg b.gif …            format, size, frames/pages, orientation, alpha, error
///     IMG42 --edit in out [--brightness 20 --contrast 30 … --look sepia --auto --demo-marks
///                         --rotate right --flip h --resize 800x600 --crop x,y,w,h --remove-background --description "…"]
///     IMG42 --stats file                    mean luminance / alpha
///     IMG42 --ocr file                      recognised text
///     IMG42 --rotate file left|right        lossless orientation change
///     IMG42 --types                         readable / writable types
///     IMG42 --smart-lasso in "x,y;x,y;…" out.png [radius] [--no-subject]   Smart Lasso selection mask for a stroke
///     IMG42 --smart-lasso-eval in [--gt mask.png] [--radius r] [--noise a] [--seed n] [--vis out.png]
///                                           simulated rough strokes around the subject: IoU of plain vs Smart Lasso
enum CLI {
    static var status: Int32 = 0

    static func run(_ args: [String]) -> Bool {
        guard args.count > 1 else { return false }
        status = 0
        switch args[1] {
        case "--info":
            guard args.count > 2 else { print("usage: --info files…"); status = 2; return true }
            info(Array(args.dropFirst(2)))
        case "--edit": edit(Array(args.dropFirst(2)))
        case "--stats", "--ocr":
            guard args.count > 2 else { print("usage: \(args[1]) files…"); status = 2; return true }
            if args[1] == "--stats" { args.dropFirst(2).forEach(stats) } else { args.dropFirst(2).forEach(ocr) }
        case "--rotate": rotate(Array(args.dropFirst(2)))
        case "--smart-lasso": smartLasso(Array(args.dropFirst(2)))
        case "--smart-lasso-eval": SmartLassoEval.run(Array(args.dropFirst(2)))
        case "--types":
            guard args.count == 2 else { print("usage: --types"); status = 2; return true }
            print("read:", ImageFormats.readableTypes.map(\.identifier).joined(separator: " "))
            print("write:", ImageWriter.writableTypes.map(\.identifier).joined(separator: " "))
        default:
            if args[1].hasPrefix("--"), !["--uitest", "--selftest", "--audit"].contains(args[1]) {
                print("unknown command \(args[1])"); status = 2; return true
            }
            return false
        }
        return true
    }

    static func info(_ files: [String]) {
        for f in files {
            let url = URL(fileURLWithPath: f)
            let l = ImageLoader.load(url)
            var parts = [url.lastPathComponent, l.typeIdentifier ?? "?"]
            if let image = l.image {
                parts.append("\(image.width)x\(image.height)")
                parts.append(l.isAnimated ? "frames=\(l.pages.count)" : "pages=\(l.pageCount)")
                parts.append("orientation=\(l.orientation.rawValue)")
                parts.append(l.hasAlpha ? "alpha" : "opaque")
                parts.append("OK")
            } else {
                parts.append("ERROR: \(l.error ?? "?")")
                status = 1
            }
            print(parts.joined(separator: "\t"))
        }
    }

    enum Failure: Error { case invalid(String) }

    static func edit(_ a: [String]) {
        guard a.count >= 2 else { print("usage: --edit in out [options]"); status = 2; return }
        let input = URL(fileURLWithPath: a[0]), output = URL(fileURLWithPath: a[1])
        let loaded = ImageLoader.load(input)
        guard var base = loaded.image, !loaded.wasDownsampled else { print("cannot read full-resolution image \(a[0])"); status = 1; return }
        guard let type = UTType(filenameExtension: output.pathExtension), ImageWriter.writableTypes.contains(type) else {
            print("unsupported output format \(output.pathExtension)"); status = 2; return
        }
        var adjustments = Adjustments()
        var marks: [Mark] = []
        var description: String?? = .none
        var i = 2
        func value() throws -> String {
            guard i + 1 < a.count else { throw Failure.invalid("missing value for " + a[i]) }
            i += 1; return a[i]
        }
        func numbers(_ text: String, separator: Character, count: Int) throws -> [Double] {
            let parts = text.split(separator: separator, omittingEmptySubsequences: false)
            let values = parts.compactMap { Double($0) }
            guard parts.count == count, values.count == count, values.allSatisfy(\.isFinite) else { throw Failure.invalid("invalid numeric dimensions") }
            return values
        }
        do {
            while i < a.count {
                let opt = a[i]
                if opt.hasPrefix("--"), let key = Adjustments.Key(rawValue: String(opt.dropFirst(2))) {
                    guard let number = Double(try value()), number.isFinite, key.range.contains(number) else { throw Failure.invalid("invalid value for " + opt) }
                    adjustments[key] = number
                } else {
                    switch opt {
                    case "--look":
                        let selected = try value()
                        guard let look = ["none": Adjustments.Look.none, "mono": .mono, "noir": .noir, "sepia": .sepia, "invert": .invert][selected] else { throw Failure.invalid("unknown look " + selected) }
                        adjustments.look = look
                    case "--auto": adjustments.autoEnhance = true
                    case "--demo-marks": marks = Mark.demo(for: CGSize(width: base.width, height: base.height))
                    case "--rotate", "--flip":
                        let selected = try value()
                        let operations: [String: Orientation.Op] = opt == "--rotate" ? ["left": .rotateLeft, "right": .rotateRight] : ["h": .flipHorizontal, "v": .flipVertical]
                        guard let operation = operations[selected] else { throw Failure.invalid("invalid operation " + selected) }
                        let transform = Orientation.transform(operation, size: CGSize(width: base.width, height: base.height))
                        marks = marks.map { $0.applying(transform) }
                        base = Orientation.apply(operation.orientation, to: base)
                    case "--resize":
                        let dimensions = try numbers(value(), separator: "x", count: 2)
                        let size = CGSize(width: dimensions[0].rounded(), height: dimensions[1].rounded())
                        guard ImageLoader.validPixelSize(size), let resized = ImageLoader.render(size: size, { $0.draw(base, in: CGRect(origin: .zero, size: size)) }) else { throw Failure.invalid("invalid image size") }
                        let transform = CGAffineTransform(scaleX: size.width / CGFloat(base.width), y: size.height / CGFloat(base.height))
                        marks = marks.map { $0.applying(transform) }; base = resized
                    case "--crop":
                        let dimensions = try numbers(value(), separator: ",", count: 4)
                        let rect = CGRect(x: dimensions[0], y: dimensions[1], width: dimensions[2], height: dimensions[3])
                        guard rect.width > 0, rect.height > 0,
                              let selection = PixelSelection.from(path: CGPath(rect: rect, transform: nil), size: CGSize(width: base.width, height: base.height), isRectangle: true),
                              let cropped = PixelOps.extract(selection, from: base) else { throw Failure.invalid("invalid crop rectangle") }
                        let transform = CGAffineTransform(translationX: -selection.bounds.integral.minX, y: -selection.bounds.integral.minY)
                        marks = marks.map { $0.applying(transform) }; base = cropped
                    case "--remove-background":
                        guard let removed = try PixelOps.removeBackground(base) else { throw Failure.invalid("no subject found") }
                        base = removed
                    case "--description": description = .some(try value())
                    default: throw Failure.invalid("unknown option " + opt)
                    }
                }
                i += 1
            }
            guard let adjusted = adjustments.renderChecked(base), let final = Mark.flattenChecked(marks, onto: adjusted) else {
                throw Failure.invalid("image rendering failed")
            }
            try ImageWriter.write(final, to: output, type: type.identifier, metadataFrom: input, description: description)
            print("wrote \(output.lastPathComponent) \(final.width)x\(final.height) \(type.identifier)")
        } catch Failure.invalid(let message) { print(message); status = 2 }
        catch { print("error: \(error.localizedDescription)"); status = 1 }
    }

    static func stats(_ f: String) {
        guard let image = ImageLoader.load(URL(fileURLWithPath: f)).image else { print("\(f): cannot read"); status = 1; return }
        let w = image.width, h = image.height
        guard let ctx = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
                                  space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue),
              let data = ctx.data else { return }
        ctx.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
        let px = data.bindMemory(to: UInt8.self, capacity: w * h * 4)
        var lum = 0.0, alpha = 0.0, sat = 0.0
        for i in 0..<(w * h) {
            let r = Double(px[i * 4]), g = Double(px[i * 4 + 1]), b = Double(px[i * 4 + 2])
            lum += 0.2126 * r + 0.7152 * g + 0.0722 * b
            sat += max(r, g, b) - min(r, g, b)
            alpha += Double(px[i * 4 + 3])
        }
        let n = Double(w * h)
        print(String(format: "%@\t%dx%d\tluminance=%.1f\tchroma=%.1f\talpha=%.1f", (f as NSString).lastPathComponent, w, h, lum / n, sat / n, alpha / n))
    }

    static func ocr(_ f: String) {
        guard let image = ImageLoader.load(URL(fileURLWithPath: f)).image else { print("\(f): cannot read"); status = 1; return }
        for line in PixelOps.recognizeText(image) { print(line.text, "\t", line.box.integral) }
    }

    static func rotate(_ a: [String]) {
        guard a.count == 2, ["left", "right", "h", "v"].contains(a[1]) else { print("usage: --rotate file left|right|h|v"); status = 2; return }
        let url = URL(fileURLWithPath: a[0])
        let loaded = ImageLoader.load(url)
        guard loaded.image != nil else { print("cannot read \(a[0])"); status = 1; return }
        let current = loaded.orientation
        let op: Orientation.Op = a[1] == "left" ? .rotateLeft : a[1] == "right" ? .rotateRight : a[1] == "v" ? .flipVertical : .flipHorizontal
        let new = Orientation.compose(op, after: current)
        let ok = Orientation.rewriteLosslessly(url, orientation: new)
        print("\(url.lastPathComponent): orientation \(current.rawValue) → \(new.rawValue) \(ok ? "lossless OK" : "FAILED")")
        if !ok { status = 1 }
    }

    static func smartLasso(_ a: [String]) {
        guard (3...5).contains(a.count), let image = ImageLoader.load(URL(fileURLWithPath: a[0])).image else { print("usage: --smart-lasso in \"x,y;…\" out.png [radius] [--no-subject]"); status = 2; return }
        let pairs = a[1].split(separator: ";", omittingEmptySubsequences: false).map { $0.split(separator: ",", omittingEmptySubsequences: false).compactMap { Double($0) } }
        guard pairs.count >= 3, pairs.allSatisfy({ $0.count == 2 && $0.allSatisfy(\.isFinite) }) else { print("invalid stroke"); status = 2; return }
        let pts = pairs.map { CGPoint(x: $0[0], y: $0[1]) }
        let extras = a.dropFirst(3).filter { $0 != "--no-subject" }
        guard extras.count <= 1, let radius = extras.first.flatMap(Double.init) ?? (extras.isEmpty ? 12 : nil),
              radius.isFinite, radius > 0, radius <= 32768 else { print("invalid radius"); status = 2; return }
        let noSubject = a.contains("--no-subject")
        let start = Date()
        let subject = noSubject ? nil : SmartLasso.subjectMask(for: image)
        let t1 = Date()
        guard let path = SmartLasso.refine(stroke: pts, radius: CGFloat(radius), image: image, subject: subject),
              let sel = PixelSelection.from(path: path, size: CGSize(width: image.width, height: image.height)) else { print("no selection"); status = 1; return }
        let t2 = Date()
        do {
            try ImageWriter.write(sel.mask, to: URL(fileURLWithPath: a[2]), type: "public.png")
            print(String(format: "wrote %@ bounds=%@ subject=%.2fs refine=%.3fs", a[2], NSStringFromRect(sel.bounds.integral), t1.timeIntervalSince(start), t2.timeIntervalSince(t1)))
        } catch { print("error: \(error)"); status = 1 }
    }
}
