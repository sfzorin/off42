import AppKit
import PDFKit

/// Headless modes for the document tools (testing without the UI); pages count from 1.
///   --bookmarks in out [clear] [headings[:levels]] "Title@page[:b][:i][:#RRGGBB]" ">Child@page" …   (">" per nesting level)
///   --bookmarks file                                  prints the outline
///   --crop in out l,t,r,b|auto [pages] [remove]       margins in points (auto: white margins, l,t,r,b padding after "auto:")
///   --page-size in out A4|Letter|…|WxH(mm) [scale|extend] [pages]
///   --link in out x,y,w,h[@page] https://…|N[:Fit|FitH] …    --autolink in out
///   --props in out title=… author=… subject=… keywords=… layout=… mode=… openpage=N zoom=Fit|FitH|100 fitwindow=1 …
///   --info file                                       prints properties, labels, initial view, attachments
///   --attach in out file … [desc=…]     --detach in out name|all     --attachments file [extract-dir]
///   --pdfa in out                                     PDF/A-2b
///   --bates in out PREFIX start [digits] [position 0…5]   --bates-batch outdir PREFIX start a.pdf b.pdf …
///   --header-footer in out "left|centre|right" "left|centre|right" [pages]
///   --labels in out "1-4:r;5-:D;9-:D:A-"   (""  removes)     --fonts file
extension ToolsCLI {
    static func runDocumentTools(_ mode: String, _ a: [String], open: (String) -> PDFDocument, save: (PDFDocument, String) -> Void,
                                 box: ExtrasBox) -> Int32? {
        let fail = ToolsCLI.fail
        switch mode {
        case "--bookmarks" where a.count == 1:
            let pdf = open(a[0])
            func walk(_ o: PDFOutline, _ depth: Int) {
                for i in 0..<o.numberOfChildren {
                    guard let c = o.child(at: i) else { continue }
                    let page = OutlineTree.destination(c)?.page.map { pdf.index(for: $0) + 1 } ?? 0
                    print(String(repeating: "  ", count: depth) + "\(c.label ?? "") @\(page)")
                    walk(c, depth + 1)
                }
            }
            if let root = pdf.outlineRoot { walk(root, 0) }
        case "--bookmarks" where a.count >= 3:
            let pdf = open(a[0])
            if pdf.outlineRoot == nil { pdf.outlineRoot = PDFOutline() }
            let root = pdf.outlineRoot!
            var stack: [PDFOutline] = [root]
            for spec in a.dropFirst(2) {
                if spec == "clear" { while root.numberOfChildren > 0 { root.child(at: 0)?.removeFromParent() }; continue }
                if spec.hasPrefix("headings") {
                    let levels = Int(spec.split(separator: ":").last ?? "") ?? 2
                    let found = HeadingDetector.headings(pdf, maxLevels: levels, ocr: false)
                    HeadingDetector.insert(found, into: root)
                    print("headings: \(found.count)")
                    continue
                }
                let depth = spec.prefix { $0 == ">" }.count
                let raw = spec.dropFirst(depth)
                // Optional style after the page: ":b" bold, ":i" italic, ":#RRGGBB" colour.
                var parts = raw.split(separator: ":", omittingEmptySubsequences: false).map(String.init)
                var style = OutlineStyle()
                while parts.count > 1, let last = parts.last, last == "b" || last == "i" || last.hasPrefix("#") {
                    if last == "b" { style.bold = true } else if last == "i" { style.italic = true }
                    else if let v = Int(last.dropFirst(), radix: 16) {
                        style.color = NSColor(srgbRed: CGFloat(v >> 16 & 255) / 255, green: CGFloat(v >> 8 & 255) / 255, blue: CGFloat(v & 255) / 255, alpha: 1)
                    }
                    parts.removeLast()
                }
                let body = Substring(parts.joined(separator: ":"))
                guard let at = body.lastIndex(of: "@"), let n = Int(body[body.index(after: at)...]), n >= 1, n <= pdf.pageCount, let page = pdf.page(at: n - 1) else {
                    fail("bad bookmark \(spec) (Title@page)")
                }
                let item = PDFOutline()
                item.label = String(body[..<at])
                let b = page.bounds(for: .cropBox)
                item.destination = PDFDestination(page: page, at: CGPoint(x: b.minX, y: b.maxY))
                stack = Array(stack.prefix(depth + 1))
                guard let parent = stack.last else { fail("bad nesting \(spec)") }
                parent.insertChild(item, at: parent.numberOfChildren)
                parent.isOpen = true
                stack.append(item)
                if !style.isPlain { box.x.outlineStyles[ObjectIdentifier(item)] = style }
            }
            box.x.dirty.insert(.outline)
            save(pdf, a[1])
            print("bookmarks: \(OutlineTree.flatten(root).count)")
        case "--crop" where a.count >= 3:
            let pdf = open(a[0])
            let pages = a.count > 3 && a[3] != "remove" ? PDFTools.pageIndices(a[3], pageCount: pdf.pageCount) : Array(0..<pdf.pageCount)
            guard let pages else { fail("bad pages") }
            let remove = a.contains("remove")
            let auto = a[2].hasPrefix("auto")
            let n = (auto ? String(a[2].dropFirst(5)) : a[2]).split(separator: ",").compactMap { Double($0) }.map { CGFloat($0) }
            guard (auto && (a[2] == "auto" || a[2].hasPrefix("auto:") && n.count == 4) || !auto && n.count == 4), n.allSatisfy({ $0.isFinite && abs($0) <= 1_000_000 }) else { fail("bad margins \(a[2]) (l,t,r,b in points, or auto[:l,t,r,b])") }
            let m = n.count == 4 ? n : [0, 0, 0, 0]
            for i in pages {
                guard let page = pdf.page(at: i) else { continue }
                let media = page.bounds(for: .mediaBox)
                var crop: CGRect
                if auto {
                    guard let content = PageGeometry.contentBox(page) else { continue }
                    crop = PageGeometry.cropBox(media: content, rotation: page.rotation, left: -m[0], top: -m[1], right: -m[2], bottom: -m[3]).intersection(media)
                } else {
                    crop = PageGeometry.cropBox(media: media, rotation: page.rotation, left: m[0], top: m[1], right: m[2], bottom: m[3])
                }
                guard !crop.isNull, !crop.isEmpty, [crop.minX, crop.minY, crop.width, crop.height].allSatisfy({ $0.isFinite }), crop.width >= 1, crop.height >= 1 else { fail("crop leaves no usable page") }
                page.setBounds(crop, for: .cropBox)
                if remove { PageGeometry.strips(media: media, crop: crop).forEach { page.addAnnotation(CropRemovalOverlay(bounds: $0)) } }
                print("p\(i + 1): crop \(Int(crop.minX)),\(Int(crop.minY)),\(Int(crop.width))x\(Int(crop.height))")
            }
            save(pdf, a[1])
        case "--page-size" where a.count >= 3:
            let pdf = open(a[0])
            var size: CGSize
            if let named = PageGeometry.paperSizes.first(where: { $0.0.caseInsensitiveCompare(a[2]) == .orderedSame }) { size = named.1 } else {
                let n = a[2].lowercased().split(separator: "x").compactMap { Double($0) }
                guard n.count == 2 else { fail("bad size \(a[2]) (A4, Letter, 210x297)") }
                size = CGSize(width: n[0] * PageGeometry.mm, height: n[1] * PageGeometry.mm)
            }
            guard size.width.isFinite, size.height.isFinite, (1...14_400).contains(size.width), (1...14_400).contains(size.height) else { fail("page size must be 1…14400 points per side") }
            let extend = a.contains("extend")
            let rangeArg = a.dropFirst(3).first { $0 != "extend" && $0 != "scale" }
            guard let pages = rangeArg.map({ PDFTools.pageIndices($0, pageCount: pdf.pageCount) }) ?? Array(0..<pdf.pageCount) else { fail("bad pages") }
            var replacements: [(Int, PDFPage)] = []
            for i in pages {
                guard let page = pdf.page(at: i) else { continue }
                let shown = EditorWindowController.displayedSize(page)
                let s = (shown.width > shown.height) == (size.width > size.height) ? size : CGSize(width: size.height, height: size.width)
                if extend {
                    let crop = page.bounds(for: .cropBox)
                    let t = page.rotation % 180 != 0 ? CGSize(width: s.height, height: s.width) : s
                    let r = CGRect(x: crop.midX - t.width / 2, y: crop.midY - t.height / 2, width: t.width, height: t.height)
                    page.setBounds(r, for: .mediaBox)
                    page.setBounds(r, for: .cropBox)
                } else if let p = PageGeometry.scaled(page, to: s) { replacements.append((i, p)) }
            }
            PageOverlay.replace(replacements, in: pdf)
            save(pdf, a[1])
            print("resized \(pages.count) pages")
        case "--link" where a.count >= 4:
            let pdf = open(a[0])
            guard (a.count - 2).isMultiple(of: 2) else { fail("link needs an area and target for each link") }
            var i = 2
            var count = 0
            while i + 1 < a.count {
                guard let (bounds, pageIndex) = area(a[i], pageCount: pdf.pageCount), let page = pdf.page(at: pageIndex) else {
                    fail("bad rect \(a[i]) (x,y,w,h[@page])")
                }
                let link = EditorWindowController.makeLink(bounds: bounds)
                let target = a[i + 1].split(separator: ":", maxSplits: 1, omittingEmptySubsequences: false).map(String.init)
                if let p = Int(target[0]) {
                    guard p >= 1, p <= pdf.pageCount, let dest = pdf.page(at: p - 1) else { fail("bad target page") }
                    let fit = target.count > 1 ? target[1] : ""
                    let b = dest.bounds(for: .cropBox)
                    link.action = PDFActionGoTo(destination: PDFDestination(page: dest, at: CGPoint(x: b.minX, y: b.maxY)))
                    if !fit.isEmpty { box.x.linkFit[ObjectIdentifier(link)] = fit }
                } else if let url = URL(string: a[i + 1]), url.scheme != nil {
                    link.action = PDFActionURL(url: url)
                } else { fail("bad target \(a[i + 1])") }
                let border = PDFBorder()
                border.lineWidth = 0
                link.border = border
                page.addAnnotation(link)
                count += 1
                i += 2
            }
            save(pdf, a[1])
            print("links: \(count)")
        case "--autolink" where a.count >= 2:
            let pdf = open(a[0])
            let n = EditorWindowController.createLinksFromURLs(pdf) { link, page in page.addAnnotation(link) }
            save(pdf, a[1])
            print("links: \(n)")
        case "--props" where a.count >= 2:
            let pdf = open(a[0])
            for pair in a.dropFirst(2) {
                let kv = pair.split(separator: "=", maxSplits: 1, omittingEmptySubsequences: false).map(String.init)
                guard kv.count == 2 else { fail("bad property \(pair) (key=value)") }
                let on = ["1", "true", "yes"].contains(kv[1].lowercased())
                switch kv[0].lowercased() {
                case "title": box.x.info.title = kv[1]; box.x.dirty.insert(.info)
                case "author": box.x.info.author = kv[1]; box.x.dirty.insert(.info)
                case "subject": box.x.info.subject = kv[1]; box.x.dirty.insert(.info)
                case "keywords": box.x.info.keywords = kv[1]; box.x.dirty.insert(.info)
                case "creator": box.x.info.creator = kv[1]; box.x.dirty.insert(.info)
                case "layout": box.x.view.pageLayout = kv[1]; box.x.dirty.insert(.view)
                case "mode": box.x.view.pageMode = kv[1]; box.x.dirty.insert(.view)
                case "openpage":
                    guard let number = Int(kv[1]), number >= 1, number <= pdf.pageCount else { fail("bad opening page") }
                    box.x.view.openPage = number - 1; box.x.dirty.insert(.view)
                case "zoom": box.x.view.openZoom = kv[1]; box.x.dirty.insert(.view)
                case "fitwindow": box.x.view.fitWindow = on; box.x.dirty.insert(.view)
                case "centerwindow": box.x.view.centerWindow = on; box.x.dirty.insert(.view)
                case "displaydoctitle": box.x.view.displayDocTitle = on; box.x.dirty.insert(.view)
                case "hidetoolbar": box.x.view.hideToolbar = on; box.x.dirty.insert(.view)
                case "hidemenubar": box.x.view.hideMenubar = on; box.x.dirty.insert(.view)
                default: fail("unknown property \(kv[0])")
                }
            }
            save(pdf, a[1])
            print("properties written")
        case "--info" where a.count >= 1:
            let pdf = open(a[0])
            let x = box.x
            print("version: \(x.version) pages: \(pdf.pageCount) xmp: \(x.hadXMP)")
            print("title: \(x.info.title) | author: \(x.info.author) | subject: \(x.info.subject) | keywords: \(x.info.keywords)")
            print("creator: \(x.info.creator) | producer: \(x.info.producer) | created: \(x.info.created.map { "\($0)" } ?? "-") | modified: \(x.info.modified.map { "\($0)" } ?? "-")")
            print("layout: \(x.view.pageLayout) mode: \(x.view.pageMode) openpage: \(x.view.openPage + 1) zoom: \(x.view.openZoom) fit: \(x.view.fitWindow) center: \(x.view.centerWindow) title: \(x.view.displayDocTitle)")
            print("labels: \(PageLabels.describe(x.labels)) -> \((0..<pdf.pageCount).map { x.label($0) ?? "\($0 + 1)" }.joined(separator: " "))")
            print("attachments: \(x.files.map { "\($0.name) (\($0.data.count) bytes)" }) annotation files: \(x.annotationFiles.map { "p\($0.page + 1) \($0.name)" })")
        case "--attach" where a.count >= 3:
            let pdf = open(a[0])
            let desc = a.first { $0.hasPrefix("desc=") }.map { String($0.dropFirst(5)) } ?? ""
            for path in a.dropFirst(2) where !path.hasPrefix("desc=") {
                let url = URL(fileURLWithPath: path)
                guard let data = try? Data(contentsOf: url) else { fail("cannot read \(path)") }
                box.x.files.removeAll { $0.name == url.lastPathComponent }
                box.x.files.append(EmbeddedFile(name: url.lastPathComponent, data: data, desc: desc, modified: Date(), mime: Writer.mime(for: url.lastPathComponent)))
            }
            box.x.dirty.insert(.files)
            save(pdf, a[1])
            print("attachments: \(box.x.files.count)")
        case "--detach" where a.count >= 3:
            let pdf = open(a[0])
            let before = box.x.files.count
            box.x.files.removeAll { a[2] == "all" || $0.name == a[2] }
            var removedAnnots = 0
            for i in 0..<pdf.pageCount {
                guard let page = pdf.page(at: i) else { continue }
                for annot in page.annotations where annot.kind == "FileAttachment" {
                    let name = box.x.annotationFile(for: annot, on: i)?.name
                    if a[2] == "all" || name == a[2] { page.removeAnnotation(annot); removedAnnots += 1 }
                }
            }
            box.x.dirty.insert(.files)
            save(pdf, a[1])
            print("removed \(before - box.x.files.count) attachments, \(removedAnnots) attachment annotations")
        case "--attachments" where a.count >= 1:
            _ = open(a[0])
            for f in box.x.files { print("\(f.name) \(f.data.count) bytes \(f.mime ?? "-") \(f.desc)") }
            for f in box.x.annotationFiles { print("p\(f.page + 1) \(f.name) \(f.data.count) bytes (annotation)") }
            if a.count > 1 {
                let dir = URL(fileURLWithPath: a[1])
                do {
                    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
                    for f in box.x.files { _ = try AttachmentExport.write(f.data, name: f.name, to: dir) }
                } catch { fail("\(error)") }
            }
        case "--pdfa" where a.count >= 2:
            let pdf = open(a[0])
            guard let (data, report) = PDFAExport.make(pdf, extras: box.x) else { fail("cannot convert") }
            write(data, to: a[1])
            report.lines.forEach { print($0) }
            print("PDF/A-2b: \(data.count) bytes")
        case "--bates" where a.count >= 4:
            let pdf = open(a[0])
            var spec = HeaderFooterSpec()
            spec.batesPrefix = a[2]
            guard let start = Int(a[3]), start >= 1, !start.addingReportingOverflow(pdf.pageCount).overflow else { fail("bad Bates start") }
            spec.batesStart = start
            if a.count > 4 { guard let digits = Int(a[4]), (0...12).contains(digits) else { fail("bad Bates digit count") }; spec.batesDigits = digits }
            let slot = a.count > 5 ? [5, 4, 3, 1, 2, 0][min(5, max(0, Int(a[5]) ?? 0))] : 5   // like page numbers: 0 = bottom centre…
            spec.texts[slot] = "<<bates>>"
            let made = HeaderFooterOverlay.make(spec, pdf: pdf, indices: Array(0..<pdf.pageCount), fileName: (a[0] as NSString).lastPathComponent,
                                                labels: box.x.labels, bates: spec.batesStart)
            for (i, o) in made.overlays { pdf.page(at: i)?.addAnnotation(o) }
            save(pdf, a[1])
            print("bates \(spec.bates(spec.batesStart))…\(spec.bates(made.next - 1))")
        case "--bates-batch" where a.count >= 4:
            var spec = HeaderFooterSpec()
            spec.batesPrefix = a[1]
            guard let start = Int(a[2]), start >= 1 else { fail("bad Bates start") }
            spec.batesStart = start
            spec.texts[5] = "<<bates>>"
            let dir = URL(fileURLWithPath: a[0])
            try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
            do {
                let written = try EditorWindowController.stampFiles(a.dropFirst(3).map { URL(fileURLWithPath: $0) }, spec: spec, startBates: spec.batesStart, into: dir)
                written.forEach { print($0.lastPathComponent) }
            } catch { fail("\(error)") }
        case "--header-footer" where a.count >= 4:
            let pdf = open(a[0])
            var spec = HeaderFooterSpec()
            let top = a[2].split(separator: "|", omittingEmptySubsequences: false).map(String.init)
            let bottom = a[3].split(separator: "|", omittingEmptySubsequences: false).map(String.init)
            for k in 0..<3 {
                spec.texts[k] = k < top.count ? top[k] : ""
                spec.texts[3 + k] = k < bottom.count ? bottom[k] : ""
            }
            guard let pages = a.count > 4 ? PDFTools.pageIndices(a[4], pageCount: pdf.pageCount) : Array(0..<pdf.pageCount) else { fail("bad pages") }
            let made = HeaderFooterOverlay.make(spec, pdf: pdf, indices: pages, fileName: (a[0] as NSString).lastPathComponent, labels: box.x.labels, bates: 1)
            for (i, o) in made.overlays { pdf.page(at: i)?.addAnnotation(o) }
            save(pdf, a[1])
            print("header/footer on \(made.overlays.count) pages")
        case "--labels" where a.count >= 3:
            let pdf = open(a[0])
            guard let ranges = PageLabels.parse(a[2], pageCount: pdf.pageCount) else { fail("bad labels \(a[2]) (1-4:r;5-:D)") }
            box.x.labels = ranges
            box.x.dirty.insert(.labels)
            save(pdf, a[1])
            print("labels: \((0..<pdf.pageCount).map { PageLabels.label($0, ranges) ?? "\($0 + 1)" }.joined(separator: " "))")
        case "--fonts" where a.count >= 1:
            guard let data = try? Data(contentsOf: URL(fileURLWithPath: a[0])) else { fail("cannot open \(a[0])") }
            for (i, fonts) in FontScanner.scan(data).sorted(by: { $0.key < $1.key }) {
                for f in fonts.sorted(by: { $0.name < $1.name }) {
                    print("p\(i + 1) \(f.name) \(f.type) embedded=\(f.embedded) subset=\(f.subset)")
                }
            }
        default:
            return nil
        }
        return 0
    }
}
