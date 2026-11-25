import AppKit
import CoreText
import Kit42
import PDFKit

/// Annotation drawn over the page in the editor and baked into the page content when the document
/// is saved (`PageOverlay.bakedData`): watermarks, page numbers, edited text. Until saved it can be
/// undone, re-edited or removed like any annotation.
class PageOverlay: PDFAnnotation {
    init(bounds: CGRect) {
        super.init(bounds: bounds, forType: .stamp, withProperties: nil)
        userName = NSFullUserName()
        modificationDate = Date()
    }

    required init?(coder: NSCoder) { nil }

    /// Edited text can be clicked, moved and re-edited; watermarks and page numbers never block the page.
    var allowsPicking: Bool { false }

    /// Draws in page space. `box` is the page's crop box, `rotation` its /Rotate.
    func render(in ctx: CGContext, box: CGRect, rotation: Int) {}

    /// Page content under this overlay that is removed from the file when it is baked.
    var redactionArea: RedactionArea? { nil }
    var redactionAreas: [RedactionArea] { redactionArea.map { [$0] } ?? [] }

    override func draw(with box: PDFDisplayBox, in context: CGContext) {
        guard let page else { return }
        context.saveGState()
        render(in: context, box: page.bounds(for: .cropBox), rotation: page.rotation)
        context.restoreGState()
    }

    /// Transform from "as displayed" coordinates (origin bottom left of the visible page, rotation applied)
    /// to page space, and the displayed size.
    static func displayTransform(box: CGRect, rotation: Int) -> (CGAffineTransform, CGSize) {
        let w = box.width, h = box.height
        switch ((rotation % 360) + 360) % 360 {
        case 90: return (CGAffineTransform(a: 0, b: 1, c: -1, d: 0, tx: box.minX + w, ty: box.minY), CGSize(width: h, height: w))
        case 180: return (CGAffineTransform(a: -1, b: 0, c: 0, d: -1, tx: box.minX + w, ty: box.minY + h), box.size)
        case 270: return (CGAffineTransform(a: 0, b: -1, c: 1, d: 0, tx: box.minX, ty: box.minY + h), CGSize(width: h, height: w))
        default: return (CGAffineTransform(translationX: box.minX, y: box.minY), box.size)
        }
    }

    static func line(_ text: String, font: NSFont, color: NSColor) -> CTLine {
        CTLineCreateWithAttributedString(NSAttributedString(string: text, attributes: [.font: font, .foregroundColor: color.cgColor]))
    }

    static func width(of line: CTLine) -> CGFloat { CGFloat(CTLineGetTypographicBounds(line, nil, nil, nil)) }

    /// Glyph outlines of a line (watermarks are drawn as shapes, so they don't pollute text search and copy).
    static func outlines(of line: CTLine) -> CGPath {
        let path = CGMutablePath()
        for run in CTLineGetGlyphRuns(line) as! [CTRun] {
            let attrs = CTRunGetAttributes(run) as NSDictionary
            let font = attrs[kCTFontAttributeName] as! CTFont
            let count = CTRunGetGlyphCount(run)
            var glyphs = [CGGlyph](repeating: 0, count: count)
            var positions = [CGPoint](repeating: .zero, count: count)
            CTRunGetGlyphs(run, CFRange(), &glyphs)
            CTRunGetPositions(run, CFRange(), &positions)
            for (g, p) in zip(glyphs, positions) {
                if let glyph = CTFontCreatePathForGlyph(font, g, nil) { path.addPath(glyph, transform: CGAffineTransform(translationX: p.x, y: p.y)) }
            }
        }
        return path
    }
}

// MARK: - Watermark

struct WatermarkSpec {
    var text = "CONFIDENTIAL"
    var image: CGImage?
    var fontSize: CGFloat = 72
    var color: NSColor = .systemRed
    var opacity: CGFloat = 0.25
    /// Degrees counter-clockwise; 45 = diagonal from bottom left to top right.
    var rotation: CGFloat = 45
}

final class WatermarkOverlay: PageOverlay {
    let spec: WatermarkSpec

    init(spec: WatermarkSpec, page: PDFPage) {
        self.spec = spec
        super.init(bounds: page.bounds(for: .cropBox))
    }

    required init?(coder: NSCoder) { nil }

