import AppKit

/// Pop-up of ink colours: presets with swatches, plus "Other Colour…" (system colour panel).
public final class InkColorPopUp: NSPopUpButton {
    public private(set) var color: NSColor
    public var onChange: ((NSColor) -> Void)?
    private let stamp: Bool
    private var customIndex: Int?

    public init(stamp: Bool, initial: InkColor) {
        self.stamp = stamp
        color = stamp ? initial.stampPad : initial.pen
        super.init(frame: .zero, pullsDown: false)
        for ink in InkColor.allCases {
            addItem(withTitle: ink.title)
            lastItem?.image = InkColor.swatch(stamp ? ink.stampPad : ink.pen)
            lastItem?.tag = ink.rawValue
        }
        menu?.addItem(.separator())
        addItem(withTitle: L("Other Colour…"))
        lastItem?.tag = -1
        selectItem(withTag: initial.rawValue)
        target = self
        action = #selector(changed)
    }

    required init?(coder: NSCoder) { fatalError("not supported") }

    public func select(_ ink: InkColor) {
        selectItem(withTag: ink.rawValue)
        color = stamp ? ink.stampPad : ink.pen
        onChange?(color)
    }

    @objc private func changed() {
        let tag = selectedTag()
        if tag == -1 {
            let panel = NSColorPanel.shared
            panel.showsAlpha = false
            panel.color = color
            panel.setTarget(self)
            panel.setAction(#selector(customColor(_:)))
            panel.orderFront(nil)
            restoreSelection()
            return
        }
        if tag == -2 { return }
        if let ink = InkColor(rawValue: tag) {
            color = stamp ? ink.stampPad : ink.pen
            onChange?(color)
        }
    }

    @objc private func customColor(_ sender: NSColorPanel) {
        color = sender.color.usingColorSpace(.sRGB) ?? sender.color
        if customIndex == nil {
            insertItem(withTitle: L("Custom Colour"), at: InkColor.allCases.count)
            customIndex = InkColor.allCases.count
            item(at: customIndex!)?.tag = -2
        }
        item(at: customIndex!)?.image = InkColor.swatch(color)
        selectItem(at: customIndex!)
        onChange?(color)
    }

    private func restoreSelection() {
        if let customIndex, InkColor.allCases.allSatisfy({ (stamp ? $0.stampPad : $0.pen) != color }) {
            selectItem(at: customIndex)
        } else if let ink = InkColor.allCases.first(where: { (stamp ? $0.stampPad : $0.pen) == color }) {
            selectItem(withTag: ink.rawValue)
        }
    }
}

/// Window that creates signatures (from names) or stamps (from a style and texts), shows previews,
/// and adds them to the shared library or inserts one impression.
public final class MarkGeneratorController: NSWindowController, NSWindowDelegate, NSTextViewDelegate, NSTextFieldDelegate {
    public enum Mode { case signature, seal, learn }

    private static var openControllers: [MarkGeneratorController] = []

