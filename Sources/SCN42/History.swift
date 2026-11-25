import AppKit
import Kit42
import PDFKit

/// Recently saved scans (newest first) with small thumbnails, in Application Support/SCN42
/// (or the test folder from `K42_SCAN_DIR`).
final class History {
    struct Entry: Codable {
        var path: String
        var date: Date
        var pages: Int
        var size: Int
        var thumb: String?
        var url: URL { URL(fileURLWithPath: path) }
    }

    static let shared = History()
    static let changed = Notification.Name("SCN42.historyChanged")
    private(set) var entries: [Entry] = []
    var onChange: (() -> Void)?

    private var folder: URL {
        if let t = Store.testDirectory { return t.appendingPathComponent(".history", isDirectory: true) }
        return FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("SCN42", isDirectory: true)
    }
    private var file: URL { folder.appendingPathComponent("history.json") }

    private func thumbnailURL(_ name: String) -> URL? {
        guard name == (name as NSString).lastPathComponent, (name as NSString).pathExtension == "jpg",
              UUID(uuidString: (name as NSString).deletingPathExtension) != nil else { return nil }
        return folder.appendingPathComponent(name)
    }

    private func removeThumbnails(_ old: [Entry]) {
        let retained = Set(entries.compactMap(\.thumb))
        for entry in old {
            if let name = entry.thumb, !retained.contains(name), let url = thumbnailURL(name) { try? FileManager.default.removeItem(at: url) }
        }
    }

    init() {
        if let data = try? Data(contentsOf: file), let e = try? JSONDecoder().decode([Entry].self, from: data) { entries = e }
    }

    func add(_ url: URL, pages: Int, thumbnail: CGImage?) {
        try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        var thumbName: String?
        if let thumbnail {
            let name = UUID().uuidString + ".jpg"
            let s = 160.0 / Double(max(thumbnail.width, thumbnail.height))
            let small = Bitmap(thumbnail, gray: false).resized(width: max(1, Int(Double(thumbnail.width) * s)), height: max(1, Int(Double(thumbnail.height) * s)))
            if let img = small.cgImage(), let data = Exporter.jpegData(img, quality: 0.8, dpi: 72) {
                try? data.write(to: folder.appendingPathComponent(name))
                thumbName = name
            }
        }
        let size = (try? url.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0
        let previous = entries
        entries.removeAll { $0.path == url.path }
        entries.insert(Entry(path: url.path, date: Date(), pages: pages, size: size, thumb: thumbName), at: 0)
        if entries.count > 200 { entries.removeLast(entries.count - 200) }
        removeThumbnails(previous)
        persist()
    }

    func remove(at i: Int) {
        guard entries.indices.contains(i) else { return }
        if let t = entries[i].thumb, let url = thumbnailURL(t) { try? FileManager.default.removeItem(at: url) }
        entries.remove(at: i)
        persist()
    }

    func clear() {
        for e in entries { if let t = e.thumb, let url = thumbnailURL(t) { try? FileManager.default.removeItem(at: url) } }
        entries = []
        persist()
    }

    func thumbnail(_ e: Entry) -> NSImage? {
        if let t = e.thumb, let url = thumbnailURL(t), let img = NSImage(contentsOf: url) { return img }
        return FileManager.default.fileExists(atPath: e.path) ? NSWorkspace.shared.icon(forFile: e.path) : nil
    }

    private func persist() {
        try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        if let data = try? JSONEncoder().encode(entries) { try? data.write(to: file, options: .atomic) }
        onChange?()
        NotificationCenter.default.post(name: Self.changed, object: self)
    }
}

/// The History inspector: recent scans; double-click opens, right-click reveals or removes.
final class HistoryPanelController: NSViewController, NSTableViewDataSource, NSTableViewDelegate {
    let table = NSTableView()
    let emptyLabel = NSTextField(wrappingLabelWithString: L("Saved scans appear here."))

