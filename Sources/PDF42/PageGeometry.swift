import AppKit
import CoreText
import Kit42
import PDFKit

/// Invisible overlay marking a strip outside the crop box: when the document is saved, the content
/// there is removed from the file by the redaction engine (Page › Crop… › remove the content outside).
final class CropRemovalOverlay: PageOverlay {
    override var redactionArea: RedactionArea? { RedactionArea(rect: bounds, fill: (1, 1, 1)) }
    override func draw(with box: PDFDisplayBox, in context: CGContext) {}
    override func render(in ctx: CGContext, box: CGRect, rotation: Int) {}
}

/// Page sets chosen in the crop, size, header/footer dialogs.
final class PageSetChooser: NSObject {
    let popup = NSPopUpButton(frame: .zero, pullsDown: false)
    let range = NSTextField()
    let view: NSStackView

    override init() {
        popup.addItems(withTitles: [L("Current page"), L("All pages"), L("Odd pages"), L("Even pages"), L("Pages:")])
        range.placeholderString = "1-3, 5"
        range.translatesAutoresizingMaskIntoConstraints = false
        range.widthAnchor.constraint(equalToConstant: 110).isActive = true
        view = NSStackView(views: [popup, range])
        super.init()
        popup.target = self
        popup.action = #selector(sync)
        sync()
    }

    @objc private func sync() { range.isEnabled = popup.indexOfSelectedItem == 4 }

    /// Page indices, nil for an invalid range.
    func indices(current: Int, selected: [Int], pageCount: Int) -> [Int]? {
        switch popup.indexOfSelectedItem {
        case 0: return selected.count > 1 ? selected : [current]
        case 1: return Array(0..<pageCount)
        case 2: return stride(from: 0, to: pageCount, by: 2).map { $0 }
        case 3: return stride(from: 1, to: pageCount, by: 2).map { $0 }
        default:
            guard let i = PDFTools.pageIndices(range.stringValue, pageCount: pageCount), !i.isEmpty else { return nil }
            return i
        }
    }
}

enum PageGeometry {
    static let mm: CGFloat = 72 / 25.4

    /// Margins as seen on screen (rotation applied) → crop box in page space.
    static func cropBox(media: CGRect, rotation: Int, left: CGFloat, top: CGFloat, right: CGFloat, bottom: CGFloat) -> CGRect {
        // Page-space edges: (minX, maxY, maxX, minY) amounts for the displayed (left, top, right, bottom).
        let (l, t, r, b): (CGFloat, CGFloat, CGFloat, CGFloat)
        switch ((rotation % 360) + 360) % 360 {
        case 90: (l, t, r, b) = (top, right, bottom, left)     // turned clockwise: the page's left edge is on top
        case 180: (l, t, r, b) = (right, bottom, left, top)
        case 270: (l, t, r, b) = (bottom, left, top, right)
        default: (l, t, r, b) = (left, top, right, bottom)
        }
        let rect = CGRect(x: media.minX + l, y: media.minY + b, width: media.width - l - r, height: media.height - t - b)
        return rect.width > 10 && rect.height > 10 ? rect : media
    }

    /// The inverse: displayed margins (left, top, right, bottom) of `crop` within `media`.
    static func margins(crop: CGRect, media: CGRect, rotation: Int) -> (CGFloat, CGFloat, CGFloat, CGFloat) {
        let l = crop.minX - media.minX, b = crop.minY - media.minY, r = media.maxX - crop.maxX, t = media.maxY - crop.maxY
        switch ((rotation % 360) + 360) % 360 {
        case 90: return (b, l, t, r)
        case 180: return (r, b, l, t)
        case 270: return (t, r, b, l)
        default: return (l, t, r, b)
        }
    }

