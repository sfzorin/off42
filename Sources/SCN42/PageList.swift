import AppKit
import Kit42
import UniformTypeIdentifiers

/// Thumbnails of the document's pages in the sidebar: select, drag to reorder, drop images/PDFs to import,
/// right-click for page commands.
final class PageListController: NSViewController, NSTableViewDataSource, NSTableViewDelegate {
    static let dragType = NSPasteboard.PasteboardType("com.sfzorin.scn42.page")
    let table = NSTableView()
    let header = NSTextField(labelWithString: "")
    var pages: [ScanPage] = [] {
        didSet { reload(preserving: table.selectedRowIndexes.compactMap { oldValue.indices.contains($0) ? oldValue[$0].id : nil }) }
    }
    /// Index after which the next scan is inserted (marked in the list), nil = at the end.
    var insertionIndex: Int? { didSet { if oldValue != insertionIndex { reload() } } }
    var onSelect: (([Int]) -> Void)?
    var onMove: ((IndexSet, Int) -> Void)?
    var onDropFiles: (([URL], Int) -> Void)?

    override func loadView() {
        let scroll = NSScrollView()
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = false
        scroll.borderType = .noBorder
        table.headerView = nil
        table.style = .plain
        table.selectionHighlightStyle = .regular
        table.rowHeight = 176
        table.intercellSpacing = NSSize(width: 0, height: 6)
        table.allowsMultipleSelection = true
        table.allowsEmptySelection = true
        table.backgroundColor = .clear
        let column = NSTableColumn(identifier: .init("page"))
        table.addTableColumn(column)
        table.dataSource = self
        table.delegate = self
        table.registerForDraggedTypes([Self.dragType, .fileURL])
        table.setDraggingSourceOperationMask(.move, forLocal: true)
        table.menu = contextMenu()
        scroll.documentView = table

        header.font = .systemFont(ofSize: 11, weight: .semibold)
        header.textColor = .secondaryLabelColor
        let root = NSView()
        for v in [header, scroll] as [NSView] {
            v.translatesAutoresizingMaskIntoConstraints = false
            root.addSubview(v)
        }
        NSLayoutConstraint.activate([
            header.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 14),
            header.trailingAnchor.constraint(lessThanOrEqualTo: root.trailingAnchor, constant: -10),
            header.topAnchor.constraint(equalTo: root.safeAreaLayoutGuide.topAnchor, constant: 8),
            scroll.leadingAnchor.constraint(equalTo: root.leadingAnchor), scroll.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            scroll.topAnchor.constraint(equalTo: header.bottomAnchor, constant: 4), scroll.bottomAnchor.constraint(equalTo: root.bottomAnchor),
            root.widthAnchor.constraint(greaterThanOrEqualToConstant: 150),
        ])
        view = root
        reload()
    }

    private func contextMenu() -> NSMenu {
        commandMenu([Cmd.rotateLeft, Cmd.rotateRight, Cmd.enhance, Cmd.copyText, Cmd.rescan, Cmd.insertHere, Cmd.split, Cmd.moveUp, Cmd.moveDown, Cmd.deletePage])
    }

    func reload(preserving selectedIDs: [UUID]? = nil) {
        let ids = Set(selectedIDs ?? table.selectedRowIndexes.compactMap { pages.indices.contains($0) ? pages[$0].id : nil })
        table.reloadData()
        let valid = IndexSet(pages.indices.filter { ids.contains(pages[$0].id) })
        table.selectRowIndexes(valid, byExtendingSelection: false)
        header.stringValue = pages.isEmpty ? L("No pages yet") : pagesText(pages.count)
    }

    var selection: [Int] { table.selectedRowIndexes.map { $0 } }

    func select(_ indices: [Int], scroll: Bool = true) {
        let valid = indices.filter { pages.indices.contains($0) }
        table.selectRowIndexes(IndexSet(valid), byExtendingSelection: false)
        if scroll, let last = valid.last { table.scrollRowToVisible(last) }
    }

    func numberOfRows(in tableView: NSTableView) -> Int { pages.count }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        let cell = (tableView.makeView(withIdentifier: PageCell.id, owner: nil) as? PageCell) ?? PageCell()
        let page = pages[row]
        cell.configure(page, number: row + 1, insertAfter: insertionIndex == row)
        return cell
    }

    func tableViewSelectionDidChange(_ notification: Notification) { onSelect?(selection) }

    func tableView(_ tableView: NSTableView, rowViewForRow row: Int) -> NSTableRowView? { PageRowView() }

    // Drag and drop

    func tableView(_ tableView: NSTableView, pasteboardWriterForRow row: Int) -> NSPasteboardWriting? {
        guard pages.indices.contains(row) else { return nil }
        let item = NSPasteboardItem()
        item.setString(pages[row].id.uuidString, forType: Self.dragType)
        return item
    }

    func tableView(_ tableView: NSTableView, validateDrop info: NSDraggingInfo, proposedRow row: Int, proposedDropOperation op: NSTableView.DropOperation) -> NSDragOperation {
        tableView.setDropRow(row, dropOperation: .above)
        if info.draggingSource as? NSTableView === tableView { return .move }
        return info.draggingPasteboard.canReadObject(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) ? .copy : []
    }

    func tableView(_ tableView: NSTableView, acceptDrop info: NSDraggingInfo, row: Int, dropOperation: NSTableView.DropOperation) -> Bool {
        if info.draggingSource as? NSTableView === tableView {
            var rows = IndexSet()
            info.enumerateDraggingItems(options: [], for: tableView, classes: [NSPasteboardItem.self], searchOptions: [:]) { item, _, _ in
                if let s = (item.item as? NSPasteboardItem)?.string(forType: Self.dragType), let id = UUID(uuidString: s),
                   let r = self.pages.firstIndex(where: { $0.id == id }) { rows.insert(r) }
            }
            guard !rows.isEmpty, let onMove else { return false }
            let movedIDs = Set(rows.map { pages[$0].id })
            onMove(rows, row)
            select(pages.indices.filter { movedIDs.contains(pages[$0].id) })
            return true
        }
        let urls = info.draggingPasteboard.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL] ?? []
        guard !urls.isEmpty, let onDropFiles else { return false }
        onDropFiles(urls, row)
        return true
    }
}

