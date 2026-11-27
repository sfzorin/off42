import AppKit
import Kit42
import UniformTypeIdentifiers

/// Command-line hooks for headless tests:
///   --stats file                                  counts (pages, words, characters, paragraphs, lines)
///   --toc in out                                  insert or update the table of contents
///   --replace in out find repl [--case] [--words] [--regex]
///   --demo out                                    a document with footnotes, comments, links, sections, columns and a TOC
///   --dump file                                   pages, footnotes, comments, links, sections, bookmarks
enum CLI {
    static func open(_ path: String) throws -> TextDocument {
        let url = URL(fileURLWithPath: path)
        let doc = TextDocument()
        let ext = url.pathExtension.lowercased()
        let type: String
        switch ext {
        // Headless binaries may receive a dynamic UTI until LaunchServices knows their bundle.
        // The reader already understands these extensions; never interpret an OOXML ZIP as text.
        case "docx", "doc", "odt", "rtf", "rtfd", "html", "md", "markdown", "txt", "epub": type = ext
        case "htm": type = "html"
        default: type = UTType(filenameExtension: ext)?.identifier ?? ext
        }
        try doc.read(from: url, ofType: type)
        return doc
    }

    static func save(_ doc: TextDocument, _ path: String) throws {
        let ext = URL(fileURLWithPath: path).pathExtension.lowercased()
        let data = ext == "epub" ? try doc.epubData() : ext == "pdf" ? doc.pdfData() : ext == "md" ? doc.markdownData() : try doc.exportData(TextDocument.documentType(for: ext == "doc" ? "com.microsoft.word.doc" : ext))
        try data.write(to: URL(fileURLWithPath: path))
        print("\(ext): \(data.count) bytes")
    }

    /// Handles a test command; false when the arguments are not one.
    static func run(_ args: [String]) throws -> Bool {
        func arg(_ name: String, _ count: Int) -> [String]? {
            guard let i = args.firstIndex(of: name), args.count > i + count else { return nil }
            return Array(args[(i + 1)...(i + count)])
        }
        if let a = arg("--stats", 1) {
            let doc = try open(a[0])
            let pages = doc.offscreenPages()
            defer { pages.detach() }
            let s = TextStats.of(doc.storage, NSRange(location: 0, length: doc.storage.length), layout: pages.layoutManager)
            print("pages \(pages.cards.count)\nwords \(s.words)\ncharacters \(s.characters)\ncharacters-no-spaces \(s.charactersNoSpaces)\nparagraphs \(s.paragraphs)\nlines \(s.lines)")
            return true
        }
        if let a = arg("--toc", 2) {
            let doc = try open(a[0])
            let pages = doc.offscreenPages()
            defer { pages.detach() }
            let storage = doc.storage
            TOC.make(storage, paged: pages, at: TOC.range(in: storage)?.location ?? firstBodyParagraph(storage)) { range, text, attributeRanges in
                if !attributeRanges.isEmpty, let text {
                    for r in attributeRanges { storage.addAttribute(.docBookmark, value: text.attribute(.docBookmark, at: r.location, effectiveRange: nil)!, range: r) }
                } else if let text {
                    storage.replaceCharacters(in: range, with: text)
                }
            }
            try save(doc, a[1])
            return true
        }
        if let a = arg("--replace", 4) {
            let doc = try open(a[0])
            let o = FindOptions(matchCase: args.contains("--case"), wholeWords: args.contains("--words"), regex: args.contains("--regex"))
            let n = TextSearch.replaceAll(doc.storage, find: a[2], with: a[3], o)
            print("replaced \(n)")
            try save(doc, a[1])
            return true
        }
        if let a = arg("--demo", 1) {
            let doc = TextDocument()
            doc.storage.setAttributedString(demo())
            addWriterFeatures(doc)
            let storage = doc.storage
            numberDemoHeadings(doc)
            let pages = doc.offscreenPages()
            TOC.make(storage, paged: pages, at: firstBodyParagraph(storage)) { range, text, attributeRanges in
                if !attributeRanges.isEmpty, let text {
                    for r in attributeRanges { storage.addAttribute(.docBookmark, value: text.attribute(.docBookmark, at: r.location, effectiveRange: nil)!, range: r) }
                } else if let text {
                    storage.replaceCharacters(in: range, with: text)
                }
            }
            pages.detach()
            try save(doc, a[0])
            return true
        }
        // --pdf in out [--pages 1-2] [--quality 0.5] [--comments] [--no-bookmarks]: Export as PDF with options.
        if let a = arg("--pdf", 2) {
            let doc = try open(a[0])
            var o = PDFOptions()
            o.pages = arg("--pages", 1)?[0] ?? ""
            o.quality = arg("--quality", 1).flatMap { Double($0[0]) }.map { CGFloat($0) } ?? 1
            o.comments = args.contains("--comments")
            o.bookmarks = !args.contains("--no-bookmarks")
            let data = try doc.pdfData(o)
            try data.write(to: URL(fileURLWithPath: a[1]))
            print("pdf: \(data.count) bytes")
            return true
        }
        if let a = arg("--dump", 1) {
            let doc = try open(a[0])
            dump(doc)
            return true
        }
        return false
    }

    /// Paragraph after the first one (the title).
    static func firstBodyParagraph(_ s: NSAttributedString) -> Int {
        let ns = s.string as NSString
        return ns.length == 0 ? 0 : NSMaxRange(ns.paragraphRange(for: NSRange(location: 0, length: 0)))
    }

