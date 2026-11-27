import AppKit
import Kit42

/// Headless checks: --info, --export, --convert, --demo, --render.
enum CLI {
    static var status: Int32 = 0

    static func load(_ path: String) throws -> Deck {
        let url = URL(fileURLWithPath: path)
        if url.pathExtension.lowercased() == "key" {
            let archive = try KeynoteArchive(url: url)
            var importer = KeynoteImport(archive)
            let result = try importer.read()
            for warning in result.warnings { FileHandle.standardError.write(Data("Keynote import warning: \(warning)\n".utf8)) }
            return result.deck
        }
        return try DeckIO.read(Data(contentsOf: url), ext: url.pathExtension)
    }

    static func save(_ deck: Deck, _ path: String) throws {
        let url = URL(fileURLWithPath: path)
        try DeckIO.write(deck, ext: url.pathExtension).write(to: url, options: .atomic)
    }

    /// Returns true when the arguments were a CLI command (then the app does not start).
    static func run(_ args: [String]) -> Bool {
        guard args.count > 1 else { return false }
        let cmd = args[1]
        let rest = Array(args.dropFirst(2))
        do {
            switch cmd {
            case "--info":
                for path in rest { print(info(try load(path), name: path)) }
            case "--demo":
                guard let out = rest.first else { usage(); return true }
                try save(Deck.demo(), out)
                print("demo: \(out)")
            case "--convert":
                guard rest.count >= 2 else { usage(); return true }
                let deck = try load(rest[0])
                try save(deck, rest[1])
                print("converted: \(rest[1]) (\(deck.slides.count) slides)")
            case "--export":
                guard rest.count >= 2 else { usage(); return true }
                let deck = try load(rest[0])
                let out = rest[1]
                let notes = rest.contains("--notes")
                let ext = (out as NSString).pathExtension.lowercased()
                if ext == "key" { throw DeckError.invalid("Keynote output is not supported") }
                if ext == "pdf" {
                    try Renderer.pdf(deck, notes: notes, includeHidden: rest.contains("--hidden")).write(to: URL(fileURLWithPath: out), options: .atomic)
                    print("pdf: \(out)")
                } else if ext == "mp4" || ext == "mov" {
                    var o = VideoExport.Options()
                    o.includeHidden = rest.contains("--hidden")
                    if let i = rest.firstIndex(of: "--seconds"), rest.indices.contains(i + 1), let v = Double(rest[i + 1]) { o.secondsPerSlide = v }
                    if let i = rest.firstIndex(of: "--width"), rest.indices.contains(i + 1), let v = Int(rest[i + 1]) { o.width = v }
                    try VideoExport.export(deck, to: URL(fileURLWithPath: out), options: o)
                    print("movie: \(out)")
                } else if ext == "rtf" || ext == "md" {
                    try Outline.export(deck, format: ext).write(to: URL(fileURLWithPath: out), options: .atomic)
                    print("\(ext): \(out)")
                } else {
                    let jpeg = rest.contains("--jpeg")
                    let width = rest.firstIndex(of: "--width").flatMap { rest.indices.contains($0 + 1) ? Int(rest[$0 + 1]) : nil } ?? 1920
                    let urls = try SlideImages.export(deck, to: URL(fileURLWithPath: out), jpeg: jpeg, width: width, includeHidden: rest.contains("--hidden"))
                    print("images: \(urls.count) in \(out)")
                }
            default:
                return false
            }
        } catch {
            FileHandle.standardError.write(Data("error: \(error.localizedDescription)\n".utf8))
            status = 1
        }
        return true
    }

    static func usage() {
        print("""
        PPT42 --info file.pptx|odp …
        PPT42 --export in.pptx out.pdf [--notes] [--hidden] | out-dir [--jpeg] [--width 1920] | out.rtf | out.md | out.mp4 [--seconds 5] [--width 1920]
        PPT42 --convert in.pptx out.odp (and back)
        PPT42 --demo out.pptx|odp
        """)
        status = 2
    }

