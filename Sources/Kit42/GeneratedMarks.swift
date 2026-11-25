import AppKit

/// A generated signature or seal kept in the library as a recipe (next to its preview PNG), so that every
/// insertion is a new impression of the same mark: same letters and layout, different pressure, grain and angle.
public struct GeneratedMark: Codable {
    public enum Kind: String, Codable { case signature, seal, image, learned }

    public var kind: Kind
    /// Signature: the name as typed.
    public var name: String?
    /// Seal: catalogue template and the texts typed for its fields.
    public var template: String?
    public var values: [String: String]?
    /// Imported stamp: file name in the library's Sources folder, and whether it is round (tilts more).
    public var source: String?
    public var round: Bool?
    /// Signatures: pen type (PenType raw value; ballpoint when absent).
    public var pen: Int?
    /// A signature learned from the user's own samples.
    public var learned: LearnedSignature?
    /// sRGB red, green, blue, alpha.
    public var ink: [Double]
    /// Stored as text: JSON numbers lose precision above 2^53.
    public var design: String

    public static func signature(_ name: String, ink: NSColor, design: UInt64, pen: PenType = .ballpoint) -> GeneratedMark {
        var m = GeneratedMark(kind: .signature, name: name, ink: components(ink), design: String(design))
        m.pen = pen.rawValue
        return m
    }

    public static func learned(_ signature: LearnedSignature, ink: NSColor, pen: PenType) -> GeneratedMark {
        var m = GeneratedMark(kind: .learned, ink: components(ink), design: "0")
        m.learned = signature
        m.pen = pen.rawValue
        return m
    }

    public var penType: PenType { pen.flatMap(PenType.init(rawValue:)) ?? .ballpoint }

    public static func seal(_ spec: SealSpec) -> GeneratedMark {
        GeneratedMark(kind: .seal, template: spec.template.id, values: spec.values, ink: components(spec.ink), design: String(spec.designSeed))
    }

    public var inkColor: NSColor {
        let c = ink + [1, 1, 1, 1]
        return NSColor(srgbRed: c[0], green: c[1], blue: c[2], alpha: c[3])
    }

    public var designSeed: UInt64 { UInt64(design) ?? 1 }

    public var sealSpec: SealSpec? {
        guard kind == .seal, let template = template.flatMap(SealTemplate.named) else { return nil }
        return SealSpec(template: template, values: values ?? [:], ink: inkColor, designSeed: designSeed)
    }

    /// Default resolution of impressions relative to the generator's preview: about 600 dpi for a 6 cm signature and
    /// over 1000 dpi for a 4 cm seal. Insertion chooses its own scale (`MarkInsertDialog`).
    public static let outputScale: CGFloat = 2

    /// A new impression (a fresh seed unless one is given). `scale`: pixels per pixel of the generator's preview;
    /// the same seed gives the same impression at every scale.
    public func impression(_ seed: UInt64 = SeededRandom.freshSeed(), scale: CGFloat = outputScale) -> CGImage? {
        let k = scale
        switch kind {
        case .signature:
            return SignatureGenerator.render(name ?? "", design: designSeed, impression: seed, ink: inkColor, pen: penType, pixelHeight: 130 * k)
        case .learned:
            return learned?.render(impression: seed, ink: inkColor, pen: penType, pixelHeight: 120 * k)
        case .seal:
            return sealSpec.flatMap { SealGenerator.render($0, impression: seed, scale: k) }
        case .image:
            guard let source, let image = StampLibrary.sourceImage(source) else { return nil }
            return SealGenerator.impress(image, round: round ?? false, impression: seed, scale: k)
        }
    }

    private static func components(_ color: NSColor) -> [Double] {
        let c = color.usingColorSpace(.sRGB) ?? color
        return [c.redComponent, c.greenComponent, c.blueComponent, c.alphaComponent].map(Double.init)
    }
}

extension StampLibrary {
    /// Folder for the original files of imported stamps (SVG, PDF, images).
    public var sourcesFolder: URL {
        let url = folder.appendingPathComponent("Sources", isDirectory: true)
        try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }

    private static var sourceCache: [String: CGImage] = [:]

    /// The imported stamp as a bitmap: vector files are drawn at 2400 px, scans at their own size; scans lose their white paper.
    static func sourceImage(_ name: String) -> CGImage? {
        if let cached = sourceCache[name] { return cached }
        let url = shared.sourcesFolder.appendingPathComponent(name)
        guard let image = rasterize(url) else { return nil }
        sourceCache[name] = image
        return image
    }

