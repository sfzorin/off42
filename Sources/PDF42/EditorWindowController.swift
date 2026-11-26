import AppKit
import Kit42
import PDFKit
import UniformTypeIdentifiers

final class EditorWindowController: NSWindowController, NSWindowDelegate, NSMenuItemValidation, NSToolbarItemValidation {
    let pdfView = EditorPDFView()
    private(set) lazy var sidebar = SidebarViewController(pdfView: pdfView)
    private let splitController = NSSplitViewController()
    let banner = BannerView()
    private let toolbarController = ToolbarController(identifier: "PDF42.toolbar", elements: Cmd.toolbar, optional: Cmd.toolbarOptional)
    private let colorButton = ColorButton(symbol: "paintpalette", toolTip: L("Colour of the tool or selected annotation"),
                                          color: .systemRed, allowsNone: false)
    private lazy var widthButton = StripButton(symbol: "lineweight", toolTip: L("Line width"), menu: widthMenu)
    private lazy var widthMenu: NSMenu = {
        let menu = NSMenu()
        for (i, w) in widths.enumerated() {
            let item = NSMenuItem(title: "\(Int(w)) pt", action: #selector(widthChosen(_:)), keyEquivalent: "")
            item.target = self
            item.tag = i
            item.state = i == 1 ? .on : .off
            menu.addItem(item)
        }
        return menu
    }()
    private lazy var markupStrip = ToolStrip([
        .group([.tool(Cmd.tool(.select))]),
        .tools([Cmd.tool(.highlight), Cmd.tool(.underline), Cmd.tool(.strikeOut)]),
        .tools([Cmd.tool(.ink), Cmd.tool(.rectangle), Cmd.tool(.oval), Cmd.tool(.whiteout)]),
        .group([.tool(Cmd.tool(.text)), .tool(Cmd.tool(.note)), .tool(Cmd.tool(.editText)), .tool(Cmd.tool(.redact)), .tool(Cmd.tool(.link))]),
        .tools([Cmd.tool(.textField), Cmd.tool(.checkBox), Cmd.tool(.choiceField)]),
        .group([.view(colorButton), .view(widthButton)]),
    ])

    private var colors: [Tool: NSColor] = Dictionary(uniqueKeysWithValues: Tool.allCases.map { ($0, $0.defaultColor) })
    private let widths: [CGFloat] = [1, 2, 3, 5, 8, 12]
    private var fontSize: CGFloat = 14
    private var warnedSignedEdit = false
    var ocrGeneration = UUID()

    /// Open "Compare With…" results for this document.
    var compareWindow: CompareWindowController?

    private var searchResults: [PDFSelection] = []
    private var searchIndex = -1
    private var lastSearch = ""

    var pdf: PDFDocument? { pdfView.document }
    var pdfDoc: PDFDoc? { document as? PDFDoc }
    var undo: UndoManager? { pdfDoc?.undoManager }

    init(pdf: PDFDocument) {
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1180, height: 840),
                              styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
                              backing: .buffered, defer: false)
        window.minSize = NSSize(width: 720, height: 480)
        window.toolbarStyle = .unified
        super.init(window: window)
        window.delegate = self
        shouldCascadeWindows = true

        setupContent()
        window.toolbar = toolbarController.toolbar
        toolbarController.searchItem?.visibilityPriority = .high
        toolbarController.item(for: Cmd.sign)?.visibilityPriority = .low
        toolbarController.item(for: Cmd.print)?.visibilityPriority = .low
        markupStrip.isShown = false
        window.addTitlebarAccessoryViewController(markupStrip)
        window.setContentSize(Self.windowSize(for: pdf))
        window.center()

        pdfView.document = pdf
        sidebar.editor = self
        sidebar.thumbnails.labelProvider = { [weak self] i in self?.pdfDoc?.extras.label(i) }
        sidebar.thumbnails.reload()   // the grid was built before the document was set
        sidebar.reloadOutline()
        sidebar.thumbnails.onMove = { [weak self] indices, target in self?.movePages(indices, to: target) }
        sidebar.thumbnails.onDropFiles = { [weak self] urls, target in self?.insertFiles(urls, at: target) }
        setTool(.select)
        updateSubtitle()

        NotificationCenter.default.addObserver(self, selector: #selector(pageChanged), name: .PDFViewPageChanged, object: pdfView)
        pdfView.postsFrameChangedNotifications = true
        NotificationCenter.default.addObserver(self, selector: #selector(pdfViewResized(_:)), name: NSView.frameDidChangeNotification, object: pdfView)
        NotificationCenter.default.addObserver(self, selector: #selector(pdfViewScaleChanged(_:)), name: .PDFViewScaleChanged, object: pdfView)
        DispatchQueue.main.async { [weak self] in self?.applyZoom() }
    }

    required init?(coder: NSCoder) { fatalError("not supported") }

    deinit { NotificationCenter.default.removeObserver(self) }

    /// Landscape documents get a wide window, portrait ones a tall one.
    static func windowSize(for pdf: PDFDocument) -> NSSize {
        let screen = NSScreen.main?.visibleFrame.size ?? NSSize(width: 1440, height: 900)
        let page = pdf.page(at: 0).map(displayedSize) ?? NSSize(width: 595, height: 842)
        let landscape = page.width > page.height
        let sidebar: CGFloat = 200, chrome: CGFloat = 90
        // The page fits whole at a comfortable size, the window fits the screen.
        let height = min(screen.height - 40, landscape ? 820 : 1000)
        let scale = (height - chrome - 40) / page.height
        let width = min(screen.width - 40, max(landscape ? 1100 : 900, page.width * scale + sidebar + 80))
        return NSSize(width: width, height: height)
    }

    /// Page size as shown: rotation applied.
    static func displayedSize(_ page: PDFPage) -> NSSize {
        let b = page.bounds(for: .cropBox).size
        return page.rotation % 180 == 0 ? b : NSSize(width: b.height, height: b.width)
    }

    // MARK: - Zoom modes

    enum ZoomMode { case page, width, custom }

    /// Whole page by default (portrait and landscape alike); re-applied when the window is resized.
    var zoomMode: ZoomMode = .page {
        didSet { applyZoom() }
    }

    private var applyingZoom = false

    func applyZoom() {
        guard !applyingZoom, zoomMode != .custom, let pdf,
              let page = pdfView.currentPage ?? pdf.page(at: 0), pdfView.bounds.width > 50, pdfView.bounds.height > 50 else { return }
        applyingZoom = true
        defer { applyingZoom = false }
        var size = Self.displayedSize(page)
        if pdfView.displayMode == .twoUp || pdfView.displayMode == .twoUpContinuous {
            let index = pdf.index(for: page)
            let first = pdfView.displaysAsBook ? (index == 0 ? 0 : 1 + (index - 1) / 2 * 2) : index / 2 * 2
            let other = index == first ? first + 1 : first
            if !(pdfView.displaysAsBook && index == 0), let adjacent = pdf.page(at: other) {
                let adjacentSize = Self.displayedSize(adjacent)
                size.width += adjacentSize.width + 8; size.height = max(size.height, adjacentSize.height)
            }
        }
        guard size.width.isFinite, size.height.isFinite, size.width > 0, size.height > 0 else { return }
        let widthScale = (pdfView.bounds.width - 32) / size.width
        let scale = zoomMode == .width ? widthScale : min(widthScale, (pdfView.bounds.height - 32) / size.height)
        pdfView.autoScales = false
        pdfView.scaleFactor = max(pdfView.minScaleFactor, min(pdfView.maxScaleFactor, scale))
        if zoomMode == .page { pdfView.go(to: page) }
    }

    @objc private func pdfViewResized(_ note: Notification) {
        if zoomMode != .custom { applyZoom() }
    }

    /// A pinch or a scale set elsewhere ends the automatic modes.
    @objc private func pdfViewScaleChanged(_ note: Notification) {
        if !applyingZoom, zoomMode != .custom { zoomMode = .custom }
    }

    private func setupContent() {
        pdfView.editor = self
        pdfView.autoScales = true
        pdfView.displayMode = .singlePageContinuous
        pdfView.displaysPageBreaks = true
        pdfView.pageShadowsEnabled = true
        pdfView.backgroundColor = .underPageBackgroundColor

        let content = NSViewController()
        let container = NSView()
        let stack = NSStackView(views: [banner, pdfView])
        stack.orientation = .vertical
        stack.spacing = 0
        stack.translatesAutoresizingMaskIntoConstraints = false
        container.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.topAnchor.constraint(equalTo: container.safeAreaLayoutGuide.topAnchor),
            stack.leadingAnchor.constraint(equalTo: container.leadingAnchor),
            stack.trailingAnchor.constraint(equalTo: container.trailingAnchor),
            stack.bottomAnchor.constraint(equalTo: container.bottomAnchor),
            banner.widthAnchor.constraint(equalTo: stack.widthAnchor),
            pdfView.widthAnchor.constraint(equalTo: stack.widthAnchor),
        ])
        content.view = container

        let sidebarItem = NSSplitViewItem(sidebarWithViewController: sidebar)
        sidebarItem.minimumThickness = 150
        sidebarItem.maximumThickness = 320
        sidebarItem.canCollapse = true
        let contentItem = NSSplitViewItem(viewController: content)
        splitController.addSplitViewItem(sidebarItem)
        splitController.addSplitViewItem(contentItem)
        splitController.splitView.autosaveName = "PDF42.split"
        window?.contentViewController = splitController

        colorButton.target = self
        colorButton.action = #selector(colorChanged(_:))
    }