    static func describe(_ o: SlideObject, indent: String = "    ") -> String {
        var s = "\(indent)#\(o.id) \(o.kind.rawValue)"
        if o.kind == .shape { s += " \(o.geometry)" }
        if let ph = o.placeholder { s += " ph=\(ph.type)\(ph.idx.map { ":\($0)" } ?? "")" }
        s += String(format: " [%.0f,%.0f %.0f×%.0f]", o.frame.minX, o.frame.minY, o.frame.width, o.frame.height)
        if o.rotation != 0 { s += String(format: " rot=%.0f", o.rotation) }
        if let m = o.media { s += " media=\(m)" }
        if let c = o.clip { s += " \(c.audio ? "audio" : "video")=\(c.media)\(c.autoplay ? " autoplay" : "")\(c.loop ? " loop" : "")" }
        if let m = o.chart { s += " chart=\(m.kind.rawValue)/\(m.grouping.rawValue) series=\(m.series.count) categories=\(m.categories.count)\(m.title.map { " title=“\($0)”" } ?? "")" }
        if o.diagram != nil { s += " smartart" }
        if o.custom != nil { s += " custom-geometry" }
        if let t = o.table { s += " table=\(t.rows.count)x\(t.columns.count)" }
        if o.unsupported != nil { s += " unsupported" }
        if let t = o.text, !t.isEmpty {
            let text = t.plainText.replacingOccurrences(of: "\n", with: " ⏎ ").replacingOccurrences(of: "\u{0B}", with: " ↵ ")
            s += " “\(text.prefix(70))”"
        }
        if let t = o.table {
            let cells = t.cells.map { $0.map { $0.text.plainText }.joined(separator: " | ") }.joined(separator: " / ")
            s += " cells: \(cells.prefix(90))"
        }
        for c in o.children { s += "\n" + describe(c, indent: indent + "  ") }
        return s
    }

    static func info(_ d: Deck, name: String) -> String {
        var s = "\(name): \(d.slides.count) slides, \(Int(d.size.width))×\(Int(d.size.height)) pt, theme \(d.theme.name) (\(d.theme.majorFont) / \(d.theme.minorFont)), \(d.layouts.count) layouts, \(d.media.count) media"
        for (i, slide) in d.slides.enumerated() {
            let layout = d.layout(of: slide)?.name ?? "?"
            s += "\n  slide \(i + 1): layout “\(layout)”, \(slide.objects.count) objects"
            if slide.hidden { s += ", hidden" }
            if slide.transition.kind != .none { s += ", transition \(slide.transition.kind.rawValue)" }
            if slide.background != nil { s += ", own background" }
            for o in slide.objects { s += "\n" + describe(o) }
            if !slide.notes.isEmpty { s += "\n    notes: “\(slide.notes.replacingOccurrences(of: "\n", with: " ⏎ ").prefix(80))”" }
            for a in slide.animations { s += "\n    animation #\(a.target) \(a.describe)" }
            for c in slide.comments {
                s += "\n    comment \(c.initials) “\(c.text.prefix(60))”\(c.resolved ? " (resolved)" : "")"
                for r in c.replies { s += "\n      reply \(r.initials) “\(r.text.prefix(60))”" }
            }
        }
        return s
    }
}

/// Reading and writing decks by file extension / type.
enum DeckIO {
    static func read(_ data: Data, ext: String) throws -> Deck {
        let e = ext.lowercased()
        if e == "ppt" || e == "pps" || e == "pot" || data.starts(with: [0xD0, 0xCF, 0x11, 0xE0]) { throw DeckError.legacyPPT }
        var deck: Deck
        if e == "odp" || e == "otp" || isODF(data) { deck = try ODPReader.read(data) } else { deck = try PPTXReader.read(data) }
        deck.normalizeIDs()
        return deck
    }

    static func write(_ deck: Deck, ext: String) throws -> Data {
        var d = deck
        d.pruneMedia()
        switch ext.lowercased() {
        case "odp": return ODPWriter.data(d)
        case "otp": return ODPWriter.data(d, template: true)
        case "pptx": return PPTXWriter.data(d)
        case "potx": return PPTXWriter.data(d, template: true)
        case "pdf": return Renderer.pdf(d)
        default: throw DeckError.invalid(L("Unsupported file type: %@", ext))
        }
    }

    static func isODF(_ data: Data) -> Bool {
        guard let zip = try? ZipArchive(data: data), let mime = zip.string("mimetype") else { return false }
        return mime.trimmingCharacters(in: .whitespacesAndNewlines).hasPrefix("application/vnd.oasis.opendocument.presentation")
    }
}