    static func demo() -> NSAttributedString {
        let out = NSMutableAttributedString()
        func para(_ text: String, _ style: Int = 4, word: String? = nil) {
            var a = Styles.all[style].attributes
            if let word { a[Styles.styleKey] = word }
            out.append(NSAttributedString(string: text + "\n", attributes: a))
        }
        let body = Styles.normal.attributes
        para("Feature Test", 0, word: "Title")
        para("Introduction", 1, word: "Heading1")
        // Footnote, link and comment in one paragraph.
        out.append(NSAttributedString(string: "This sentence has a footnote", attributes: body))
        out.append(Notes.reference(DocNote("First footnote: the note text at the bottom of the page."), attributes: body))
        out.append(NSAttributedString(string: " and a ", attributes: body))
        var link = body
        link[.link] = URL(string: "https://example.com/")!
        link[.foregroundColor] = DocWindowController.linkColor
        link[.underlineStyle] = NSUnderlineStyle.single.rawValue
        out.append(NSAttributedString(string: "link to example.com", attributes: link))
        out.append(NSAttributedString(string: ". Here is ", attributes: body))
        var commented = body
        commented[.docComment] = DocComment(author: "Test Reviewer", date: Date(timeIntervalSince1970: 1_700_000_000), text: "Please check this phrase.")
        out.append(NSAttributedString(string: "commented text", attributes: commented))
        out.append(NSAttributedString(string: " in a paragraph.\n", attributes: body))
        para("Details", 2, word: "Heading2")
        para("Special characters: — – « » „ “ ” № § © ® ™ ° ± × ÷ ≤ ≥ ≠ ≈ € ₽ ₺ $ £ … • → ← ↑ ↓ ½ ¼ ¾ ¹ ² ³ µ Ω π α β.")
        // Section 1 ends (portrait); section 2 is landscape with two columns.
        var brk = body
        brk[.docSection] = PageSetup().section
        out.append(NSAttributedString(string: "\u{000C}\n", attributes: brk))
        out.removeAttribute(.docSection, range: NSRange(location: out.length - 1, length: 1))
        para("Landscape Section", 1, word: "Heading1")
        let lorem = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. "
        for k in 1...7 { para("Column paragraph \(k). " + lorem + lorem) }
        var landscape = PageSetup()
        landscape.size = NSSize(width: 842, height: 595)
        landscape.columns = 2
        brk[.docSection] = landscape.section
        out.append(NSAttributedString(string: "\u{000C}\n", attributes: brk))
        out.removeAttribute(.docSection, range: NSRange(location: out.length - 1, length: 1))
        para("Conclusion", 1, word: "Heading1")
        out.append(NSAttributedString(string: "Back to portrait. A second note", attributes: body))
        out.append(Notes.reference(DocNote("Second footnote."), attributes: body))
        var resolved = body
        resolved[.docComment] = DocComment(author: "Test Reviewer", date: Date(timeIntervalSince1970: 1_700_000_000), text: "Done.", resolved: true)
        out.append(NSAttributedString(string: " and a resolved comment", attributes: resolved))
        out.append(NSAttributedString(string: ".\n", attributes: body))
        return out
    }

    static func dump(_ doc: TextDocument) {
        let s = doc.storage
        let ns = s.string as NSString
        let pages = doc.offscreenPages()
        defer { pages.detach() }
        print("setup \(doc.setup.size) columns \(doc.setup.columns)")
        for (i, c) in pages.cards.enumerated() {
            print("page \(i + 1): \(Int(c.setup.size.width))x\(Int(c.setup.size.height)) columns \(c.textViews.count) notes \(c.notes.map { "\($0.0)" }.joined(separator: ","))")
        }
        for (k, (r, n)) in Notes.all(in: s).enumerated() { print("footnote \(k + 1) at \(r.location) page \(pages.pageNumber(at: r.location)): \(n.text)") }
        for (r, (c)) in commentsOf(s) {
            print("comment \"\(ns.substring(with: r))\" by \(c.author) resolved=\(c.resolved): \(c.text)")
        }
        if doc.trackChanges { print("track changes on") }
        for c in Revisions.changes(in: s) {
            print("\(c.kind.rawValue) by \(c.author) at \(c.range.location): \"\(ns.substring(with: c.range).replacingOccurrences(of: "\n", with: "¶"))\"")
        }
        s.enumerateAttribute(.link, in: NSRange(location: 0, length: s.length)) { v, r, _ in
            guard let v else { return }
            print("link \"\(ns.substring(with: r).replacingOccurrences(of: "\t", with: "→"))\" → \((v as? URL)?.absoluteString ?? "\(v)")")
        }
        for (i, sec) in pages.sections { print("section break at \(i): \(Int(sec.pageSize.width))x\(Int(sec.pageSize.height)) columns \(sec.columns)") }
        s.enumerateAttribute(.docBookmark, in: NSRange(location: 0, length: s.length)) { v, r, _ in
            if let v { print("bookmark \(v): \(ns.substring(with: r).trimmingCharacters(in: .newlines))") }
        }
        if let r = TOC.range(in: s) { print("toc:\n" + ns.substring(with: r).replacingOccurrences(of: "\t", with: " .... ")) }
        dumpWriterFeatures(doc, pages)
    }

    static func commentsOf(_ s: NSAttributedString) -> [(NSRange, DocComment)] {
        var out: [(NSRange, DocComment)] = []
        s.enumerateAttribute(.docComment, in: NSRange(location: 0, length: s.length)) { v, r, _ in
            guard let c = v as? DocComment else { return }
            if let i = out.firstIndex(where: { $0.1 === c }) { out[i].0 = NSUnionRange(out[i].0, r) } else { out.append((r, c)) }
        }
        return out
    }
}