    /// Bounding box of everything that is not white on the page (page space), nil for a blank page.
    static func contentBox(_ page: PDFPage) -> CGRect? {
        let media = page.bounds(for: .mediaBox)
        let scale: CGFloat = 1.5
        guard media.width.isFinite, media.height.isFinite, media.width > 0, media.height > 0,
              media.width * scale < 40_000_000, media.height * scale < 40_000_000,
              media.width * media.height * scale * scale < 40_000_000 else { return nil }
        let w = Int(media.width * scale), h = Int(media.height * scale)
        guard w > 0, h > 0,
              let ctx = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w, space: CGColorSpaceCreateDeviceGray(),
                                  bitmapInfo: CGImageAlphaInfo.none.rawValue) else { return nil }
        ctx.setFillColor(gray: 1, alpha: 1)
        ctx.fill(CGRect(x: 0, y: 0, width: w, height: h))
        ctx.scaleBy(x: scale, y: scale)
        ctx.translateBy(x: -media.minX, y: -media.minY)
        let rotation = page.rotation
        page.rotation = 0
        page.draw(with: .mediaBox, to: ctx)
        page.rotation = rotation
        guard let data = ctx.data else { return nil }
        let px = data.bindMemory(to: UInt8.self, capacity: w * h)
        var minX = w, minY = h, maxX = -1, maxY = -1
        for y in 0..<h {
            let row = px + y * w
            for x in 0..<w where row[x] < 245 {
                if x < minX { minX = x }
                if x > maxX { maxX = x }
                if y < minY { minY = y }
                if y > maxY { maxY = y }
            }
        }
        guard maxX >= minX, maxY >= minY else { return nil }
        // Bitmap rows run top to bottom.
        let rect = CGRect(x: CGFloat(minX) / scale + media.minX, y: CGFloat(h - 1 - maxY) / scale + media.minY,
                          width: CGFloat(maxX - minX + 1) / scale, height: CGFloat(maxY - minY + 1) / scale)
        return rect.intersection(media)
    }

    /// The parts of the media box outside `crop` (up to four strips).
    static func strips(media: CGRect, crop: CGRect) -> [CGRect] {
        var out: [CGRect] = []
        if crop.minX > media.minX + 0.5 { out.append(CGRect(x: media.minX, y: media.minY, width: crop.minX - media.minX, height: media.height)) }
        if crop.maxX < media.maxX - 0.5 { out.append(CGRect(x: crop.maxX, y: media.minY, width: media.maxX - crop.maxX, height: media.height)) }
        if crop.minY > media.minY + 0.5 { out.append(CGRect(x: crop.minX, y: media.minY, width: crop.width, height: crop.minY - media.minY)) }
        if crop.maxY < media.maxY - 0.5 { out.append(CGRect(x: crop.minX, y: crop.maxY, width: crop.width, height: media.maxY - crop.maxY)) }
        return out
    }

    static let paperSizes: [(String, CGSize)] = [
        ("A3", CGSize(width: 842, height: 1191)), ("A4", CGSize(width: 595, height: 842)), ("A5", CGSize(width: 420, height: 595)),
        ("Letter", CGSize(width: 612, height: 792)), ("Legal", CGSize(width: 612, height: 1008)), ("Tabloid", CGSize(width: 792, height: 1224)),
    ]

    static func paperName(_ size: CGSize) -> String? {
        let s = CGSize(width: min(size.width, size.height), height: max(size.width, size.height))
        return paperSizes.first { abs($0.1.width - s.width) < 2 && abs($0.1.height - s.height) < 2 }?.0
    }

    private static func resizedFont(_ font: NSFont, to size: CGFloat) -> NSFont {
        // A system UI font has a special AppKit identity, not a loadable PostScript name. A CoreText
        // resize turns it into a private SF font that PDFKit resolves as Helvetica when assigning it.
        // Recreate UI fonts through AppKit's factory, keeping their weight, width and italic trait.
        if font.familyName == NSFont.systemFont(ofSize: font.pointSize).familyName {
            let traits = font.fontDescriptor.object(forKey: .traits) as? [NSFontDescriptor.TraitKey: Any] ?? [:]
            let weight = CGFloat((traits[.weight] as? NSNumber)?.doubleValue ?? 0)
            let width = CGFloat((traits[.width] as? NSNumber)?.doubleValue ?? 0)
            let resized = NSFont.systemFont(ofSize: size, weight: .init(rawValue: weight), width: .init(rawValue: width))
            if font.fontDescriptor.symbolicTraits.contains(.italic) {
                return NSFontManager.shared.convert(resized, toHaveTrait: .italicFontMask)
            }
            return resized
        }
        return CTFontCreateCopyWithAttributes(font as CTFont, size, nil, nil) as NSFont
    }

    /// New page of `size` (as displayed) with the old page's visible content scaled to fit and centred.
    /// Annotations move with the content (their rectangles are scaled).
    static func scaled(_ page: PDFPage, to size: CGSize) -> PDFPage? {
        let crop = page.bounds(for: .cropBox)
        guard size.width.isFinite, size.height.isFinite, size.width >= 36, size.height >= 36,
              size.width <= 14400, size.height <= 14400, crop.width.isFinite, crop.height.isFinite,
              crop.width > 0, crop.height > 0 else { return nil }
        let rotation = page.rotation
        let target = rotation % 180 != 0 ? CGSize(width: size.height, height: size.width) : size
        let k = min(target.width / crop.width, target.height / crop.height)
        let dx = (target.width - crop.width * k) / 2, dy = (target.height - crop.height * k) / 2
        let transform = CGAffineTransform(translationX: dx, y: dy).scaledBy(x: k, y: k).translatedBy(x: -crop.minX, y: -crop.minY)
        let data = NSMutableData()
        var box = CGRect(origin: .zero, size: target)
        guard let consumer = CGDataConsumer(data: data), let ctx = CGContext(consumer: consumer, mediaBox: &box, nil) else { return nil }
        ctx.beginPDFPage(nil)
        ctx.saveGState()
        ctx.concatenate(transform)
        ctx.clip(to: crop)
        if let reference = page.pageRef { ctx.drawPDFPage(reference) }
        ctx.restoreGState()
        ctx.endPDFPage()
        ctx.closePDF()
        guard let newPage = PDFDocument(data: data as Data)?.page(at: 0) else { return nil }
        newPage.rotation = rotation
        for a in page.annotations where !(a is LinkBorderMarker) {
            guard let copy = a.duplicateForEditing(on: newPage) else { return nil }
            copy.bounds = a.bounds.applying(transform)
            if let border = copy.border { border.lineWidth *= k }
            if let font = a.font { copy.font = resizedFont(font, to: font.pointSize * k) }
            let local = CGAffineTransform(scaleX: k, y: k)
            if a.isMarkup, let points = a.quadrilateralPoints, !points.isEmpty { copy.quadrilateralPoints = points.map { NSValue(point: $0.pointValue.applying(local)) } }
            if a.isKind(.line) { copy.startPoint = a.startPoint.applying(local); copy.endPoint = a.endPoint.applying(local) }
            if let paths = a.paths {
                copy.paths?.forEach { copy.remove($0) }
                for original in paths {
                    guard let path = original.copy() as? NSBezierPath else { continue }
                    path.transform(using: AffineTransform(m11: k, m12: 0, m21: 0, m22: k, tX: 0, tY: 0))
                    path.lineWidth *= k
                    copy.add(path)
                }
            }
            newPage.addAnnotation(copy)
        }
        return newPage
    }
}