    override func loadView() {
        let scroll = NSScrollView()
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = false
        scroll.borderType = .noBorder
        table.headerView = nil
        table.style = .inset
        table.rowHeight = 58
        table.backgroundColor = .clear
        table.addTableColumn(NSTableColumn(identifier: .init("entry")))
        table.dataSource = self
        table.delegate = self
        table.target = self
        table.doubleAction = #selector(openSelected)
        let menu = NSMenu()
        menu.addItem(withTitle: L("Open"), action: #selector(openSelected), keyEquivalent: "").target = self
        menu.addItem(withTitle: L("Show in Finder"), action: #selector(revealSelected), keyEquivalent: "").target = self
        menu.addItem(.separator())
        menu.addItem(withTitle: L("Remove from History"), action: #selector(removeSelected), keyEquivalent: "").target = self
        table.menu = menu
        scroll.documentView = table

        let clear = NSButton(title: L("Clear History"), target: self, action: #selector(clearAll))
        clear.controlSize = .small
        clear.bezelStyle = .push
        emptyLabel.textColor = .tertiaryLabelColor
        emptyLabel.alignment = .center
        let root = NSView()
        for v in [scroll, clear, emptyLabel] as [NSView] {
            v.translatesAutoresizingMaskIntoConstraints = false
            root.addSubview(v)
        }
        NSLayoutConstraint.activate([
            scroll.leadingAnchor.constraint(equalTo: root.leadingAnchor), scroll.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            scroll.topAnchor.constraint(equalTo: root.topAnchor, constant: 6),
            scroll.bottomAnchor.constraint(equalTo: clear.topAnchor, constant: -8),
            clear.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -16),
            clear.bottomAnchor.constraint(equalTo: root.bottomAnchor, constant: -12),
            emptyLabel.centerXAnchor.constraint(equalTo: root.centerXAnchor),
            emptyLabel.centerYAnchor.constraint(equalTo: root.centerYAnchor),
            emptyLabel.widthAnchor.constraint(lessThanOrEqualTo: root.widthAnchor, constant: -40),
        ])
        view = root
        NotificationCenter.default.addObserver(self, selector: #selector(historyChanged), name: History.changed, object: History.shared)
        reload()
    }

    @objc private func historyChanged(_ notification: Notification) { reload() }

    func reload() {
        table.reloadData()
        emptyLabel.isHidden = !History.shared.entries.isEmpty
    }

    func numberOfRows(in tableView: NSTableView) -> Int { History.shared.entries.count }

    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        let e = History.shared.entries[row]
        let cell = (tableView.makeView(withIdentifier: .init("HistoryCell"), owner: nil) as? HistoryCell) ?? HistoryCell()
        cell.configure(e, image: History.shared.thumbnail(e))
        return cell
    }

    private var target: Int? {
        let r = table.clickedRow >= 0 ? table.clickedRow : table.selectedRow
        return History.shared.entries.indices.contains(r) ? r : nil
    }

    @objc func openSelected() {
        guard let i = target else { return }
        let url = History.shared.entries[i].url
        if FileManager.default.fileExists(atPath: url.path) { NSWorkspace.shared.open(url) } else { NSSound.beep() }
    }

    @objc func revealSelected() {
        guard let i = target else { return }
        NSWorkspace.shared.activateFileViewerSelecting([History.shared.entries[i].url])
    }

    @objc func removeSelected() {
        guard let i = target else { return }
        History.shared.remove(at: i)
    }

    @objc func clearAll() { History.shared.clear() }
}

final class HistoryCell: NSTableCellView {
    let thumb = NSImageView()
    let name = NSTextField(labelWithString: "")
    let detail = NSTextField(labelWithString: "")

    init() {
        super.init(frame: .zero)
        identifier = .init("HistoryCell")
        thumb.imageScaling = .scaleProportionallyUpOrDown
        name.font = .systemFont(ofSize: 12.5, weight: .medium)
        name.lineBreakMode = .byTruncatingMiddle
        detail.font = .systemFont(ofSize: 11)
        detail.textColor = .secondaryLabelColor
        detail.lineBreakMode = .byTruncatingTail
        let text = NSStackView(views: [name, detail])
        text.orientation = .vertical
        text.alignment = .leading
        text.spacing = 2
        for v in [thumb, text] as [NSView] {
            v.translatesAutoresizingMaskIntoConstraints = false
            addSubview(v)
        }
        NSLayoutConstraint.activate([
            thumb.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 4),
            thumb.centerYAnchor.constraint(equalTo: centerYAnchor),
            thumb.widthAnchor.constraint(equalToConstant: 40), thumb.heightAnchor.constraint(equalToConstant: 48),
            text.leadingAnchor.constraint(equalTo: thumb.trailingAnchor, constant: 8),
            text.trailingAnchor.constraint(lessThanOrEqualTo: trailingAnchor, constant: -4),
            text.centerYAnchor.constraint(equalTo: centerYAnchor),
        ])
    }

    required init?(coder: NSCoder) { fatalError("not supported") }

    func configure(_ e: History.Entry, image: NSImage?) {
        thumb.image = image
        name.stringValue = e.url.lastPathComponent
        let date = DateFormatter.localizedString(from: e.date, dateStyle: .short, timeStyle: .short)
        detail.stringValue = "\(date) · " + L("%d pages", e.pages) + " · " + Exporter.sizeText(e.size)
        name.textColor = FileManager.default.fileExists(atPath: e.path) ? .labelColor : .tertiaryLabelColor
        toolTip = e.path
    }
}