    override func render(in ctx: CGContext, box: CGRect, rotation: Int) {
        let (t, size) = Self.displayTransform(box: box, rotation: rotation)
        ctx.concatenate(t)
        ctx.translateBy(x: size.width / 2, y: size.height / 2)
        ctx.rotate(by: spec.rotation * .pi / 180)
        ctx.setAlpha(spec.opacity)
        if let image = spec.image {
            let w = size.width * 0.5, h = w * CGFloat(image.height) / max(1, CGFloat(image.width))
            ctx.interpolationQuality = .high
            ctx.draw(image, in: CGRect(x: -w / 2, y: -h / 2, width: w, height: h))
            return
        }
        let font = NSFont(name: "Helvetica-Bold", size: spec.fontSize) ?? .boldSystemFont(ofSize: spec.fontSize)
        let line = Self.line(spec.text, font: font, color: spec.color)
        let w = Self.width(of: line)
        ctx.translateBy(x: -w / 2, y: -(font.capHeight) / 2)
        ctx.addPath(Self.outlines(of: line))
        ctx.setFillColor((spec.color.usingColorSpace(.sRGB) ?? spec.color).cgColor)
        ctx.fillPath()
    }
}

// MARK: - Page numbers

struct PageNumberSpec {
    enum Position: Int, CaseIterable { case bottomCenter, bottomRight, bottomLeft, topCenter, topRight, topLeft }
    enum Format: Int, CaseIterable { case plain, pageOfTotal, dashes }
    var position: Position = .bottomCenter
    var format: Format = .plain
    var start = 1
    var skipFirst = false
    var fontSize: CGFloat = 10
    var margin: CGFloat = 28

    func text(_ n: Int, of total: Int) -> String {
        switch format {
        case .plain: return "\(n)"
        case .pageOfTotal: return L("Page %d of %d", n, total)
        case .dashes: return "- \(n) -"
        }
    }

    /// Number text for each page index of `indices` (pages in document order).
    func labels(for indices: [Int]) -> [(index: Int, text: String)] {
        let numbered = skipFirst ? Array(indices.sorted().dropFirst()) : indices.sorted()
        guard !numbered.isEmpty else { return [] }
        let (total, overflow) = start.addingReportingOverflow(numbered.count - 1)
        guard !overflow else { return [] }
        return numbered.enumerated().map { ($0.element, text(start + $0.offset, of: total)) }
    }
}

final class PageNumberOverlay: PageOverlay {
    let text: String
    let spec: PageNumberSpec

    init(text: String, spec: PageNumberSpec, page: PDFPage) {
        self.text = text
        self.spec = spec
        super.init(bounds: page.bounds(for: .cropBox))
    }

    required init?(coder: NSCoder) { nil }

    override func render(in ctx: CGContext, box: CGRect, rotation: Int) {
        let (t, size) = Self.displayTransform(box: box, rotation: rotation)
        ctx.concatenate(t)
        let font = NSFont(name: "Helvetica", size: spec.fontSize) ?? .systemFont(ofSize: spec.fontSize)
        let line = Self.line(text, font: font, color: .black)
        let w = Self.width(of: line), m = spec.margin
        let x: CGFloat
        switch spec.position {
        case .bottomCenter, .topCenter: x = (size.width - w) / 2
        case .bottomRight, .topRight: x = size.width - m - w
        case .bottomLeft, .topLeft: x = m
        }
        let top = [.topCenter, .topRight, .topLeft].contains(spec.position)
        ctx.textPosition = CGPoint(x: x, y: top ? size.height - m - font.ascender : m)
        CTLineDraw(line, ctx)
    }
}

// MARK: - Edited text

/// Replacement for a run of page text: a white cover over the original glyphs and the new text
/// on the same baseline, in the original font (when installed) and size.
final class EditedTextOverlay: PageOverlay {
    var text: String
    let textFont: NSFont
    let originalText: String
    /// Original cover width; the cover grows when the new text is longer.
    let originalWidth: CGFloat
    /// The source text stays removed when the replacement is moved on the page.
    let originalCover: CGRect
    /// Baseline above `bounds.minY`.
    let baseline: CGFloat

    init(cover: CGRect, text: String, originalText: String, font: NSFont, color: NSColor, baseline: CGFloat) {
        self.text = text
        self.textFont = font
        self.originalText = originalText
        self.originalWidth = cover.width
        self.originalCover = cover
        self.baseline = baseline
        super.init(bounds: cover)
        self.color = color
        self.bounds = Self.cover(for: text, font: font, origin: cover.origin, minWidth: cover.width, height: cover.height)
        contents = text
    }

    required init?(coder: NSCoder) { nil }