    /// Any image, SVG or PDF as a transparent bitmap (vector: 2400 px on the long side; raster: ≤ 2400 px). Raster files without transparency
    /// get their white background removed, as for a scanned stamp.
    public static func rasterize(_ url: URL) -> CGImage? {
        guard let picture = NSImage(contentsOf: url), picture.size.width > 0, picture.size.height > 0 else { return nil }
        let vector = ["svg", "pdf", "eps"].contains(url.pathExtension.lowercased())
        var size = picture.size
        if let rep = picture.representations.first, rep.pixelsWide > 0, !vector {
            size = NSSize(width: rep.pixelsWide, height: rep.pixelsHigh)
        }
        let k = min(2400 / max(size.width, size.height), vector ? 2400 / max(size.width, size.height) : 1)
        let w = Int(size.width * k), h = Int(size.height * k)
        guard w > 0, h > 0, let ctx = InkDraw.context(w, h) else { return nil }
        let gc = NSGraphicsContext(cgContext: ctx, flipped: false)
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = gc
        picture.draw(in: NSRect(x: 0, y: 0, width: w, height: h))
        NSGraphicsContext.restoreGraphicsState()
        guard let image = ctx.makeImage() else { return nil }
        if !vector, image.alphaInfo == .none || !hasTransparency(image) {
            return ImageTools.removeWhiteBackground(image) ?? image
        }
        return image
    }

    private static func hasTransparency(_ image: CGImage) -> Bool {
        guard let ctx = InkDraw.context(image.width, image.height), let data = ctx.data?.assumingMemoryBound(to: UInt8.self) else { return false }
        ctx.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
        let n = image.width * image.height
        var i = 0
        while i < n { if data[i * 4 + 3] < 250 { return true }; i += 7 }
        return false
    }

    /// Imports a stamp file: keeps the original and stores a recipe, so each insertion is a new impression.
    @discardableResult
    public func importStamp(_ url: URL) throws -> URL {
        let name = UUID().uuidString + "." + url.pathExtension.lowercased()
        try FileManager.default.copyItem(at: url, to: sourcesFolder.appendingPathComponent(name))
        guard let image = Self.sourceImage(name) else { throw NSError(domain: NSCocoaErrorDomain, code: NSFileReadCorruptFileError) }
        let ratio = CGFloat(image.width) / CGFloat(max(1, image.height))
        var mark = GeneratedMark(kind: .image, ink: [0, 0, 0, 1], design: "0")
        mark.source = name
        mark.round = abs(ratio - 1) < 0.15
        guard let preview = mark.impression() else { throw NSError(domain: NSCocoaErrorDomain, code: NSFileReadCorruptFileError) }
        return try add(preview, mark: mark)
    }

    /// Adds a generated mark: its preview PNG plus a JSON recipe with the same name.
    @discardableResult
    public func add(_ preview: CGImage, mark: GeneratedMark) throws -> URL {
        let url = try add(preview)
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        try encoder.encode(mark).write(to: Self.recipeURL(for: url))
        return url
    }

    public static func recipeURL(for url: URL) -> URL { url.deletingPathExtension().appendingPathExtension("json") }

    public static func mark(at url: URL) -> GeneratedMark? {
        guard let data = try? Data(contentsOf: recipeURL(for: url)) else { return nil }
        return try? JSONDecoder().decode(GeneratedMark.self, from: data)
    }

    /// What to insert for a library item: a new impression for generated marks, the stored picture otherwise.
    public static func impression(at url: URL) -> CGImage? {
        mark(at: url)?.impression() ?? image(at: url)
    }
}