/// Thumbnail with a soft frame, the page number and small badges (blank, text recognised, next scan goes here).
final class PageCell: NSTableCellView {
    static let id = NSUserInterfaceItemIdentifier("PageCell")
    let thumb = NSImageView()
    let number = NSTextField(labelWithString: "")
    let badges = NSStackView()

    init() {
        super.init(frame: .zero)
        identifier = Self.id
        thumb.imageScaling = .scaleProportionallyUpOrDown
        thumb.wantsLayer = true
        thumb.shadow = {
            let s = NSShadow()
            s.shadowColor = NSColor.black.withAlphaComponent(0.3)
            s.shadowBlurRadius = 3
            s.shadowOffset = NSSize(width: 0, height: -1)
            return s
        }()
        number.font = .monospacedDigitSystemFont(ofSize: 11, weight: .medium)
        number.textColor = .secondaryLabelColor
        badges.spacing = 4
        let bottom = NSStackView(views: [number, badges])
        bottom.spacing = 6
        for v in [thumb, bottom] as [NSView] {
            v.translatesAutoresizingMaskIntoConstraints = false
            addSubview(v)
        }
        NSLayoutConstraint.activate([
            thumb.topAnchor.constraint(equalTo: topAnchor, constant: 6),
            thumb.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 12),
            thumb.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -12),
            thumb.bottomAnchor.constraint(equalTo: bottom.topAnchor, constant: -4),
            bottom.centerXAnchor.constraint(equalTo: centerXAnchor),
            bottom.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -2),
            bottom.heightAnchor.constraint(equalToConstant: 16),
        ])
    }

    required init?(coder: NSCoder) { fatalError("not supported") }

    func configure(_ page: ScanPage, number n: Int, insertAfter: Bool) {
        thumb.image = page.thumbnail
        number.stringValue = "\(n)"
        badges.arrangedSubviews.forEach { $0.removeFromSuperview() }
        func badge(_ symbol: String, _ tip: String, _ color: NSColor) {
            let v = NSImageView(image: Theme42.symbol(symbol, size: 10, weight: .semibold) ?? NSImage())
            v.contentTintColor = color
            v.toolTip = tip
            badges.addArrangedSubview(v)
        }
        if page.report.blank { badge("exclamationmark.triangle.fill", L("Looks blank"), .systemOrange) }
        if page.text != nil { badge("text.viewfinder", L("Text recognised"), .secondaryLabelColor) }
        if insertAfter { badge("arrow.down.to.line", L("The next scan is inserted after this page"), .controlAccentColor) }
        toolTip = String(format: "%d × %d px, %.0f dpi", page.image.width, page.image.height, page.dpi)
    }
}

/// Rounded accent selection drawn directly (no material), the same on screen and in snapshots.
final class PageRowView: NSTableRowView {
    override func drawSelection(in dirtyRect: NSRect) {
        let r = bounds.insetBy(dx: 8, dy: 1)
        (isEmphasized ? NSColor.controlAccentColor.withAlphaComponent(0.28) : NSColor.secondaryLabelColor.withAlphaComponent(0.18)).setFill()
        NSBezierPath(roundedRect: r, xRadius: 8, yRadius: 8).fill()
    }
}