    /// Opens the generator as a sheet on `parent` (or as a window). `onInsert` nil hides Insert.
    /// `onInsertMark`, when given, receives the recipe instead of one impression (the caller draws it at its resolution).
    public static func show(_ mode: Mode, over parent: NSWindow?, onInsert: ((CGImage) -> Void)?,
                            onInsertMark: ((GeneratedMark) -> Void)? = nil, onLibraryChange: (() -> Void)? = nil) {
        let c = MarkGeneratorController(mode: mode, onInsert: onInsert, onLibraryChange: onLibraryChange)
        c.onInsertMark = onInsertMark
        openControllers.append(c)
        if let parent, parent.attachedSheet == nil {
            parent.beginSheet(c.window!)
        } else {
            c.window?.center()
            c.showWindow(nil)
        }
        c.refresh()
        // K42_GENERATOR_LAYOUT_AUDIT=1 (UI audit): no label or pop-up in the generator may be clipped. Regression for
        // the Form/Pen/Ink row, which used to squeeze "Form:" to nothing and truncate the pop-ups in long languages.
        if ProcessInfo.processInfo.environment["K42_GENERATOR_LAYOUT_AUDIT"] != nil, let window = c.window {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
                window.contentView?.layoutSubtreeIfNeeded()
                func all(_ v: NSView) -> [NSView] { [v] + v.subviews.flatMap(all) }
                var clipped: [String] = []
                for v in all(window.contentView ?? NSView()) where !v.isHiddenOrHasHiddenAncestor {
                    if let p = v as? NSPopUpButton, let title = p.titleOfSelectedItem, !title.isEmpty, !p.pullsDown,
                       p.bounds.width + 1 < p.intrinsicContentSize.width { clipped.append("pop-up \(title)") }
                    else if let f = v as? NSTextField, !f.isEditable, f.cell?.wraps == false, !f.stringValue.isEmpty,
                            f.bounds.width + 1 < f.intrinsicContentSize.width { clipped.append("label \(f.stringValue)") }
                }
                print("Generator layout: \(clipped.isEmpty ? "PASS" : "FAIL") \(Language.current.rawValue) \(clipped.joined(separator: "; "))")
            }
        }
    }

    private let mode: Mode
    private let onInsert: ((CGImage) -> Void)?
    private var onInsertMark: ((GeneratedMark) -> Void)?
    private let onLibraryChange: (() -> Void)?
    private var generation = 0

    // Signature mode
    private let names = NSTextView()
    private let variantsStack = NSStackView()
    private var designs: [[UInt64]] = []            // per name, three design seeds
    private let formPopup = NSPopUpButton()
    private var selected: [Int] = []                // per name, chosen variant
    private var activeRow = 0
    private var rowNames: [String] = []
    private let penPopup = NSPopUpButton()

    // Learn mode: the user's own signature
    private let pad = SignaturePad(frame: NSRect(x: 0, y: 0, width: 600, height: 190))
    private var samples: [SignatureSample] = []
    private let samplesLabel = NSTextField(labelWithString: "")
    private let learnTiles = NSStackView()
    private var copySeeds: [UInt64] = (0..<3).map { _ in SeededRandom.freshSeed() }

    // Seal mode
    private let categoryPopup = NSPopUpButton()
    private let stylePopup = NSPopUpButton()
    private let fieldsHost = NSStackView()
    private var fieldControls: [(key: String, view: NSView)] = []
    private let preview = NSImageView()
    private var sealDesign = SeededRandom.freshSeed()
    private var sealImpression = SeededRandom.freshSeed()

    private let ink: InkColorPopUp
    private var debounce: DispatchWorkItem?

