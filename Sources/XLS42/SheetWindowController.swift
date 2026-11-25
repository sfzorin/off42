import AppKit
import Kit42

final class SheetWindowController: NSWindowController, NSWindowDelegate, NSMenuItemValidation, NSToolbarItemValidation, NSTextFieldDelegate {
    let grid = GridView(frame: NSRect(x: 0, y: 0, width: 800, height: 600))
    let scrollView = NSScrollView()
    private lazy var columnHeader = HeaderView(grid: grid, columns: true)
    private lazy var rowHeader = HeaderView(grid: grid, columns: false)
    lazy var freezeOverlay = FreezeOverlay(grid: grid)
    private lazy var rowGutter = OutlineGutter(grid: grid, columns: false)
    private lazy var colGutter = OutlineGutter(grid: grid, columns: true)
    private var rowGutterWidth: NSLayoutConstraint?
    private var colGutterHeight: NSLayoutConstraint?
    private let signatureHint = NSTextField(labelWithString: "")
    var functionWizard: FunctionWizard?
    var findPanel: FindReplacePanel?
    /// Tests: refusals and validation errors are recorded instead of shown.
    var suppressAlerts = false
    var lastAlertMessage: String?
    /// Tests: the last hyperlink opened (links aren't opened while alerts are suppressed).
    var lastOpenedLink: String?
    var lastCreatedLabelSource: LabelSource?
    /// Status bar: selection statistics and zoom (View › Status Bar).
    let statusBar = NSVisualEffectView()
    private var statusBarHeight: NSLayoutConstraint?
    let zoomSlider = NSSlider(value: 100, minValue: 30, maxValue: 400, target: nil, action: nil)
    let zoomLabel = NSTextField(labelWithString: "100%")
    var statusBarHidden: Bool { statusBar.isHidden }
    private let toolbarController = ToolbarController(identifier: "XLS42.toolbar", elements: Cmd.toolbar,
                                                                   optional: Cmd.toolbarOptional)
    private let nameBox = NSTextField()
    private let formulaField = NSTextField()
    /// Field editor of the formula bar while it is being edited.
    var formulaFieldEditor: NSTextView? { formulaField.currentEditor() as? NSTextView }
    var formulaBarString: String { formulaField.stringValue }
    private let tabs = NSStackView()
    let status = NSTextField(labelWithString: "")
    private let sheetInfo = NSTextField(labelWithString: "")
    private lazy var fontPopup = stripPopUp(["Calibri", "Arial", "Helvetica Neue", "Times New Roman", "Georgia", "Verdana", "Courier New", "Menlo"],
                                            width: 128, toolTip: L("Font"), target: self, action: #selector(fontChanged(_:)))
    private lazy var sizeStepper: SizeStepper = {
        let s = SizeStepper(toolTip: L("Font size"))
        s.target = self
        s.action = #selector(sizeChanged(_:))
        s.value = 11
        return s
    }()
    private lazy var textColorButton = colorButton("textformat", L("Text colour"), .black, #selector(textColorChanged(_:)))
    private lazy var fillColorButton = colorButton("paintbrush.pointed", L("Fill colour"), nil, #selector(fillColorChanged(_:)))
    private lazy var numberFormatPopup = stripPopUp(Self.numberFormats.map(\.title), width: 116, toolTip: L("Number format"),
                                                    target: self, action: #selector(numberFormatChanged(_:)))
    private lazy var alignButton = StripButton(symbol: "text.alignleft", toolTip: L("Alignment"),
                                               menu: commandMenu([Cmd.alignLeft, Cmd.alignCenter, Cmd.alignRight]))
    private lazy var bordersButton = StripButton(symbol: "squareshape.split.3x3", toolTip: L("Borders"),
                                                 menu: commandMenu([Cmd.bordersAll, Cmd.bordersOutside, Cmd.bordersBottom, Cmd.bordersNone]))
    private lazy var formatStrip = ToolStrip([
        .group([.view(fontPopup), .view(sizeStepper)]),
        .buttons([Cmd.bold, Cmd.italic, Cmd.underline, Cmd.strike]),
        .group([.view(textColorButton), .view(fillColorButton)]),
        .group([.view(alignButton), .command(Cmd.wrap), .command(Cmd.merge)]),
        .group([.view(numberFormatPopup), .command(Cmd.decreaseDecimals), .command(Cmd.increaseDecimals)]),
        .group([.view(bordersButton)]),
    ])

    private func colorButton(_ symbol: String, _ tip: String, _ color: NSColor?, _ action: Selector) -> ColorButton {
        let b = ColorButton(symbol: symbol, toolTip: tip, color: color)
        b.target = self
        b.action = action
        return b
    }
    private let formulaBar = NSTitlebarAccessoryViewController()

    var sheetIndex = 0
    var statusBarHiddenBeforePanels: Bool?
    private var searchText = ""

    var doc: SheetDocument? { document as? SheetDocument }
    var sheet: Sheet {
        guard let wb = doc?.workbook, sheetIndex < wb.sheets.count else { return Sheet(name: "") }
        return wb.sheets[sheetIndex]
    }

    init() {
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1200, height: 820),
                              styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
                              backing: .buffered, defer: false)
        window.minSize = NSSize(width: 640, height: 400)
        window.toolbarStyle = .unified
        super.init(window: window)
        window.delegate = self
        shouldCascadeWindows = true
        setupContent()
        window.toolbar = toolbarController.toolbar
        window.addTitlebarAccessoryViewController(formatStrip)
        window.addTitlebarAccessoryViewController(formulaBar)
        window.center()
    }

    required init?(coder: NSCoder) { fatalError("not supported") }

    override var document: AnyObject? {
        didSet {
            guard document != nil else { return }
            reloadTabs()
            grid.rebuildLayout()
            updateOutlineGutters()
            grid.select(CellRef(row: 0, col: 0))
            // A split saved in the file, once the scroll view has its size.
            DispatchQueue.main.async { [weak self] in self?.restoreSplit() }
        }
    }

    // MARK: - Setup

    private func setupContent() {
        grid.controller = self
        scrollView.documentView = grid
        scrollView.hasVerticalScroller = true
        scrollView.hasHorizontalScroller = true
        scrollView.backgroundColor = NSColor(name: nil) { SheetPalette.current($0).background }
        scrollView.drawsBackground = true
        scrollView.automaticallyAdjustsContentInsets = false
        scrollView.contentView.postsBoundsChangedNotifications = true
        scrollView.contentView.postsFrameChangedNotifications = true
        for name in [NSView.boundsDidChangeNotification, NSView.frameDidChangeNotification] {
            NotificationCenter.default.addObserver(forName: name, object: scrollView.contentView, queue: .main) { [weak self] _ in
                self?.grid.extendIfNeeded()
                self?.headersNeedDisplay()
                self?.freezeOverlay.panesNeedDisplay()
                self?.grid.repositionEditor()
            }
        }
        grid.overlay = freezeOverlay
        grid.hostEditor(in: freezeOverlay)

        // Bottom bar: sheet tabs + selection statistics
        tabs.spacing = 2
        status.font = .systemFont(ofSize: 11)
        status.textColor = .secondaryLabelColor
        status.alignment = .right
        status.lineBreakMode = .byTruncatingHead
        status.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        let bottom = NSStackView(views: [tabs, NSView()])
        bottom.edgeInsets = NSEdgeInsets(top: 0, left: 8, bottom: 0, right: 12)
        // Status bar: selection statistics (right-click to choose them) and a zoom slider.
        zoomSlider.target = self
        zoomSlider.action = #selector(zoomSliderChanged(_:))
        zoomSlider.controlSize = .small
        zoomSlider.toolTip = L("Zoom")
        zoomSlider.translatesAutoresizingMaskIntoConstraints = false
        zoomSlider.widthAnchor.constraint(equalToConstant: 120).isActive = true
        zoomLabel.font = .monospacedDigitSystemFont(ofSize: 11, weight: .regular)
        zoomLabel.textColor = .secondaryLabelColor
        zoomLabel.alignment = .right
        zoomLabel.translatesAutoresizingMaskIntoConstraints = false
        zoomLabel.widthAnchor.constraint(equalToConstant: 40).isActive = true
        status.menu = statusMenu
        let statusRow = NSStackView(views: [sheetInfo, NSView(), status, zoomSlider, zoomLabel])
        statusRow.spacing = 10
        statusRow.edgeInsets = NSEdgeInsets(top: 0, left: 10, bottom: 0, right: 12)
        statusRow.translatesAutoresizingMaskIntoConstraints = false
        statusBar.material = .titlebar
        statusBar.translatesAutoresizingMaskIntoConstraints = false
        statusBar.addSubview(statusRow)
        sheetInfo.font = .systemFont(ofSize: 11)
        sheetInfo.textColor = .secondaryLabelColor
        bottom.translatesAutoresizingMaskIntoConstraints = false
        let bottomBackground = NSVisualEffectView()
        bottomBackground.material = .titlebar
        bottomBackground.translatesAutoresizingMaskIntoConstraints = false
        bottomBackground.addSubview(bottom)
        let line = NSBox()
        line.boxType = .separator
        line.translatesAutoresizingMaskIntoConstraints = false

        let root = NSView()
        let corner = CornerView()
        corner.onClick = { [weak self] in self?.grid.selectAllCells() }
        signatureHint.drawsBackground = true
        signatureHint.backgroundColor = NSColor(name: nil) { SheetPalette.current($0).hintFill }
        signatureHint.isBordered = true
        signatureHint.isHidden = true
        signatureHint.lineBreakMode = .byTruncatingTail
        for v in [scrollView, freezeOverlay, bottomBackground, line, columnHeader, rowHeader, corner, rowGutter, colGutter, signatureHint, statusBar] as [NSView] {
            v.translatesAutoresizingMaskIntoConstraints = false
            root.addSubview(v)
        }
        let rgw = rowGutter.widthAnchor.constraint(equalToConstant: 0), cgh = colGutter.heightAnchor.constraint(equalToConstant: 0)
        let sbh = statusBar.heightAnchor.constraint(equalToConstant: 22)
        statusBarHeight = sbh
        rowGutterWidth = rgw
        colGutterHeight = cgh
        NSLayoutConstraint.activate([
            rgw, cgh, sbh,
            statusBar.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            statusBar.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            statusBar.bottomAnchor.constraint(equalTo: root.bottomAnchor),
            statusRow.leadingAnchor.constraint(equalTo: statusBar.leadingAnchor),
            statusRow.trailingAnchor.constraint(equalTo: statusBar.trailingAnchor),
            statusRow.topAnchor.constraint(equalTo: statusBar.topAnchor),
            statusRow.bottomAnchor.constraint(equalTo: statusBar.bottomAnchor),
            colGutter.topAnchor.constraint(equalTo: root.safeAreaLayoutGuide.topAnchor),
            colGutter.leadingAnchor.constraint(equalTo: rowGutter.trailingAnchor),
            colGutter.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            rowGutter.topAnchor.constraint(equalTo: root.safeAreaLayoutGuide.topAnchor),
            rowGutter.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            rowGutter.bottomAnchor.constraint(equalTo: scrollView.bottomAnchor),
            signatureHint.topAnchor.constraint(equalTo: root.safeAreaLayoutGuide.topAnchor, constant: 1),
            signatureHint.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 120),
            signatureHint.trailingAnchor.constraint(lessThanOrEqualTo: root.trailingAnchor, constant: -10),
            corner.topAnchor.constraint(equalTo: colGutter.bottomAnchor),
            corner.leadingAnchor.constraint(equalTo: rowGutter.trailingAnchor),
            corner.widthAnchor.constraint(equalToConstant: 46),
            corner.heightAnchor.constraint(equalToConstant: 22),
            columnHeader.topAnchor.constraint(equalTo: corner.topAnchor),
            columnHeader.leadingAnchor.constraint(equalTo: corner.trailingAnchor),
            columnHeader.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            columnHeader.heightAnchor.constraint(equalTo: corner.heightAnchor),
            rowHeader.topAnchor.constraint(equalTo: corner.bottomAnchor),
            rowHeader.leadingAnchor.constraint(equalTo: rowGutter.trailingAnchor),
            rowHeader.widthAnchor.constraint(equalTo: corner.widthAnchor),
            rowHeader.bottomAnchor.constraint(equalTo: scrollView.bottomAnchor),
            freezeOverlay.topAnchor.constraint(equalTo: scrollView.topAnchor),
            freezeOverlay.leadingAnchor.constraint(equalTo: scrollView.leadingAnchor),
            freezeOverlay.trailingAnchor.constraint(equalTo: scrollView.trailingAnchor),
            freezeOverlay.bottomAnchor.constraint(equalTo: scrollView.bottomAnchor),
            scrollView.topAnchor.constraint(equalTo: corner.bottomAnchor),
            scrollView.leadingAnchor.constraint(equalTo: corner.trailingAnchor),
            scrollView.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            scrollView.bottomAnchor.constraint(equalTo: bottomBackground.topAnchor),
            line.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            line.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            line.bottomAnchor.constraint(equalTo: bottomBackground.topAnchor),
            bottomBackground.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            bottomBackground.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            bottomBackground.bottomAnchor.constraint(equalTo: statusBar.topAnchor),
            bottomBackground.heightAnchor.constraint(equalToConstant: 30),
            bottom.leadingAnchor.constraint(equalTo: bottomBackground.leadingAnchor),
            bottom.trailingAnchor.constraint(equalTo: bottomBackground.trailingAnchor),
            bottom.topAnchor.constraint(equalTo: bottomBackground.topAnchor),
            bottom.bottomAnchor.constraint(equalTo: bottomBackground.bottomAnchor),
        ])
        window?.contentView = root

        setupFormulaBar()
        setupFormatControls()
    }

    private func setupFormulaBar() {
        nameBox.font = .monospacedDigitSystemFont(ofSize: 12, weight: .regular)
        nameBox.alignment = .center
        nameBox.bezelStyle = .roundedBezel
        nameBox.delegate = self
        nameBox.toolTip = L("Cell address — type one to go there")
        nameBox.translatesAutoresizingMaskIntoConstraints = false
        nameBox.widthAnchor.constraint(equalToConstant: 90).isActive = true

        let fx = NSButton(title: "fx", target: self, action: #selector(insertFunctionAction(_:)))
        fx.isBordered = false
        fx.font = NSFont(descriptor: NSFont.systemFont(ofSize: 13).fontDescriptor.withSymbolicTraits(.italic), size: 13)
        fx.contentTintColor = .secondaryLabelColor
        fx.toolTip = L("Insert Function…")

        formulaField.font = .systemFont(ofSize: 13)
        formulaField.bezelStyle = .roundedBezel
        formulaField.placeholderString = L("Value or formula, e.g. =SUM(A1:A10)")
        formulaField.delegate = self
        formulaField.lineBreakMode = .byTruncatingTail

        let row = NSStackView(views: [nameBox, fx, formulaField])
        row.spacing = 8
        row.edgeInsets = NSEdgeInsets(top: 0, left: 10, bottom: 0, right: 10)
        row.translatesAutoresizingMaskIntoConstraints = false
        let container = NSView(frame: NSRect(x: 0, y: 0, width: 800, height: 34))
        container.addSubview(row)
        NSLayoutConstraint.activate([
            row.leadingAnchor.constraint(equalTo: container.leadingAnchor),
            row.trailingAnchor.constraint(equalTo: container.trailingAnchor),
            row.centerYAnchor.constraint(equalTo: container.centerYAnchor),
            container.heightAnchor.constraint(equalToConstant: 34),
        ])
        formulaBar.view = container
        formulaBar.layoutAttribute = .bottom
    }

    private func setupFormatControls() {}

    static var currencyFormat: String {
        let symbol = Locale.current.currencySymbol ?? "$"
        return Locale.current.currency?.identifier == "USD" ? "\"\(symbol)\"#,##0.00" : "#,##0.00 \"\(symbol)\""
    }

    static var numberFormats: [(title: String, code: String)] {
        [(L("General"), "General"), (L("Number"), "#,##0.00"), (L("Integer"), "#,##0"), (L("Currency"), currencyFormat),
         (L("Percent"), "0.00%"), (L("Date"), "dd.mm.yyyy"), (L("Time"), "hh:mm"), (L("Date and time"), "dd.mm.yyyy hh:mm"),
         (L("Scientific"), "0.00E+00"), (L("Text"), "@")]
    }

    func debugScroll(to p: NSPoint) {
        scrollView.contentView.scroll(to: p)
        scrollView.reflectScrolledClipView(scrollView.contentView)
    }

    // MARK: - Refresh

    func headersNeedDisplay() {
        columnHeader.needsDisplay = true
        rowHeader.needsDisplay = true
        rowGutter.needsDisplay = true
        colGutter.needsDisplay = true
    }

    func workbookDidChange() {
        if let doc, !doc.workbook.sheets.indices.contains(sheetIndex) || doc.workbook.sheets[sheetIndex].hidden {
            sheetIndex = doc.workbook.sheets.firstIndex { !$0.hidden } ?? 0
        }
        reloadTabs()
        grid.rebuildLayout()
        updateOutlineGutters()
        selectionDidChange()
    }

    /// Shows the grouping gutters only when the sheet has outline levels.
    func updateOutlineGutters() {
        let w = OutlineGutter.thickness(sheet.maxRowLevel), h = OutlineGutter.thickness(sheet.maxColLevel)
        if rowGutterWidth?.constant != w { rowGutterWidth?.constant = w }
        if colGutterHeight?.constant != h { colGutterHeight?.constant = h }
        rowGutter.leading = h + 22
        colGutter.leading = 46
        headersNeedDisplay()
    }

    func showSignatureHint(_ text: NSAttributedString) {
        signatureHint.attributedStringValue = text
        signatureHint.isHidden = false
    }

    func hideSignatureHint() { signatureHint.isHidden = true }

    func selectionDidChange() {
        let sel = grid.selection
        nameBox.stringValue = sel.count > 1 && sel.end.row < RefPart.maxRow ? sel.a1 : grid.active.a1
        if !grid.isEditing {
            formulaField.stringValue = formulaBarText(at: grid.active)
            hideSignatureHint()
        }
        let style = sheet[grid.active].style
        fontPopup.selectItem(withTitle: style.fontName ?? "Calibri")
        if fontPopup.selectedItem == nil, let name = style.fontName { fontPopup.addItem(withTitle: name); fontPopup.selectItem(withTitle: name) }
        sizeStepper.value = style.fontSize
        textColorButton.color = style.textColor ?? .black
        fillColorButton.color = style.fill
        formatStrip.setState(style.bold, for: #selector(toggleBold(_:)))
        formatStrip.setState(style.italic, for: #selector(toggleItalic(_:)))
        formatStrip.setState(style.underline, for: #selector(toggleUnderline(_:)))
        formatStrip.setState(style.strike, for: #selector(toggleStrike(_:)))
        formatStrip.setState(style.wrap, for: #selector(toggleWrap(_:)))
        formatStrip.setState(sheet.merge(containing: grid.active) != nil, for: #selector(mergeAction(_:)))
        alignButton.setSymbol(style.hAlign == "center" ? "text.aligncenter" : style.hAlign == "right" ? "text.alignright" : "text.alignleft")
        if let i = Self.numberFormats.firstIndex(where: { $0.code == style.numberFormat }) {
            numberFormatPopup.selectItem(at: i)
        } else {
            numberFormatPopup.selectItem(at: -1)
            numberFormatPopup.setTitle(style.numberFormat)
        }
        updateStatus()
        window?.toolbar?.validateVisibleItems()
    }

    private func updateStatus() {
        status.stringValue = statusText(for: clampedSelection)
        let visible = visibleSheets
        sheetInfo.stringValue = L("Sheet %d of %d", (visible.firstIndex(of: sheetIndex) ?? 0) + 1, visible.count)
        // Position, as in the other apps: "Sheet name · A1" or "Sheet name · A1:C4".
        let subtitle = sheet.name + " · " + nameBox.stringValue
        if window?.subtitle != subtitle { window?.subtitle = subtitle }
    }

    /// Current status bar text (tests).
    var statusBarText: String { status.stringValue }

    @objc func zoomSliderChanged(_ sender: NSSlider) { grid.zoom = CGFloat((sender.doubleValue / 5).rounded() * 5 / 100) }

    func zoomDidChange() {
        zoomSlider.doubleValue = Double(grid.zoom * 100)
        zoomLabel.stringValue = "\(Int((grid.zoom * 100).rounded()))%"
    }

    @objc func toggleStatusBarAction(_ sender: Any?) { setStatusBarHidden(!statusBar.isHidden) }

    func setStatusBarHidden(_ hidden: Bool) {
        statusBar.isHidden = hidden
        statusBarHeight?.constant = hidden ? 0 : 22
    }

    /// Caret in the formula bar while it is being edited.
    var formulaCaret: NSRange? { window?.firstResponder === formulaField.currentEditor() ? formulaField.currentEditor()?.selectedRange : nil }

    func editorTextChanged(_ text: String) {
        if window?.firstResponder !== formulaField.currentEditor() { formulaField.stringValue = text }
        updateSignatureHint(text)
    }

    /// Limit header selections along their unbounded axis; preserve explicitly selected blank cells.
    var clampedSelection: CellRange {
        let s = grid.selection
        let used = sheet.usedRange
        let end = CellRef(row: grid.selectionAxis == .columns ? min(s.end.row, max(used?.end.row ?? 0, s.start.row)) : s.end.row,
                          col: grid.selectionAxis == .rows ? min(s.end.col, max(used?.end.col ?? 0, s.start.col)) : s.end.col)
        return CellRange(s.start, end)
    }

    static let maximumCellOperationCount = 100_000

    /// Dense operations create cells or clipboard entries even for blank positions.
    func guardCellOperation(_ range: CellRange) -> Bool {
        guard range.rows.count <= Self.maximumCellOperationCount / range.cols.count else {
            report(L("This operation supports up to %d cells. Select a smaller range.", Self.maximumCellOperationCount))
            return false
        }
        return true
    }

    // MARK: - Sheet tabs

    private func reloadTabs() {
        tabs.arrangedSubviews.forEach { $0.removeFromSuperview() }
        for (i, s) in (doc?.workbook.sheets ?? []).enumerated() where !s.hidden {
            let b = SheetTabButton(title: s.name, target: self, action: #selector(tabClicked(_:)))
            b.tabColor = s.tabColor
            b.tag = i
            b.bezelStyle = .accessoryBarAction
            b.setButtonType(.pushOnPushOff)
            b.state = i == sheetIndex ? .on : .off
            b.font = .systemFont(ofSize: 12, weight: i == sheetIndex ? .semibold : .regular)
            let menu = NSMenu()
            menu.addItem(withTitle: L("Rename…"), action: #selector(renameSheet(_:)), keyEquivalent: "").tag = i
            menu.addItem(withTitle: L("Duplicate"), action: #selector(duplicateSheet(_:)), keyEquivalent: "").tag = i
            menu.addItem(withTitle: L("Move or Copy…"), action: #selector(moveCopySheetAction(_:)), keyEquivalent: "").tag = i + 1
            menu.addItem(withTitle: L("Tab Colour…"), action: #selector(tabColorAction(_:)), keyEquivalent: "").tag = i + 1
            menu.addItem(withTitle: L("Hide Sheet"), action: #selector(hideSheetAction(_:)), keyEquivalent: "").tag = i + 1
            menu.addItem(withTitle: L("Show Sheet…"), action: #selector(showSheetAction(_:)), keyEquivalent: "")
            menu.addItem(.separator())
            menu.addItem(withTitle: L("Delete Sheet"), action: #selector(deleteSheet(_:)), keyEquivalent: "").tag = i
            menu.items.forEach { $0.target = self }
            b.menu = menu
            tabs.addArrangedSubview(b)
        }
        let add = NSButton(image: Theme42.symbol("plus", size: 12) ?? NSImage(), target: self, action: #selector(addSheet(_:)))
        add.bezelStyle = .accessoryBarAction
        add.toolTip = L("New Sheet")
        tabs.addArrangedSubview(add)
    }

    func selectSheet(_ index: Int) {
        guard let doc, doc.workbook.sheets.indices.contains(index) else { return }
        guard grid.commitEdit(move: nil) else { return }
        storeSplit()
        sheetIndex = index
        reloadTabs()
        grid.rebuildLayout()
        updateOutlineGutters()
        grid.select(CellRef(row: 0, col: 0))
        restoreSplit()
    }

    @objc private func tabClicked(_ sender: NSButton) {
        guard grid.commitEdit(move: nil) else { return }
        if sender.tag == sheetIndex, NSApp.currentEvent?.clickCount == 2 {
            renameSheet(sender)
            return
        }
        storeSplit()
        sheetIndex = sender.tag
        reloadTabs()
        grid.rebuildLayout()
        updateOutlineGutters()
        grid.select(CellRef(row: 0, col: 0))
        restoreSplit()
    }

    @objc func addSheet(_ sender: Any?) {
        guard let doc, guardStructure() else { return }
        guard grid.commitEdit(move: nil) else { return }
        var n = doc.workbook.sheets.count + 1
        while doc.workbook.sheetIndex(named: L("Sheet%d", n)) != nil { n += 1 }
        doc.change(L("New Sheet")) { $0.sheets.append(Sheet(name: L("Sheet%d", n))) }
        sheetIndex = doc.workbook.sheets.count - 1
        workbookDidChange()
    }

    @objc private func renameSheet(_ sender: Any?) { renameSheet(at: commandTag(of: sender)) }

    func renameSheet(at i: Int) {
        guard let doc, doc.workbook.sheets.indices.contains(i), guardStructure() else { return }
        guard grid.commitEdit(move: nil) else { return }
        let field = NSTextField(string: doc.workbook.sheets[i].name)
        let alert = NSAlert.form(L("Rename Sheet"), ok: L("Rename"), rows: [(L("Name:"), field)])
        alert.window.initialFirstResponder = field
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        let name = field.stringValue.trimmingCharacters(in: .whitespaces)
        guard Self.validSheetName(name), doc.workbook.sheetIndex(named: name) == nil || doc.workbook.sheetIndex(named: name) == i else { NSSound.beep(); return }
        let old = doc.workbook.sheets[i].name
        doc.change(L("Rename Sheet")) { wb in
            wb.sheets[i].name = name
            Self.renameReferences(&wb, from: old, to: name)
        }
    }

    static func renameSheetRefs(_ e: Expr, from: String, to: String) -> Expr {
        func m(_ e: Expr) -> Expr {
            switch e {
            case .ref(let s, let p): return .ref(sheet: s?.caseInsensitiveCompare(from) == .orderedSame ? to : s, p)
            case .range(let s, let a, let b): return .range(sheet: s?.caseInsensitiveCompare(from) == .orderedSame ? to : s, a, b)
            case .unary(let o, let x): return .unary(o, m(x))
            case .percent(let x): return .percent(m(x))
            case .binary(let o, let l, let r): return .binary(o, m(l), m(r))
            case .call(let n, let args): return .call(n, args.map(m))
            default: return e
            }
        }
        return m(e)
    }

    @objc private func duplicateSheet(_ sender: Any?) {
        let i = commandTag(of: sender)
        guard let doc, doc.workbook.sheets.indices.contains(i), guardStructure() else { return }
        guard grid.commitEdit(move: nil) else { return }
        moveSheet(i, before: i + 1, copy: true)
    }

    @objc private func deleteSheet(_ sender: Any?) { deleteSheet(at: commandTag(of: sender)) }

    func deleteSheet(at i: Int) {
        guard let doc, doc.workbook.sheets.indices.contains(i), guardStructure(), visibleSheets.count > 1 || doc.workbook.sheets[i].hidden, doc.workbook.sheets.count > 1 else { NSSound.beep(); return }
        let alert = NSAlert()
        alert.messageText = L("Delete sheet “%@”?", doc.workbook.sheets[i].name)
        alert.informativeText = L("You can undo this with ⌘Z.")
        alert.addButton(withTitle: L("Delete"))
        alert.addButton(withTitle: L("Cancel"))
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        guard grid.commitEdit(move: nil) else { return }
        doc.change(L("Delete Sheet")) { $0.sheets.remove(at: i) }
        sheetIndex = min(sheetIndex, doc.workbook.sheets.count - 1)
        if doc.workbook.sheets[sheetIndex].hidden, let v = visibleSheets.last { sheetIndex = v }
        workbookDidChange()
    }

    // MARK: - Editing cells

    /// Stores typed text: canonical formula or constant, applying an automatic format for %, dates, currency.
    @discardableResult
    func setInput(_ text: String, at ref: CellRef) -> Bool {
        guard let doc else { return false }
        var input = text
        if input.hasPrefix("="), input.count > 1 {
            if let e = try? Formula.parse(input) { input = "=" + Formula.serialize(e) }
        } else if input.hasPrefix("+") || input.hasPrefix("-"), input.count > 1, (try? Formula.parse("=" + input)) != nil,
                  input.dropFirst().contains(where: { !$0.isNumber && $0 != "." && $0 != "," }) {
            input = "=" + input
        }
        let current = sheet[ref]
        guard current.input != input else { return true }
        if let a = sheet.array(containing: ref) {
            // A cell of an array formula: unchanged text is no change; other text only replaces a one-cell array.
            if input == sheet[a.anchor].input { selectionDidChange(); return true }
            if a.range.count > 1 { report(Self.partOfArrayMessage); selectionDidChange(); return false }
        }
        guard guardEditable(CellRange(ref, ref)), validateInput(input, at: ref) else { selectionDidChange(); return false }
        doc.change(L("Typing")) { wb in
            var cell = wb.sheets[sheetIndex][ref]
            if wb.sheets[sheetIndex].arrays[ref] != nil { wb.sheets[sheetIndex].arrays[ref] = nil; cell.array = false }
            cell.input = input
            cell.value = input.hasPrefix("=") && input.count > 1 ? .empty : XLSX.constant(input, date1904: wb.date1904)
            if cell.style.numberFormat == "General", !input.hasPrefix("=") {
                let t = input.trimmingCharacters(in: .whitespaces)
                if t.hasSuffix("%"), case .number = cell.value { cell.style.numberFormat = t.contains(".") || t.contains(",") ? "0.00%" : "0%" }
                else if case .number = cell.value, Double(t.replacingOccurrences(of: ",", with: ".")) == nil, DateMath.parse(t, date1904: wb.date1904) != nil {
                    cell.style.numberFormat = t.contains(":") ? "dd.mm.yyyy hh:mm" : "dd.mm.yyyy"
                }
            }
            wb.sheets[sheetIndex][ref] = cell
        }
        return true
    }

    private func modifyCells(_ name: String, _ body: (inout Cell) -> Void) {
        guard grid.commitEdit(move: nil) else { return }
        let range = clampedSelection
        guard let doc, guardCellOperation(range), guardEditable(range) else { return }
        doc.change(name) { wb in
            for ref in range.cells {
                var cell = wb.sheets[sheetIndex][ref]
                body(&cell)
                wb.sheets[sheetIndex][ref] = cell
            }
        }
    }

    private func modifyStyle(_ name: String, _ body: (inout CellStyle) -> Void) {
        guard grid.commitEdit(move: nil) else { return }
        let range = clampedSelection
        guard let doc, guardCellOperation(range), guardEditable(range) else { return }
        doc.change(name, recalc: false) { wb in
            for ref in range.cells {
                var cell = wb.sheets[sheetIndex][ref]
                body(&cell.style)
                wb.sheets[sheetIndex][ref] = cell
            }
        }
    }

    @objc func clearContents(_ sender: Any?) {
        guard grid.commitEdit(move: nil) else { return }
        let range = clampedSelection
        guard let doc, guardArrays(range), guardEditable(range) else { return }
        doc.change(L("Clear")) { wb in
            dropArrays(in: range, of: &wb)
            // Clearing blank positions is a no-op, including in very large selections.
            let refs = wb.sheets[sheetIndex].cells.keys.filter { range.contains($0) }
            for ref in refs {
                var cell = wb.sheets[sheetIndex][ref]
                cell.input = ""
                cell.value = .empty
                wb.sheets[sheetIndex][ref] = cell
            }
        }
    }

    @objc func clearFormats(_ sender: Any?) {
        let range = clampedSelection
        guard let doc, guardEditable(range) else { return }
        doc.change(L("Clear Formats"), recalc: false) { wb in
            let refs = wb.sheets[sheetIndex].cells.keys.filter { range.contains($0) }
            for ref in refs {
                var cell = wb.sheets[sheetIndex][ref]
                cell.style = .default
                wb.sheets[sheetIndex][ref] = cell
            }
        }
    }

    // MARK: - Clipboard

    struct Clip {
        var origin: CellRef
        var rows: Int
        var cols: Int
        var cells: [CellRef: Cell]   // offsets
        var changeCount: Int
        var arrays: [CellRef: CellRange] = [:] // offsets; only complete legacy arrays
        var external = false
    }

    static var clip: Clip?
    /// UI audits use a private native pasteboard, avoiding clipboard-manager/user races.
    static let pasteboard: NSPasteboard = ProcessInfo.processInfo.environment["K42_PASTEBOARD"] == "1"
        ? NSPasteboard(name: NSPasteboard.Name("com.sfzorin.xls42.audit.\(ProcessInfo.processInfo.processIdentifier)")) : .general

    @objc func copy(_ sender: Any?) {
        guard grid.commitEdit(move: nil) else { return }
        let range = clampedSelection
        guard guardCellOperation(range) else { return }
        var cells: [CellRef: Cell] = [:]
        var lines: [String] = []
        var arrays: [CellRef: CellRange] = [:]
        func offset(_ ref: CellRef) -> CellRef { CellRef(row: ref.row - range.start.row, col: ref.col - range.start.col) }
        for (anchor, array) in sheet.arrays where range.contains(array.start) && range.contains(array.end) {
            arrays[offset(anchor)] = CellRange(offset(array.start), offset(array.end))
        }
        for r in range.rows {
            var fields: [String] = []
            for c in range.cols {
                let ref = CellRef(row: r, col: c)
                var cell = sheet[ref]
                // Spilled values: part of a copied spill re-spills from its formula, otherwise they're copied as values.
                if let array = sheet.array(containing: ref), arrays[offset(array.anchor)] == nil {
                    cell.input = XLSX.inputText(for: cell.value, style: cell.style, date1904: doc?.workbook.date1904 ?? false)
                    cell.array = false
                } else if cell.input.isEmpty, !cell.value.isEmpty {
                    if let anchor = sheet.spills.first(where: { $0.value.contains(ref) })?.key, range.contains(anchor) {
                        // Keep cached values for Paste Values; ordinary Paste recalculates the spill.
                    } else { cell.input = XLSX.inputText(for: cell.value, style: cell.style, date1904: doc?.workbook.date1904 ?? false) }
                }
                cells[CellRef(row: r - range.start.row, col: c - range.start.col)] = cell
                let text = NumberFormat.format(cell.value, cell.style.numberFormat, date1904: doc?.workbook.date1904 ?? false).text
                fields.append(text.contains(where: { "\t\n\r\"".contains($0) }) ? "\"" + text.replacingOccurrences(of: "\"", with: "\"\"") + "\"" : text)
            }
            lines.append(fields.joined(separator: "\t"))
        }
        let pb = Self.pasteboard
        pb.clearContents()
        pb.setString(lines.joined(separator: "\n"), forType: .string)
        Self.clip = Clip(origin: range.start, rows: range.rows.count, cols: range.cols.count, cells: cells, changeCount: pb.changeCount, arrays: arrays)
    }

    @objc func cut(_ sender: Any?) {
        guard grid.commitEdit(move: nil), guardCellOperation(clampedSelection), guardEditable(clampedSelection), guardArrays(clampedSelection) else { return }
        copy(sender)
        clearContents(sender)
    }

    @objc func paste(_ sender: Any?) {
        performPaste(.all, ordinary: true)
    }

    @objc override func selectAll(_ sender: Any?) { grid.selectAllCells() }

    @objc func fillDown(_ sender: Any?) { fill(down: true) }
    @objc func fillRight(_ sender: Any?) { fill(down: false) }

    private func fill(down: Bool) {
        guard grid.commitEdit(move: nil) else { return }
        applyFill(clampedSelection, down ? .down : .right)
    }

    // MARK: - Formatting

    private func toggle(_ name: String, _ key: WritableKeyPath<CellStyle, Bool>) {
        let on = !sheet[grid.active].style[keyPath: key]
        modifyStyle(name) { $0[keyPath: key] = on }
    }

    @objc func toggleBold(_ sender: Any?) { toggle(L("Bold"), \.bold) }
    @objc func toggleItalic(_ sender: Any?) { toggle(L("Italic"), \.italic) }
    @objc func toggleUnderline(_ sender: Any?) { toggle(L("Underline"), \.underline) }
    @objc func toggleStrike(_ sender: Any?) { toggle(L("Strikethrough"), \.strike) }
    @objc func toggleWrap(_ sender: Any?) { toggle(L("Wrap Text"), \.wrap) }

    @objc func alignAction(_ sender: Any?) {
        let value = ["left", "center", "right"][max(0, min(2, commandTag(of: sender)))]
        let current = sheet[grid.active].style.hAlign
        modifyStyle(L("Alignment")) { $0.hAlign = current == value ? nil : value }
    }

    @objc private func fontChanged(_ sender: NSPopUpButton) {
        let name = sender.titleOfSelectedItem
        modifyStyle(L("Font")) { $0.fontName = name }
    }

    @objc private func sizeChanged(_ sender: SizeStepper) {
        let size = sender.value
        modifyStyle(L("Font Size")) { $0.fontSize = size }
        autofitRows(clampedSelection.rows, undoable: false)
    }

    @objc private func textColorChanged(_ sender: ColorButton) {
        let color = sender.color
        modifyStyle(L("Text Colour")) { $0.textColor = color }
    }

    @objc private func fillColorChanged(_ sender: ColorButton) {
        let color = sender.color
        modifyStyle(L("Fill Colour")) { $0.fill = color }
    }

    @objc func noFillAction(_ sender: Any?) { modifyStyle(L("Fill Colour")) { $0.fill = nil } }

    @objc private func numberFormatChanged(_ sender: NSPopUpButton) {
        let i = sender.indexOfSelectedItem
        guard i >= 0, i < Self.numberFormats.count else { return }
        setNumberFormat(Self.numberFormats[i].code)
    }

    @objc func numberFormatAction(_ sender: Any?) {
        let i = commandTag(of: sender)
        guard i >= 0, i < Self.numberFormats.count else { return }
        setNumberFormat(Self.numberFormats[i].code)
    }

    private func setNumberFormat(_ code: String) {
        modifyCells(L("Number Format")) { cell in
            cell.style.numberFormat = code
            if code == "@", case .number = cell.value { cell.value = .string(cell.input) }
        }
    }

    @objc func increaseDecimals(_ sender: Any?) { changeDecimals(by: 1) }
    @objc func decreaseDecimals(_ sender: Any?) { changeDecimals(by: -1) }

    private func changeDecimals(by delta: Int) {
        var code = sheet[grid.active].style.numberFormat
        if code == "General" {
            if case .number(let d) = sheet[grid.active].value {
                let s = Value.formatGeneral(d)
                let decimals = s.split(separator: ".").dropFirst().first?.count ?? 0
                code = decimals > 0 ? "0." + String(repeating: "0", count: decimals) : "0"
            } else { code = "0" }
        }
        let section = String(code.split(separator: ";").first ?? Substring(code))
        let new: String
        if let dot = section.firstIndex(of: ".") {
            let after = section[section.index(after: dot)...]
            let zeros = after.prefix { $0 == "0" || $0 == "#" }.count
            let rest = after.dropFirst(zeros)
            let count = max(0, zeros + delta)
            new = String(section[..<dot]) + (count > 0 ? "." + String(repeating: "0", count: count) : "") + rest
        } else if delta > 0, let last = section.lastIndex(where: { $0 == "0" || $0 == "#" }) {
            new = String(section[...last]) + ".0" + section[section.index(after: last)...]
        } else { return }
        setNumberFormat(new)
    }

    @objc func mergeAction(_ sender: Any?) {
        guard grid.commitEdit(move: nil), let doc, guardEditable(nil) else { return }
        let range = clampedSelection
        guard guardCellOperation(range), !sheet.arrays.values.contains(where: { $0.intersects(range) }) else { report(Self.partOfArrayMessage); return }
        let existing = sheet.merges.filter { $0 == range || range.contains($0.start) }
        doc.change(L("Merge Cells")) { wb in
            if !existing.isEmpty {
                wb.sheets[sheetIndex].merges.removeAll { m in existing.contains(m) }
            } else if range.count > 1 {
                wb.sheets[sheetIndex].merges.removeAll { $0.intersects(range) }
                wb.sheets[sheetIndex].merges.append(range)
                var top = wb.sheets[sheetIndex][range.start]
                top.style.hAlign = "center"
                wb.sheets[sheetIndex][range.start] = top
            }
        }
    }

    @objc func bordersAction(_ sender: Any?) {
        guard grid.commitEdit(move: nil) else { return }
        let mode = commandTag(of: sender)   // 0 all, 1 outside, 2 bottom, 3 none
        let range = clampedSelection
        guard guardCellOperation(range), guardEditable(range) else { return }
        let side = BorderSide(style: "thin", color: .black)
        doc?.change(L("Borders")) { wb in
            for ref in range.cells {
                var s = wb.sheets[sheetIndex][ref].style
                switch mode {
                case 0: (s.left, s.right, s.top, s.bottom) = (side, side, side, side)
                case 1:
                    if ref.col == range.start.col { s.left = side }
                    if ref.col == range.end.col { s.right = side }
                    if ref.row == range.start.row { s.top = side }
                    if ref.row == range.end.row { s.bottom = side }
                case 2: if ref.row == range.end.row { s.bottom = side }
                default: (s.left, s.right, s.top, s.bottom) = (nil, nil, nil, nil)
                }
                var cell = wb.sheets[sheetIndex][ref]
                cell.style = s
                wb.sheets[sheetIndex][ref] = cell
            }
        }
    }

    // MARK: - Rows & columns

    enum Axis { case row(Int), column(Int) }

    func previewSize(_ axis: Axis, _ size: CGFloat) {
        doc?.preview { wb in
            switch axis {
            case .row(let r): wb.sheets[sheetIndex].rowHeights[r] = size
            case .column(let c): wb.sheets[sheetIndex].columnWidths[c] = size
            }
        }
    }

    func commitSize(_ axis: Axis, _ size: CGFloat, original: CGFloat) {
        guard size != original else { return }
        previewSize(axis, original)
        let sel = grid.selection
        doc?.change(L("Resize")) { wb in
            switch axis {
            case .row(let r):
                let rows = sel.rows.contains(r) && sel.cols.count >= grid.colCount - 1 ? Array(sel.rows) : [r]
                rows.forEach { wb.sheets[sheetIndex].rowHeights[$0] = size }
            case .column(let c):
                let cols = sel.cols.contains(c) && sel.rows.count >= grid.rowCount - 1 ? Array(sel.cols) : [c]
                cols.forEach { wb.sheets[sheetIndex].columnWidths[$0] = size }
            }
        }
    }

    func autofit(_ axis: Axis) {
        guard grid.commitEdit(move: nil), let doc, guardEditable(nil) else { return }
        switch axis {
        case .column(let c):
            var width: CGFloat = 0
            for (ref, cell) in sheet.cells where ref.col == c && !cell.value.isEmpty && sheet.merge(containing: ref) == nil {
                let text = NumberFormat.format(cell.value, cell.style.numberFormat, date1904: self.doc?.workbook.date1904 ?? false).text
                width = max(width, (text as NSString).size(withAttributes: [.font: cell.style.font()]).width + 8)
            }
            guard width > 0 else { return }
            doc.change(L("Autofit")) { $0.sheets[sheetIndex].columnWidths[c] = min(600, ceil(width)) }
        case .row(let r):
            autofitRows(r...r)
        }
    }

    private func autofitRows(_ rows: ClosedRange<Int>, undoable: Bool = true) {
        guard let doc else { return }
        var heights: [Int: CGFloat] = [:]
        for (ref, cell) in sheet.cells where rows.contains(ref.row) {
            let font = cell.style.font()
            var h = ceil(font.ascender - font.descender + font.leading) + 3
            if cell.style.wrap {
                let text = NumberFormat.format(cell.value, cell.style.numberFormat, date1904: self.doc?.workbook.date1904 ?? false).text
                let w = max(1, sheet.width(ref.col) - 4)
                h = ceil((text as NSString).boundingRect(with: NSSize(width: w, height: .greatestFiniteMagnitude),
                                                          options: [.usesLineFragmentOrigin], attributes: [.font: font]).height) + 4
            }
            heights[ref.row] = max(heights[ref.row] ?? 0, h)
        }
        let changes = heights.filter { undoable ? abs($0.value - sheet.height($0.key)) > 0.5 : $0.value > sheet.height($0.key) + 0.5 }
        guard !changes.isEmpty else { return }
        let apply: (inout Workbook) -> Void = { wb in for (r, h) in changes { wb.sheets[self.sheetIndex].rowHeights[r] = h } }
        if undoable { doc.change(L("Autofit"), recalc: false, apply) } else { doc.preview(apply) }
    }

    @objc func insertRowsAction(_ sender: Any?) { shift(rows: true, insert: true) }
    @objc func insertColumnsAction(_ sender: Any?) { shift(rows: false, insert: true) }
    @objc func deleteRowsAction(_ sender: Any?) { shift(rows: true, insert: false) }
    @objc func deleteColumnsAction(_ sender: Any?) { shift(rows: false, insert: false) }

    private func shift(rows: Bool, insert: Bool) {
        guard grid.commitEdit(move: nil) else { return }
        guard let doc, guardEditable(nil) else { return }
        let sel = grid.selection
        let index = rows ? sel.start.row : sel.start.col
        let n = rows ? sel.rows.count : sel.cols.count
        let count = insert ? n : -n
        let name = rows ? (insert ? L("Insert Rows") : L("Delete Rows")) : (insert ? L("Insert Columns") : L("Delete Columns"))
        doc.change(name) { DataTools.shiftCells(&$0, sheet: sheetIndex, rows: rows, at: index, count: count) }
    }

    // MARK: - Data

    @objc func autoSumAction(_ sender: Any?) {
        guard grid.commitEdit(move: nil) else { return }
        let ref = grid.active
        var top = ref.row - 1
        func isNumber(_ r: CellRef) -> Bool { if case .number = sheet[r].value { return true }; return false }
        var formula: String?
        if top >= 0, isNumber(CellRef(row: top, col: ref.col)) {
            while top > 0, isNumber(CellRef(row: top - 1, col: ref.col)) { top -= 1 }
            formula = "=SUM(\(CellRange(CellRef(row: top, col: ref.col), CellRef(row: ref.row - 1, col: ref.col)).a1))"
        } else if ref.col > 0, isNumber(CellRef(row: ref.row, col: ref.col - 1)) {
            var left = ref.col - 1
            while left > 0, isNumber(CellRef(row: ref.row, col: left - 1)) { left -= 1 }
            formula = "=SUM(\(CellRange(CellRef(row: ref.row, col: left), CellRef(row: ref.row, col: ref.col - 1)).a1))"
        }
        if let formula { setInput(formula, at: ref) } else { grid.beginEdit(initial: "=SUM(") }
    }

    @objc func sortAscending(_ sender: Any?) { sort(ascending: true) }
    @objc func sortDescending(_ sender: Any?) { sort(ascending: false) }

    private func sort(ascending: Bool) {
        guard grid.commitEdit(move: nil) else { return }
        guard let doc, let used = sheet.usedRange, guardEditable(nil) else { return }
        var range = clampedSelection
        let key = grid.active.col
        if range.count == 1 {
            // Whole table; keep a text header row in place.
            let headerIsText = used.cols.allSatisfy { c -> Bool in
                if case .number = sheet[CellRef(row: used.start.row, col: c)].value { return false }
                return true
            }
            range = CellRange(CellRef(row: used.start.row + (headerIsText ? 1 : 0), col: used.start.col), used.end)
        }
        guard range.rows.count > 1, guardCellOperation(range) else { return }
        guard !sheet.arrays.values.contains(where: { $0.intersects(range) }) else { report(Self.partOfArrayMessage); return }
        let calc = Calculator(doc.workbook)
        let order = range.rows.sorted { a, b in
            let va = sheet[CellRef(row: a, col: key)].value, vb = sheet[CellRef(row: b, col: key)].value
            if va.isEmpty != vb.isEmpty { return !va.isEmpty }
            let c = calc.compare(va, vb)
            return ascending ? c < 0 : c > 0
        }
        doc.change(ascending ? L("Sort Ascending") : L("Sort Descending")) { wb in
            let original = wb.sheets[sheetIndex]
            for (i, from) in order.enumerated() {
                let to = range.start.row + i
                for c in range.cols {
                    var cell = original[CellRef(row: from, col: c)]
                    if cell.isFormula { cell.input = Formula.shifted(cell.input, rows: to - from, cols: 0) }
                    wb.sheets[sheetIndex][CellRef(row: to, col: c)] = cell
                }
            }
        }
    }

    /// Freezes rows above and columns left of the active cell; again to unfreeze.
    @objc func freezeAction(_ sender: Any?) {
        let ref = grid.active
        let frozen = sheet.frozenRows > 0 || sheet.frozenCols > 0
        setFreeze(rows: frozen ? 0 : ref.row, cols: frozen ? 0 : ref.col)
    }

    @objc func freezeTopRowAction(_ sender: Any?) { setFreeze(rows: sheet.frozenRows == 1 && sheet.frozenCols == 0 ? 0 : 1, cols: 0) }
    @objc func freezeFirstColumnAction(_ sender: Any?) { setFreeze(rows: 0, cols: sheet.frozenCols == 1 && sheet.frozenRows == 0 ? 0 : 1) }

    private func setFreeze(rows: Int, cols: Int) {
        // Freezing replaces a split window (as in Excel and Calc).
        if grid.split != nil, rows > 0 || cols > 0 { setSplit(nil) }
        // Start from the top-left so the frozen part shows the first rows/columns.
        scrollView.contentView.scroll(to: .zero)
        scrollView.reflectScrolledClipView(scrollView.contentView)
        doc?.change(L("Freeze Panes")) { wb in
            wb.sheets[sheetIndex].frozenRows = rows
            wb.sheets[sheetIndex].frozenCols = cols
        }
        freezeOverlay.panesNeedDisplay()
    }

    @objc func toggleGridlines(_ sender: Any?) {
        doc?.change(L("Gridlines")) { $0.sheets[sheetIndex].showGridlines.toggle() }
    }

    // MARK: - Export

    /// Selection for printing (limited to the used area).
    var printSelection: CellRange { clampedSelection }

    @objc func exportPDFAction(_ sender: Any?) {
        guard let doc, let window else { return }
        guard grid.commitEdit(move: nil) else { return }
        var options = SheetExport.Options.saved
        let scope = NSPopUpButton(frame: .zero, pullsDown: false)
        scope.addItems(withTitles: [L("Current sheet"), L("All sheets"), L("Selection")])
        scope.selectItem(at: options.scope.rawValue)
        let orientation = NSPopUpButton(frame: .zero, pullsDown: false)
        orientation.addItems(withTitles: [L("Automatic"), L("Portrait"), L("Landscape")])
        orientation.selectItem(at: options.orientation.rawValue)
        let fit = NSButton(checkboxWithTitle: L("Fit all columns to page width"), target: nil, action: nil)
        fit.state = options.fitWidth ? .on : .off
        let gridlines = NSButton(checkboxWithTitle: L("Print gridlines"), target: nil, action: nil)
        gridlines.state = options.gridlines ? .on : .off
        let grid = NSGridView(views: [
            [NSTextField(labelWithString: L("Export:")), scope],
            [NSTextField(labelWithString: L("Orientation:")), orientation],
            [NSGridCell.emptyContentView, fit],
            [NSGridCell.emptyContentView, gridlines],
        ])
        grid.rowSpacing = 8
        grid.column(at: 0).xPlacement = .trailing
        let box = NSView(frame: NSRect(origin: .zero, size: NSSize(width: grid.fittingSize.width + 40, height: grid.fittingSize.height + 24)))
        grid.frame.origin = NSPoint(x: 20, y: 12)
        grid.frame.size = grid.fittingSize
        box.addSubview(grid)

        let panel = NSSavePanel()
        panel.allowedContentTypes = [.pdf]
        panel.nameFieldStringValue = (doc.fileURL?.deletingPathExtension().lastPathComponent ?? doc.displayName ?? "XLS42") + ".pdf"
        panel.accessoryView = box
        let selection = clampedSelection
        let active = sheetIndex
        panel.beginSheetModal(for: window) { response in
            guard response == .OK, let url = panel.url else { return }
            options.scope = SheetExport.Scope(rawValue: scope.indexOfSelectedItem) ?? .sheet
            options.orientation = SheetExport.Orientation(rawValue: orientation.indexOfSelectedItem) ?? .auto
            options.fitWidth = fit.state == .on
            options.gridlines = gridlines.state == .on
            options.save()
            let data = SheetExport.pdf(doc.workbook, activeSheet: active, selection: selection, options: options, base: doc.printInfo)
            do { try data.write(to: url, options: .atomic) } catch { NSApp.presentError(error) }
        }
    }

    @objc func exportCSVAction(_ sender: Any?) {
        guard let window else { return }
        guard grid.commitEdit(move: nil) else { return }
        let delimiter = NSPopUpButton(frame: .zero, pullsDown: false)
        delimiter.addItems(withTitles: [L("Semicolon (;) — Excel with comma decimals"), L("Comma (,)"), L("Tab")])
        delimiter.selectItem(at: NumberFormat.decimal == "," ? 0 : 1)
        let row = NSStackView(views: [NSTextField(labelWithString: L("Delimiter:")), delimiter])
        row.edgeInsets = NSEdgeInsets(top: 12, left: 20, bottom: 12, right: 20)
        row.frame.size = row.fittingSize
        let panel = NSSavePanel()
        panel.allowedContentTypes = [.commaSeparatedText]
        panel.nameFieldStringValue = "\(sheet.name).csv"
        panel.accessoryView = row
        let current = sheet
        let date1904 = doc?.workbook.date1904 ?? false
        panel.beginSheetModal(for: window) { response in
            guard response == .OK, let url = panel.url else { return }
            let d: Character = [";", ",", "\t"][max(0, delimiter.indexOfSelectedItem)]
            do { try SheetExport.csv(current, delimiter: d, bom: true, date1904: date1904).write(to: url, options: .atomic) } catch { NSApp.presentError(error) }
        }
    }

    @objc func importCSVAction(_ sender: Any?) {
        guard let doc, let window else { return }
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.commaSeparatedText, .tabSeparatedText, .plainText]
        panel.message = L("The data is added as a new sheet")
        panel.beginSheetModal(for: window) { [self] response in
            guard response == .OK, let url = panel.url, let data = try? Data(contentsOf: url) else { return }
            var imported = XLSX.readCSV(data).sheets[0]
            var name = url.deletingPathExtension().lastPathComponent
            var n = 2
            while doc.workbook.sheetIndex(named: name) != nil { name = "\(url.deletingPathExtension().lastPathComponent) (\(n))"; n += 1 }
            imported.name = name
            doc.change(L("Import CSV")) { $0.sheets.append(imported) }
            sheetIndex = doc.workbook.sheets.count - 1
            workbookDidChange()
        }
    }

    // MARK: - View

    @objc func zoomInAction(_ sender: Any?) { grid.zoom = min(4, grid.zoom + 0.1) }
    @objc func zoomOutAction(_ sender: Any?) { grid.zoom = max(0.3, grid.zoom - 0.1) }
    @objc func actualSizeAction(_ sender: Any?) { grid.zoom = 1 }

    @objc func toggleFormatBarAction(_ sender: Any?) { formatStrip.isShown.toggle() }
    @objc func toggleFormulaBarAction(_ sender: Any?) { formulaBar.isHidden.toggle() }
    @objc func toggleAllPanelsAction(_ sender: Any?) {
        // The status bar hides with the other panels and comes back as it was.
        let wasHidden = statusBar.isHidden
        Panels.toggleAll(in: window) { [weak self] hidden in
            guard let self else { return }
            if hidden { self.statusBarHiddenBeforePanels = wasHidden; self.setStatusBarHidden(true) }
            else { self.setStatusBarHidden(self.statusBarHiddenBeforePanels ?? false); self.statusBarHiddenBeforePanels = nil }
        }
    }

    func window(_ window: NSWindow, willUseFullScreenPresentationOptions proposedOptions: NSApplication.PresentationOptions) -> NSApplication.PresentationOptions {
        Panels.fullScreenOptions   // bars slide in only at the top edge
    }


    @objc func goToCellAction(_ sender: Any?) { window?.makeFirstResponder(nameBox) }

    @objc func searchChanged(_ sender: NSSearchField) {
        let q = sender.stringValue
        guard !q.isEmpty else { return }
        let restart = q != searchText
        searchText = q
        findNext(from: restart ? CellRef(row: 0, col: -1) : grid.active)
    }

    @objc func findNextAction(_ sender: Any?) { findNext(from: grid.active) }
    @objc func focusSearchAction(_ sender: Any?) { toolbarController.searchItem?.beginSearchInteraction() }

    private func findNext(from: CellRef) {
        guard !searchText.isEmpty else { return }
        let matches = sheet.cells.filter { _, cell in
            NumberFormat.format(cell.value, cell.style.numberFormat, date1904: self.doc?.workbook.date1904 ?? false).text.localizedCaseInsensitiveContains(searchText)
                || cell.input.localizedCaseInsensitiveContains(searchText)
        }.keys.sorted()
        guard let next = matches.first(where: { $0 > from }) ?? matches.first else { NSSound.beep(); return }
        grid.select(next)
    }

    // MARK: - Formula bar & name box

    func controlTextDidBeginEditing(_ obj: Notification) {
        if (obj.object as? NSTextField) === formulaField, !grid.isEditing {
            // {=…} of an array formula is edited without the braces.
            let text = formulaField.stringValue
            if text.hasPrefix("{="), text.hasSuffix("}") {
                formulaField.stringValue = String(text.dropFirst().dropLast())
                formulaFieldEditor?.string = formulaField.stringValue
            }
            grid.beginEdit(initial: formulaField.stringValue)
            guard grid.isEditing else {
                // Refused (protected cell): give the text back.
                formulaField.abortEditing()
                formulaField.stringValue = sheet[grid.active].input
                window?.makeFirstResponder(grid)
                return
            }
            window?.makeFirstResponder(formulaField)
        }
    }

    func controlTextDidChange(_ obj: Notification) {
        if (obj.object as? NSTextField) === formulaField { grid.setEditorText(formulaField.stringValue) }
    }

    func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        if control === nameBox, selector == #selector(insertNewline(_:)) {
            let text = nameBox.stringValue.trimmingCharacters(in: .whitespaces)
            if let range = CellRange(text) {
                grid.select(range.start)
                if range.count > 1 { grid.select(range.end, extend: true) }
            } else { NSSound.beep() }
            window?.makeFirstResponder(grid)
            return true
        }
        if control === formulaField {
            switch selector {
            case #selector(insertNewline(_:)):
                grid.setEditorText(formulaField.stringValue)
                grid.commitEdit(move: (1, 0))
                return true
            case #selector(cancelOperation(_:)):
                grid.cancelEdit()
                return true
            default: break
            }
        }
        return false
    }

    // MARK: - Validation

    func validateToolbarItem(_ item: NSToolbarItem) -> Bool { true }

    func validateMenuItem(_ item: NSMenuItem) -> Bool {
        let style = sheet[grid.active].style
        switch item.action {
        case #selector(insertTechnicalElementAction(_:)): return !sheet.isProtected
        case #selector(editTechnicalElementAction(_:)): return !sheet.isProtected && selectedTechnicalElement != nil
        case #selector(toggleAllPanelsAction(_:)):
            item.title = Panels.allHidden(in: window) ? L("Show All Panels") : L("Hide All Panels")
        case #selector(toggleFormulaBarAction(_:)):
            item.title = formulaBar.isHidden ? L("Show Formula Bar") : L("Hide Formula Bar")
        case #selector(toggleBold(_:)): item.state = style.bold ? .on : .off
        case #selector(toggleItalic(_:)): item.state = style.italic ? .on : .off
        case #selector(toggleUnderline(_:)): item.state = style.underline ? .on : .off
        case #selector(toggleStrike(_:)): item.state = style.strike ? .on : .off
        case #selector(toggleWrap(_:)): item.state = style.wrap ? .on : .off
        case #selector(toggleGridlines(_:)): item.state = sheet.showGridlines ? .on : .off
        case #selector(toggleFormatBarAction(_:)):
            item.title = formatStrip.isShown ? L("Hide Format Bar") : L("Show Format Bar")
        case #selector(freezeAction(_:)):
            item.title = sheet.frozenRows > 0 || sheet.frozenCols > 0 ? L("Unfreeze Panes") : L("Freeze Panes")
        case #selector(freezeTopRowAction(_:)): item.state = sheet.frozenRows == 1 && sheet.frozenCols == 0 ? .on : .off
        case #selector(freezeFirstColumnAction(_:)): item.state = sheet.frozenCols == 1 && sheet.frozenRows == 0 ? .on : .off
        case #selector(alignAction(_:)):
            item.state = ["left", "center", "right"][max(0, min(2, item.tag))] == style.hAlign ? .on : .off
        case #selector(numberFormatAction(_:)):
            item.state = item.tag < Self.numberFormats.count && Self.numberFormats[item.tag].code == style.numberFormat ? .on : .off
        case #selector(toggleSplitAction(_:)): item.title = grid.split != nil ? L("Remove Split") : L("Split Window")
        case #selector(toggleStatusBarAction(_:)): item.title = statusBar.isHidden ? L("Show Status Bar") : L("Hide Status Bar")
        case #selector(hideSheetAction(_:)): return visibleSheets.count > 1 && !(doc?.workbook.structureProtected ?? false)
        case #selector(showSheetAction(_:)): return doc?.workbook.sheets.contains { $0.hidden } ?? false
        case #selector(protectWorkbookAction(_:)): item.state = doc?.workbook.structureProtected == true ? .on : .off
        case #selector(removeHyperlinkAction(_:)): return sheet.hyperlinks[grid.active] != nil
        case #selector(openHyperlinkAction(_:)): return hyperlinkTarget(at: grid.active) != nil
        case #selector(resetFilterAction(_:)): return !sheet.filterHidden.isEmpty || !sheet.filterValues.isEmpty || !sheet.customFilters.isEmpty
        case #selector(applyCellStyleAction(_:)):
            if let name = item.representedObject as? String { item.state = name == style.styleName ? .on : .off; break }
            item.state = item.tag < CellStyles.presets.count && CellStyles.presets[item.tag].name == (style.styleName ?? "Normal") ? .on : .off
        case #selector(fillUpAction(_:)), #selector(fillDown(_:)): return grid.selection.rows.count > 1
        case #selector(fillLeftAction(_:)): return grid.selection.cols.count > 1
        case #selector(fillRight(_:)): return grid.selection.cols.count > 1
        case #selector(protectSheetAction(_:)): item.title = sheet.isProtected ? L("Unprotect Sheet…") : L("Protect Sheet…")
        case #selector(toggleLockedAction(_:)): item.state = style.locked ? .on : .off
        case #selector(refreshPivotAction(_:)): return sheet.pivot != nil
        case #selector(clearValidationAction(_:)): return sheet.validations.contains { $0.ranges.contains { $0.intersects(grid.selection) } }
        case #selector(collapseAllAction(_:)), #selector(expandAllAction(_:)): return sheet.maxRowLevel > 0 || sheet.maxColLevel > 0
        default: break
        }
        return true
    }

    func windowWillReturnUndoManager(_ window: NSWindow) -> UndoManager? { doc?.undoManager }
}

extension SheetWindowController: PanelHiding {}