    // MARK: - Document lifecycle

    func documentDidLoad() {
        DispatchQueue.main.async { [self] in
            unlockIfNeeded()
            updateSignatureBanner()
            if let undo {
                NotificationCenter.default.addObserver(self, selector: #selector(didEdit), name: .NSUndoManagerDidCloseUndoGroup, object: undo)
            }
            applyInitialView()
            extrasChanged()
        }
    }

    func replaceDocument(_ pdf: PDFDocument) {
        ocrGeneration = UUID()
        pdfView.cancelTextEdit()
        pdfView.cancelInteraction()
        compareWindow?.close(); compareWindow = nil
        pdfView.selectAnnotation(nil)
        pdfView.document = pdf
        sidebar.thumbnails.reload()
        sidebar.reloadOutline()
        sidebar.reloadAttachments()
        resetSearch()
        updateSubtitle()
        warnedSignedEdit = false
        updateSignatureBanner()
    }

    func prepareForSave() {
        pdfView.commitTextEdit()
        pdfView.cancelInteraction()
        pdfView.selectAnnotation(nil)
        window?.makeFirstResponder(pdfView)
    }

    func unlockIfNeeded() {
        guard let pdf, pdf.isLocked else { return }
        while pdf.isLocked {
            let field = NSSecureTextField()
            let alert = NSAlert.form(L("The document is password-protected"), info: L("Enter the password to open “%@”.", window?.title ?? ""),
                                     ok: L("Open"), rows: [(L("Password:"), field)])
            alert.window.initialFirstResponder = field
            guard alert.runModal() == .alertFirstButtonReturn else {
                pdfDoc?.close()
                return
            }
            pdf.unlock(withPassword: field.stringValue)
        }
        pdfView.layoutDocumentView()
        sidebar.reloadOutline()
        updateSubtitle()
    }

    func windowWillReturnUndoManager(_ window: NSWindow) -> UndoManager? { undo }

    @objc private func pageChanged(_ note: Notification) { updateSubtitle(); applyZoom() }

    func updateSubtitleNow() { updateSubtitle() }

    private func updateSubtitle() {
        guard let pdf, let page = pdfView.currentPage else {
            window?.subtitle = ""
            return
        }
        let index = pdf.index(for: page)
        var text = L("Page %d of %d", index + 1, pdf.pageCount)
        if let label = pdfDoc?.extras.label(index), label != "\(index + 1)" { text = L("Page %@ (%d of %d)", label, index + 1, pdf.pageCount) }
        if !lastSearch.isEmpty {
            text += searchResults.isEmpty ? L(" · not found") : L(" · %d of %d", searchIndex + 1, searchResults.count)
        }
        window?.subtitle = text
    }

    func updateSignatureBanner() {
        guard let signatures = pdfDoc?.signatures, !signatures.isEmpty else {
            banner.hide()
            return
        }
        let names = Array(Set(signatures.map(\.signer))).sorted().joined(separator: ", ")
        let details = L("Details")
        if signatures.contains(where: { $0.status == .invalid }) {
            banner.show(.error, L("Signature invalid — the document was changed after signing (%@)", names), action: details, handler: showSignatures)
        } else if signatures.contains(where: { $0.status == .notChecked }) {
            banner.show(.warning, L("Signed by %@ with a GOST signature. Install CryptoPro CSP to verify it", names), action: details, handler: showSignatures)
        } else if signatures.contains(where: { $0.powerOfAttorney.contains { $0.level == .error } }) {
            banner.show(.error, L("Signed by %@, but the power of attorney does not match the signature", names), action: details, handler: showSignatures)
        } else if signatures.contains(where: { $0.status == .untrusted }) {
            banner.show(.warning, L("Signed by %@. The signature is intact, but the certificate is not confirmed by a certification authority", names), action: details, handler: showSignatures)
        } else if signatures.last?.coversWholeFile == false {
            banner.show(.info, L("Signed by %@. The document was extended after signing", names), action: details, handler: showSignatures)
        } else {
            banner.show(.success, L("Signed by %@ · all signatures are valid", names), action: details, handler: showSignatures)
        }
    }

    @objc private func didEdit(_ note: Notification) {
        guard !warnedSignedEdit, pdfDoc?.signatures.isEmpty == false else { return }
        warnedSignedEdit = true
        banner.show(.warning, L("The document is signed: saving changes will invalidate the signatures. Use “Save As…”"),
                    action: L("Details"), handler: showSignatures)
    }

    private func showSignatures() {
        SignDialog.showResults(pdfDoc?.signatures ?? [], window: window)
    }

    // MARK: - Tools

    func setTool(_ tool: Tool) {
        pdfView.tool = tool
        markupStrip.select(tag: tool.rawValue)
        pdfView.color = colors[tool] ?? tool.defaultColor
        annotationSelectionChanged()
        if tool != .select, !markupStrip.isShown { markupStrip.isShown = true }
    }

    func annotationSelectionChanged() {
        let tool = pdfView.tool
        let annotation = pdfView.selectedAnnotation
        colorButton.color = annotation.map { $0.isKind(.freeText) ? ($0.fontColor ?? .black) : $0.color }
            ?? colors[tool] ?? tool.defaultColor
        colorButton.isEnabled = tool != .whiteout && !tool.isFormField && !tool.isAreaTool
            && !(annotation?.isKind(.stamp) == true && !(annotation is PageOverlay)) && annotation?.isWhiteout != true
        widthButton.isEnabled = annotation.map(\.hasEditableBorder)
            ?? [.ink, .rectangle, .oval].contains(tool)
        let width = annotation?.border?.lineWidth ?? pdfView.lineWidth
        widthMenu.items.forEach { $0.state = widths.indices.contains($0.tag) && widths[$0.tag] == width ? .on : .off }
    }

    @objc func selectToolAction(_ sender: Any?) {
        setTool(Tool(rawValue: commandTag(of: sender)) ?? .select)
    }

    @objc func toggleMarkupAction(_ sender: Any?) {
        markupStrip.isShown.toggle()
        if !markupStrip.isShown { setTool(.select) }
    }

    @objc private func colorChanged(_ sender: ColorButton) {
        let color = sender.color ?? .black
        if let annotation = pdfView.selectedAnnotation, let page = pdfView.selectedPage {
            recolor(annotation, to: color, on: page)
            return
        }
        colors[pdfView.tool] = color
        pdfView.color = color
    }

    @objc private func widthChosen(_ sender: NSMenuItem) {
        guard widths.indices.contains(sender.tag) else { return }
        let width = widths[sender.tag]
        if let annotation = pdfView.selectedAnnotation, let page = pdfView.selectedPage {
            guard annotation.hasEditableBorder else { return }
            setLineWidth(annotation, to: width, on: page)
        } else { pdfView.lineWidth = width }
        annotationSelectionChanged()
    }

    private func setLineWidth(_ annotation: PDFAnnotation, to width: CGFloat, on page: PDFPage) {
        let old = annotation.border?.copy() as? PDFBorder
        guard old?.lineWidth != width else { return }
        let border = old?.copy() as? PDFBorder ?? PDFBorder()
        border.lineWidth = width
        setBorder(annotation, border, on: page)
    }

    private func setBorder(_ annotation: PDFAnnotation, _ border: PDFBorder?, on page: PDFPage) {
        let old = annotation.border?.copy() as? PDFBorder
        annotation.border = border?.copy() as? PDFBorder
        pdfView.annotationsChanged(on: page)
        sidebar.thumbnails.refresh(page)
        annotationSelectionChanged()
        undo?.registerUndo(withTarget: self) { $0.setBorder(annotation, old, on: page) }
        undo?.setActionName(L("Line width"))
    }

    private var currentColor: NSColor { colors[pdfView.tool] ?? pdfView.tool.defaultColor }

    // MARK: - Annotation editing (undoable)

    private func stamp(_ annotation: PDFAnnotation) {
        annotation.userName = NSFullUserName()
        annotation.modificationDate = Date()
    }

    func add(_ annotation: PDFAnnotation, to page: PDFPage, name: String) {
        page.addAnnotation(annotation)
        pdfView.annotationsChanged(on: page)
        sidebar.thumbnails.refresh(page)
        undo?.registerUndo(withTarget: self) { $0.remove(annotation, from: page, name: name) }
        undo?.setActionName(name)
    }

    func remove(_ annotation: PDFAnnotation, from page: PDFPage, name: String) {
        if pdfView.selectedAnnotation === annotation { pdfView.selectAnnotation(nil) }
        let index = page.annotations.firstIndex { $0 === annotation } ?? page.annotations.count
        let popup = annotation.popup
        let popupIndex = popup.flatMap { p in page.annotations.firstIndex { $0 === p } }
        if let popup { page.removeAnnotation(popup) }
        page.removeAnnotation(annotation)
        pdfView.annotationsChanged(on: page)
        sidebar.thumbnails.refresh(page)
        undo?.registerUndo(withTarget: self) { $0.restoreAnnotation(annotation, at: index, popup: popup, popupIndex: popupIndex, on: page, name: name) }
        undo?.setActionName(name)
    }

    private func restoreAnnotation(_ annotation: PDFAnnotation, at index: Int, popup: PDFAnnotation?, popupIndex: Int?, on page: PDFPage, name: String) {
        var annotations = page.annotations
        let restored = [(index, annotation)] + (popup.flatMap { p in popupIndex.map { [($0, p)] } } ?? [])
        for (position, item) in restored.sorted(by: { $0.0 < $1.0 }) {
            annotations.insert(item, at: min(position, annotations.count))
        }
        page.annotations.forEach { page.removeAnnotation($0) }
        annotations.forEach { page.addAnnotation($0) }
        pdfView.annotationsChanged(on: page)
        sidebar.thumbnails.refresh(page)
        undo?.registerUndo(withTarget: self) { $0.remove(annotation, from: page, name: name) }
        undo?.setActionName(name)
    }

    func didChangeBounds(_ annotation: PDFAnnotation, from original: CGRect, on page: PDFPage, name: String) {
        let changed = annotation.bounds
        annotation.bounds = original
        setBounds(annotation, changed, on: page, name: name)
    }

    private func setBounds(_ annotation: PDFAnnotation, _ rect: CGRect, on page: PDFPage, name: String) {
        let old = annotation.bounds
        annotation.bounds = rect
        pdfView.annotationsChanged(on: page)
        pdfView.refreshMarker()
        sidebar.thumbnails.refresh(page)
        undo?.registerUndo(withTarget: self) { $0.setBounds(annotation, old, on: page, name: name) }
        undo?.setActionName(name)
    }

    private func recolor(_ annotation: PDFAnnotation, to color: NSColor, on page: PDFPage) {
        setAnnotationColor(annotation, to: annotation.isKind(.highlight) ? color.withAlphaComponent(0.45) : color, on: page)
    }

    private func setAnnotationColor(_ annotation: PDFAnnotation, to color: NSColor, on page: PDFPage) {
        let isText = annotation.isKind(.freeText)
        let old = (isText ? annotation.fontColor : annotation.color) ?? .black
        guard old != color else { return }
        if isText { annotation.fontColor = color } else { annotation.color = color }
        pdfView.annotationsChanged(on: page)
        sidebar.thumbnails.refresh(page)
        annotationSelectionChanged()
        undo?.registerUndo(withTarget: self) { $0.setAnnotationColor(annotation, to: old, on: page) }
        undo?.setActionName(L("Colour"))
    }

    func addMarkup(for selection: PDFSelection, tool: Tool) {
        let subtype: PDFAnnotationSubtype = tool == .highlight ? .highlight : tool == .underline ? .underline : .strikeOut
        let color = colors[tool] ?? tool.defaultColor
        for line in selection.selectionsByLine() {
            for page in line.pages {
                let b = line.bounds(for: page)
                guard b.width > 0.5, b.height > 0.5 else { continue }
                let annotation = PDFAnnotation(bounds: b, forType: subtype, withProperties: nil)
                annotation.color = tool == .highlight ? color.withAlphaComponent(0.45) : color
                annotation.quadrilateralPoints = [
                    NSValue(point: NSPoint(x: 0, y: b.height)), NSValue(point: NSPoint(x: b.width, y: b.height)),
                    NSValue(point: NSPoint(x: 0, y: 0)), NSValue(point: NSPoint(x: b.width, y: 0)),
                ]
                stamp(annotation)
                add(annotation, to: page, name: tool.title)
            }
        }
    }

    /// Font for text boxes (not the system font, which extracts Cyrillic incorrectly from PDFs).
    static func textFont(_ size: CGFloat) -> NSFont { NSFont(name: "Helvetica Neue", size: size) ?? .systemFont(ofSize: size) }

    private func textSize(_ text: String, font: NSFont) -> CGSize {
        let r = (text as NSString).boundingRect(with: CGSize(width: 460, height: CGFloat.greatestFiniteMagnitude),
                                                options: [.usesLineFragmentOrigin, .usesFontLeading],
                                                attributes: [.font: font])
        return CGSize(width: ceil(r.width) + 14, height: ceil(r.height) + 8)
    }

    private func makeTextBox(_ text: String, font: NSFont, color: NSColor, topLeft: CGPoint) -> PDFAnnotation {
        let s = textSize(text, font: font)
        let annotation = PDFAnnotation(bounds: CGRect(x: topLeft.x, y: topLeft.y - s.height, width: s.width, height: s.height),
                                       forType: .freeText, withProperties: nil)
        annotation.contents = text
        annotation.font = font
        annotation.fontColor = color
        annotation.color = .clear
        let border = PDFBorder()
        border.lineWidth = 0
        annotation.border = border
        stamp(annotation)
        return annotation
    }

    func createTextBox(on page: PDFPage, at point: CGPoint) {
        guard let (text, size) = promptText(title: L("New Text Box"), initial: "", fontSize: fontSize) else { return }
        fontSize = size
        let annotation = makeTextBox(text, font: Self.textFont(size), color: currentColor, topLeft: point)
        add(annotation, to: page, name: L("Text Box"))
        setTool(.select)
        pdfView.selectAnnotation(annotation, on: page)
    }

    func createNote(on page: PDFPage, at point: CGPoint) {
        guard let (text, _) = promptText(title: L("New Note"), initial: "", fontSize: nil) else { return }
        let annotation = PDFAnnotation(bounds: CGRect(x: point.x - 10, y: point.y - 10, width: 22, height: 22), forType: .text, withProperties: nil)
        annotation.contents = text
        annotation.color = currentColor
        annotation.iconType = .comment
        stamp(annotation)
        add(annotation, to: page, name: L("Note"))
    }

    func editAnnotationText(_ annotation: PDFAnnotation, on page: PDFPage) {
        if annotation.technicalElement != nil { editTechnicalElement(annotation, on: page); return }
        if let edited = annotation as? EditedTextOverlay { editPageText(edited, on: page); return }
        guard annotation.isKind(.freeText) || annotation.isKind(.text) else { return }
        let isText = annotation.isKind(.freeText)
        let currentFont = annotation.font ?? Self.textFont(fontSize)
        guard let (text, size) = promptText(title: isText ? L("Edit Text") : L("Note"), initial: annotation.contents ?? "",
                                            fontSize: isText ? currentFont.pointSize : nil) else { return }
        guard text != (annotation.contents ?? "") || (isText && size != currentFont.pointSize) else { return }
        var bounds = annotation.bounds
        var font: NSFont? = annotation.font
        if isText {
            let f = NSFont(descriptor: currentFont.fontDescriptor, size: size) ?? Self.textFont(size)
            font = f
            let s = textSize(text, font: f)
            bounds = CGRect(x: bounds.minX, y: bounds.maxY - s.height, width: s.width, height: s.height)
        }
        setText(annotation, text: text, font: font, bounds: bounds, on: page)
    }

    private func setText(_ annotation: PDFAnnotation, text: String, font: NSFont?, bounds: CGRect, on page: PDFPage) {
        let old = (annotation.contents ?? "", annotation.font, annotation.bounds)
        annotation.contents = text
        if let font { annotation.font = font }
        annotation.bounds = bounds
        stamp(annotation)
        pdfView.annotationsChanged(on: page)
        pdfView.refreshMarker()
        undo?.registerUndo(withTarget: self) { $0.setText(annotation, text: old.0, font: old.1, bounds: old.2, on: page) }
        undo?.setActionName(L("Edit Text"))
    }

    /// Replaces selected page text: covers it with white and places an editable text box on top,
    /// matching the original size and colour.
    @objc func replaceSelectedTextAction(_ sender: Any?) {
        guard let selection = pdfView.currentSelection, let original = selection.string,
              !original.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            // Nothing selected: switch to the Edit Text tool (click a line of text to edit it).
            setTool(.editText)
            banner.show(.info, L("Click a line of text to edit it. ⌥-click edits one word. Return applies, Esc cancels."))
            DispatchQueue.main.asyncAfter(deadline: .now() + 5) { [weak self] in self?.updateSignatureBanner() }
            return
        }
        // One line: edit it in place, in the original font.
        if selection.selectionsByLine().count == 1, selection.pages.count == 1, let page = selection.pages.first {
            pdfView.clearSelection()
            editPageText(selection, on: page)
            return
        }
        let attributes = selection.attributedString?.attributes(at: 0, effectiveRange: nil) ?? [:]
        let sourceFont = attributes[.font] as? NSFont
        let sourceColor = (attributes[.foregroundColor] as? NSColor) ?? .black
        let lines = selection.selectionsByLine()
        guard let firstLine = lines.first, let firstPage = firstLine.pages.first,
              let (text, size) = promptText(title: L("Edit Text"), initial: original, fontSize: sourceFont?.pointSize ?? 12)
        else { return }

        for line in lines {
            for page in line.pages {
                let cover = PDFAnnotation(bounds: line.bounds(for: page).insetBy(dx: -1, dy: -1), forType: .square, withProperties: nil)
                let border = PDFBorder()
                border.lineWidth = 0
                cover.border = border
                cover.color = .white
                cover.interiorColor = .white
                stamp(cover)
                add(cover, to: page, name: L("Edit Text"))
            }
        }
        let font = sourceFont.flatMap { NSFont(descriptor: $0.fontDescriptor, size: size) } ?? Self.textFont(size)
        let b = firstLine.bounds(for: firstPage)
        let box = makeTextBox(text, font: font, color: sourceColor, topLeft: CGPoint(x: b.minX - 4, y: b.maxY + 3))
        add(box, to: firstPage, name: L("Edit Text"))
        pdfView.clearSelection()
        setTool(.select)
        pdfView.selectAnnotation(box, on: firstPage)
    }