    override var allowsPicking: Bool { true }

    override var redactionArea: RedactionArea? { RedactionArea(rect: bounds, fill: (1, 1, 1)) }
    override var redactionAreas: [RedactionArea] {
        [RedactionArea(rect: originalCover, fill: (1, 1, 1)), RedactionArea(rect: bounds, fill: (1, 1, 1))]
    }

    static func cover(for text: String, font: NSFont, origin: CGPoint, minWidth: CGFloat, height: CGFloat) -> CGRect {
        let w = width(of: line(text, font: font, color: .black)) + 2
        return CGRect(x: origin.x, y: origin.y, width: max(minWidth, w), height: height)
    }

    /// Changes the text, keeping the left edge and cover height.
    func setText(_ text: String) {
        self.text = text
        contents = text
        bounds = Self.cover(for: text, font: textFont, origin: bounds.origin, minWidth: originalWidth, height: bounds.height)
    }

    override func render(in ctx: CGContext, box: CGRect, rotation: Int) {
        ctx.setFillColor(NSColor.white.cgColor)
        ctx.fill(originalCover)
        ctx.fill(bounds)
        ctx.textPosition = CGPoint(x: bounds.minX + 1, y: bounds.minY + baseline)
        CTLineDraw(Self.line(text, font: textFont, color: color), ctx)
    }

    /// Font of the page text: the embedded font's family when it is installed, else Helvetica (bold/italic kept).
    static func matchingFont(_ source: NSFont?, size: CGFloat) -> NSFont {
        guard let source else { return NSFont(name: "Helvetica", size: size)! }
        var name = source.fontName
        if let plus = name.firstIndex(of: "+"), name.distance(from: name.startIndex, to: plus) == 6 { name = String(name[name.index(after: plus)...]) }
        if let font = NSFont(name: name, size: size) { return font }
        let lower = name.lowercased()
        let bold = lower.contains("bold") || lower.contains("black") || lower.contains("heavy")
        let italic = lower.contains("italic") || lower.contains("oblique")
        let serif = lower.contains("times") || lower.contains("serif") && !lower.contains("sans") || lower.contains("georgia") || lower.contains("garamond")
        let family = serif ? "Times New Roman" : "Helvetica"
        var traits: NSFontTraitMask = []
        if bold { traits.insert(.boldFontMask) }
        if italic { traits.insert(.italicFontMask) }
        return NSFontManager.shared.font(withFamily: family, traits: traits, weight: 5, size: size) ?? NSFont(name: "Helvetica", size: size)!
    }

    /// Overlay replacing the text of `line` (a single-line selection) with `text`.
    static func make(replacing line: PDFSelection, on page: PDFPage, with text: String) -> EditedTextOverlay {
        let b = line.bounds(for: page)
        let attrs = line.attributedString.flatMap { $0.length > 0 ? $0.attributes(at: 0, effectiveRange: nil) : nil } ?? [:]
        let source = attrs[.font] as? NSFont
        let size = source.map(\.pointSize).flatMap { $0 > 1 ? $0 : nil } ?? max(4, b.height * 0.8)
        let font = matchingFont(source, size: size)
        let color = (attrs[.foregroundColor] as? NSColor) ?? .black
        // PDFKit's line bounds span the font's descent..ascent; put the baseline at the descent.
        let scale = b.height / max(1, font.ascender - font.descender)
        let baseline = -font.descender * min(1.2, max(0.8, scale))
        let original = (line.string ?? "").trimmingCharacters(in: .newlines)
        return EditedTextOverlay(cover: b.insetBy(dx: -1, dy: -1), text: text, originalText: original,
                                 font: font, color: color, baseline: baseline + 1)
    }
}

// MARK: - Redaction mark

/// Area marked with the Redact tool: a filled box (black unless another colour is chosen). When the
/// document is saved, or with Markup › Apply Redactions, the text, images and annotations under it
/// are removed from the file. Until then it can be moved, resized and deleted.
final class RedactOverlay: PageOverlay {
    init(bounds: CGRect, color: NSColor) {
        super.init(bounds: bounds)
        self.color = color
        contents = L("Redaction")
    }

    required init?(coder: NSCoder) { nil }

    override var allowsPicking: Bool { true }

    var fillRGB: (r: CGFloat, g: CGFloat, b: CGFloat) {
        let c = color.usingColorSpace(.sRGB) ?? .black
        return (c.redComponent, c.greenComponent, c.blueComponent)
    }

