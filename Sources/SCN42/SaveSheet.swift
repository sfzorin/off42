import AppKit
import Kit42

func pagesText(_ n: Int) -> String { n == 1 ? L("1 page") : L("%d pages", n) }

/// Save options as a sheet: format, file size (with the expected size), searchable text, name template
/// with a live preview, folder, and what to open afterwards.
final class SaveSheetController: NSWindowController, NSTextFieldDelegate {
    private(set) var options: SaveOptions
    private let pages: [ExportPage]
    private let title0: String?
    var onSave: ((SaveOptions) -> Void)?

    let format = NSPopUpButton(frame: .zero, pullsDown: false)
    let size = NSPopUpButton(frame: .zero, pullsDown: false)
    let quality = NSSlider(value: 0.85, minValue: 0.3, maxValue: 1, target: nil, action: nil)
    let estimate = NSTextField(labelWithString: "")
    let ocr = NSButton(checkboxWithTitle: L("Searchable text (OCR)"), target: nil, action: nil)
    let name = NSTextField()
    let tokens = NSPopUpButton(frame: .zero, pullsDown: true)
    let preview = NSTextField(labelWithString: "")
    let folder = NSPathControl()
    let choose = NSButton(title: L("Choose…"), target: nil, action: nil)
    let openAfter = NSPopUpButton(frame: .zero, pullsDown: false)
    private var grid: NSGridView!
    private var estimateWork: DispatchWorkItem?
    private var estimateGeneration = UUID()

