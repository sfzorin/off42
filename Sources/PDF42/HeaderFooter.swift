import AppKit
import CoreText
import Kit42
import PDFKit

/// Header & footer text in six positions with tokens: <<page>>, <<pages>>, <<label>>, <<date>>, <<file>>, <<bates>>.
struct HeaderFooterSpec {
    /// Top left, top centre, top right, bottom left, bottom centre, bottom right.
    var texts = ["", "", "", "", "", ""]
    var fontName = "Helvetica"
    var fontSize: CGFloat = 10
    var color: NSColor = .black
    /// Margins in points: from the top/bottom edge to the text, from the left/right edge.
    var vertical: CGFloat = 28
    var horizontal: CGFloat = 36
    var batesPrefix = ""
    var batesSuffix = ""
    var batesDigits = 6
    var batesStart = 1
    var date = Date()

    static let fonts = ["Helvetica", "Helvetica-Bold", "Times-Roman", "Times-Bold", "Courier"]
    static let fontTitles = ["Helvetica", "Helvetica Bold", "Times", "Times Bold", "Courier"]

    var usesBates: Bool { texts.contains { $0.contains("<<bates>>") } }
    var isEmpty: Bool { texts.allSatisfy { $0.trimmingCharacters(in: .whitespaces).isEmpty } }

    func bates(_ n: Int) -> String {
        let digits = String(n)
        return batesPrefix + String(repeating: "0", count: max(0, min(12, batesDigits) - digits.count)) + digits + batesSuffix
    }

    /// Text for position `slot` on a page: `page`/`pages` are 1-based numbers in the document, `bates` the running number.
    func text(_ slot: Int, page: Int, pages: Int, label: String, file: String, bates n: Int) -> String {
        let f = DateFormatter()
        f.dateStyle = .short
        f.timeStyle = .none
        return texts[slot].replacingOccurrences(of: "<<page>>", with: "\(page)")
            .replacingOccurrences(of: "<<pages>>", with: "\(pages)")
            .replacingOccurrences(of: "<<label>>", with: label)
            .replacingOccurrences(of: "<<date>>", with: f.string(from: date))
            .replacingOccurrences(of: "<<file>>", with: file)
            .replacingOccurrences(of: "<<bates>>", with: bates(n))
    }
}

/// Header/footer of one page; drawn as real (searchable) text when baked on save.
final class HeaderFooterOverlay: PageOverlay {
    let spec: HeaderFooterSpec
    let lines: [String]

    init(spec: HeaderFooterSpec, lines: [String], page: PDFPage) {
        self.spec = spec
        self.lines = lines
        super.init(bounds: page.bounds(for: .cropBox))
    }

    required init?(coder: NSCoder) { nil }

    override func render(in ctx: CGContext, box: CGRect, rotation: Int) {
        let (t, size) = Self.displayTransform(box: box, rotation: rotation)
        ctx.concatenate(t)
        let font = NSFont(name: spec.fontName, size: spec.fontSize) ?? .systemFont(ofSize: spec.fontSize)
        let color = spec.color.usingColorSpace(.sRGB) ?? spec.color
        for (slot, text) in lines.enumerated() where !text.isEmpty {
            let line = Self.line(text, font: font, color: color)
            let w = Self.width(of: line), m = spec.horizontal
            let x: CGFloat
            switch slot % 3 {
            case 0: x = m
            case 1: x = (size.width - w) / 2
            default: x = size.width - m - w
            }
            let y = slot < 3 ? size.height - spec.vertical - font.ascender : spec.vertical
            ctx.textPosition = CGPoint(x: x, y: y)
            CTLineDraw(line, ctx)
        }
    }

    /// Overlays for `indices` of `pdf`; Bates numbers run from `bates` up. Returns the next Bates number.
    static func make(_ spec: HeaderFooterSpec, pdf: PDFDocument, indices: [Int], fileName: String, labels: [PageLabelRange],
                     bates: Int) -> (overlays: [(Int, HeaderFooterOverlay)], next: Int) {
        let chosen = Array(Set(indices.filter { $0 >= 0 && $0 < pdf.pageCount })).sorted()
        guard !bates.addingReportingOverflow(chosen.count).overflow else { return ([], bates) }
        var n = bates
        var out: [(Int, HeaderFooterOverlay)] = []
        for i in chosen {
            guard let page = pdf.page(at: i) else { continue }
            let label = PageLabels.label(i, labels) ?? "\(i + 1)"
            let lines = (0..<6).map { spec.text($0, page: i + 1, pages: pdf.pageCount, label: label, file: fileName, bates: n) }
            out.append((i, HeaderFooterOverlay(spec: spec, lines: lines, page: page)))
            n += 1
        }
        return (out, n)
    }
}

