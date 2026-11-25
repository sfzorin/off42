import AppKit
import Kit42
import PDFKit

/// Bookmark (outline) editing. Every change is undoable: the whole tree is snapshotted before it.
extension EditorWindowController {
    struct OutlineNode {
        var label: String
        var destination: PDFDestination?
        var action: PDFAction?
        var isOpen: Bool
        var style: OutlineStyle?
        var children: [OutlineNode]
    }

    func outlineSnapshot() -> [OutlineNode] {
        let styles = pdfDoc?.extras.outlineStyles ?? [:]
        func node(_ o: PDFOutline) -> OutlineNode {
            OutlineNode(label: o.label ?? "", destination: o.destination, action: o.destination == nil ? o.action : nil, isOpen: o.isOpen,
                        style: styles[ObjectIdentifier(o)], children: (0..<o.numberOfChildren).compactMap { o.child(at: $0) }.map(node))
        }
        guard let root = pdf?.outlineRoot else { return [] }
        return (0..<root.numberOfChildren).compactMap { root.child(at: $0) }.map(node)
    }

    /// Rebuilds the outline from a snapshot; returns the new items in document order.
    @discardableResult
    func restoreOutline(_ nodes: [OutlineNode]) -> [PDFOutline] {
        guard let pdf else { return [] }
        var styles: [ObjectIdentifier: OutlineStyle] = [:]
        var made: [PDFOutline] = []
        func build(_ n: OutlineNode) -> PDFOutline {
            let o = PDFOutline()
            o.label = n.label
            if let d = n.destination { o.destination = d } else if let a = n.action { o.action = a }
            made.append(o)
            if let s = n.style { styles[ObjectIdentifier(o)] = s }
            for (i, c) in n.children.enumerated() { o.insertChild(build(c), at: i) }
            o.isOpen = n.isOpen
            return o
        }
        let root = PDFOutline()
        for (i, n) in nodes.enumerated() { root.insertChild(build(n), at: i) }
        pdf.outlineRoot = root
        pdfDoc?.extras.outlineStyles = styles
        return made
    }

    private func applyOutline(_ nodes: [OutlineNode], name: String) {
        let current = outlineSnapshot()
        restoreOutline(nodes)
        pdfDoc?.extras.dirty.insert(.outline)
        sidebar.reloadOutline()
        undo?.registerUndo(withTarget: self) { $0.applyOutline(current, name: name) }
        undo?.setActionName(name)
    }

    /// Runs `change` on the live outline (creating the root when there is none), undoably.
    func editOutline(_ name: String, select: (() -> PDFOutline?)? = nil, _ change: (PDFOutline) -> Void) {
        guard let pdf else { return }
        let before = outlineSnapshot()
        if pdf.outlineRoot == nil { pdf.outlineRoot = PDFOutline() }
        guard let root = pdf.outlineRoot else { return }
        change(root)
        pdfDoc?.extras.dirty.insert(.outline)
        undo?.registerUndo(withTarget: self) { $0.applyOutline(before, name: name) }
        undo?.setActionName(name)
        sidebar.reloadOutline(select: select?())
    }

    /// The current view: page and top-left point visible in the editor.
    var currentViewDestination: PDFDestination? {
        guard let page = pdfView.currentPage else { return nil }
        if let d = pdfView.currentDestination, let p = d.page {
            let box = p.bounds(for: .cropBox)
            let y = min(box.maxY, max(box.minY, d.point.y))
            return PDFDestination(page: p, at: CGPoint(x: max(box.minX, d.point.x), y: y))
        }
        let box = page.bounds(for: .cropBox)
        return PDFDestination(page: page, at: CGPoint(x: box.minX, y: box.maxY))
    }

    // MARK: Actions

    @objc func addBookmarkAction(_ sender: Any?) { addBookmark(asChild: false) }
    @objc func addChildBookmarkAction(_ sender: Any?) { addBookmark(asChild: true) }

    private func addBookmark(asChild: Bool) {
        guard let pdf, let dest = currentViewDestination else { return }
        let selected = pdfView.currentSelection?.string?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        let index = dest.page.map { pdf.index(for: $0) } ?? 0
        let title = !selected.isEmpty && selected.count < 200 ? selected.replacingOccurrences(of: "\n", with: " ")
            : L("Page %@", pdfDoc?.extras.displayLabel(index) ?? "\(index + 1)")
        let item = PDFOutline()
        item.label = title
        item.destination = dest
        let anchor = sidebar.selectedBookmark
        editOutline(L("Add Bookmark"), select: { item }) { root in
            if asChild, let anchor {
                anchor.insertChild(item, at: anchor.numberOfChildren)
                anchor.isOpen = true
            } else if let anchor, let parent = anchor.parent {
                parent.insertChild(item, at: anchor.index + 1)
            } else {
                root.insertChild(item, at: root.numberOfChildren)
            }
        }
        if sidebar.mode != .outline { showOutlineAction(nil) }
        DispatchQueue.main.async { [weak self] in self?.sidebar.beginRename(item) }
    }