    private init(mode: Mode, onInsert: ((CGImage) -> Void)?, onLibraryChange: (() -> Void)?) {
        self.mode = mode
        self.onInsert = onInsert
        self.onLibraryChange = onLibraryChange
        ink = InkColorPopUp(stamp: mode == .seal, initial: mode == .seal ? (SealTemplate.all.first?.ink ?? .blue) : .blue)
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 640, height: 520),
                              styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.title = mode == .signature ? L("Create Signature") : mode == .learn ? L("My Signature") : L("Create Stamp")
        super.init(window: window)
        window.delegate = self
        for pen in PenType.allCases { penPopup.addItem(withTitle: pen.title); penPopup.lastItem?.tag = pen.rawValue }
        penPopup.target = self
        penPopup.action = #selector(penChanged)
        switch mode {
        case .signature: window.contentView = buildSignature()
        case .seal: window.contentView = buildSeal()
        case .learn:
            window.contentView = buildLearn()
            window.initialFirstResponder = pad   // Continuity imports go to the first responder
        }
        ink.onChange = { [weak self] _ in self?.refresh() }
    }

    required init?(coder: NSCoder) { fatalError("not supported") }

    // MARK: - Layout

    private func buttonsRow(regenerate title: String, extra: NSView? = nil) -> NSView {
        let again = NSButton(title: title, target: self, action: #selector(regenerateAction))
        again.keyEquivalent = "r"
        again.keyEquivalentModifierMask = .command
        again.toolTip = title + " (⌘R)"
        let cancel = NSButton(title: L("Cancel"), target: self, action: #selector(close(_:)))
        cancel.keyEquivalent = "\u{1b}"
        let add = NSButton(title: L("Add to Library"), target: self, action: #selector(addToLibrary))
        let insert = NSButton(title: L("Insert"), target: self, action: #selector(insertMark))
        insert.keyEquivalent = "\r"
        insert.isHidden = onInsert == nil
        if onInsert == nil { add.keyEquivalent = "\r" }
        let spacer = NSView()
        spacer.setContentHuggingPriority(.init(1), for: .horizontal)
        let views = [again] + (extra.map { [$0] } ?? []) + [spacer, cancel, add, insert]
        let row = NSStackView(views: views)
        row.spacing = 8
        return row
    }

    private func wrap(_ views: [NSView]) -> NSView {
        let stack = NSStackView(views: views)
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 12
        stack.translatesAutoresizingMaskIntoConstraints = false
        let root = NSView()
        root.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.topAnchor.constraint(equalTo: root.topAnchor, constant: 20),
            stack.bottomAnchor.constraint(equalTo: root.bottomAnchor, constant: -20),
            stack.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 20),
            stack.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -20),
            root.widthAnchor.constraint(equalToConstant: 640),
        ])
        for v in views { v.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true }
        return root
    }

    /// "Label: control" pairs on one line. Longer translations (Russian, Italian, Portuguese, Turkish…) do not fit the
    /// 600-point content width and the stack used to squeeze labels to nothing and truncate the pop-ups; then the
    /// pairs wrap onto further lines instead, each at its natural width.
    private func labelledRow(_ pairs: [(String, NSView)]) -> NSView {
        func line(_ part: ArraySlice<(String, NSView)>) -> NSStackView {
            var views: [NSView] = []
            for (text, control) in part {
                let label = NSTextField(labelWithString: text)
                label.setContentCompressionResistancePriority(.required, for: .horizontal)
                control.setContentCompressionResistancePriority(.defaultHigh, for: .horizontal)
                views += [label, control]
            }
            let row = NSStackView(views: views)
            for (_, control) in part.dropLast() { row.setCustomSpacing(18, after: control) }
            return row
        }
        let single = line(pairs[...])
        let available: CGFloat = 600
        guard single.fittingSize.width > available else { return single }
        // Greedy wrap: as many pairs per line as fit.
        var lines: [NSStackView] = [], start = pairs.startIndex
        while start < pairs.endIndex {
            var end = start + 1
            while end < pairs.endIndex, line(pairs[start...end]).fittingSize.width <= available { end += 1 }
            lines.append(line(pairs[start..<end]))
            start = end
        }
        let column = NSStackView(views: lines)
        column.orientation = .vertical
        column.alignment = .leading
        column.spacing = 8
        return column
    }

    private func textArea(_ tv: NSTextView, height: CGFloat) -> NSScrollView {
        let scroll = NSScrollView()
        scroll.borderType = .bezelBorder
        scroll.hasVerticalScroller = true
        scroll.documentView = tv
        tv.isRichText = false
        tv.font = .systemFont(ofSize: 13)
        tv.isAutomaticQuoteSubstitutionEnabled = false
        // A text view made with a zero frame stays zero wide inside the scroll view: give it a real size.
        tv.frame = NSRect(x: 0, y: 0, width: 400, height: height)
        tv.minSize = NSSize(width: 0, height: height)
        tv.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        tv.isVerticallyResizable = true
        tv.isHorizontallyResizable = false
        tv.autoresizingMask = [.width]
        tv.textContainer?.containerSize = NSSize(width: 400, height: CGFloat.greatestFiniteMagnitude)
        tv.textContainer?.widthTracksTextView = true
        tv.delegate = self
        scroll.translatesAutoresizingMaskIntoConstraints = false
        scroll.heightAnchor.constraint(equalToConstant: height).isActive = true
        return scroll
    }

    private func buildSignature() -> NSView {
        let hint = NSTextField(labelWithString: L("Names, one per line. Each line becomes its own signature."))
        hint.textColor = .secondaryLabelColor
        // K42_GENERATOR_NAME: a fixed sample name for screenshots instead of the account name.
        names.string = ProcessInfo.processInfo.environment["K42_GENERATOR_NAME"] ?? NSFullUserName()
        for form in SignatureForm.allCases { formPopup.addItem(withTitle: form.title); formPopup.lastItem?.tag = form.rawValue }
        formPopup.target = self
        formPopup.action = #selector(formChanged)
        let inkRow = labelledRow([(L("Form:"), formPopup), (L("Pen:"), penPopup), (L("Ink:"), ink)])
        variantsStack.orientation = .vertical
        variantsStack.alignment = .leading
        variantsStack.spacing = 14
        let note = NSTextField(wrappingLabelWithString: L("Click a variant to choose it. Every insertion is written slightly differently, like a real signature."))
        note.textColor = .secondaryLabelColor
        note.font = .systemFont(ofSize: 11)
        return wrap([hint, textArea(names, height: 58), inkRow, variantsStack, note, buttonsRow(regenerate: L("New Variants"))])
    }

    private var pen: PenType { PenType(rawValue: penPopup.selectedTag()) ?? .ballpoint }

    @objc private func penChanged() { refresh() }

    /// Learn mode: sign a few times (or bring a picture), see new copies in your hand.
    private func buildLearn() -> NSView {
        let hint = NSTextField(wrappingLabelWithString: L("Sign in the box, then Add Sample. Three to five samples teach your hand; one is enough to start. A scan or an iPhone sketch works too."))
        hint.textColor = .secondaryLabelColor
        pad.translatesAutoresizingMaskIntoConstraints = false
        pad.heightAnchor.constraint(equalToConstant: 190).isActive = true
        pad.wantsLayer = true
        pad.layer?.cornerRadius = 8
        pad.layer?.borderWidth = 0.5
        pad.layer?.borderColor = NSColor(white: 0.5, alpha: 0.5).cgColor
        pad.onChange = { [weak self] in self?.scheduleRefresh() }
        pad.onPicture = { [weak self] image in self?.addPicture(image) }
        let add = NSButton(title: L("Add Sample"), target: self, action: #selector(addSample))
        let clear = NSButton(title: L("Clear"), target: self, action: #selector(clearPad))
        let open = NSButton(title: L("Open Picture…"), target: self, action: #selector(openPicture))
        // Continuity: the system fills this item with "Add Sketch", "Take Photo" and "Scan Documents".
        let device = NSPopUpButton(frame: .zero, pullsDown: true)
        device.addItem(withTitle: L("From iPhone or iPad"))
        let importItem = NSMenuItem(title: L("From iPhone or iPad"), action: nil, keyEquivalent: "")
        importItem.identifier = NSMenuItem.importFromDeviceIdentifier
        device.menu?.addItem(importItem)
        let removeLast = NSButton(title: L("Remove Last"), target: self, action: #selector(removeSample))
        let row = NSStackView(views: [add, clear, open, device])
        row.spacing = 8
        let samplesRow = NSStackView(views: [samplesLabel, removeLast])
        samplesRow.spacing = 10
        let inkRow = labelledRow([(L("Pen:"), penPopup), (L("Ink:"), ink)])
        learnTiles.spacing = 10
        for _ in 0..<3 {
            let b = NSImageView()
            b.imageScaling = .scaleProportionallyDown
            b.wantsLayer = true
            b.layer?.backgroundColor = NSColor.white.cgColor
            b.layer?.cornerRadius = 8
            b.layer?.borderWidth = 0.5
            b.layer?.borderColor = NSColor(white: 0.5, alpha: 0.5).cgColor
            b.translatesAutoresizingMaskIntoConstraints = false
            b.widthAnchor.constraint(equalToConstant: 190).isActive = true
            b.heightAnchor.constraint(equalToConstant: 84).isActive = true
            learnTiles.addArrangedSubview(b)
        }
        updateSamplesLabel()
        return wrap([hint, pad, row, samplesRow, inkRow, learnTiles, buttonsRow(regenerate: L("New Copies"))])
    }

    private func updateSamplesLabel() {
        samplesLabel.stringValue = L("Samples: %d", samples.count)
    }

    /// Samples used for the preview: the saved ones, or what is in the box while there are none yet.
    private var learnedSignature: LearnedSignature? {
        let list = samples.isEmpty ? (pad.sample.map { [$0] } ?? []) : samples
        return list.isEmpty ? nil : LearnedSignature(samples: list)
    }

    @objc private func addSample() {
        guard let s = pad.sample else { NSSound.beep(); return }
        samples.append(s)
        pad.clear()
        updateSamplesLabel()
        refresh()
    }

    @objc private func clearPad() { pad.clear() }

    @objc private func removeSample() {
        guard !samples.isEmpty else { return }
        samples.removeLast()
        updateSamplesLabel()
        refresh()
    }

    @objc private func openPicture() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.image, .pdf]
        panel.message = L("Choose a scan or photo of your signature, or of a sheet you signed several times (dark ink on light paper)")
        guard let window, panel.runModal() == .OK, let url = panel.url,
              let image = StampLibrary.image(at: url) ?? StampLibrary.rasterize(url) else { return }
        _ = window
        addPicture(image)
    }

    /// A picture with one signature, or a photo of a sheet signed many times: every signature on it becomes a sample.
    private func addPicture(_ image: CGImage) {
        let found = SignatureSheet.samples(from: image)
        guard !found.isEmpty else {
            let alert = NSAlert()
            alert.messageText = L("No signature found in the picture")
            alert.informativeText = L("Use dark ink on light paper. A sheet with several signatures works too: leave space between them.")
            alert.runModal()
            return
        }
        samples += found
        updateSamplesLabel()
        refresh()
    }

    private func buildSeal() -> NSView {
        for category in SealCategory.allCases where SealTemplate.all.contains(where: { $0.category == category }) {
            categoryPopup.addItem(withTitle: category.title)
            categoryPopup.lastItem?.tag = category.rawValue
        }
        categoryPopup.target = self
        categoryPopup.action = #selector(categoryChanged)
        stylePopup.target = self
        stylePopup.action = #selector(styleChanged)
        let top = NSGridView(views: [
            [NSTextField(labelWithString: L("Category:")), categoryPopup],
            [NSTextField(labelWithString: L("Style:")), stylePopup],
        ])
        for g in [top] {
            g.column(at: 0).xPlacement = .trailing
            g.column(at: 0).width = 190
            g.rowAlignment = .firstBaseline
            g.rowSpacing = 8
        }
        fieldsHost.orientation = .vertical
        fieldsHost.alignment = .leading
        let inkRow = NSGridView(views: [[NSTextField(labelWithString: L("Ink:")), ink]])
        inkRow.column(at: 0).xPlacement = .trailing
        inkRow.column(at: 0).width = 190
        inkRow.rowAlignment = .firstBaseline
        preview.imageScaling = .scaleProportionallyDown
        preview.wantsLayer = true
        preview.layer?.backgroundColor = NSColor.white.cgColor
        preview.layer?.cornerRadius = 8
        preview.layer?.borderWidth = 0.5
        preview.layer?.borderColor = NSColor(white: 0.5, alpha: 0.5).cgColor
        preview.translatesAutoresizingMaskIntoConstraints = false
        preview.heightAnchor.constraint(equalToConstant: 280).isActive = true
        let count = NSTextField(labelWithString: L("%d stamp styles", SealTemplate.all.count))
        count.textColor = .secondaryLabelColor
        count.font = .systemFont(ofSize: 11)
        let note = NSTextField(wrappingLabelWithString: L("The texts are examples — type your own. Every insertion is a new impression of the same stamp: pressure, ink and angle differ, as on paper."))
        note.textColor = .secondaryLabelColor
        note.font = .systemFont(ofSize: 11)
        // K42_GENERATOR_SEAL=<template id>: open on that stamp style (screenshots).
        let wanted = ProcessInfo.processInfo.environment["K42_GENERATOR_SEAL"].flatMap(SealTemplate.named)
        if let wanted { categoryPopup.selectItem(withTag: wanted.category.rawValue) }
        fillStyles()
        if let wanted, let i = stylePopup.itemArray.firstIndex(where: { $0.representedObject as? String == wanted.id }) {
            stylePopup.selectItem(at: i)
            applyStyleFields()
        }
        return wrap([top, fieldsHost, inkRow, preview, note, buttonsRow(regenerate: L("Press Again"), extra: count)])
    }

    private var templates: [SealTemplate] {
        SealTemplate.all.filter { $0.category.rawValue == categoryPopup.selectedTag() }
    }

    private var currentTemplate: SealTemplate {
        SealTemplate.named(stylePopup.selectedItem?.representedObject as? String ?? "") ?? templates.first ?? SealTemplate.all[0]
    }

    private func fillStyles() {
        stylePopup.removeAllItems()
        for t in templates {
            stylePopup.addItem(withTitle: t.title)
            stylePopup.lastItem?.representedObject = t.id
        }
        applyStyleFields()
    }

    /// One labelled control per field of the template (multi-line fields get a small text area).
    private func applyStyleFields() {
        fieldsHost.arrangedSubviews.forEach { $0.removeFromSuperview() }
        fieldControls = []
        var rows: [[NSView]] = []
        for field in currentTemplate.fields {
            let label = NSTextField(labelWithString: L(field.label) + ":")
            label.alignment = .right
            if field.multiline {
                let tv = NSTextView()
                let scroll = textArea(tv, height: 50)
                scroll.widthAnchor.constraint(equalToConstant: 400).isActive = true
                tv.string = field.example
                rows.append([label, scroll])
                fieldControls.append((field.key, tv))
            } else {
                let tf = NSTextField(string: field.example.trimmingCharacters(in: .whitespaces))
                tf.placeholderString = field.example.trimmingCharacters(in: .whitespaces).isEmpty ? L(field.label) : field.example
                tf.delegate = self
                tf.lineBreakMode = .byTruncatingTail
                tf.translatesAutoresizingMaskIntoConstraints = false
                tf.widthAnchor.constraint(equalToConstant: 400).isActive = true
                rows.append([label, tf])
                fieldControls.append((field.key, tf))
            }
        }
        let grid = NSGridView(views: rows)
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 190
        grid.rowAlignment = .firstBaseline
        grid.rowSpacing = 8
        fieldsHost.addArrangedSubview(grid)
        window?.contentView?.layoutSubtreeIfNeeded()
        if let size = window?.contentView?.fittingSize { window?.setContentSize(size) }
    }

    private var fieldValues: [String: String] {
        var values: [String: String] = [:]
        for (key, view) in fieldControls {
            if let tf = view as? NSTextField { values[key] = tf.stringValue }
            if let tv = view as? NSTextView { values[key] = tv.string }
        }
        return values
    }

    // MARK: - Rendering

    private var sealSpec: SealSpec {
        // A field left empty stays empty (an empty date means today); untouched fields keep the example.
        SealSpec(template: currentTemplate, values: fieldValues, ink: ink.color, designSeed: sealDesign)
    }

    private func refresh() {
        generation += 1
        let token = generation
        let color = ink.color
        switch mode {
        case .seal:
            let spec = sealSpec, seed = sealImpression
            DispatchQueue.global(qos: .userInitiated).async {
                let image = SealGenerator.render(spec, impression: seed)
                DispatchQueue.main.async { [weak self] in
                    guard let self, token == self.generation else { return }
                    self.preview.image = image.map { NSImage(cgImage: $0, size: NSSize(width: $0.width, height: $0.height)) }
                }
            }
        case .learn:
            guard let learned = learnedSignature else {
                for case let v as NSImageView in learnTiles.arrangedSubviews { v.image = nil }
                return
            }
            let seeds = copySeeds, pen = self.pen
            DispatchQueue.global(qos: .userInitiated).async {
                for (i, seed) in seeds.enumerated() {
                    let image = learned.render(impression: seed, ink: color, pen: pen)
                    DispatchQueue.main.async { [weak self] in
                        guard let self, token == self.generation, let image,
                              let tile = self.learnTiles.arrangedSubviews[safe: i] as? NSImageView else { return }
                        tile.image = Self.fitted(image, in: NSSize(width: 176, height: 72))
                    }
                }
            }
        case .signature:
            let list = names.string.split(separator: "\n").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }.prefix(6)
            rowNames = Array(list)
            while designs.count < rowNames.count {
                // K42_GENERATOR_DESIGNS=a,b,c: fixed variants for screenshots.
                let preset = (ProcessInfo.processInfo.environment["K42_GENERATOR_DESIGNS"] ?? "").split(separator: ",").compactMap { UInt64($0) }
                designs.append(preset.count == 3 ? preset : SignatureGenerator.designSeeds(3, form: form)); selected.append(0)
            }
            designs = Array(designs.prefix(rowNames.count))
            selected = Array(selected.prefix(rowNames.count))
            activeRow = min(activeRow, max(0, rowNames.count - 1))
            buildVariantRows()
            let pen = self.pen
            let jobs = rowNames.enumerated().flatMap { r, name in designs[r].enumerated().map { (r, $0.offset, name, $0.element) } }
            DispatchQueue.global(qos: .userInitiated).async {
                for (r, v, name, design) in jobs {
                    let image = SignatureGenerator.render(name, design: design, impression: design &+ 1, ink: color, pen: pen)
                    DispatchQueue.main.async { [weak self] in
                        guard let self, token == self.generation, let image else { return }
                        self.tile(r, v)?.image = Self.fitted(image, in: NSSize(width: 176, height: 72))
                        self.tile(r, v)?.toolTip = L("Style %d of %d", SignatureGenerator.styleNumber(design), SignatureGenerator.styleCount)
                    }
                }
            }
        }
    }

    private func buildVariantRows() {
        variantsStack.arrangedSubviews.forEach { $0.removeFromSuperview() }
        for (r, name) in rowNames.enumerated() {
            let label = NSTextField(labelWithString: name)
            label.font = .systemFont(ofSize: 11, weight: .medium)
            label.textColor = .secondaryLabelColor
            let tiles = (0..<3).map { v -> NSButton in
                let b = NSButton(image: NSImage(), target: self, action: #selector(tileClicked(_:)))
                b.tag = r * 10 + v
                b.bezelStyle = .smallSquare
                b.isBordered = false
                b.imageScaling = .scaleProportionallyDown
                b.wantsLayer = true
                b.layer?.backgroundColor = NSColor.white.cgColor
                b.layer?.cornerRadius = 8
                b.translatesAutoresizingMaskIntoConstraints = false
                b.widthAnchor.constraint(equalToConstant: 190).isActive = true
                b.heightAnchor.constraint(equalToConstant: 84).isActive = true
                return b
            }
            let row = NSStackView(views: tiles)
            row.spacing = 10
            let block = NSStackView(views: [label, row])
            block.orientation = .vertical
            block.alignment = .leading
            block.spacing = 4
            variantsStack.addArrangedSubview(block)
        }
        updateSelection()
        window?.contentView?.layoutSubtreeIfNeeded()
        if let size = window?.contentView?.fittingSize { window?.setContentSize(size) }
    }

    /// Preview scaled to fit a tile (buttons do not scale their images down reliably).
    private static func fitted(_ image: CGImage, in box: NSSize) -> NSImage {
        let k = min(box.width / CGFloat(image.width), box.height / CGFloat(image.height), 1)
        let size = NSSize(width: CGFloat(image.width) * k, height: CGFloat(image.height) * k)
        return NSImage(size: size, flipped: false) { rect in
            NSGraphicsContext.current?.imageInterpolation = .high
            NSGraphicsContext.current?.cgContext.draw(image, in: rect)
            return true
        }
    }

    private func tile(_ row: Int, _ variant: Int) -> NSButton? {
        guard row < variantsStack.arrangedSubviews.count,
              let block = variantsStack.arrangedSubviews[row] as? NSStackView,
              let tiles = block.arrangedSubviews.last as? NSStackView, variant < tiles.arrangedSubviews.count else { return nil }
        return tiles.arrangedSubviews[variant] as? NSButton
    }

    private func updateSelection() {
        for r in 0..<rowNames.count {
            for v in 0..<3 {
                let on = selected[r] == v
                tile(r, v)?.layer?.borderWidth = on ? (r == activeRow ? 3 : 2) : 0.5
                tile(r, v)?.layer?.borderColor = on ? NSColor.controlAccentColor.cgColor : NSColor(white: 0.5, alpha: 0.5).cgColor
            }
        }
    }

    @objc private func tileClicked(_ sender: NSButton) {
        selected[sender.tag / 10] = sender.tag % 10
        activeRow = sender.tag / 10
        updateSelection()
    }

    public func textDidChange(_ notification: Notification) { scheduleRefresh() }
    public func controlTextDidChange(_ obj: Notification) { scheduleRefresh() }

    private func scheduleRefresh() {
        debounce?.cancel()
        let work = DispatchWorkItem { [weak self] in self?.refresh() }
        debounce = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.35, execute: work)
    }

    // MARK: - Actions

    @objc private func categoryChanged() {
        fillStyles()
        ink.select(currentTemplate.ink)
        refresh()
    }

    @objc private func styleChanged() {
        ink.select(currentTemplate.ink)
        applyStyleFields()
        refresh()
    }

    @objc private func regenerateAction() {
        switch mode {
        case .signature: designs = rowNames.map { _ in SignatureGenerator.designSeeds(3, form: form) }
        case .seal: sealImpression = SeededRandom.freshSeed()
        case .learn: copySeeds = (0..<3).map { _ in SeededRandom.freshSeed() }
        }
        refresh()
    }

    private var form: SignatureForm { SignatureForm(rawValue: formPopup.selectedTag()) ?? .any }

    @objc private func formChanged() {
        designs = rowNames.map { _ in SignatureGenerator.designSeeds(3, form: form) }
        refresh()
    }

    @objc private func otherLayout() {
        sealDesign = SeededRandom.freshSeed()
        refresh()
    }

    private func marks() -> [GeneratedMark] {
        switch mode {
        case .seal: return [.seal(sealSpec)]
        case .learn: return learnedSignature.map { [.learned($0, ink: ink.color, pen: pen)] } ?? []
        case .signature:
            return rowNames.enumerated().map { r, name in .signature(name, ink: ink.color, design: designs[r][selected[r]], pen: pen) }
        }
    }

    @objc private func addToLibrary() {
        do {
            for mark in marks() {
                // The stored preview is one impression; insertions make new ones from the recipe.
                if let image = mark.impression() { try StampLibrary.shared.add(image, mark: mark) }
            }
        } catch {
            NSApp.presentError(error)
            return
        }
        onLibraryChange?()
        close(nil)
    }

    @objc private func insertMark() {
        let all = marks()
        let mark = mode == .signature ? all[safe: activeRow] : all.first
        if let mark, let onInsertMark {
            close(nil)
            onInsertMark(mark)
            return
        }
        guard let image = mark?.impression() else { NSSound.beep(); return }
        close(nil)
        onInsert?(image)
    }

    @objc private func close(_ sender: Any?) {
        guard let window else { return }
        if let parent = window.sheetParent { parent.endSheet(window) } else { window.close() }
        Self.openControllers.removeAll { $0 === self }
    }

    public func windowWillClose(_ notification: Notification) {
        Self.openControllers.removeAll { $0 === self }
    }
}