extension EditorWindowController {
    // MARK: - Crop

    @objc func cropAction(_ sender: Any?) {
        guard let page = pdfView.currentPage else { return }
        cropDialog(rect: nil, on: page)
    }

    /// Crop dialog; `rect` is a rectangle drawn with the Crop tool (page space).
    func cropDialog(rect: CGRect?, on page: PDFPage) {
        guard let pdf else { return }
        let media = page.bounds(for: .mediaBox)
        let crop = rect.map { $0.intersection(media) } ?? page.bounds(for: .cropBox)
        let m = PageGeometry.margins(crop: crop, media: media, rotation: page.rotation)
        func field(_ v: CGFloat) -> NSTextField {
            let f = NSTextField(string: String(format: "%.1f", v / PageGeometry.mm))
            f.translatesAutoresizingMaskIntoConstraints = false
            f.widthAnchor.constraint(equalToConstant: 60).isActive = true
            return f
        }
        let left = field(m.0), top = field(m.1), right = field(m.2), bottom = field(m.3)
        let originalValues = [m.0, m.1, m.2, m.3], originalStrings = [left, top, right, bottom].map(\.stringValue)
        let marginRow1 = NSStackView(views: [NSTextField(labelWithString: L("Left")), left, NSTextField(labelWithString: L("Right")), right])
        let marginRow2 = NSStackView(views: [NSTextField(labelWithString: L("Top")), top, NSTextField(labelWithString: L("Bottom")), bottom])
        let auto = NSButton(checkboxWithTitle: L("Remove white margins automatically (margins above are added around the content)"), target: nil, action: nil)
        let remove = NSButton(checkboxWithTitle: L("Delete the content outside the crop area from the file when saving"), target: nil, action: nil)
        let pages = PageSetChooser()
        if rect == nil, page.bounds(for: .cropBox) == media { [left, top, right, bottom].forEach { $0.stringValue = "0" } }
        let size = { (r: CGRect) in String(format: "%.0f × %.0f mm", r.width / PageGeometry.mm, r.height / PageGeometry.mm) }
        let alert = NSAlert.form(L("Crop Pages"), info: L("Margins in millimetres from the page edges (media box %@). Draw a rectangle with the Crop tool to set them by eye. Cropping hides the content outside; it stays in the file unless deleted.", size(media)),
                                 ok: L("Crop"), rows: [
                                    (L("Margins (mm):"), marginRow1), ("", marginRow2), ("", auto), (L("Pages:"), pages.view), ("", remove),
                                 ])
        alert.addButton(withTitle: L("Remove Crop"))
        let response = alert.runModal()
        guard response == .alertFirstButtonReturn || response == .alertThirdButtonReturn else { return }
        guard let targets = pages.indices(current: pdf.index(for: page), selected: targetIndices, pageCount: pdf.pageCount) else {
            NSSound.beep()
            return
        }
        let fields = [left, top, right, bottom]
        var values = originalValues
        for (i, field) in fields.enumerated() where response == .alertFirstButtonReturn {
            if field.stringValue == originalStrings[i] { continue }
            guard let n = Double(field.stringValue.replacingOccurrences(of: ",", with: ".")), n.isFinite, n >= 0, n <= 100000 else { NSSound.beep(); return }
            values[i] = n * PageGeometry.mm
        }
        func value(_ field: NSTextField) -> CGFloat { values[fields.firstIndex { $0 === field }!] }
        var boxes: [(PDFPage, CGRect)] = []
        for i in targets {
            guard let p = pdf.page(at: i) else { continue }
            let media = p.bounds(for: .mediaBox)
            if response == .alertThirdButtonReturn { boxes.append((p, media)); continue }
            if auto.state == .on {
                guard let content = PageGeometry.contentBox(p) else { continue }
                // Padding from the fields, in displayed directions.
                let out = PageGeometry.cropBox(media: content, rotation: p.rotation,
                                               left: -value(left), top: -value(top), right: -value(right), bottom: -value(bottom))
                boxes.append((p, out.intersection(media)))
            } else {
                let shown = EditorWindowController.displayedSize(p)
                guard value(left) + value(right) < shown.width - 10, value(top) + value(bottom) < shown.height - 10 else { NSSound.beep(); return }
                boxes.append((p, PageGeometry.cropBox(media: media, rotation: p.rotation, left: value(left), top: value(top),
                                                      right: value(right), bottom: value(bottom))))
            }
        }
        setCropBoxes(boxes, removeOutside: remove.state == .on && response == .alertFirstButtonReturn, name: L("Crop Pages"))
    }