    @objc func renameBookmarkAction(_ sender: Any?) {
        guard let item = sidebar.selectedBookmark else { return }
        sidebar.beginRename(item)
    }

    func renameBookmark(_ item: PDFOutline, to text: String) {
        editOutline(L("Rename Bookmark"), select: { item }) { _ in item.label = text }
    }

    @objc func deleteBookmarkAction(_ sender: Any?) {
        guard let item = sidebar.selectedBookmark, let parent = item.parent else { return }
        let next = item.index + 1 < parent.numberOfChildren ? parent.child(at: item.index + 1) : item.index > 0 ? parent.child(at: item.index - 1) : parent.parent == nil ? nil : parent
        editOutline(L("Delete Bookmark"), select: { next }) { _ in item.removeFromParent() }
    }

    @objc func setBookmarkDestinationAction(_ sender: Any?) {
        guard let item = sidebar.selectedBookmark, let dest = currentViewDestination else { return }
        editOutline(L("Set Destination"), select: { item }) { _ in
            item.action = nil
            item.destination = dest
        }
    }

    /// Moves `item` into `parent` at `index` (drag and drop in the sidebar).
    func moveBookmark(_ item: PDFOutline, to parent: PDFOutline, at index: Int) {
        var ancestor: PDFOutline? = parent
        while let node = ancestor { if node === item { return }; ancestor = node.parent }
        var target = index
        if item.parent === parent, item.index < index { target -= 1 }
        guard !(item.parent === parent && item.index == target) else { return }
        editOutline(L("Move Bookmark"), select: { item }) { _ in
            item.removeFromParent()
            parent.insertChild(item, at: max(0, min(target, parent.numberOfChildren)))
            parent.isOpen = true
        }
    }

    /// Makes the bookmark a child of the one above it.
    @objc func indentBookmarkAction(_ sender: Any?) {
        guard let item = sidebar.selectedBookmark, item.index > 0, let parent = item.parent, let above = parent.child(at: item.index - 1) else { return }
        editOutline(L("Indent Bookmark"), select: { item }) { _ in
            item.removeFromParent()
            above.insertChild(item, at: above.numberOfChildren)
            above.isOpen = true
        }
    }

    /// Moves the bookmark up one level, after its parent.
    @objc func outdentBookmarkAction(_ sender: Any?) {
        guard let item = sidebar.selectedBookmark, let parent = item.parent, let grand = parent.parent else { return }
        editOutline(L("Outdent Bookmark"), select: { item }) { _ in
            let at = parent.index + 1
            item.removeFromParent()
            grand.insertChild(item, at: min(at, grand.numberOfChildren))
        }
    }

    static let bookmarkColors: [(String, NSColor?)] = [
        (L("Default Colour"), nil), (L("Red"), .systemRed), (L("Blue"), .systemBlue), (L("Green"), .systemGreen), (L("Orange"), .systemOrange),
    ]

    /// Tag 1 bold, 2 italic, 10 + n colour n of `bookmarkColors`.
    @objc func bookmarkStyleAction(_ sender: Any?) {
        guard let item = sidebar.selectedBookmark, let doc = pdfDoc else { return }
        let tag = commandTag(of: sender)
        var style = doc.extras.outlineStyles[ObjectIdentifier(item)] ?? OutlineStyle()
        switch tag {
        case 1: style.bold.toggle()
        case 2: style.italic.toggle()
        default:
            let i = tag - 10
            guard i >= 0, i < Self.bookmarkColors.count else { return }
            style.color = Self.bookmarkColors[i].1.map { $0.usingColorSpace(.sRGB) ?? $0 }
        }
        editOutline(L("Bookmark Style"), select: { item }) { _ in
            doc.extras.outlineStyles[ObjectIdentifier(item)] = style.isPlain ? nil : style
        }
    }

    // MARK: Headings

