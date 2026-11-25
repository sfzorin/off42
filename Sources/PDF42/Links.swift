import AppKit
import Kit42
import PDFKit

/// Dashed frame shown over a link in the editor; never saved or printed.
final class LinkBorderMarker: PDFAnnotation {
    init(around link: PDFAnnotation) {
        super.init(bounds: link.bounds, forType: .stamp, withProperties: nil)
        let b = PDFBorder()
        b.lineWidth = 1
        b.style = .dashed
        b.dashPattern = [3, 2]
        border = b
        color = .systemBlue
        shouldPrint = false
    }

    required init?(coder: NSCoder) { nil }

    override func draw(with box: PDFDisplayBox, in context: CGContext) {
        context.saveGState()
        context.setStrokeColor(NSColor.systemBlue.cgColor)
        context.setFillColor(NSColor.systemBlue.withAlphaComponent(0.08).cgColor)
        context.fill(bounds)
        context.setLineWidth(1)
        context.setLineDash(phase: 0, lengths: [3, 2])
        context.stroke(bounds.insetBy(dx: 0.5, dy: 0.5))
        context.restoreGState()
    }
}

/// Where a link goes.
enum LinkTarget {
    case url(URL)
    /// Page index and magnification: "" keeps the reader's zoom, "Fit" whole page, "FitH" page width.
    case page(Int, zoom: String)
    case destination(PDFDestination)
}

extension EditorWindowController {
    static let linkZooms: [(String, String)] = [(L("Keep the current zoom"), ""), (L("Whole page"), "Fit"), (L("Page width"), "FitH")]

    /// Link dialog. Returns the target, `nil` on Cancel; `delete` is set when the user removes the link.
    private func askLinkTarget(existing: PDFAnnotation?, delete: inout Bool) -> (LinkTarget, visible: Bool)? {
        guard let pdf else { return nil }
        let kind = NSPopUpButton(frame: .zero, pullsDown: false)
        kind.addItems(withTitles: [L("Web address (URL)"), L("Page in this document"), L("Bookmark")])
        let url = NSTextField()
        url.placeholderString = "https://example.com"
        let page = NSTextField()
        page.placeholderString = "1–\(pdf.pageCount)"
        let zoom = NSPopUpButton(frame: .zero, pullsDown: false)
        zoom.addItems(withTitles: Self.linkZooms.map(\.0))
        let bookmarks = NSPopUpButton(frame: .zero, pullsDown: false)
        let items = pdf.outlineRoot.map(OutlineTree.flatten) ?? []
        func depth(_ o: PDFOutline) -> Int { var d = 0, p = o.parent; while let q = p?.parent { d += 1; p = q }; return d }
        bookmarks.addItems(withTitles: items.isEmpty ? [L("No bookmarks")] : items.enumerated().map { "\(String(repeating: "   ", count: depth($0.element)))\($0.element.label ?? "")" + String(repeating: "\u{200B}", count: $0.offset) })
        bookmarks.isEnabled = !items.isEmpty
        let visible = NSButton(checkboxWithTitle: L("Show a border around the link"), target: nil, action: nil)

        // Pre-fill from the existing link.
        if let existing {
            if let u = (existing.action as? PDFActionURL)?.url ?? existing.url {
                kind.selectItem(at: 0)
                url.stringValue = u.absoluteString
            } else if let d = existing.destination ?? (existing.action as? PDFActionGoTo)?.destination, let p = d.page {
                kind.selectItem(at: 1)
                page.stringValue = pdfDoc?.extras.displayLabel(pdf.index(for: p)) ?? "\(pdf.index(for: p) + 1)"
                let fit = pdfDoc?.extras.linkFit[ObjectIdentifier(existing)] ?? ""
                zoom.selectItem(at: Self.linkZooms.firstIndex { $0.1 == fit } ?? 0)
            }
            visible.state = (existing.border?.lineWidth ?? 0) > 0 ? .on : .off
        } else {
            page.stringValue = pdfDoc?.extras.displayLabel(currentIndex) ?? "\(currentIndex + 1)"
            if let s = pdfView.currentSelection?.string?.trimmingCharacters(in: .whitespacesAndNewlines), s.contains(".") , !s.contains(" ") {
                url.stringValue = s.contains("://") ? s : "https://" + s
            }
        }
        let alert = NSAlert.form(existing == nil ? L("New Link") : L("Edit Link"), ok: existing == nil ? L("Add") : L("Done"), rows: [
            (L("Link to:"), kind), (L("Address:"), url), (L("Page:"), page), (L("Zoom:"), zoom), (L("Bookmark:"), bookmarks), ("", visible),
        ])
        if existing != nil { alert.addButton(withTitle: L("Delete Link")) }
        let sync = ClosureTarget {
            let k = kind.indexOfSelectedItem
            url.isEnabled = k == 0
            page.isEnabled = k == 1
            zoom.isEnabled = k == 1
            bookmarks.isEnabled = k == 2 && !items.isEmpty
        }
        kind.target = sync
        kind.action = #selector(ClosureTarget.fire)
        sync.fire()
        alert.window.initialFirstResponder = kind.indexOfSelectedItem == 0 ? url : page
        let response = alert.runModal()
        if response == .alertThirdButtonReturn { delete = true; return nil }
        guard response == .alertFirstButtonReturn else { return nil }
        switch kind.indexOfSelectedItem {
        case 0:
            var text = url.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !text.isEmpty else { NSSound.beep(); return nil }
            if text.contains("@"), !text.contains(":") { text = "mailto:" + text }
            else if !text.contains(":") { text = "https://" + text }
            guard !text.isEmpty, let u = URL(string: text) else { NSSound.beep(); return nil }
            return (.url(u), visible.state == .on)
        case 1:
            guard let i = PageLabels.index(of: page.stringValue, pdfDoc?.extras.labels ?? [], pageCount: pdf.pageCount) else { NSSound.beep(); return nil }
            return (.page(i, zoom: Self.linkZooms[max(0, zoom.indexOfSelectedItem)].1), visible.state == .on)
        default:
            let i = bookmarks.indexOfSelectedItem
            guard i >= 0, i < items.count, let d = OutlineTree.destination(items[i]) else { NSSound.beep(); return nil }
            return (.destination(d), visible.state == .on)
        }
    }