/// Sample sheet for review and regression checks: `PDF42 --mark-samples DIR` writes PNGs of several signature
/// designs (four copies each, to compare natural variation) and every seal style (two impressions each).
public enum MarkSamples {
    public static func write(to folder: URL) throws {
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        func save(_ image: CGImage?, _ name: String) throws {
            guard let image, let data = NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:]) else { return }
            try data.write(to: folder.appendingPathComponent(name + ".png"))
        }
        // Each name in every form (one style per form) and four copies of each style.
        let names = ["Sergey Zorin", "Иван Петров", "Ayşe Yılmaz", "John A. Smith"]
        for (n, name) in names.enumerated() {
            for (f, form) in SignatureForm.allCases.dropFirst().enumerated() {
                // A fixed style per name and form, so the sheet is stable between runs.
                let design = SignatureGenerator.design(form: form, variant: n * 3 + f, seed: UInt64(31 + n * 7 + f))
                for c in 0..<4 {
                    try save(SignatureGenerator.render(name, design: design, impression: UInt64(c + 1) &* 7919, ink: InkColor.blue.pen),
                             "signature-\(n)-form\(f)-copy\(c)")
                }
            }
        }
        // Learning: three recorded samples of one hand (with timing, as from the trackpad) → four new copies;
        // and the same hand from a picture (as a scan or an iPhone sketch) → four copies made from its ink.
        let design = SignatureGenerator.design(form: .initialSurname, variant: 1, seed: 501)
        let recorded = (0..<3).compactMap { SignatureGenerator.recordedSample("Sergey Zorin", design: design, impression: UInt64(900 + $0)) }
        let learned = LearnedSignature(samples: recorded)
        for c in 0..<4 { try save(learned.render(impression: UInt64(c + 1) &* 6151, ink: InkColor.blue.pen, pen: .ballpoint), "learned-timed-copy\(c)") }
        if let picture = SignatureGenerator.render("Sergey Zorin", design: design, impression: 77, ink: InkColor.black.pen) {
            try save(picture, "learned-source-picture")
            if let traced = SignatureSample.from(picture: picture) {
                let fromPicture = LearnedSignature(samples: [traced])
                for c in 0..<4 { try save(fromPicture.render(impression: UInt64(c + 11) &* 6151, ink: InkColor.blue.pen, pen: .ballpoint), "learned-traced-copy\(c)") }
            }
        }
        // Pens: one signature in each pen type.
        for pen in PenType.allCases {
            try save(SignatureGenerator.render("Sergey Zorin", design: design, impression: 5, ink: InkColor.blue.pen, pen: pen), "pen-\(pen.rawValue)")
        }
        // An imported SVG stamp, three impressions (the import path used for the user's own stamps).
        let svg = folder.appendingPathComponent("imported-stamp.svg")
        try """
        <svg xmlns="http://www.w3.org/2000/svg" width="400" height="400" viewBox="0 0 400 400">
        <circle cx="200" cy="200" r="186" fill="none" stroke="#2a4fb3" stroke-width="10"/>
        <circle cx="200" cy="200" r="150" fill="none" stroke="#2a4fb3" stroke-width="4"/>
        <text x="200" y="215" font-family="Helvetica" font-weight="bold" font-size="48" fill="#2a4fb3" text-anchor="middle">MY STAMP</text>
        </svg>
        """.write(to: svg, atomically: true, encoding: .utf8)
        if let picture = StampLibrary.rasterize(svg) {
            for c in 0..<3 { try save(SealGenerator.impress(picture, round: true, impression: UInt64(c + 3) &* 7_919), "imported-svg-copy\(c)") }
            try save(SealGenerator.impress(picture, round: true, impression: 3 &* 7_919, scale: GeneratedMark.outputScale), "hires-imported-svg")
        }
        // The resolution actually inserted into documents (GeneratedMark.impression): same impressions as above, sharper.
        let k = GeneratedMark.outputScale
        try save(SignatureGenerator.render("Sergey Zorin", design: design, impression: 5, ink: InkColor.blue.pen, pixelHeight: 130 * k), "hires-signature")
        try save(learned.render(impression: 6151, ink: InkColor.blue.pen, pen: .ballpoint, pixelHeight: 120 * k), "hires-learned-timed")
        if let template = SealTemplate.all.first {
            let spec = SealSpec(template: template, values: [:], ink: template.ink.stampPad, designSeed: 42)
            try save(SealGenerator.render(spec, impression: 104_729, scale: k), "hires-seal-000")
        }
        try? FileManager.default.removeItem(at: svg)
        // Every catalogue template once (its example texts), plus a second impression of a few to show variation.
        for (i, template) in SealTemplate.all.enumerated() {
            let spec = SealSpec(template: template, values: [:], ink: template.ink.stampPad, designSeed: 42)
            try save(SealGenerator.render(spec, impression: UInt64(i + 1) &* 104_729), String(format: "seal-%03d-%@", i, template.id))
            if i % 20 == 0 { try save(SealGenerator.render(spec, impression: UInt64(i + 7) &* 15_485_863), String(format: "seal-%03d-%@-b", i, template.id)) }
        }
    }
}