extension EditorWindowController {
    private static var lastHeaderFooter: HeaderFooterSpec = {
        var s = HeaderFooterSpec()
        s.texts[4] = "<<page>> / <<pages>>"
        return s
    }()

    var hasHeaderFooterOverlays: Bool {
        guard let pdf else { return false }
        return (0..<pdf.pageCount).contains { pdf.page(at: $0)?.annotations.contains { $0 is HeaderFooterOverlay } == true }
    }

    @objc func removeHeaderFooterAction(_ sender: Any?) {
        guard let pdf else { return }
        for i in 0..<pdf.pageCount {
            guard let page = pdf.page(at: i) else { continue }
            for a in page.annotations where a is HeaderFooterOverlay { remove(a, from: page, name: L("Remove Headers and Footers")) }
        }
    }

    @objc func headerFooterAction(_ sender: Any?) {
        guard let pdf else { return }
        var spec = Self.lastHeaderFooter
        func field(_ s: String) -> NSTextField {
            let f = NSTextField(string: s)
            f.translatesAutoresizingMaskIntoConstraints = false
            f.widthAnchor.constraint(equalToConstant: 150).isActive = true
            return f
        }
        let slots = spec.texts.map(field)
        let header = NSStackView(views: Array(slots[0..<3]))
        let footer = NSStackView(views: Array(slots[3..<6]))
        let font = NSPopUpButton(frame: .zero, pullsDown: false)
        font.addItems(withTitles: HeaderFooterSpec.fontTitles)
        font.selectItem(at: HeaderFooterSpec.fonts.firstIndex(of: spec.fontName) ?? 0)
        let size = field("\(Int(spec.fontSize))")
        let color = NSPopUpButton(frame: .zero, pullsDown: false)
        color.addItems(withTitles: Self.overlayColors.map(\.0))
        color.selectItem(at: Self.overlayColors.firstIndex { $0.1 == spec.color } ?? Self.overlayColors.count - 1)
        let fontRow = NSStackView(views: [font, size, NSTextField(labelWithString: "pt"), color])
        let vertical = field(String(format: "%.0f", spec.vertical / PageGeometry.mm))
        let horizontal = field(String(format: "%.0f", spec.horizontal / PageGeometry.mm))
        let marginRow = NSStackView(views: [NSTextField(labelWithString: L("Top/bottom")), vertical, NSTextField(labelWithString: L("Sides")), horizontal])
        let prefix = field(spec.batesPrefix), suffix = field(spec.batesSuffix)
        let digits = field("\(spec.batesDigits)"), start = field("\(spec.batesStart)")
        let batesRow1 = NSStackView(views: [NSTextField(labelWithString: L("Prefix")), prefix, NSTextField(labelWithString: L("Suffix")), suffix])
        let batesRow2 = NSStackView(views: [NSTextField(labelWithString: L("Digits")), digits, NSTextField(labelWithString: L("Start at")), start])
        let pages = PageSetChooser()
        pages.popup.selectItem(at: 1)
        pages.range.isEnabled = false
        let batch = NSButton(checkboxWithTitle: L("Also number other PDF files (Bates numbers continue)…"), target: nil, action: nil)
        let alert = NSAlert.form(L("Header & Footer"),
                                 info: L("Codes: <<page>>, <<pages>>, <<label>> (page label), <<date>>, <<file>>, <<bates>>. The text becomes page content when the document is saved; until then it can be removed."),
                                 ok: L("Add"), rows: [
                                    (L("Header (left, centre, right):"), header), (L("Footer (left, centre, right):"), footer),
                                    (L("Font:"), fontRow), (L("Margins (mm):"), marginRow),
                                    (L("Bates number:"), batesRow1), ("", batesRow2), (L("Pages:"), pages.view), ("", batch),
                                 ])
        alert.window.initialFirstResponder = slots[4]
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        guard let targets = pages.indices(current: currentIndex, selected: targetIndices, pageCount: pdf.pageCount) else { NSSound.beep(); return }
        func number(_ field: NSTextField) -> Double? {
            guard let n = Double(field.stringValue.replacingOccurrences(of: ",", with: ".")), n.isFinite else { return nil }
            return n
        }
        guard let fontSize = number(size), fontSize >= 4, fontSize <= 72,
              let topMargin = number(vertical), topMargin >= 0, topMargin <= 5000,
              let sideMargin = number(horizontal), sideMargin >= 0, sideMargin <= 5000,
              let first = Int(start.stringValue), first >= 0, first <= Int.max - targets.count,
              let places = Int(digits.stringValue), places >= 1, places <= 12 else { NSSound.beep(); return }
        spec.texts = slots.map(\.stringValue)
        spec.fontName = HeaderFooterSpec.fonts[max(0, font.indexOfSelectedItem)]
        spec.fontSize = CGFloat(fontSize)
        spec.color = Self.overlayColors[max(0, color.indexOfSelectedItem)].1
        spec.vertical = CGFloat(topMargin) * PageGeometry.mm
        spec.horizontal = CGFloat(sideMargin) * PageGeometry.mm
        spec.batesPrefix = prefix.stringValue
        spec.batesSuffix = suffix.stringValue
        spec.batesDigits = places
        spec.batesStart = first
        spec.date = Date()
        guard !spec.isEmpty else { return }
        Self.lastHeaderFooter = spec
        let name = pdfDoc?.displayName ?? ""
        let made = HeaderFooterOverlay.make(spec, pdf: pdf, indices: targets, fileName: name, labels: pdfDoc?.extras.labels ?? [], bates: spec.batesStart)
        for (i, overlay) in made.overlays {
            guard let page = pdf.page(at: i) else { continue }
            add(overlay, to: page, name: L("Header & Footer"))
        }
        if batch.state == .on { batchHeaderFooter(spec, next: made.next) }
    }