    /// Sets crop boxes (undoable); optionally marks the content outside for removal on save.
    func setCropBoxes(_ boxes: [(PDFPage, CGRect)], removeOutside: Bool, name: String) {
        guard let pdf else { return }
        let valid = boxes.filter { page, rect in
            pdf.index(for: page) != NSNotFound && rect.origin.x.isFinite && rect.origin.y.isFinite && rect.width.isFinite && rect.height.isFinite && rect.width > 0 && rect.height > 0
        }
        let state: [(PDFPage, CGRect, [PDFAnnotation])] = valid.map { page, rect in
            // Earlier removal marks follow the new crop box.
            let had = page.annotations.contains { $0 is CropRemovalOverlay }
            let marks = removeOutside || had ? PageGeometry.strips(media: page.bounds(for: .mediaBox), crop: rect).map { CropRemovalOverlay(bounds: $0) } : []
            return (page, rect, marks)
        }
        setCropState(state, name: name)
    }

    private func setCropState(_ state: [(PDFPage, CGRect, [PDFAnnotation])], name: String) {
        let old = state.map { s in (s.0, s.0.bounds(for: .cropBox), s.0.annotations.filter { $0 is CropRemovalOverlay }) }
        guard zip(old, state).contains(where: { $0.0.1 != $0.1.1 || $0.0.2.map(\.bounds) != $0.1.2.map(\.bounds) }) else { return }
        for (page, rect, marks) in state {
            page.setBounds(rect, for: .cropBox)
            page.annotations.filter { $0 is CropRemovalOverlay }.forEach { page.removeAnnotation($0) }
            marks.forEach { page.addAnnotation($0) }
        }
        pdfView.layoutDocumentView()
        sidebar.thumbnails.reload()
        undo?.registerUndo(withTarget: self) { $0.setCropState(old, name: name) }
        undo?.setActionName(name)
    }