    init(pages: [ExportPage], title: String?, options: SaveOptions) {
        self.pages = pages
        self.options = options
        self.title0 = title
        let w = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 520, height: 360), styleMask: [.titled], backing: .buffered, defer: false)
        super.init(window: w)
        build()
        load()
    }

    required init?(coder: NSCoder) { fatalError("not supported") }

    private func build() {
        guard let w = window else { return }
        let heading = NSTextField(labelWithString: L("Save Scan"))
        heading.font = .systemFont(ofSize: 15, weight: .semibold)
        let sub = NSTextField(labelWithString: pagesText(pages.count))
        sub.textColor = .secondaryLabelColor

        for f in SaveFormat.allCases { format.addItem(withTitle: f.title) }
        for c in Compression.allCases { size.addItem(withTitle: c.title) }
        for o in OpenAfter.allCases { openAfter.addItem(withTitle: o.title) }
        for p in [format, size, openAfter] {
            p.target = self
            p.action = #selector(changed)
        }
        quality.target = self
        quality.action = #selector(changed)
        quality.isContinuous = false
        ocr.target = self
        ocr.action = #selector(changed)
        estimate.textColor = .secondaryLabelColor
        estimate.font = .monospacedDigitSystemFont(ofSize: 11, weight: .regular)
        name.delegate = self
        name.placeholderString = SaveOptions.defaultTemplate
        tokens.addItem(withTitle: L("Insert"))
        let items: [(String, String)] = [(L("Date (YYYY-MM-DD)"), "YYYY-MM-DD"), (L("Time (HH-mm)"), "HH-mm"),
                                         (L("Counter (001)"), "{n}"), (L("First line of text"), "{title}")]
        for (t, token) in items {
            tokens.addItem(withTitle: t)
            tokens.lastItem?.representedObject = token
        }
        tokens.target = self
        tokens.action = #selector(insertToken)
        tokens.controlSize = .small
        (tokens.cell as? NSPopUpButtonCell)?.arrowPosition = .arrowAtBottom
        preview.textColor = .secondaryLabelColor
        preview.font = .systemFont(ofSize: 11)
        preview.lineBreakMode = .byTruncatingMiddle
        folder.pathStyle = .popUp
        folder.isEditable = false
        choose.target = self
        choose.action = #selector(chooseFolder)
        choose.controlSize = .small

        let sizeRow = NSStackView(views: [size, estimate])
        sizeRow.spacing = 8
        let nameRow = NSStackView(views: [name, tokens])
        nameRow.spacing = 6
        let folderRow = NSStackView(views: [folder, choose])
        folderRow.spacing = 6
        let label: (String) -> NSTextField = { s in
            let t = NSTextField(labelWithString: s)
            t.alignment = .right
            return t
        }
        grid = NSGridView(views: [
            [label(L("Format:")), format],
            [label(L("File size:")), sizeRow],
            [label(L("Quality:")), quality],
            [NSGridCell.emptyContentView, ocr],
            [label(L("Name:")), nameRow],
            [NSGridCell.emptyContentView, preview],
            [label(L("Folder:")), folderRow],
            [label(L("Then open:")), openAfter],
        ])
        grid.rowSpacing = 9
        grid.columnSpacing = 8
        grid.column(at: 0).xPlacement = .trailing
        grid.rowAlignment = .firstBaseline
        for v in [format, openAfter, size] as [NSView] {
            v.translatesAutoresizingMaskIntoConstraints = false
            v.widthAnchor.constraint(equalToConstant: 230).isActive = true
        }
        name.translatesAutoresizingMaskIntoConstraints = false
        name.widthAnchor.constraint(equalToConstant: 230).isActive = true
        folder.translatesAutoresizingMaskIntoConstraints = false
        folder.widthAnchor.constraint(equalToConstant: 230).isActive = true
        quality.translatesAutoresizingMaskIntoConstraints = false
        quality.widthAnchor.constraint(equalToConstant: 230).isActive = true
        preview.translatesAutoresizingMaskIntoConstraints = false
        preview.widthAnchor.constraint(equalToConstant: 320).isActive = true

        let cancel = NSButton(title: L("Cancel"), target: self, action: #selector(cancelAction))
        cancel.keyEquivalent = "\u{1b}"
        let save = NSButton(title: L("Save"), target: self, action: #selector(saveAction))
        save.keyEquivalent = "\r"
        let buttons = NSStackView(views: [NSView(), cancel, save])
        buttons.spacing = 10

        let titleRow = NSStackView(views: [heading, sub])
        titleRow.spacing = 8
        titleRow.alignment = .firstBaseline
        let stack = NSStackView(views: [titleRow, grid, buttons])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 16
        stack.edgeInsets = NSEdgeInsets(top: 20, left: 24, bottom: 20, right: 24)
        buttons.translatesAutoresizingMaskIntoConstraints = false
        stack.translatesAutoresizingMaskIntoConstraints = false
        let content = NSView()
        content.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: content.leadingAnchor), stack.trailingAnchor.constraint(equalTo: content.trailingAnchor),
            stack.topAnchor.constraint(equalTo: content.topAnchor), stack.bottomAnchor.constraint(equalTo: content.bottomAnchor),
            buttons.widthAnchor.constraint(equalTo: stack.widthAnchor, constant: -48),
        ])
        w.contentView = content
        fitWindow()
    }

    private func load() {
        format.selectItem(at: options.format.rawValue)
        size.selectItem(at: options.compression.rawValue)
        quality.doubleValue = options.quality
        ocr.state = options.ocr ? .on : .off
        name.stringValue = options.nameTemplate
        folder.url = options.folderURL
        openAfter.selectItem(at: options.openAfter.rawValue)
        refresh()
    }

    private func read() {
        options.format = SaveFormat(rawValue: format.indexOfSelectedItem) ?? .pdf
        options.compression = Compression(rawValue: size.indexOfSelectedItem) ?? .balanced
        options.quality = quality.doubleValue
        options.ocr = ocr.state == .on
        let visibleName = (name.currentEditor() as? NSTextView)?.string ?? name.stringValue
        options.nameTemplate = visibleName.isEmpty ? SaveOptions.defaultTemplate : visibleName
        options.openAfter = OpenAfter(rawValue: openAfter.indexOfSelectedItem) ?? .none
    }

    /// Rows that only matter for some formats; the name preview; the expected size (computed in the background).
    private func refresh() {
        let f = options.format
        grid.row(at: 1).isHidden = !f.isPDF
        grid.row(at: 2).isHidden = !(f == .jpeg || f == .heic)
        grid.row(at: 3).isHidden = !f.isPDF
        let base = Exporter.expand(options.nameTemplate, counter: options.counter, title: title0)
        let many = !f.isMultiPage && pages.count > 1
        preview.stringValue = many ? L("Files: %@ 001.%@ … %@ %03d.%@", base, f.ext, base, pages.count, f.ext) : L("File: %@.%@", base, f.ext)
        estimate.stringValue = L("estimating…")
        estimateWork?.cancel()
        estimateGeneration = UUID()
        let generation = estimateGeneration
        let o = options, p = pages
        let work = DispatchWorkItem { [weak self] in
            let bytes = Exporter.estimate(p, o)
            DispatchQueue.main.async {
                guard let self, self.estimateGeneration == generation else { return }
                self.estimate.stringValue = "≈ " + Exporter.sizeText(bytes)
            }
        }
        estimateWork = work
        DispatchQueue.global(qos: .userInitiated).asyncAfter(deadline: .now() + 0.15, execute: work)
        fitWindow()
    }

    /// Height from the visible rows; a fixed width with room on both sides.
    private func fitWindow() {
        guard let content = window?.contentView else { return }
        content.layoutSubtreeIfNeeded()
        window?.setContentSize(NSSize(width: max(520, content.fittingSize.width + 24), height: content.fittingSize.height))
    }

    @objc private func changed() {
        read()
        refresh()
    }

    func controlTextDidChange(_ obj: Notification) { changed() }

    @objc private func insertToken() {
        guard let token = tokens.selectedItem?.representedObject as? String else { return }
        let editor = name.currentEditor()
        if let editor { editor.replaceCharacters(in: editor.selectedRange, with: token) } else { name.stringValue += " " + token }
        tokens.selectItem(at: 0)
        changed()
    }

    @objc private func chooseFolder() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = true
        panel.directoryURL = options.folderURL
        panel.prompt = L("Choose")
        guard let w = window else { return }
        panel.beginSheetModal(for: w) { [self] r in
            guard r == .OK, let url = panel.url else { return }
            options.folder = url.path
            folder.url = url
        }
    }

    @objc private func cancelAction() { close(.cancel) }

    @objc private func saveAction() {
        read()
        close(.OK)
        onSave?(options)
    }

    private func close(_ code: NSApplication.ModalResponse) {
        estimateGeneration = UUID(); estimateWork?.cancel()
        guard let w = window, let parent = w.sheetParent else { window?.close(); return }
        parent.endSheet(w, returnCode: code)
    }
}