    @objc func highlightSelectionAction(_ sender: Any?) {
        guard let selection = pdfView.currentSelection, !(selection.string ?? "").isEmpty else { NSSound.beep(); return }
        addMarkup(for: selection, tool: .highlight)
        pdfView.clearSelection()
    }

    @objc func deleteSelectedAnnotation(_ sender: Any?) {
        guard let annotation = pdfView.selectedAnnotation, let page = pdfView.selectedPage else { return }
        remove(annotation, from: page, name: L("Delete Annotation"))
    }

    // MARK: - Signatures & stamps (images)

    @objc func showStampsAction(_ sender: Any?) {
        let popover = NSPopover()
        let vc = StampLibraryViewController { [weak self] image in self?.insertImage(image) }
        // The resolution dialog: the scan's own resolution on a scanned page, the maximum on a vector page.
        vc.placement = { [weak self] aspect in
            guard let self, let page = self.pdfView.currentPage else { return MarkPlacement.standard(aspect: aspect) }
            return MarkPlacement(widthPoints: Self.markWidth(aspect: aspect, on: page), scanDPI: ScanResolution.dpi(of: page))
        }
        vc.popover = popover
        popover.contentViewController = vc
        popover.behavior = .transient
        if let item = toolbarController.item(for: Cmd.stamps), window?.toolbar?.isVisible == true {
            popover.show(relativeTo: item)
        } else {
            popover.show(relativeTo: NSRect(x: pdfView.bounds.midX, y: pdfView.bounds.maxY - 20, width: 1, height: 1), of: pdfView, preferredEdge: .minY)
        }
    }