    override var redactionArea: RedactionArea? { RedactionArea(rect: bounds, fill: fillRGB, paint: true) }

    override func render(in ctx: CGContext, box: CGRect, rotation: Int) {
        let f = fillRGB
        ctx.setFillColor(red: f.r, green: f.g, blue: f.b, alpha: 1)
        ctx.fill(bounds)
    }

    /// On screen a pending mark also gets a red dashed outline, so it is told apart from page content.
    override func draw(with box: PDFDisplayBox, in context: CGContext) {
        super.draw(with: box, in: context)
        context.saveGState()
        context.setStrokeColor(NSColor.systemRed.cgColor)
        context.setLineWidth(1)
        context.setLineDash(phase: 0, lengths: [4, 2])
        context.stroke(bounds.insetBy(dx: 0.5, dy: 0.5))
        context.restoreGState()
    }
}

extension PDFAnnotation {
    /// Whiteout box (Whiteout tool): an opaque white square without border.
    var isWhiteout: Bool {
        guard isKind(.square), !(self is PageOverlay), let fill = interiorColor?.usingColorSpace(.sRGB) else { return false }
        let white = fill.redComponent > 0.98 && fill.greenComponent > 0.98 && fill.blueComponent > 0.98 && fill.alphaComponent > 0.98
        return white && (border?.lineWidth ?? 0) <= 0.01
    }
}

// MARK: - Baking on save

extension PageOverlay {
    /// File bytes with overlays drawn into the page content. Temporary annotations (compare highlights)
    /// are dropped unless `keepTemporary`. The live document is left as it was.
    static func bakedData(_ pdf: PDFDocument, keepTemporary: Bool = false) -> Data? {
        bake(pdf, keepTemporary: keepTemporary)?.data
    }

