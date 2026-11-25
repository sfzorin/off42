import AppKit
import Kit42
import PDFKit

/// Page › Page Labels…: ranges of logical page numbers (i, ii, iii… 1, 2, 3, "A-1"…).
final class PageLabelsEditor: NSObject {
    private var ranges: [PageLabelRange]
    private let pageCount: Int
    private let rows = NSStackView()
    private let preview = NSTextField(wrappingLabelWithString: "")
    private var controls: [(start: NSTextField, style: NSPopUpButton, prefix: NSTextField, first: NSTextField)] = []

    init(ranges: [PageLabelRange], pageCount: Int) {
        self.ranges = ranges.isEmpty ? [PageLabelRange(start: 0)] : ranges
        self.pageCount = pageCount
    }

    /// The edited ranges, nil on Cancel; an empty array removes the labels.
    func run() -> [PageLabelRange]? {
        let alert = NSAlert()
        alert.messageText = L("Page Labels")
        alert.informativeText = L("Each range starts at a page and numbers the pages from there on, until the next range. Labels are shown in the thumbnails and the title bar and used by Go to Page.")
        alert.addButton(withTitle: L("Apply"))
        alert.addButton(withTitle: L("Cancel"))
        alert.addButton(withTitle: L("Remove Labels"))
        alert.accessoryView = makeView()
        alert.layout()
        switch alert.runModal() {
        case .alertFirstButtonReturn: return read()
        case .alertThirdButtonReturn: return []
        default: return nil
        }
    }

    private func makeView() -> NSView {
        rows.orientation = .vertical
        rows.alignment = .leading
        rows.spacing = 6
        let header = NSStackView(views: [L("From page"), L("Style"), L("Prefix"), L("Start at")].enumerated().map { i, t in
            let l = NSTextField(labelWithString: t)
            l.font = .systemFont(ofSize: 11)
            l.textColor = .secondaryLabelColor
            l.translatesAutoresizingMaskIntoConstraints = false
            l.widthAnchor.constraint(equalToConstant: [70, 130, 80, 60][i]).isActive = true
            return l
        })
        rows.addArrangedSubview(header)
        for r in ranges { addRow(r) }
        let add = NSButton(title: L("Add Range"), target: self, action: #selector(addRange))
        add.controlSize = .small
        let remove = NSButton(title: L("Remove Last"), target: self, action: #selector(removeRange))
        remove.controlSize = .small
        preview.textColor = .secondaryLabelColor
        preview.font = .systemFont(ofSize: 11)
        preview.preferredMaxLayoutWidth = 420
        let stack = NSStackView(views: [rows, NSStackView(views: [add, remove]), preview])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 10
        stack.frame.size = NSSize(width: 440, height: 220)
        update()
        let container = NSView(frame: NSRect(x: 0, y: 0, width: 440, height: 230))
        stack.translatesAutoresizingMaskIntoConstraints = false
        container.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.topAnchor.constraint(equalTo: container.topAnchor),
            stack.leadingAnchor.constraint(equalTo: container.leadingAnchor),
            stack.trailingAnchor.constraint(equalTo: container.trailingAnchor),
        ])
        return container
    }

    private func addRow(_ r: PageLabelRange) {
        func field(_ s: String, _ w: CGFloat) -> NSTextField {
            let f = NSTextField(string: s)
            f.translatesAutoresizingMaskIntoConstraints = false
            f.widthAnchor.constraint(equalToConstant: w).isActive = true
            f.target = self
            f.action = #selector(update)
            return f
        }
        let start = field("\(r.start + 1)", 70)
        let style = NSPopUpButton(frame: .zero, pullsDown: false)
        style.addItems(withTitles: PageLabelRange.Style.allCases.map(\.title))
        style.selectItem(at: PageLabelRange.Style.allCases.firstIndex(of: r.style) ?? 0)
        style.target = self
        style.action = #selector(update)
        style.translatesAutoresizingMaskIntoConstraints = false
        style.widthAnchor.constraint(equalToConstant: 130).isActive = true
        let prefix = field(r.prefix, 80)
        let first = field("\(r.first)", 60)
        controls.append((start, style, prefix, first))
        rows.addArrangedSubview(NSStackView(views: [start, style, prefix, first]))
    }

    @objc private func addRange() {
        let last = (read() ?? []).last?.start ?? 0
        addRow(PageLabelRange(start: min(pageCount - 1, last + 1)))
        update()
    }

    @objc private func removeRange() {
        guard controls.count > 1, let row = rows.arrangedSubviews.last else { return }
        controls.removeLast()
        rows.removeArrangedSubview(row)
        row.removeFromSuperview()
        update()
    }

    private func read() -> [PageLabelRange]? {
        var out: [PageLabelRange] = []
        for c in controls {
            guard let start = Int(c.start.stringValue), start >= 1, start <= pageCount else { return nil }
            guard let first = Int(c.first.stringValue), first >= 1, first <= Int.max - max(0, pageCount - start),
                  PageLabelRange.Style.allCases.indices.contains(c.style.indexOfSelectedItem) else { return nil }
            let style = PageLabelRange.Style.allCases[c.style.indexOfSelectedItem]
            out.append(PageLabelRange(start: start - 1, style: style, prefix: c.prefix.stringValue, first: first))
        }
        var seen = Set<Int>()
        return out.sorted { $0.start < $1.start }.filter { seen.insert($0.start).inserted }
    }

    @objc private func update() {
        guard let r = read() else {
            preview.stringValue = L("Use page numbers from 1 to %d.", pageCount)
            return
        }
        let shown = (0..<min(pageCount, 12)).map { PageLabels.label($0, r) ?? "\($0 + 1)" }
        preview.stringValue = L("Preview: %@", shown.joined(separator: ", ") + (pageCount > 12 ? "…" : ""))
    }
}

extension EditorWindowController {
    @objc func pageLabelsAction(_ sender: Any?) {
        guard let pdf, let doc = pdfDoc else { return }
        guard let ranges = PageLabelsEditor(ranges: doc.extras.labels, pageCount: pdf.pageCount).run() else { return }
        setExtras({ $0.labels = ranges }, part: .labels, name: L("Page Labels"))
    }
}