    // MARK: - Page size

    @objc func pageSizeAction(_ sender: Any?) {
        guard let pdf, let page = pdfView.currentPage else { return }
        let size = NSPopUpButton(frame: .zero, pullsDown: false)
        size.addItems(withTitles: PageGeometry.paperSizes.map(\.0) + [L("Custom")])
        let current = EditorWindowController.displayedSize(page)
        size.selectItem(at: PageGeometry.paperSizes.firstIndex { $0.0 == PageGeometry.paperName(current) } ?? PageGeometry.paperSizes.count)
        let width = NSTextField(string: String(format: "%.0f", current.width / PageGeometry.mm))
        let height = NSTextField(string: String(format: "%.0f", current.height / PageGeometry.mm))
        for f in [width, height] { f.translatesAutoresizingMaskIntoConstraints = false; f.widthAnchor.constraint(equalToConstant: 60).isActive = true }
        let custom = NSStackView(views: [width, NSTextField(labelWithString: "×"), height, NSTextField(labelWithString: L("mm"))])
        let orientation = NSPopUpButton(frame: .zero, pullsDown: false)
        orientation.addItems(withTitles: [L("Same as each page"), L("Portrait"), L("Landscape")])
        let mode = NSPopUpButton(frame: .zero, pullsDown: false)
        mode.addItems(withTitles: [L("Scale the content to fit"), L("Keep the content size (extend or trim the canvas)")])
        let pages = PageSetChooser()
        pages.popup.selectItem(at: 1)
        pages.range.isEnabled = false
        let sync = ClosureTarget {
            let isCustom = size.indexOfSelectedItem == PageGeometry.paperSizes.count
            width.isEnabled = isCustom
            height.isEnabled = isCustom
        }
        size.target = sync
        size.action = #selector(ClosureTarget.fire)
        sync.fire()
        let alert = NSAlert.form(L("Page Size"), info: L("Current page: %@.", String(format: "%.0f × %.0f mm", current.width / PageGeometry.mm, current.height / PageGeometry.mm)),
                                 ok: L("Apply"), rows: [
                                    (L("Size:"), size), (L("Custom size:"), custom), (L("Orientation:"), orientation), (L("Content:"), mode), (L("Pages:"), pages.view),
                                 ])
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        guard let targets = pages.indices(current: currentIndex, selected: targetIndices, pageCount: pdf.pageCount) else { NSSound.beep(); return }
        var target: CGSize
        if size.indexOfSelectedItem < PageGeometry.paperSizes.count { target = PageGeometry.paperSizes[size.indexOfSelectedItem].1 } else {
            let w = (Double(width.stringValue.replacingOccurrences(of: ",", with: ".")) ?? 0) * PageGeometry.mm
            let h = (Double(height.stringValue.replacingOccurrences(of: ",", with: ".")) ?? 0) * PageGeometry.mm
            guard w >= 36, h >= 36, w < 14400, h < 14400 else { NSSound.beep(); return }
            target = CGSize(width: w, height: h)
        }
        resizePages(targets, to: target, orientation: orientation.indexOfSelectedItem, scale: mode.indexOfSelectedItem == 0)
    }