    /// Baking with redaction: whatever lies under edited text, whiteout boxes and redaction marks is
    /// removed from the page content (`Redactor`), annotations there are dropped, and the file is
    /// rewritten without the old objects. `only` limits which overlays are baked and redacted (the
    /// others are lifted and left out of the result, for the caller to put back).
    static func bake(_ pdf: PDFDocument, keepTemporary: Bool = false,
                     only: ((PageOverlay) -> Bool)? = nil) -> (data: Data, report: RedactionReport)? {
        var overlays: [Int: [PageOverlay]] = [:]
        var areas: [Int: [RedactionArea]] = [:]
        var finalAreas: [Int: [RedactionArea]] = [:]
        var covers: [Int: [PDFAnnotation]] = [:]   // whiteout boxes: stay as annotations, content under them goes
        var lifted: [(PDFAnnotation, PDFPage)] = []
        var annotationOrder: [(PDFPage, [PDFAnnotation])] = []
        for i in 0..<pdf.pageCount {
            guard let page = pdf.page(at: i) else { continue }
            annotationOrder.append((page, page.annotations))
            for a in page.annotations {
                if let o = a as? PageOverlay {
                    lifted.append((a, page))
                    guard only?(o) ?? true else { continue }
                    overlays[i, default: []].append(o)
                    areas[i, default: []].append(contentsOf: o.redactionAreas)
                    if o is RedactOverlay || o is CropRemovalOverlay { finalAreas[i, default: []].append(contentsOf: o.redactionAreas) }
                } else if a is TemporaryHighlight, !keepTemporary {
                    lifted.append((a, page))
                } else if a is LinkBorderMarker {
                    lifted.append((a, page))
                } else if only == nil, a.isWhiteout {
                    covers[i, default: []].append(a)
                    let area = RedactionArea(rect: a.bounds, fill: (1, 1, 1))
                    areas[i, default: []].append(area)
                    finalAreas[i, default: []].append(area)
                }
            }
        }
        guard !lifted.isEmpty || !areas.isEmpty else { return pdf.dataRepresentation().map { ($0, RedactionReport()) } }
        lifted.forEach { $0.1.removeAnnotation($0.0) }
        defer {
            // Re-appending only lifted overlays would move them above every other annotation after Save.
            for (page, annotations) in annotationOrder {
                page.annotations.forEach { page.removeAnnotation($0) }
                annotations.forEach { page.addAnnotation($0) }
            }
        }
        guard var data = pdf.dataRepresentation() else { return nil }
        guard !overlays.isEmpty || !areas.isEmpty else { return (data, RedactionReport()) }

        var report = RedactionReport()
        var rasterize: [Int: String] = [:]
        if !areas.isEmpty {
            if let result = Redactor.redact(data, areas: areas) {
                data = result.data
                report = result.report
                rasterize = result.failed
            } else {
                for i in areas.keys { rasterize[i] = "file structure not supported" }
            }
        }
        guard let out = PDFDocument(data: data) else { return nil }
        var replacements: [(Int, PDFPage)] = []
        for i in Set(overlays.keys).union(areas.keys).sorted() {
            guard let page = out.page(at: i) else { continue }
            var list = overlays[i] ?? []
            if let pageAreas = areas[i] {
                let removed = removeAnnotations(on: page, under: pageAreas, keeping: covers[i]?.map(\.bounds) ?? [], explicit: finalAreas[i] ?? [])
                if removed > 0 { report.update(i) { $0.annotations += removed } }
            }
            let rotation = page.rotation, box = page.bounds(for: .cropBox)
            let draw: (CGContext) -> Void = { ctx in
                for o in list { ctx.saveGState(); o.render(in: ctx, box: box, rotation: rotation); ctx.restoreGState() }
            }
            if let reason = rasterize[i] {
                // The content could not be rewritten safely: the page becomes a picture (with an OCR text layer).
                guard let raster = RedactionRaster.page(page, areas: areas[i] ?? [], overlay: draw) else { return nil }
                replacements.append((i, raster))
                report.update(i) { $0.rasterized = reason }
                continue
            }
            // Redaction boxes are already in the rewritten content: no need to redraw the page for them.
            list.removeAll { $0 is RedactOverlay || $0 is CropRemovalOverlay }
            if !list.isEmpty {
                guard let baked = redraw(page, overlay: { ctx in
                    for o in list { ctx.saveGState(); o.render(in: ctx, box: box, rotation: rotation); ctx.restoreGState() }
                }) else { return nil }
                replacements.append((i, baked))
            }
        }
        replace(replacements, in: out)
        guard var result = out.dataRepresentation() else { return nil }
        // Redacting only the original page is insufficient: newly drawn edited text or headers can lie
        // under an explicit redaction/whiteout too. Remove those glyphs after all overlays are rendered.
        let hasDrawnOverlays = overlays.values.contains { list in list.contains { !($0 is RedactOverlay) && !($0 is CropRemovalOverlay) } }
        if hasDrawnOverlays, !finalAreas.isEmpty {
            let final = Redactor.redact(result, areas: finalAreas)
            let failed = final?.failed ?? Dictionary(uniqueKeysWithValues: finalAreas.keys.map { ($0, "file structure not supported") })
            if let final {
                result = final.data
                for page in final.report.pages {
                    report.update(page.index) { item in
                        item.text += page.text; item.glyphs += page.glyphs; item.images += page.images
                        item.annotations += page.annotations
                    }
                }
            }
            if !failed.isEmpty {
                guard let work = PDFDocument(data: result) else { return nil }
                var replacement: [(Int, PDFPage)] = []
                for (index, reason) in failed {
                    guard let page = work.page(at: index),
                          let raster = RedactionRaster.page(page, areas: finalAreas[index] ?? [], overlay: { _ in }) else { return nil }
                    replacement.append((index, raster))
                    report.update(index) { $0.rasterized = reason }
                }
                replace(replacement, in: work)
                guard let rasterized = work.dataRepresentation() else { return nil }
                result = rasterized
            }
        }
        if let dir = ProcessInfo.processInfo.environment["K42_REDACT_DEBUG"] {   // intermediate files, for testing
            try? data.write(to: URL(fileURLWithPath: dir + "/redacted-incremental.pdf"))
            try? result.write(to: URL(fileURLWithPath: dir + "/pdfkit-saved.pdf"))
        }
        if !areas.isEmpty {
            // PDFKit keeps every object of the file it read, the replaced ones too: drop them.
            guard let compact = PDFCompactor.compact(result) else { return nil }
            result = compact
        }
        return (result, report)
    }