    @objc func insertImageAction(_ sender: Any?) {
        guard let image = StampLibrary.pickImage(window: window) else { return }
        insertImage(image)
    }

    /// Width of a newly placed signature or stamp (points): 6 cm, smaller on small pages.
    static func markWidth(aspect: CGFloat, on page: PDFPage) -> CGFloat {
        let box = page.bounds(for: .cropBox)
        return min(170, box.width * 0.4, box.height * 0.6 * aspect)
    }

    /// Places an image (signature, seal) in the middle of the visible part of the current page.
    func insertImage(_ image: CGImage) {
        guard let page = pdfView.currentPage else { return }
        let pageBox = page.bounds(for: .cropBox)
        let center = pdfView.convert(CGPoint(x: pdfView.bounds.midX, y: pdfView.bounds.midY), to: page)
        let aspect = CGFloat(image.width) / max(1, CGFloat(image.height))
        let width = Self.markWidth(aspect: aspect, on: page)
        let size = CGSize(width: width, height: width / aspect)
        var origin = CGPoint(x: center.x - size.width / 2, y: center.y - size.height / 2)
        origin.x = min(max(origin.x, pageBox.minX), pageBox.maxX - size.width)
        origin.y = min(max(origin.y, pageBox.minY), pageBox.maxY - size.height)
        let annotation = ImageStampAnnotation(image: image, bounds: CGRect(origin: origin, size: size))
        stamp(annotation)
        add(annotation, to: page, name: L("Signature or Stamp"))
        setTool(.select)
        pdfView.selectAnnotation(annotation, on: page)
    }

    @objc func signDocumentAction(_ sender: Any?) {
        guard let doc = pdfDoc, let pdf else { return }
        let alreadySigned = !doc.signatures.isEmpty
        if alreadySigned && doc.isDocumentEdited {
            let alert = NSAlert()
            alert.messageText = L("The document is already signed and has been changed")
            alert.informativeText = L("Signing the changed version will invalidate the existing signatures. To add a second signature, undo your changes first.")
            alert.addButton(withTitle: L("Sign Anyway"))
            alert.addButton(withTitle: L("Cancel"))
            guard alert.runModal() == .alertFirstButtonReturn else { return }
        }
        let stamp = pdfView.selectedAnnotation as? ImageStampAnnotation
        let stampPage = pdfView.selectedPage
        guard let (chosen, visible, chosenAttachments) = SignDialog.run(stampSelected: stamp != nil) else { return }
        var options = chosen

        prepareForSave()
        // A selected signature image becomes the visible signature (removed from the base copy).
        var removedStamp: (PDFAnnotation, PDFPage)?
        var stampOrder: [PDFAnnotation]?
        if visible, let page = stamp != nil ? stampPage : pdfView.currentPage {
            let rect: CGRect
            if let stamp { rect = stamp.bounds } else {
                let box = page.bounds(for: .cropBox)
                rect = CGRect(x: box.maxX - 36 - 220, y: box.minY + 36, width: 220, height: 72)
            }
            guard let appearance = Self.signatureAppearance(size: rect.size, image: stamp?.image, name: options.signerName,
                                                             reason: options.reason, date: Date()) else {
                presentError(NSError(domain: NSCocoaErrorDomain, code: NSFileWriteUnknownError,
                                     userInfo: [NSLocalizedDescriptionKey: L("The image size is invalid")]))
                return
            }
            options.visible = (pdf.index(for: page), rect, appearance)
            if let stamp, !(alreadySigned && !doc.isDocumentEdited) {
                stampOrder = page.annotations
                page.removeAnnotation(stamp)
                removedStamp = (stamp, page)
            }
        }
        let unsignedBase: Data
        do {
            // Restore the live stamp even when serialization fails, including its original stacking order.
            defer {
                if let (a, p) = removedStamp {
                    p.addAnnotation(a)
                    if let annotations = stampOrder {
                        p.annotations.forEach { p.removeAnnotation($0) }
                        annotations.forEach { p.addAnnotation($0) }
                    }
                }
            }
            if alreadySigned, !doc.isDocumentEdited, let original = doc.originalData { unsignedBase = original }
            else { unsignedBase = try doc.data(ofType: "com.adobe.pdf") }
        } catch { presentError(error); return }

        // Embed the power of attorney before signing so the signature covers its bytes.
        let base: Data
        do { base = try PDFSigner.embed(chosenAttachments, into: unsignedBase) } catch { presentError(error); return }

        var signed: PAdES.Result
        do { signed = try PAdES.sign(base, options: options) } catch {
            // Offline or TSA problem: offer to sign without the timestamp.
            guard options.timestampURL != nil, error is Timestamp.TSAError || error is URLError else { presentError(error); return }
            let ask = NSAlert()
            ask.messageText = L("Could not get a trusted timestamp")
            ask.informativeText = error.localizedDescription
            ask.addButton(withTitle: L("Sign Without Timestamp"))
            ask.addButton(withTitle: L("Cancel"))
            guard ask.runModal() == .alertFirstButtonReturn else { return }
            options.timestampURL = nil
            do { signed = try PAdES.sign(base, options: options) } catch { presentError(error); return }
        }
        saveSignedCopy(signed, suffix: alreadySigned ? nil : L("signed"))
    }