    /// Applies a target to a link annotation (action, border).
    private func configure(_ link: PDFAnnotation, _ target: LinkTarget, visible: Bool) {
        guard let pdf else { return }
        link.destination = nil
        switch target {
        case .url(let u):
            link.action = PDFActionURL(url: u)
            pdfDoc?.extras.linkFit[ObjectIdentifier(link)] = nil
        case .page(let i, let zoom):
            guard let p = pdf.page(at: i) else { return }
            let box = p.bounds(for: .cropBox)
            let u = kPDFDestinationUnspecifiedValue
            let d = PDFDestination(page: p, at: zoom == "Fit" ? CGPoint(x: u, y: u) : CGPoint(x: zoom == "FitH" ? u : box.minX, y: box.maxY))
            link.action = PDFActionGoTo(destination: d)
            pdfDoc?.extras.linkFit[ObjectIdentifier(link)] = zoom.isEmpty ? nil : zoom
        case .destination(let d):
            guard let page = d.page, pdf.index(for: page) != NSNotFound else { return }
            link.action = PDFActionGoTo(destination: PDFDestination(page: page, at: d.point))
            pdfDoc?.extras.linkFit[ObjectIdentifier(link)] = nil
        }
        let b = PDFBorder()
        b.lineWidth = visible ? 1 : 0
        link.border = b
        link.color = visible ? .systemBlue : .clear
    }

    static func makeLink(bounds: CGRect) -> PDFAnnotation {
        let link = PDFAnnotation(bounds: bounds, forType: .link, withProperties: nil)
        link.modificationDate = Date()
        return link
    }

    func createLink(in rect: CGRect, on page: PDFPage) {
        var delete = false
        guard let (target, visible) = askLinkTarget(existing: nil, delete: &delete) else { return }
        let link = Self.makeLink(bounds: rect)
        configure(link, target, visible: visible)
        add(link, to: page, name: L("Add Link"))
        pdfView.updateLinkBorders()
    }

    func editLink(_ link: PDFAnnotation, on page: PDFPage) {
        var delete = false
        let result = askLinkTarget(existing: link, delete: &delete)
        if delete {
            remove(link, from: page, name: L("Delete Link"))
            pdfView.updateLinkBorders()
            return
        }
        guard let (target, visible) = result else { return }
        let copy = Self.makeLink(bounds: link.bounds)
        configure(copy, target, visible: visible)
        // Replacing the annotation keeps undo simple: remove the old, add the new.
        undo?.beginUndoGrouping()
        remove(link, from: page, name: L("Edit Link"))
        add(copy, to: page, name: L("Edit Link"))
        undo?.endUndoGrouping()
        pdfView.updateLinkBorders()
    }

    @objc func toggleLinkBordersAction(_ sender: Any?) { pdfView.showsLinkBorders.toggle() }

    /// Adds links over web and e-mail addresses found in the page text (where there is no link yet).
    @objc func createLinksFromURLsAction(_ sender: Any?) {
        guard let pdf else { return }
        let count = Self.createLinksFromURLs(pdf) { [weak self] link, page in self?.add(link, to: page, name: L("Create Links from URLs")) }
        pdfView.updateLinkBorders()
        banner.show(count > 0 ? .success : .info, count > 0 ? L("Links created: %d", count) : L("No web or e-mail addresses without a link were found"))
        DispatchQueue.main.asyncAfter(deadline: .now() + 4) { [weak self] in self?.updateSignatureBanner() }
    }

    /// Detected addresses → link annotations (one per line of a match); returns how many were added.
    @discardableResult
    static func createLinksFromURLs(_ pdf: PDFDocument, add: (PDFAnnotation, PDFPage) -> Void) -> Int {
        guard let detector = try? NSDataDetector(types: NSTextCheckingResult.CheckingType.link.rawValue) else { return 0 }
        var count = 0
        for i in 0..<pdf.pageCount {
            guard let page = pdf.page(at: i), let text = page.string else { continue }
            let existing = page.annotations.filter { $0.isKind(.link) }.map(\.bounds)
            for match in detector.matches(in: text, range: NSRange(location: 0, length: (text as NSString).length)) {
                guard var url = match.url, let selection = page.selection(for: match.range) else { continue }
                if url.scheme == nil, let u = URL(string: "https://" + url.absoluteString) { url = u }
                for line in selection.selectionsByLine() {
                    let b = line.bounds(for: page).insetBy(dx: -1, dy: -1)
                    guard b.width > 2, b.height > 2 else { continue }
                    let covered = existing.contains { e in
                        let x = e.intersection(b)
                        return !x.isNull && x.width * x.height > 0.5 * b.width * b.height
                    }
                    guard !covered else { continue }
                    let link = makeLink(bounds: b)
                    link.action = PDFActionURL(url: url)
                    let border = PDFBorder()
                    border.lineWidth = 0
                    link.border = border
                    add(link, page)
                    count += 1
                }
            }
        }
        return count
    }
}