    /// The same header/footer on other files, written to a folder; Bates numbers continue from this document.
    private func batchHeaderFooter(_ spec: HeaderFooterSpec, next: Int) {
        let files = NSOpenPanel()
        files.allowedContentTypes = [.pdf]
        files.allowsMultipleSelection = true
        files.message = L("Choose the other PDF files, in numbering order")
        guard files.runModal() == .OK, !files.urls.isEmpty else { return }
        let folder = NSOpenPanel()
        folder.canChooseDirectories = true
        folder.canChooseFiles = false
        folder.canCreateDirectories = true
        folder.prompt = L("Save Here")
        folder.message = L("Folder for the numbered copies")
        guard folder.runModal() == .OK, let dir = folder.url else { return }
        let urls = files.urls.sorted { $0.lastPathComponent.localizedStandardCompare($1.lastPathComponent) == .orderedAscending }
        do {
            let result = try Self.stampFiles(urls, spec: spec, startBates: next, into: dir)
            banner.show(.success, L("Saved %d files", result.count))
            DispatchQueue.main.asyncAfter(deadline: .now() + 4) { [weak self] in self?.updateSignatureBanner() }
            NSWorkspace.shared.activateFileViewerSelecting(result)
        } catch { presentError(error) }
    }

    /// Writes stamped copies of `urls` into `dir`; returns the files written.
    static func stampFiles(_ urls: [URL], spec: HeaderFooterSpec, startBates: Int, into dir: URL) throws -> [URL] {
        var n = startBates
        var written: [URL] = []
        for url in urls {
            guard let data = try? Data(contentsOf: url), let doc = PDFDocument(data: data), !doc.isLocked else { continue }
            let extras = DocumentExtras.read(data, pdf: doc)
            let made = HeaderFooterOverlay.make(spec, pdf: doc, indices: Array(0..<doc.pageCount), fileName: url.lastPathComponent,
                                                labels: extras.labels, bates: n)
            n = made.next
            for (i, o) in made.overlays { doc.page(at: i)?.addAnnotation(o) }
            guard let baked = PageOverlay.bake(doc) else { continue }
            let out = DocumentWriter.apply(AcroFormFixer.ensureFields(baked.data), extras: extras, pdf: doc)
            let target = dir.appendingPathComponent(url.lastPathComponent)
            try out.write(to: target, options: .atomic)
            written.append(target)
        }
        return written
    }
}

private extension CGFloat {
    func clamped(_ lo: CGFloat, _ hi: CGFloat) -> CGFloat { Swift.min(hi, Swift.max(lo, self)) }
}