    /// Save panel for a signed/extended copy; warnings (missing revocation data) are shown first.
    private func saveSignedCopy(_ result: PAdES.Result, suffix: String?) {
        guard let doc = pdfDoc else { return }
        let signed = result.data
        if !result.warnings.isEmpty {
            let alert = NSAlert()
            alert.messageText = L("Validation data is incomplete")
            alert.informativeText = Array(Set(result.warnings)).sorted().joined(separator: "\n")
            alert.runModal()
        }
        let panel = NSSavePanel()
        panel.allowedContentTypes = [.pdf]
        let baseName = doc.fileURL?.deletingPathExtension().lastPathComponent ?? doc.displayName ?? L("Document")
        panel.nameFieldStringValue = suffix.map { "\(baseName) (\($0)).pdf" } ?? "\(baseName).pdf"
        panel.directoryURL = doc.fileURL?.deletingLastPathComponent()
        panel.message = L("Save the signed document")
        panel.beginSheetModal(for: window!) { response in
            guard response == .OK, let url = panel.url else { return }
            do {
                try signed.write(to: url, options: .atomic)
                if url == doc.fileURL {
                    try doc.revert(toContentsOf: url, ofType: doc.fileType ?? "com.adobe.pdf")
                } else {
                    NSDocumentController.shared.openDocument(withContentsOf: url, display: true) { _, _, _ in }
                }
            } catch { self.presentError(error) }
        }
    }

    @objc func verifySignaturesAction(_ sender: Any?) { showSignatures() }

    /// Adds the DSS (certificates, OCSP responses, CRLs) for the existing signatures: PAdES B-LT.
    @objc func enableLTVAction(_ sender: Any?) {
        guard let doc = pdfDoc, !doc.signatures.isEmpty, let original = doc.originalData else { return }
        guard !doc.isDocumentEdited else {
            presentError(PDFStructureError.unsupported(L("undo your changes first: validation data is added to the signed version")))
            return
        }
        let result: PAdES.Result
        do { result = try PAdES.addValidationData(original) } catch { presentError(error); return }
        saveSignedCopy(result, suffix: nil)
    }

    /// RFC 3161 document timestamp over the whole file (B-LTA for signed files).
    @objc func documentTimestampAction(_ sender: Any?) {
        guard let doc = pdfDoc else { return }
        let signed = !doc.signatures.isEmpty
        if signed && doc.isDocumentEdited {
            presentError(PDFStructureError.unsupported(L("undo your changes first: the timestamp is added to the signed version")))
            return
        }
        let d = UserDefaults.standard
        let tsaString = (d.string(forKey: "sign.tsaURL") ?? "http://timestamp.digicert.com").trimmingCharacters(in: .whitespacesAndNewlines)
        guard let tsa = URL(string: tsaString), ["http", "https"].contains(tsa.scheme?.lowercased() ?? ""),
              tsa.host?.isEmpty == false else {
            presentError(PDFStructureError.unsupported(L("Enter a valid HTTP or HTTPS timestamp URL.")))
            return
        }
        prepareForSave()
        let base: Data
        do {
            if signed, let original = doc.originalData { base = original }
            else { base = try doc.data(ofType: "com.adobe.pdf") }
        } catch { presentError(error); return }
        do {
            var result = PAdES.Result(data: try PAdES.addDocumentTimestamp(base, tsa: tsa))
            if signed { result = try PAdES.addValidationData(result.data) }
            saveSignedCopy(result, suffix: signed ? nil : L("timestamped"))
        } catch { presentError(error) }
    }

