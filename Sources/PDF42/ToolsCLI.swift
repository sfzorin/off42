import AppKit
import PDFKit

/// Headless modes for the page tools (testing without the UI):
///   PDF42 --watermark in.pdf out.pdf "TEXT" [pages]      PDF42 --number-pages in.pdf out.pdf [format 0|1|2] [position 0…5] [skip-first]
///   PDF42 --split in.pdf outdir 1-2,3 | every:N | each   PDF42 --merge out.pdf a.pdf b.png …
///   PDF42 --compare a.pdf b.pdf [marked.pdf]                      PDF42 --edit-text in.pdf out.pdf "old" "new"
///   PDF42 --redact in.pdf out.pdf x,y,w,h[@page] … [white]   (page space, pages from 1; prints what was removed)
///   PDF42 --add-field in.pdf out.pdf                     PDF42 --fill in.pdf out.pdf name=value …     PDF42 --fields file.pdf
/// Document tools (DocumentCLI.swift): --bookmarks, --crop, --page-size, --link, --autolink, --props, --attach, --detach,
///   --attachments, --pdfa, --bates, --header-footer, --labels, --fonts, --info.
/// Document parts of the file opened by a command-line tool (shared by its open and save).
final class ExtrasBox { var x = DocumentExtras() }

enum ToolsCLI {
    static func run(_ args: [String]) -> Int32? {
        let commands: Set<String> = ["--watermark", "--number-pages", "--split", "--merge", "--compare", "--edit-text", "--redact", "--add-field", "--fill", "--fields", "--bookmarks", "--crop", "--page-size", "--link", "--autolink", "--props", "--info", "--attach", "--detach", "--attachments", "--pdfa", "--bates", "--bates-batch", "--header-footer", "--labels", "--fonts"]
        guard args.count >= 2, commands.contains(args[1]) else { return nil }
        guard args.count >= 3 else { fail("missing arguments for \(args[1])") }
        let a = Array(args.dropFirst(2))
        let box = ExtrasBox()
        func open(_ path: String) -> PDFDocument {
            guard let data = try? Data(contentsOf: URL(fileURLWithPath: path)), let d = PDFDocument(data: data), !d.isLocked else { fail("cannot open \(path)") }
            box.x = DocumentExtras.read(data, pdf: d)
            return d
        }
        func save(_ pdf: PDFDocument, _ path: String) {
            guard let baked = PageOverlay.bake(pdf) else { fail("cannot write") }
            let data = DocumentWriter.apply(AcroFormFixer.ensureFields(baked.data), extras: box.x, pdf: pdf)
            write(data, to: path)
            baked.report.lines.forEach { print($0) }
        }
        if let code = runDocumentTools(args[1], a, open: open, save: save, box: box) { return code }
        switch args[1] {
        case "--watermark" where a.count >= 3:
            let pdf = open(a[0])
            var spec = WatermarkSpec()
            spec.text = a[2]
            guard let pages = PDFTools.pageIndices(a.count > 3 ? a[3] : "", pageCount: pdf.pageCount) else { fail("bad pages") }
            for i in pages { if let p = pdf.page(at: i) { p.addAnnotation(WatermarkOverlay(spec: spec, page: p)) } }
            save(pdf, a[1])
            print("watermarked \(pages.count) pages")
        case "--number-pages" where a.count >= 2:
            let pdf = open(a[0])
            var spec = PageNumberSpec()
            if a.count > 2 { guard let f = Int(a[2]).flatMap(PageNumberSpec.Format.init) else { fail("bad number format") }; spec.format = f }
            if a.count > 3 { guard let p = Int(a[3]).flatMap(PageNumberSpec.Position.init) else { fail("bad number position") }; spec.position = p }
            spec.skipFirst = a.contains("skip-first")
            let labels = spec.labels(for: Array(0..<pdf.pageCount))
            for (i, text) in labels { if let p = pdf.page(at: i) { p.addAnnotation(PageNumberOverlay(text: text, spec: spec, page: p)) } }
            save(pdf, a[1])
            print("numbered \(labels.count) pages")
        case "--split" where a.count >= 3:
            let pdf = open(a[0])
            let mode = a[2]
            let groups: [[Int]]?
            if mode == "each" { groups = PDFTools.everyN(1, pageCount: pdf.pageCount) }
            else if mode.hasPrefix("every:"), let n = Int(mode.dropFirst(6)), n > 0 { groups = PDFTools.everyN(n, pageCount: pdf.pageCount) }
            else { groups = PDFTools.parseRanges(mode, pageCount: pdf.pageCount) }
            guard let groups, !groups.isEmpty else { fail("bad ranges \(mode)") }
            let dir = URL(fileURLWithPath: a[1])
            do { try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true) } catch { fail("\(error)") }
            let base = URL(fileURLWithPath: a[0]).deletingPathExtension().lastPathComponent
            guard let data = PageOverlay.bakedData(pdf), let source = PDFDocument(data: data) else { fail("cannot prepare split pages") }
            for (group, part) in zip(groups, PDFTools.split(source, groups: groups)) {
                let url = dir.appendingPathComponent(PDFTools.partName(base, group))
                guard let data = part.dataRepresentation() else { fail("cannot write split pages") }
                write(AcroFormFixer.ensureFields(data), to: url.path)
                print("\(url.lastPathComponent): \(part.pageCount) pages")
            }
        case "--merge" where a.count >= 2:
            do {
                let out = try PDFTools.mergeChecked(a.dropFirst().map { URL(fileURLWithPath: $0) })
                guard let data = out.dataRepresentation() else { fail("cannot write merged pages") }
                write(AcroFormFixer.ensureFields(data), to: a[0])
                print("pages: \(out.pageCount)")
            } catch { fail("\(error)") }
        case "--compare" where a.count >= 2:
            let base = open(a[0])
            let changes = PDFTools.compare(base, open(a[1]))
            if a.count > 2 {   // also write the base document with the changes highlighted
                EditorWindowController.addCompareHighlights(changes, to: base)
                guard let data = base.dataRepresentation() else { fail("cannot write comparison") }
                write(AcroFormFixer.ensureFields(data), to: a[2])
            }
            for c in changes {
                print("p\(c.page + 1): -[\(c.removed.map(\.text).joined(separator: " "))] +[\(c.added.joined(separator: " "))]")
            }
            print("changes: \(changes.count)")
        case "--edit-text" where a.count >= 4:
            let pdf = open(a[0])
            guard let hit = pdf.findString(a[2], withOptions: []).first, let page = hit.pages.first else { fail("text not found: \(a[2])") }
            page.addAnnotation(EditedTextOverlay.make(replacing: hit, on: page, with: a[3]))
            save(pdf, a[1])
            print("edited on page \(pdf.index(for: page) + 1)")
        case "--redact" where a.count >= 3:
            let pdf = open(a[0])
            let white = a.contains("white")
            var marks = 0
            for spec in a.dropFirst(2) where spec != "white" {
                guard let (bounds, pageIndex) = area(spec, pageCount: pdf.pageCount), let page = pdf.page(at: pageIndex) else {
                    fail("bad area \(spec) (x,y,w,h[@page])")
                }
                page.addAnnotation(RedactOverlay(bounds: bounds, color: white ? .white : .black))
                marks += 1
            }
            save(pdf, a[1])
            print("redactions: \(marks)")
        case "--add-field" where a.count >= 2:
            let pdf = open(a[0])
            guard let page = pdf.page(at: 0) else { fail("no pages") }
            let box = page.bounds(for: .cropBox)
            let top = box.maxY - 120
            page.addAnnotation(PDFTools.makeField(.text, name: "Name", bounds: CGRect(x: 72, y: top, width: 220, height: 22)))
            page.addAnnotation(PDFTools.makeField(.checkBox, name: "Agree", bounds: CGRect(x: 72, y: top - 40, width: 14, height: 14)))
            page.addAnnotation(PDFTools.makeField(.choice, name: "Colour", bounds: CGRect(x: 72, y: top - 80, width: 160, height: 22),
                                                  options: ["Red", "Green", "Blue"]))
            save(pdf, a[1])
            print("fields: \(PDFTools.fields(pdf).count)")
        case "--fill" where a.count >= 3:
            let pdf = open(a[0])
            for pair in a.dropFirst(2) {
                let kv = pair.split(separator: "=", maxSplits: 1, omittingEmptySubsequences: false).map(String.init)
                guard kv.count == 2 else { fail("bad field assignment \(pair)") }
                let matching = PDFTools.fields(pdf).filter { $0.1.fieldName == kv[0] }
                guard !matching.isEmpty else { fail("unknown field \(kv[0])") }
                for (_, field) in matching { PDFTools.setValue(kv[1], of: field) }
            }
            save(pdf, a[1])
            print("filled")
        case "--fields":
            for (i, f) in PDFTools.fields(open(a[0])) {
                print("p\(i + 1) \(f.fieldName ?? "?") type=\(f.widgetFieldType.rawValue) value=\(PDFTools.value(of: f)) choices=\(f.choices ?? [])")
            }
        default:
            fail("missing or invalid arguments for \(args[1])")
        }
        return 0
    }

    static func write(_ data: Data, to path: String) {
        do { try data.write(to: URL(fileURLWithPath: path), options: .atomic) } catch { fail("\(error)") }
    }

    /// Strict page-space area: reject malformed, nonfinite and allocation-hostile input before mutation.
    static func area(_ text: String, pageCount: Int) -> (CGRect, Int)? {
        let parts = text.split(separator: "@", omittingEmptySubsequences: false)
        guard (1...2).contains(parts.count), pageCount > 0 else { return nil }
        let components = parts[0].split(separator: ",", omittingEmptySubsequences: false)
        let values = components.compactMap { Double($0) }
        guard components.count == 4, values.count == 4,
              values.allSatisfy({ $0.isFinite && abs($0) <= 1_000_000 }), values[2] > 0, values[3] > 0 else { return nil }
        let number = parts.count == 2 ? Int(parts[1]) : 1
        guard let number, number >= 1, number <= pageCount else { return nil }
        return (CGRect(x: values[0], y: values[1], width: values[2], height: values[3]), number - 1)
    }

    static func fail(_ message: String) -> Never {
        FileHandle.standardError.write(Data((message + "\n").utf8))
        exit(1)
    }
}