    /// An explicit redaction removes every overlapping annotation in full: partially covered text
    /// boxes/widgets still retain their entire contents/value. Whiteout covers themselves stay.
    /// Edited-text replacement areas retain the previous majority/centre test for unrelated annotations.
    static func removeAnnotations(on page: PDFPage, under areas: [RedactionArea], keeping: [CGRect], explicit: [RedactionArea] = []) -> Int {
        var removed = 0
        for a in page.annotations where !(a.isWhiteout && keeping.contains(a.bounds)) {
            let b = a.bounds
            let area = max(0.01, b.width * b.height)
            let explicitlyCovered = explicit.contains { redaction in
                let intersection = b.intersection(redaction.rect)
                return !intersection.isNull && intersection.width > 0 && intersection.height > 0
            }
            let hit = explicitlyCovered || areas.contains { r in
                let i = b.intersection(r.rect)
                return !i.isNull && (i.width * i.height / area >= 0.5 || r.rect.contains(CGPoint(x: b.midX, y: b.midY)))
            }
            guard hit else { continue }
            if a.isKind(.widget) { a.widgetStringValue = "" }
            a.contents = nil
            if let popup = a.popup { page.removeAnnotation(popup) }
            page.removeAnnotation(a)
            removed += 1
        }
        return removed
    }

    /// Bakes overlays into a copy of the document (export, print).
    static func bakedDocument(_ pdf: PDFDocument) -> PDFDocument? {
        bakedData(pdf).flatMap(PDFDocument.init(data:))
    }

    /// New page: the original content plus `overlay`, drawn in page space. Annotations (form fields,
    /// links, markup) are copied to the new page, the crop box and rotation are kept.
    static func redraw(_ page: PDFPage, overlay: (CGContext) -> Void) -> PDFPage? {
        let media = page.bounds(for: .mediaBox), crop = page.bounds(for: .cropBox)
        let rotation = page.rotation
        let annotations = page.annotations
        annotations.forEach { page.removeAnnotation($0) }
        page.rotation = 0
        defer {
            page.rotation = rotation
            annotations.forEach { page.addAnnotation($0) }
        }
        let data = NSMutableData()
        var box = media
        guard let consumer = CGDataConsumer(data: data), let ctx = CGContext(consumer: consumer, mediaBox: &box, nil) else { return nil }
        ctx.beginPDFPage(nil)
        ctx.saveGState()
        page.draw(with: .mediaBox, to: ctx)
        ctx.restoreGState()
        overlay(ctx)
        ctx.endPDFPage()
        ctx.closePDF()
        guard let newPage = PDFDocument(data: data as Data)?.page(at: 0) else { return nil }
        newPage.setBounds(crop, for: .cropBox)
        newPage.rotation = rotation
        for annotation in annotations {
            guard let copied = annotation.duplicateForEditing(on: newPage) else { return nil }
            newPage.addAnnotation(copied)
        }
        return newPage
    }

    /// Swaps pages in `pdf` and points outline entries and links at the new pages.
    static func replace(_ replacements: [(Int, PDFPage)], in pdf: PDFDocument) {
        var seen = Set<Int>()
        let replacements = replacements.filter { index, page in
            index >= 0 && index < pdf.pageCount && seen.insert(index).inserted && pdf.page(at: index) !== page
        }
        var map: [ObjectIdentifier: PDFPage] = [:]
        for (i, page) in replacements {
            if let old = pdf.page(at: i) { map[ObjectIdentifier(old)] = page }
        }
        // Destinations lose their page when it is removed: collect the new ones first.
        var fixes: [() -> Void] = []
        func remap(_ d: PDFDestination?) -> PDFDestination? {
            guard let d, let p = d.page, let new = map[ObjectIdentifier(p)] else { return nil }
            let destination = PDFDestination(page: new, at: d.point)
            destination.zoom = d.zoom
            return destination
        }
        func walk(_ item: PDFOutline) {
            if let d = remap(item.destination) { fixes.append { item.destination = d } }
            if let a = item.action as? PDFActionGoTo, let d = remap(a.destination) { fixes.append { item.action = PDFActionGoTo(destination: d) } }
            for i in 0..<item.numberOfChildren { if let c = item.child(at: i) { walk(c) } }
        }
        if let root = pdf.outlineRoot { walk(root) }
        let pages = (0..<pdf.pageCount).compactMap { pdf.page(at: $0) } + replacements.map(\.1)
        for page in pages {
            for a in page.annotations {
                if let d = remap(a.destination) { fixes.append { a.destination = d } }
                if let g = a.action as? PDFActionGoTo, let d = remap(g.destination) { fixes.append { a.action = PDFActionGoTo(destination: d) } }
            }
        }
        for (i, page) in replacements {
            pdf.removePage(at: i)
            pdf.insert(page, at: i)
        }
        fixes.forEach { $0() }
    }
}

/// Highlight from "Compare With…": shown in the editor, not saved unless the user asks.
final class TemporaryHighlight: PDFAnnotation {}