    /// Appearance of a visible signature: optional signature image, name, reason and date (3× resolution).
    static func signatureAppearance(size: CGSize, image: CGImage?, name: String, reason: String, date: Date) -> CGImage? {
        let scale: CGFloat = 3
        guard size.width.isFinite, size.height.isFinite, size.width >= 1, size.height >= 1,
              size.width * scale <= 16384, size.height * scale <= 16384,
              size.width * size.height * scale * scale <= 64_000_000 else { return nil }
        let w = Int(size.width * scale), h = Int(size.height * scale)
        guard let ctx = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0, space: CGColorSpaceCreateDeviceRGB(),
                                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return nil }
        ctx.scaleBy(x: scale, y: scale)
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(cgContext: ctx, flipped: false)
        let rect = CGRect(origin: .zero, size: size)
        NSColor(srgbRed: 0.1, green: 0.3, blue: 0.7, alpha: 0.8).setStroke()
        let frame = NSBezierPath(roundedRect: rect.insetBy(dx: 1, dy: 1), xRadius: 4, yRadius: 4)
        frame.lineWidth = 1
        frame.stroke()
        var textRect = rect.insetBy(dx: 6, dy: 4)
        if let image {
            let side = min(rect.height - 8, rect.width * 0.45)
            let aspect = CGFloat(image.width) / max(1, CGFloat(image.height))
            let imgRect = CGRect(x: 6, y: (rect.height - side / aspect.clamped(0.2, 5)) / 2, width: side, height: side / aspect.clamped(0.2, 5))
            ctx.draw(image, in: imgRect)
            textRect.origin.x = imgRect.maxX + 6
            textRect.size.width = rect.width - imgRect.maxX - 12
        }
        let font = NSFont(name: "Helvetica Neue", size: min(9, size.height / 6)) ?? .systemFont(ofSize: 8)
        let bold = NSFont(name: "Helvetica Neue Bold", size: font.pointSize + 1) ?? font
        let blue = NSColor(srgbRed: 0.1, green: 0.25, blue: 0.55, alpha: 1)
        let text = NSMutableAttributedString(string: L("Digitally signed by") + "\n", attributes: [.font: font, .foregroundColor: blue])
        text.append(NSAttributedString(string: name + "\n", attributes: [.font: bold, .foregroundColor: blue]))
        text.append(NSAttributedString(string: date.formatted(date: .numeric, time: .shortened), attributes: [.font: font, .foregroundColor: blue]))
        if !reason.isEmpty { text.append(NSAttributedString(string: "\n" + reason, attributes: [.font: font, .foregroundColor: blue])) }
        let bounds = text.boundingRect(with: textRect.size, options: [.usesLineFragmentOrigin])
        text.draw(with: CGRect(x: textRect.minX, y: textRect.midY - bounds.height / 2, width: textRect.width, height: bounds.height),
                  options: [.usesLineFragmentOrigin, .truncatesLastVisibleLine])
        NSGraphicsContext.restoreGraphicsState()
        return ctx.makeImage()
    }

    // MARK: - Export

    @objc func exportWithPasswordAction(_ sender: Any?) {
        guard pdf != nil, let window else { return }
        let p1 = NSSecureTextField(), p2 = NSSecureTextField()
        let alert = NSAlert.form(L("Export with Password"), info: L("The copy will open only with the password."),
                                 ok: L("Continue…"), rows: [(L("Password:"), p1), (L("Repeat:"), p2)])
        alert.window.initialFirstResponder = p1
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        guard !p1.stringValue.isEmpty, p1.stringValue == p2.stringValue else {
            let e = NSAlert()
            e.messageText = L("Passwords do not match")
            e.beginSheetModal(for: window)
            return
        }
        let panel = NSSavePanel()
        panel.allowedContentTypes = [.pdf]
        panel.nameFieldStringValue = L("%@ (protected).pdf", pdfDoc?.displayName ?? L("Document"))
        panel.beginSheetModal(for: window) { [self] response in
            guard response == .OK, let url = panel.url else { return }
            prepareForSave()
            do { try PDFOutput.protectedData(exportSnapshot(), password: p1.stringValue).write(to: url, options: .atomic) }
            catch { presentError(error) }
        }
    }

    @objc func exportPNGAction(_ sender: Any?) {
        guard pdf != nil, let window else { return }
        let format = NSPopUpButton(frame: .zero, pullsDown: false)
        format.addItems(withTitles: ["PNG", "JPEG"])
        let dpi = NSPopUpButton(frame: .zero, pullsDown: false)
        dpi.addItems(withTitles: ["72 dpi", "150 dpi", "200 dpi", "300 dpi"])
        dpi.selectItem(at: 2)
        let row = NSStackView(views: [NSTextField(labelWithString: L("Format:")), format, NSTextField(labelWithString: L("Resolution:")), dpi])
        row.edgeInsets = NSEdgeInsets(top: 12, left: 20, bottom: 12, right: 20)
        row.frame.size = row.fittingSize
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = true
        panel.prompt = L("Export")
        panel.message = L("Folder for the images (pages selected in thumbnails, or the current page)")
        panel.accessoryView = row
        panel.isAccessoryViewDisclosed = true
        let indices = targetIndices
        panel.beginSheetModal(for: window) { [self] response in
            guard response == .OK, let dir = panel.url else { return }
            prepareForSave()
            let name = pdfDoc?.displayName.map { ($0 as NSString).deletingPathExtension } ?? L("Page")
            let resolution = [72.0, 150, 200, 300][min(3, max(0, dpi.indexOfSelectedItem))]
            let jpeg = format.indexOfSelectedItem == 1
            do {
                let snapshot = try exportSnapshot()
                for i in indices {
                    guard let page = snapshot.page(at: i) else { continue }
                    let data = try PDFOutput.imageData(page, dpi: resolution, jpeg: jpeg)
                    try data.write(to: dir.appendingPathComponent("\(name) — \(i + 1).\(jpeg ? "jpg" : "png")"), options: .atomic)
                }
            } catch { presentError(error); return }
            NSWorkspace.shared.activateFileViewerSelecting([dir])
        }
    }

    static let exportFormats: [(title: String, ext: String)] = [
        ("Word (.docx)", "docx"), ("Rich Text (.rtf)", "rtf"), (L("Plain Text"), "txt"), ("HTML", "html"), ("PDF/A-2b", "pdf"),
    ]

    /// PDF text with fonts, one page per section (page breaks between pages).
    var pagesAsText: NSAttributedString { pdf.map(PDFDoc.attributedText) ?? NSAttributedString() }

    @objc func exportAsAction(_ sender: Any?) {
        guard let window else { return }
        let formats = Self.exportFormats
        let popup = NSPopUpButton(frame: .zero, pullsDown: false)
        popup.addItems(withTitles: formats.map(\.title))
        let row = NSStackView(views: [NSTextField(labelWithString: L("Format:")), popup])
        row.edgeInsets = NSEdgeInsets(top: 12, left: 20, bottom: 12, right: 20)
        row.frame.size = row.fittingSize
        let panel = NSSavePanel()
        panel.accessoryView = row
        let base = pdfDoc?.fileURL?.deletingPathExtension().lastPathComponent ?? pdfDoc?.displayName ?? L("Document")
        let observer = ClosureTarget { [weak panel] in
            let f = formats[max(0, popup.indexOfSelectedItem)]
            panel?.allowedContentTypes = [UTType(filenameExtension: f.ext) ?? .data]
            panel?.nameFieldStringValue = base + "." + f.ext
        }
        observer.fire()
        popup.target = observer
        popup.action = #selector(ClosureTarget.fire)
        panel.beginSheetModal(for: window) { [self] response in
            _ = observer
            guard response == .OK, let url = panel.url else { return }
            prepareForSave()
            if formats[max(0, popup.indexOfSelectedItem)].title == "PDF/A-2b" { exportPDFA(to: url); return }
            do { try exportTextData(format: formats[max(0, popup.indexOfSelectedItem)].ext).write(to: url, options: .atomic) }
            catch { presentError(error) }
        }
    }

    // MARK: - Dialogs

    private func promptText(title: String, initial: String, fontSize: CGFloat?) -> (String, CGFloat)? {
        let scroll = NSTextView.scrollableTextView()
        scroll.borderType = .bezelBorder
        let textView = scroll.documentView as! NSTextView
        textView.string = initial
        textView.font = .systemFont(ofSize: 13)
        textView.isRichText = false
        textView.textContainerInset = NSSize(width: 4, height: 4)
        scroll.translatesAutoresizingMaskIntoConstraints = false
        scroll.widthAnchor.constraint(equalToConstant: 360).isActive = true
        scroll.heightAnchor.constraint(equalToConstant: 120).isActive = true

        let sizes: [CGFloat] = [6, 7, 8, 9, 10, 11, 12, 13, 14, 16, 18, 20, 24, 28, 36, 48, 72]
        let sizePopup = NSPopUpButton(frame: .zero, pullsDown: false)
        var initialSizeIndex = -1
        let stack = NSStackView(views: [scroll])
        stack.orientation = .vertical
        stack.alignment = .leading
        if let fontSize {
            sizePopup.addItems(withTitles: sizes.map { "\(Int($0)) pt" })
            let nearest = sizes.enumerated().min { abs($0.element - fontSize) < abs($1.element - fontSize) }?.offset ?? 8
            sizePopup.selectItem(at: nearest)
            initialSizeIndex = nearest
            stack.addArrangedSubview(NSStackView(views: [NSTextField(labelWithString: L("Font size:")), sizePopup]))
        }
        stack.frame.size = stack.fittingSize

        let alert = NSAlert()
        alert.messageText = title
        alert.addButton(withTitle: L("Done"))
        alert.addButton(withTitle: L("Cancel"))
        alert.accessoryView = stack
        alert.layout()
        alert.window.initialFirstResponder = textView
        guard alert.runModal() == .alertFirstButtonReturn else { return nil }
        let text = textView.string
        guard !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return nil }
        let chosenSize = sizePopup.indexOfSelectedItem == initialSizeIndex ? (fontSize ?? 0) : sizes[max(0, sizePopup.indexOfSelectedItem)]
        return (text, fontSize == nil ? 0 : chosenSize)
    }

    // MARK: - View

    @objc func showThumbnailsAction(_ sender: Any?) { showSidebarMode(.thumbnails) }
    @objc func showOutlineAction(_ sender: Any?) { showSidebarMode(.outline) }

    func showSidebarMode(_ mode: SidebarViewController.Mode) {
        sidebar.mode = mode
        if splitController.splitViewItems.first?.isCollapsed == true { splitController.toggleSidebar(nil) }
    }

    @objc func zoomInAction(_ sender: Any?) { zoomMode = .custom; pdfView.zoomIn(nil) }
    @objc func zoomOutAction(_ sender: Any?) { zoomMode = .custom; pdfView.zoomOut(nil) }
    @objc func zoomToFitAction(_ sender: Any?) { zoomMode = .page }
    @objc func fitWidthAction(_ sender: Any?) { zoomMode = .width }
    @objc func actualSizeAction(_ sender: Any?) { zoomMode = .custom; pdfView.autoScales = false; pdfView.scaleFactor = 1 }

    @objc func setDisplayModeAction(_ sender: Any?) {
        pdfView.displayMode = PDFDisplayMode(rawValue: commandTag(of: sender)) ?? .singlePageContinuous
        applyZoom()
    }

    @objc func nextPageAction(_ sender: Any?) { pdfView.goToNextPage(nil) }
    @objc func previousPageAction(_ sender: Any?) { pdfView.goToPreviousPage(nil) }

    @objc func goToPageAction(_ sender: Any?) {
        guard let pdf else { return }
        let field = NSTextField()
        let labels = pdfDoc?.extras.labels ?? []
        field.placeholderString = labels.isEmpty ? "1–\(pdf.pageCount)"
            : "\(PageLabels.label(0, labels) ?? "1")–\(PageLabels.label(pdf.pageCount - 1, labels) ?? "\(pdf.pageCount)")"
        let alert = NSAlert.form(L("Go to Page"), info: labels.isEmpty ? nil : L("Enter a page label (as printed on the page) or a page number."),
                                 ok: L("Go"), rows: [(L("Page:"), field)])
        alert.window.initialFirstResponder = field
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        let index = PageLabels.index(of: field.stringValue, labels, pageCount: pdf.pageCount)
            ?? Int(field.stringValue).map { min(max($0, 1), pdf.pageCount) - 1 }
        guard let index, let page = pdf.page(at: index) else { NSSound.beep(); return }
        pdfView.go(to: page)
    }

    // MARK: - Search

    @objc func searchChanged(_ sender: NSSearchField) {
        let query = sender.stringValue
        if query != lastSearch {
            lastSearch = query
            searchResults = query.isEmpty ? [] : (pdf?.findString(query, withOptions: [.caseInsensitive, .diacriticInsensitive]) ?? [])
            searchResults.forEach { $0.color = .systemYellow }
            pdfView.highlightedSelections = searchResults.isEmpty ? nil : searchResults
            searchIndex = -1
        }
        showSearchResult(step: 1)
    }

    private func showSearchResult(step: Int) {
        guard !searchResults.isEmpty else {
            updateSubtitle()
            return
        }
        searchIndex = (searchIndex + step + searchResults.count) % searchResults.count
        let selection = searchResults[searchIndex]
        pdfView.setCurrentSelection(selection, animate: true)
        pdfView.go(to: selection)
        updateSubtitle()
    }

    private func resetSearch() {
        lastSearch = ""
        searchResults = []
        searchIndex = -1
        pdfView.highlightedSelections = nil
        toolbarController.searchItem?.searchField.stringValue = ""
    }

    @objc func focusSearchAction(_ sender: Any?) { toolbarController.searchItem?.beginSearchInteraction() }
    @objc func findNextAction(_ sender: Any?) { showSearchResult(step: 1) }
    @objc func findPreviousAction(_ sender: Any?) { showSearchResult(step: -1) }

    // MARK: - Pages (undoable)

    var currentIndex: Int {
        guard let pdf, let page = pdfView.currentPage else { return 0 }
        return pdf.index(for: page)
    }

    /// Pages selected in the thumbnail sidebar, or the current page.
    var targetIndices: [Int] {
        guard let pdf else { return [] }
        let selected = sidebar.thumbnails.selectedPages.map { pdf.index(for: $0) }.filter { $0 >= 0 && $0 < pdf.pageCount }
        if !selected.isEmpty { return Array(Set(selected)).sorted() }
        return currentIndex >= 0 && currentIndex < pdf.pageCount ? [currentIndex] : []
    }

    @objc func rotateLeftAction(_ sender: Any?) { rotate(targetIndices, by: -90) }
    @objc func rotateRightAction(_ sender: Any?) { rotate(targetIndices, by: 90) }

    private func rotate(_ indices: [Int], by degrees: Int) {
        guard let pdf else { return }
        prepareForSave()
        let pages = Array(Set(indices.filter { $0 >= 0 && $0 < pdf.pageCount })).sorted().compactMap { pdf.page(at: $0) }
        guard !pages.isEmpty else { return }
        for page in pages { page.rotation = ((page.rotation + degrees) % 360 + 360) % 360 }
        pdfView.layoutDocumentView()
        sidebar.thumbnails.reload()
        sidebar.thumbnails.selectPages(pages)
        undo?.registerUndo(withTarget: self) { $0.rotate(indices, by: -degrees) }
        undo?.setActionName(L("Rotate"))
    }

    private struct PageArrangement {
        var pages: [PDFPage]
        var current: PDFPage?
        var selected: [PDFPage]
        var extras: DocumentExtras?
        var destinations: [() -> Void]
    }

    private func pageArrangement() -> PageArrangement? {
        guard let pdf else { return nil }
        // PDFKit invalidates destination.page during removal. Capture target identities, then recreate the
        // destinations only after every page has been reinserted. Outline paths survive rebuilt Undo trees.
        var destinations: [() -> Void] = []
        func restored(_ d: PDFDestination) -> (() -> PDFDestination?)? {
            guard let page = d.page else { return nil }
            let point = d.point, zoom = d.zoom
            return { guard pdf.index(for: page) != NSNotFound else { return nil }; let value = PDFDestination(page: page, at: point); value.zoom = zoom; return value }
        }
        func outline(_ item: PDFOutline, path: [Int]) {
            if let d = item.destination ?? (item.action as? PDFActionGoTo)?.destination, let make = restored(d) {
                destinations.append {
                    var found = pdf.outlineRoot
                    for index in path { found = found?.child(at: index) }
                    if let dest = make() { found?.destination = dest }
                }
            }
            for i in 0..<item.numberOfChildren { if let child = item.child(at: i) { outline(child, path: path + [i]) } }
        }
        if let root = pdf.outlineRoot { outline(root, path: []) }
        for i in 0..<pdf.pageCount {
            for a in pdf.page(at: i)?.annotations ?? [] {
                if let d = a.destination, let make = restored(d) { destinations.append { if let dest = make() { a.destination = dest } } }
                if let d = (a.action as? PDFActionGoTo)?.destination, let make = restored(d) { destinations.append { if let dest = make() { a.action = PDFActionGoTo(destination: dest) } } }
            }
        }
        return PageArrangement(pages: (0..<pdf.pageCount).compactMap { pdf.page(at: $0) }, current: pdfView.currentPage,
                               selected: sidebar.thumbnails.selectedPages, extras: pdfDoc?.extras, destinations: destinations)
    }

    /// Page identity is shared by bookmarks and links. Retain it while rebuilding the order, and restore the UI
    /// selection and attachment lookup together with the pages on Undo/Redo.
    private func applyPageArrangement(_ state: PageArrangement, name: String) {
        guard let pdf, let old = pageArrangement() else { return }
        pdfView.selectAnnotation(nil)
        while pdf.pageCount > 0 { pdf.removePage(at: pdf.pageCount - 1) }
        for (i, page) in state.pages.enumerated() { pdf.insert(page, at: i) }
        state.destinations.forEach { $0() }
        if let extras = state.extras { pdfDoc?.extras = extras }
        pdfView.layoutDocumentView()
        if let current = state.current, pdf.index(for: current) != NSNotFound { pdfView.go(to: current) }
        else if let page = state.pages.first { pdfView.go(to: page) }
        sidebar.thumbnails.reload()
        sidebar.thumbnails.selectPages(state.selected)
        sidebar.reloadOutline()
        sidebar.reloadAttachments()
        undo?.registerUndo(withTarget: self) { $0.applyPageArrangement(old, name: name) }
        undo?.setActionName(name)
        updateSubtitleNow()
    }

    private func arrangePages(_ pages: [PDFPage], current: PDFPage? = nil, selected: [PDFPage]? = nil, name: String) {
        guard let old = pageArrangement(), old.pages.map(ObjectIdentifier.init) != pages.map(ObjectIdentifier.init) else { return }
        let positions = Dictionary(uniqueKeysWithValues: pages.enumerated().map { (ObjectIdentifier($0.element), $0.offset) })
        var extras = old.extras
        extras?.annotationFiles = old.extras?.annotationFiles.compactMap { file in
            guard old.pages.indices.contains(file.page), let index = positions[ObjectIdentifier(old.pages[file.page])] else { return nil }
            var copy = file; copy.page = index; return copy
        } ?? []
        if let open = extras?.view.openPage, old.pages.indices.contains(open) {
            let updated = positions[ObjectIdentifier(old.pages[open])] ?? 0
            if updated != open { extras?.view.openPage = updated; extras?.dirty.insert(.view) }
        }
        if (pdf?.outlineRoot?.numberOfChildren ?? 0) > 0 { extras?.dirty.insert(.outline) }
        applyPageArrangement(PageArrangement(pages: pages, current: current ?? old.current,
                                            selected: selected ?? old.selected.filter { positions[ObjectIdentifier($0)] != nil },
                                            extras: extras, destinations: old.destinations), name: name)
    }

    @objc func deletePagesAction(_ sender: Any?) {
        guard let pdf else { return }
        let indices = targetIndices
        guard !indices.isEmpty, indices.count < pdf.pageCount else { NSSound.beep(); return }
        prepareForSave()
        let chosen = Set(indices)
        let remaining = (0..<pdf.pageCount).filter { !chosen.contains($0) }.compactMap { pdf.page(at: $0) }
        let next = remaining[min(indices[0], remaining.count - 1)]
        arrangePages(remaining, current: next, selected: [next], name: L("Delete Pages"))
    }

    private func insertPages(_ pages: [PDFPage], at indices: [Int]) {
        guard let pdf, pages.count == indices.count, !pages.isEmpty else { return }
        prepareForSave()
        var order = (0..<pdf.pageCount).compactMap { pdf.page(at: $0) }
        for (page, index) in zip(pages, indices).sorted(by: { $0.1 < $1.1 }) {
            guard !order.contains(where: { $0 === page }) else { continue }
            order.insert(page, at: min(max(0, index), order.count))
        }
        arrangePages(order, current: pages.first, selected: pages, name: L("Insert Pages"))
    }

    @objc func insertBlankPageAction(_ sender: Any?) {
        let size = pdfView.currentPage?.bounds(for: .mediaBox).size ?? CGSize(width: 595, height: 842)
        insertPages([PDFPage.blank(size: size)], at: [(pdf?.pageCount ?? 0) == 0 ? 0 : currentIndex + 1])
    }

    @objc func insertFromFileAction(_ sender: Any?) {
        guard let window, let pdf else { return }
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.pdf, .image]
        panel.allowsMultipleSelection = true
        panel.message = L("Pages of the chosen PDFs will be inserted after the current page")
        panel.beginSheetModal(for: window) { [weak self] response in
            guard response == .OK, let self else { return }
            var pages: [PDFPage] = []
            for url in panel.urls {
                if UTType(filenameExtension: url.pathExtension)?.conforms(to: .image) == true {
                    if let image = NSImage(contentsOf: url), let page = PDFDoc.page(from: image) { pages.append(page) }
                    continue
                }
                guard let other = PDFDocument(url: url) else { continue }
                for i in 0..<other.pageCount {
                    if let page = other.page(at: i)?.copy() as? PDFPage { pages.append(page) }
                }
            }
            guard !pages.isEmpty else { return }
            let start = pdf.pageCount == 0 ? 0 : self.currentIndex + 1
            self.insertPages(pages, at: Array(start..<(start + pages.count)))
        }
    }

    /// Drag & drop in thumbnails: moves valid, distinct `indices` before position `target`.
    func movePages(_ indices: [Int], to target: Int) {
        guard let pdf else { return }
        let chosen = Array(Set(indices.filter { $0 >= 0 && $0 < pdf.pageCount })).sorted()
        guard !chosen.isEmpty else { return }
        prepareForSave()
        let order = (0..<pdf.pageCount).compactMap { pdf.page(at: $0) }
        let moving = chosen.map { order[$0] }
        let set = Set(chosen), bounded = min(max(0, target), order.count)
        var rest = order.enumerated().filter { !set.contains($0.offset) }.map(\.element)
        let insertAt = bounded - chosen.filter { $0 < bounded }.count
        rest.insert(contentsOf: moving, at: insertAt)
        arrangePages(rest, selected: moving, name: L("Move Pages"))
    }

    /// Files dropped on the thumbnails: PDF pages or images inserted at `index`.
    func insertFiles(_ urls: [URL], at index: Int) {
        var pages: [PDFPage] = []
        for url in urls {
            if UTType(filenameExtension: url.pathExtension)?.conforms(to: .image) == true {
                if let image = NSImage(contentsOf: url), let page = PDFDoc.page(from: image) { pages.append(page) }
            } else if let other = PDFDocument(url: url) {
                for i in 0..<other.pageCount { if let p = other.page(at: i)?.copy() as? PDFPage { pages.append(p) } }
            }
        }
        guard !pages.isEmpty else { return }
        let start = min(max(0, index), pdf?.pageCount ?? 0)
        insertPages(pages, at: Array(start..<(start + pages.count)))
    }

    @objc func movePageUpAction(_ sender: Any?) { moveSelectedPages(by: -1) }
    @objc func movePageDownAction(_ sender: Any?) { moveSelectedPages(by: 1) }

    private func moveSelectedPages(by delta: Int) {
        guard let pdf, delta == -1 || delta == 1 else { return }
        let indices = targetIndices
        guard let first = indices.first, let last = indices.last, first + delta >= 0, last + delta < pdf.pageCount else { return }
        prepareForSave()
        var order = (0..<pdf.pageCount).compactMap { pdf.page(at: $0) }
        let selected = indices.map { order[$0] }
        for index in delta > 0 ? indices.reversed().map({ $0 }) : indices { order.swapAt(index, index + delta) }
        arrangePages(order, selected: selected, name: L("Move Pages"))
    }

    @objc func extractPagesAction(_ sender: Any?) {
        guard pdf != nil else { return }
        prepareForSave()
        let newPDF = PDFDocument()
        let source: PDFDocument
        do { source = try exportSnapshot() } catch { presentError(error); return }
        for i in targetIndices {
            if let page = source.page(at: i)?.copy() as? PDFPage { newPDF.insert(page, at: newPDF.pageCount) }
        }
        let doc = PDFDoc(pdf: newPDF)
        NSDocumentController.shared.addDocument(doc)
        doc.makeWindowControllers()
        doc.showWindows()
        doc.updateChangeCount(.changeDone)
    }

    // MARK: - Validation

    func validateToolbarItem(_ item: NSToolbarItem) -> Bool { validate(item.action, tag: item.tag, menuItem: nil) }

    @objc func toggleAllPanelsAction(_ sender: Any?) { Panels.toggleAll(in: window) }

    func window(_ window: NSWindow, willUseFullScreenPresentationOptions proposedOptions: NSApplication.PresentationOptions) -> NSApplication.PresentationOptions {
        Panels.fullScreenOptions   // bars slide in only at the top edge
    }

    func validateMenuItem(_ item: NSMenuItem) -> Bool {
        if item.action == #selector(toggleAllPanelsAction(_:)) {
            item.title = Panels.allHidden(in: window) ? L("Show All Panels") : L("Hide All Panels")
            return true
        }
        return validate(item.action, tag: item.tag, menuItem: item)
    }

    private func validate(_ action: Selector?, tag: Int, menuItem: NSMenuItem?) -> Bool {
        guard let pdf else { return false }
        guard !pdf.isLocked else { return action == #selector(propertiesAction(_:)) || action == #selector(verifySignaturesAction(_:)) }
        switch action {
        case #selector(editTechnicalElementAction(_:)):
            return pdfView.selectedAnnotation?.technicalElement != nil
        case #selector(insertTechnicalElementAction(_:)), #selector(createLabelsAction(_:)), #selector(selectLabelRegionAction(_:)):
            return pdfView.currentPage != nil
        case #selector(selectToolAction(_:)):
            menuItem?.state = tag == pdfView.tool.rawValue ? .on : .off
        case #selector(setDisplayModeAction(_:)):
            menuItem?.state = tag == pdfView.displayMode.rawValue ? .on : .off
        case #selector(toggleMarkupAction(_:)):
            menuItem?.title = markupStrip.isShown ? L("Hide Markup Toolbar") : L("Show Markup Toolbar")
        case #selector(showThumbnailsAction(_:)):
            menuItem?.state = sidebar.mode == .thumbnails ? .on : .off
        case #selector(showOutlineAction(_:)):
            menuItem?.state = sidebar.mode == .outline ? .on : .off
        case #selector(deleteSelectedAnnotation(_:)):
            return pdfView.selectedAnnotation != nil
        case #selector(deletePagesAction(_:)):
            return !targetIndices.isEmpty && targetIndices.count < pdf.pageCount
        case #selector(movePageUpAction(_:)):
            return targetIndices.min().map { $0 > 0 } ?? false
        case #selector(movePageDownAction(_:)):
            return targetIndices.max().map { $0 < pdf.pageCount - 1 } ?? false
        case #selector(nextPageAction(_:)):
            return pdfView.canGoToNextPage
        case #selector(previousPageAction(_:)):
            return pdfView.canGoToPreviousPage
        case #selector(highlightSelectionAction(_:)):
            return pdfView.currentSelection?.string?.isEmpty == false
        case #selector(findNextAction(_:)), #selector(findPreviousAction(_:)):
            return !searchResults.isEmpty
        case #selector(verifySignaturesAction(_:)):
            return true
        case #selector(enableLTVAction(_:)):
            return !(pdfDoc?.signatures.isEmpty ?? true)
        case #selector(removeWatermarksAction(_:)):
            return hasRemovableOverlays
        case #selector(applyRedactionsAction(_:)):
            return !pendingRedactions.isEmpty
        case #selector(clearCompareHighlightsAction(_:)):
            return hasCompareHighlights
        case #selector(splitAction(_:)), #selector(crossPageSealAction(_:)):
            return pdf.pageCount > 1
        case #selector(showAttachmentsAction(_:)):
            menuItem?.state = sidebar.mode == .attachments ? .on : .off
        case #selector(renameBookmarkAction(_:)), #selector(deleteBookmarkAction(_:)), #selector(setBookmarkDestinationAction(_:)),
             #selector(addChildBookmarkAction(_:)):
            return sidebar.selectedBookmark != nil
        case #selector(bookmarkStyleAction(_:)):
            guard let item = sidebar.selectedBookmark else { return false }
            let style = pdfDoc?.extras.outlineStyles[ObjectIdentifier(item)] ?? OutlineStyle()
            switch tag {
            case 1: menuItem?.state = style.bold ? .on : .off
            case 2: menuItem?.state = style.italic ? .on : .off
            default:
                let c = tag - 10 < Self.bookmarkColors.count && tag >= 10 ? Self.bookmarkColors[tag - 10].1 : nil
                let same = c == nil ? style.color == nil : style.color.map { Self.sameColor($0, c!) } ?? false
                menuItem?.state = same ? .on : .off
            }
        case #selector(indentBookmarkAction(_:)):
            return (sidebar.selectedBookmark?.index ?? 0) > 0
        case #selector(outdentBookmarkAction(_:)):
            return sidebar.selectedBookmark?.parent?.parent != nil
        case #selector(deleteAttachmentAction(_:)), #selector(saveAttachmentAction(_:)), #selector(openAttachmentAction(_:)):
            return sidebar.selectedAttachment != nil
        case #selector(toggleLinkBordersAction(_:)):
            menuItem?.state = pdfView.showsLinkBorders ? .on : .off
        case #selector(removeHeaderFooterAction(_:)):
            return hasHeaderFooterOverlays
        default:
            break
        }
        return true
    }
}

/// Target object for controls whose action should run a closure.
final class ClosureTarget: NSObject {
    let handler: () -> Void
    init(_ handler: @escaping () -> Void) { self.handler = handler }
    @objc func fire() { handler() }
}

private extension CGFloat {
    func clamped(_ lo: CGFloat, _ hi: CGFloat) -> CGFloat { Swift.min(hi, Swift.max(lo, self)) }
}

extension EditorWindowController: PanelHiding {}