    @objc func bookmarksFromHeadingsAction(_ sender: Any?) {
        guard let pdf else { return }
        let replace = NSButton(checkboxWithTitle: L("Replace the existing bookmarks"), target: nil, action: nil)
        replace.state = (pdf.outlineRoot?.numberOfChildren ?? 0) > 0 ? .off : .on
        replace.isEnabled = (pdf.outlineRoot?.numberOfChildren ?? 0) > 0
        let ocr = NSButton(checkboxWithTitle: L("Recognize text on pages without text (OCR)"), target: nil, action: nil)
        let levels = NSPopUpButton(frame: .zero, pullsDown: false)
        levels.addItems(withTitles: ["1", "2", "3"])
        levels.selectItem(at: 1)
        let alert = NSAlert.form(L("Create Bookmarks from Headings"),
                                 info: L("Lines in a larger font than the body text become bookmarks; the largest sizes are the top level."),
                                 ok: L("Create"), rows: [(L("Levels:"), levels), ("", ocr), ("", replace)])
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        let found = HeadingDetector.headings(pdf, maxLevels: levels.indexOfSelectedItem + 1, ocr: ocr.state == .on)
        guard !found.isEmpty else {
            let a = NSAlert()
            a.messageText = L("No headings found")
            a.informativeText = L("No lines in a larger font than the body text were found.")
            a.runModal()
            return
        }
        editOutline(L("Create Bookmarks from Headings")) { root in
            if replace.state == .on { while root.numberOfChildren > 0 { root.child(at: 0)?.removeFromParent() } }
            HeadingDetector.insert(found, into: root)
        }
        showOutlineAction(nil)
        banner.show(.success, L("Bookmarks created: %d", found.count))
        DispatchQueue.main.asyncAfter(deadline: .now() + 4) { [weak self] in self?.updateSignatureBanner() }
    }
}

/// Finds headings by font size: lines set larger than the body text.
enum HeadingDetector {
    struct Heading {
        var title: String
        var level: Int
        var destination: PDFDestination
    }

    struct Line {
        var text: String
        var size: CGFloat
        var page: PDFPage
        var top: CGPoint
        var bold: Bool
    }

    static func lines(_ page: PDFPage) -> [Line] {
        guard let attributed = page.attributedString, let string = page.string, attributed.length == (string as NSString).length else { return [] }
        var out: [Line] = []
        let ns = string as NSString
        ns.enumerateSubstrings(in: NSRange(location: 0, length: ns.length), options: [.byLines]) { sub, range, _, _ in
            guard let sub, range.length > 0 else { return }
            let text = sub.trimmingCharacters(in: .whitespaces)
            guard text.count >= 2, text.rangeOfCharacter(from: .letters) != nil else { return }
            var size: CGFloat = 0
            var bold = false
            attributed.enumerateAttribute(.font, in: range) { value, r, _ in
                guard let f = value as? NSFont, r.length * 2 >= range.length || size == 0 else { return }
                size = max(size, f.pointSize)
                bold = bold || f.fontDescriptor.symbolicTraits.contains(.bold) || f.fontName.lowercased().contains("bold")
            }
            let bounds = page.selection(for: range)?.bounds(for: page) ?? .zero
            if size <= 1 { size = bounds.height * 0.8 }   // OCR text and fonts without a size: the line height
            out.append(Line(text: text, size: (size * 2).rounded() / 2, page: page, top: CGPoint(x: bounds.minX, y: bounds.maxY + 2), bold: bold))
        }
        return out
    }

    static func headings(_ pdf: PDFDocument, maxLevels: Int, ocr: Bool) -> [Heading] {
        var all: [Line] = []
        for i in 0..<pdf.pageCount {
            guard let page = pdf.page(at: i) else { continue }
            var found = lines(page)
            if found.isEmpty, ocr, let recognized = EditorWindowController.ocrPage(page) {
                // Positions from the recognised copy, destinations on the real page.
                found = lines(recognized).map { var l = $0; l.page = page; return l }
            }
            all += found
        }
        guard !all.isEmpty else { return [] }
        // Body size: the size covering the most characters.
        var weight: [CGFloat: Int] = [:]
        for l in all { weight[l.size, default: 0] += l.text.count }
        guard let body = weight.max(by: { $0.value < $1.value })?.key else { return [] }
        let candidates = all.filter { $0.size >= body * 1.15 && $0.text.count <= 120 }
        let sizes = Array(Set(candidates.map(\.size))).sorted(by: >)
        // Rare huge sizes (a title page) still count as level 1; only `maxLevels` distinct sizes are used.
        let used = Array(sizes.prefix(maxLevels))
        return candidates.compactMap { l in
            guard let level = used.firstIndex(of: l.size) else { return nil }
            return Heading(title: l.text, level: level, destination: PDFDestination(page: l.page, at: l.top))
        }
    }

    /// Adds headings as a tree: each one under the last heading of a higher level.
    static func insert(_ headings: [Heading], into root: PDFOutline) {
        var stack: [(level: Int, item: PDFOutline)] = []
        for h in headings {
            let item = PDFOutline()
            item.label = h.title
            item.destination = h.destination
            while let last = stack.last, last.level >= h.level { stack.removeLast() }
            let parent = stack.last?.item ?? root
            parent.insertChild(item, at: parent.numberOfChildren)
            parent.isOpen = true
            stack.append((h.level, item))
        }
    }
}