/// Slides as PNG or JPEG files.
enum SlideImages {
    static func export(_ deck: Deck, to dir: URL, jpeg: Bool, width: Int, includeHidden: Bool = false) throws -> [URL] {
        guard Renderer.bitmapSize(deck: deck, pixelWidth: width) != nil else { throw DeckError.invalid(L("The image size is invalid")) }
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        var out: [URL] = []
        for (i, slide) in deck.slides.enumerated() where includeHidden || !slide.hidden {
            var o = RenderOptions()
            o.number = Renderer.number(of: i, deck: deck)
            guard let cg = Renderer.image(slide, deck: deck, pixelWidth: width, options: o) else { throw DeckError.invalid(L("Could not render the slide")) }
            let rep = NSBitmapImageRep(cgImage: cg)
            guard let data = rep.representation(using: jpeg ? .jpeg : .png, properties: jpeg ? [.compressionFactor: 0.9] : [:]) else { throw DeckError.invalid(L("Could not render the slide")) }
            let url = dir.appendingPathComponent(String(format: "Slide %02d.%@", i + 1, jpeg ? "jpg" : "png"))
            try data.write(to: url, options: .atomic)
            out.append(url)
        }
        return out
    }
}

/// Text outline of a deck: titles and bullets (View › Outline, export to RTF/Markdown).
enum Outline {
    static func bodyObjects(_ s: Slide) -> [SlideObject] {
        s.objects.filter { $0.placeholder != nil && $0.placeholder?.isTitle == false && $0.placeholder?.isFooter == false && $0.text != nil }
    }

    static func markdown(_ deck: Deck) -> String {
        var md = ""
        for (i, s) in deck.slides.enumerated() {
            let title = s.title.replacingOccurrences(of: "\u{0B}", with: " ")
            md += "## \(i + 1). \(title.isEmpty ? L("Untitled") : title)\n\n"
            for o in bodyObjects(s) {
                for p in o.text?.paragraphs ?? [] where !p.text.isEmpty {
                    md += String(repeating: "  ", count: p.level) + "- " + p.text.replacingOccurrences(of: "\u{0B}", with: " ") + "\n"
                }
            }
            if !s.notes.isEmpty { md += "\n> " + s.notes.replacingOccurrences(of: "\n", with: "\n> ") + "\n" }
            md += "\n"
        }
        return md
    }

    static func export(_ deck: Deck, format: String) -> Data {
        if format == "md" { return Data(markdown(deck).utf8) }
        let out = NSMutableAttributedString()
        let titleFont = NSFont(name: "Helvetica Neue Bold", size: 18) ?? .boldSystemFont(ofSize: 18)
        let bodyFont = NSFont(name: "Helvetica Neue", size: 13) ?? .systemFont(ofSize: 13)
        for (i, s) in deck.slides.enumerated() {
            let ps = NSMutableParagraphStyle()
            ps.paragraphSpacingBefore = i == 0 ? 0 : 14
            ps.paragraphSpacing = 4
            out.append(NSAttributedString(string: "\(i + 1). \(s.title.isEmpty ? L("Untitled") : s.title.replacingOccurrences(of: "\u{0B}", with: " "))\n",
                                          attributes: [.font: titleFont, .paragraphStyle: ps]))
            for o in bodyObjects(s) {
                for p in o.text?.paragraphs ?? [] where !p.text.isEmpty {
                    let bp = NSMutableParagraphStyle()
                    bp.headIndent = CGFloat(p.level + 1) * 18 + 12
                    bp.firstLineHeadIndent = CGFloat(p.level + 1) * 18
                    bp.tabStops = [NSTextTab(textAlignment: .left, location: bp.headIndent)]
                    out.append(NSAttributedString(string: "•\t" + p.text.replacingOccurrences(of: "\u{0B}", with: " ") + "\n", attributes: [.font: bodyFont, .paragraphStyle: bp]))
                }
            }
        }
        return (try? out.data(from: NSRange(location: 0, length: out.length), documentAttributes: [.documentType: NSAttributedString.DocumentType.rtf])) ?? Data()
    }
}