    /// orientation: 0 as the page, 1 portrait, 2 landscape.
    func resizePages(_ indices: [Int], to size: CGSize, orientation: Int, scale: Bool) {
        guard let pdf, size.width.isFinite, size.height.isFinite, size.width >= 36, size.height >= 36,
              size.width <= 14400, size.height <= 14400 else { return }
        prepareForSave()
        let chosen = Array(Set(indices.filter { $0 >= 0 && $0 < pdf.pageCount })).sorted()
        let needsBake = scale && chosen.contains { pdf.page(at: $0)?.annotations.contains { $0 is PageOverlay || $0.isWhiteout } == true }
        var source = pdf
        if needsBake {
            guard let bytes = PageOverlay.bakedData(pdf), let snapshot = PDFDocument(data: bytes) else {
                presentError(NSError(domain: NSCocoaErrorDomain, code: NSFileWriteUnknownError)); return
            }
            source = snapshot
        }
        var replacements: [(Int, PDFPage)] = []
        var boxes: [(PDFPage, CGRect, CGRect)] = []
        for i in chosen {
            guard let page = pdf.page(at: i) else { continue }
            let shown = EditorWindowController.displayedSize(page)
            let landscape = orientation == 0 ? shown.width > shown.height : orientation == 2
            let s = landscape == (size.width > size.height) ? size : CGSize(width: size.height, height: size.width)
            if scale {
                guard let input = source.page(at: i), let p = PageGeometry.scaled(input, to: s) else { return }
                if needsBake {
                    let areas = page.annotations.flatMap { ($0 as? PageOverlay)?.redactionAreas ?? ($0.isWhiteout ? [RedactionArea(rect: $0.bounds, fill: (1, 1, 1))] : []) }
                    let crop = page.bounds(for: .cropBox), target = p.bounds(for: .mediaBox)
                    let k = min(target.width / crop.width, target.height / crop.height)
                    let t = CGAffineTransform(translationX: (target.width - crop.width * k) / 2, y: (target.height - crop.height * k) / 2)
                        .scaledBy(x: k, y: k).translatedBy(x: -crop.minX, y: -crop.minY)
                    for marker in page.annotations where marker is TemporaryHighlight && !areas.contains(where: { $0.rect.intersects(marker.bounds) }) {
                        if let copy = marker.duplicateForEditing(on: p) {
                            copy.bounds = marker.bounds.applying(t)
                            copy.quadrilateralPoints = marker.quadrilateralPoints?.map { NSValue(point: $0.pointValue.applying(CGAffineTransform(scaleX: k, y: k))) }
                            p.addAnnotation(copy)
                        }
                    }
                }
                replacements.append((i, p))
            } else {
                // The canvas grows or shrinks around the visible area's centre.
                let crop = page.bounds(for: .cropBox)
                let t = page.rotation % 180 != 0 ? CGSize(width: s.height, height: s.width) : s
                let rect = CGRect(x: crop.midX - t.width / 2, y: crop.midY - t.height / 2, width: t.width, height: t.height)
                boxes.append((page, rect, rect))
            }
        }
        if !replacements.isEmpty { swapPages(replacements, name: L("Page Size")) }
        if !boxes.isEmpty { setPageBoxes(boxes, name: L("Page Size")) }
    }

    /// Media and crop boxes (undoable).
    func setPageBoxes(_ boxes: [(PDFPage, CGRect, CGRect)], name: String) {
        let old = boxes.map { ($0.0, $0.0.bounds(for: .mediaBox), $0.0.bounds(for: .cropBox)) }
        for (page, media, crop) in boxes {
            page.setBounds(media, for: .mediaBox)
            page.setBounds(crop, for: .cropBox)
        }
        pdfView.layoutDocumentView()
        sidebar.thumbnails.reload()
        undo?.registerUndo(withTarget: self) { $0.setPageBoxes(old, name: name) }
        undo?.setActionName(name)
    }

    /// Swaps pages (undoable); outline entries and links follow the new pages.
    func swapPages(_ replacements: [(Int, PDFPage)], name: String) {
        swapPages(replacements, name: name, restoringExtras: nil)
    }

    private func swapPages(_ replacements: [(Int, PDFPage)], name: String, restoringExtras: DocumentExtras?) {
        guard let pdf else { return }
        var seen = Set<Int>()
        let valid = replacements.filter { $0.0 >= 0 && $0.0 < pdf.pageCount && seen.insert($0.0).inserted }
        guard !valid.isEmpty else { return }
        let old = valid.compactMap { r in pdf.page(at: r.0).map { (r.0, $0) } }
        let current = currentIndex, selected = targetIndices, previousExtras = pdfDoc?.extras
        var adjusted = restoringExtras ?? previousExtras
        if restoringExtras == nil {
            for (index, replacement) in valid {
                guard let previous = pdf.page(at: index) else { continue }
                let oldLinks = previous.annotations.filter { $0.isKind(.link) }, newLinks = replacement.annotations.filter { $0.isKind(.link) }
                if oldLinks.count == newLinks.count {
                    for (before, after) in zip(oldLinks, newLinks) {
                        let fit = adjusted?.linkFit.removeValue(forKey: ObjectIdentifier(before))
                        if let fit { adjusted?.linkFit[ObjectIdentifier(after)] = fit }
                    }
                }
                let oldFiles = previous.annotations.filter { $0.kind == "FileAttachment" }, newFiles = replacement.annotations.filter { $0.kind == "FileAttachment" }
                let previousFiles = adjusted?.annotationFiles ?? []
                let remappedFiles: [AnnotationFile] = previousFiles.compactMap { file in
                    guard file.page == index else { return file }
                    let at = oldFiles.firstIndex { $0 === file.annotation || $0.bounds == file.rect }
                    let match: PDFAnnotation?
                    if oldFiles.count == newFiles.count, let at { match = newFiles[at] }
                    else { match = newFiles.first { $0.bounds == file.rect } }
                    guard let match else { return nil }
                    var moved = file; moved.annotation = match; moved.rect = match.bounds; return moved
                }
                adjusted?.annotationFiles = remappedFiles
            }
        }
        pdfView.selectAnnotation(nil)
        PageOverlay.replace(valid, in: pdf)
        if let adjusted { pdfDoc?.extras = adjusted }
        pdfView.layoutDocumentView()
        if let page = pdf.page(at: min(current, pdf.pageCount - 1)) { pdfView.go(to: page) }
        sidebar.thumbnails.reload()
        sidebar.thumbnails.selectPages(selected.compactMap { pdf.page(at: $0) })
        sidebar.reloadOutline()
        sidebar.reloadAttachments()
        undo?.registerUndo(withTarget: self) { $0.swapPages(old, name: name, restoringExtras: previousExtras) }
        undo?.setActionName(name)
        updateSubtitleNow()
    }
}
