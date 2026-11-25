import AppKit
import Kit42

/// Flipped container so stacked settings start at the top of the scroll view.
final class FlippedView: NSView {
    override var isFlipped: Bool { true }
}

/// The settings inspector: a few clear choices (preset, source, colour, resolution, paper, clean-up),
/// the rest behind "More Options", and one big Scan button at the bottom.
final class SettingsPanelController: NSViewController, NSTextFieldDelegate {
    private(set) var settings = Store.settings
    var onChange: ((ScanSettings) -> Void)?
    var onPreset: ((Int) -> Void)?

    let preset = NSPopUpButton(frame: .zero, pullsDown: false)
    let source = NSSegmentedControl()
    let duplex = NSButton(checkboxWithTitle: L("Both sides (duplex)"), target: nil, action: nil)
    let color = NSSegmentedControl()
    let resolution = NSPopUpButton(frame: .zero, pullsDown: false)
    let paper = NSPopUpButton(frame: .zero, pullsDown: false)
    let customW = NSTextField()
    let customH = NSTextField()
    let customRow = NSStackView()
    let crop = NSButton(checkboxWithTitle: L("Crop and straighten"), target: nil, action: nil)
    let rotate = NSButton(checkboxWithTitle: L("Turn pages upright"), target: nil, action: nil)
    let blank = NSButton(checkboxWithTitle: L("Remove blank pages"), target: nil, action: nil)
    let photos = NSButton(checkboxWithTitle: L("Separate photos on the glass"), target: nil, action: nil)
    let descreen = NSButton(checkboxWithTitle: L("Descreen (magazines)"), target: nil, action: nil)
    let enhancement = NSPopUpButton(frame: .zero, pullsDown: false)
    let more = NSButton()
    let advanced = NSStackView()
    let depth = NSPopUpButton(frame: .zero, pullsDown: false)
    let brightness = NSSlider(value: 0, minValue: -1, maxValue: 1, target: nil, action: nil)
    let contrast = NSSlider(value: 0, minValue: -1, maxValue: 1, target: nil, action: nil)
    let threshold = NSSlider(value: 0.5, minValue: 0.1, maxValue: 0.9, target: nil, action: nil)
    let scanButton = NSButton(title: L("Scan"), target: nil, action: #selector(ScanWindowController.scanAction(_:)))
    let overviewButton = NSButton(title: L("Overview"), target: nil, action: #selector(ScanWindowController.overviewAction(_:)))
    let hint = NSTextField(wrappingLabelWithString: "")
    private var resolutions = ScanSettings.resolutions
    private var sources: [ScanSource] = ScanSource.allCases
    private var supportsDuplex = false
    private var capabilitiesKnown = false
    private var mainGrid: NSGridView?

    override func loadView() {
        let root = NSView()
        let content = FlippedView()
        let stack = NSStackView()
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 8
        stack.edgeInsets = NSEdgeInsets(top: 14, left: 16, bottom: 16, right: 16)

        // Preset
        reloadPresets()
        preset.target = self
        preset.action = #selector(presetChanged)
        preset.toolTip = L("Ready-made settings for documents, photos, receipts and drafts")

        source.segmentStyle = .rounded
        source.trackingMode = .selectOne
        source.target = self
        source.action = #selector(changed)

        color.segmentCount = ColorMode.allCases.count
        for m in ColorMode.allCases {
            color.setLabel([L("In colour"), L("Grey"), L("B&W")][m.rawValue], forSegment: m.rawValue)
            color.setToolTip(m.title, forSegment: m.rawValue)
        }
        color.trackingMode = .selectOne
        color.target = self
        color.action = #selector(changed)

        resolution.target = self
        resolution.action = #selector(changed)
        for p in PaperSize.allCases { paper.addItem(withTitle: p.title) }
        paper.target = self
        paper.action = #selector(changed)
        for f in [customW, customH] {
            f.controlSize = .small
            f.alignment = .right
            f.target = self
            f.action = #selector(changed)
            f.delegate = self
            f.translatesAutoresizingMaskIntoConstraints = false
            f.widthAnchor.constraint(equalToConstant: 54).isActive = true
            let n = NumberFormatter()
            n.minimum = 1
            n.maximum = 60
            n.maximumFractionDigits = 1
            f.formatter = n
        }
        customRow.setViews([customW, NSTextField(labelWithString: "×"), customH, NSTextField(labelWithString: L("cm"))], in: .leading)
        customRow.spacing = 4

        for e in Enhancement.allCases { enhancement.addItem(withTitle: e.title) }
        enhancement.target = self
        enhancement.action = #selector(changed)
        for b in [duplex, crop, rotate, blank, photos, descreen] {
            b.target = self
            b.action = #selector(changed)
        }
        crop.toolTip = L("Finds the page edges, cuts away the glass and straightens tilted pages")
        rotate.toolTip = L("Reads the text direction and turns sideways or upside-down pages")
        blank.toolTip = L("Drops pages with almost no ink (back sides of single-sided sheets)")
        photos.toolTip = L("Finds every photo lying on the glass and saves each as its own straight image")
        descreen.toolTip = L("Removes the dot pattern of printed pictures")

        // More options
        more.setButtonType(.pushOnPushOff)
        more.bezelStyle = .disclosure
        more.title = ""
        more.target = self
        more.action = #selector(toggleMore)
        let moreLabel = NSTextField(labelWithString: L("More Options"))
        moreLabel.font = .systemFont(ofSize: 12, weight: .medium)
        let moreRow = NSStackView(views: [more, moreLabel])
        moreRow.spacing = 4
        let moreClick = NSClickGestureRecognizer(target: self, action: #selector(toggleMoreLabel))
        moreLabel.addGestureRecognizer(moreClick)

        depth.addItems(withTitles: [L("8 bits per channel"), L("16 bits per channel")])
        depth.target = self
        depth.action = #selector(changed)
        for s in [brightness, contrast, threshold] {
            s.target = self
            s.action = #selector(changed)
            s.isContinuous = false
            s.controlSize = .small
        }
        threshold.toolTip = L("Black & White: where gray becomes black (middle = automatic)")
        let advancedGrid = grid([
            (L("Bit depth"), depth), (L("Brightness"), brightness), (L("Contrast"), contrast), (L("B&W threshold"), threshold),
        ])
        advanced.setViews([advancedGrid, descreen], in: .leading)
        advanced.orientation = .vertical
        advanced.alignment = .leading
        advanced.spacing = 8

        let main = grid([
            (L("Preset"), preset), (L("Source"), source), ("", duplex), (L("Colour"), color), (L("Resolution"), resolution),
            (L("Paper size"), paper), ("", customRow),
        ])
        mainGrid = main
        let cleanup = grid([(L("Clean-up"), enhancement)])
        let checks = NSStackView(views: [crop, rotate, blank, photos])
        checks.orientation = .vertical
        checks.alignment = .leading
        checks.spacing = 6

        stack.setViews([header(L("Scan")), main, separator(), header(L("After scanning")), cleanup, checks, separator(), moreRow, advanced], in: .top)
        stack.setCustomSpacing(12, after: main)
        stack.setCustomSpacing(12, after: checks)
        for v in stack.arrangedSubviews where v is NSBox || v is NSGridView {
            v.widthAnchor.constraint(equalTo: stack.widthAnchor, constant: -32).isActive = true
        }
        advancedGrid.widthAnchor.constraint(equalTo: stack.widthAnchor, constant: -32).isActive = true
        source.segmentDistribution = .fillEqually
        color.segmentDistribution = .fillEqually
        // One label width for all sections, so the controls line up.
        let all = [main, cleanup, advancedGrid]
        let labelWidth = all.flatMap { g in (0..<g.numberOfRows).compactMap { g.cell(atColumnIndex: 0, rowIndex: $0).contentView?.fittingSize.width } }.max() ?? 80
        all.forEach { $0.column(at: 0).width = ceil(labelWidth) }

        stack.translatesAutoresizingMaskIntoConstraints = false
        content.addSubview(stack)
        let scroll = NSScrollView()
        scroll.drawsBackground = false
        scroll.hasVerticalScroller = true
        scroll.autohidesScrollers = true
        scroll.borderType = .noBorder
        scroll.documentView = content
        content.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: content.leadingAnchor), stack.trailingAnchor.constraint(equalTo: content.trailingAnchor),
            stack.topAnchor.constraint(equalTo: content.topAnchor), stack.bottomAnchor.constraint(equalTo: content.bottomAnchor),
            content.leadingAnchor.constraint(equalTo: scroll.contentView.leadingAnchor),
            content.trailingAnchor.constraint(equalTo: scroll.contentView.trailingAnchor),
            content.topAnchor.constraint(equalTo: scroll.contentView.topAnchor),
        ])

        // Bottom: Overview + the big Scan button, and a one-line hint.
        scanButton.bezelStyle = .push
        scanButton.controlSize = .large
        scanButton.keyEquivalent = "\r"
        scanButton.font = .systemFont(ofSize: 15, weight: .semibold)
        scanButton.image = Theme42.symbol("scanner", size: 15, weight: .semibold)
        scanButton.imagePosition = .imageLeading
        // Default button in the system accent, like the segmented controls next to it; the app colour stays on the icon.
        scanButton.toolTip = Cmd.scan.fullToolTip
        overviewButton.bezelStyle = .push
        overviewButton.controlSize = .large
        overviewButton.toolTip = Cmd.overview.fullToolTip
        hint.font = .systemFont(ofSize: 11)
        hint.textColor = .secondaryLabelColor
        hint.alignment = .center
        let buttons = NSStackView(views: [overviewButton, scanButton])
        buttons.distribution = .fill
        buttons.spacing = 8
        scanButton.setContentHuggingPriority(.defaultLow, for: .horizontal)
        let bottom = NSStackView(views: [buttons, hint])
        bottom.orientation = .vertical
        bottom.spacing = 6
        bottom.edgeInsets = NSEdgeInsets(top: 12, left: 16, bottom: 14, right: 16)
        let line = separator()

        for v in [scroll, line, bottom] as [NSView] {
            v.translatesAutoresizingMaskIntoConstraints = false
            root.addSubview(v)
        }
        NSLayoutConstraint.activate([
            scroll.leadingAnchor.constraint(equalTo: root.leadingAnchor), scroll.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            scroll.topAnchor.constraint(equalTo: root.topAnchor), scroll.bottomAnchor.constraint(equalTo: line.topAnchor),
            line.leadingAnchor.constraint(equalTo: root.leadingAnchor), line.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            line.bottomAnchor.constraint(equalTo: bottom.topAnchor),
            bottom.leadingAnchor.constraint(equalTo: root.leadingAnchor), bottom.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            bottom.bottomAnchor.constraint(equalTo: root.bottomAnchor),
            buttons.widthAnchor.constraint(equalTo: bottom.widthAnchor, constant: -32),
            hint.widthAnchor.constraint(equalTo: bottom.widthAnchor, constant: -32),
            root.widthAnchor.constraint(greaterThanOrEqualToConstant: 280),
        ])
        view = root
        update(scanner: nil)
        apply(settings)
        advanced.isHidden = true
    }

    private func header(_ s: String) -> NSTextField {
        let t = NSTextField(labelWithString: s.uppercased())
        t.font = .systemFont(ofSize: 10.5, weight: .semibold)
        t.textColor = .secondaryLabelColor
        return t
    }

    private func separator() -> NSBox {
        let b = NSBox()
        b.boxType = .separator
        return b
    }

    private func grid(_ rows: [(String, NSView)]) -> NSGridView {
        let g = NSGridView(views: rows.map { r -> [NSView] in
            let l = NSTextField(labelWithString: r.0)
            l.textColor = .secondaryLabelColor
            l.alignment = .right
            return [l, r.1]
        })
        g.rowSpacing = 8
        g.columnSpacing = 8
        g.column(at: 0).xPlacement = .trailing
        g.rowAlignment = .firstBaseline
        // Labels take what they need; controls fill the rest of the panel (long translations still fit).
        g.column(at: 1).xPlacement = .fill
        for (_, v) in rows where v is NSPopUpButton || v is NSSegmentedControl || v is NSSlider {
            v.translatesAutoresizingMaskIntoConstraints = false
            v.widthAnchor.constraint(greaterThanOrEqualToConstant: 150).isActive = true
            v.setContentHuggingPriority(.defaultLow, for: .horizontal)
            if let p = v as? NSPopUpButton { (p.cell as? NSPopUpButtonCell)?.lineBreakMode = .byTruncatingTail }
        }
        return g
    }

    func reloadPresets() {
        preset.removeAllItems()
        for p in Store.allPresets { preset.addItem(withTitle: p.name) }
        preset.addItem(withTitle: L("Custom"))
        let i = Store.presetIndex
        preset.selectItem(at: i >= 0 && i < Store.allPresets.count ? i : preset.numberOfItems - 1)
    }

    func markCustom() {
        Store.presetIndex = -1
        preset.selectItem(at: preset.numberOfItems - 1)
    }

    /// The scanner's possibilities: sources, duplex, resolutions.
    func update(scanner: ScannerBackend?, capabilitiesReady: Bool = true) {
        sources = scanner?.sources ?? [.flatbed, .feeder]
        source.segmentCount = sources.count
        for (i, s) in sources.enumerated() {
            source.setLabel(s == .flatbed ? L("Glass") : s == .feeder ? L("Feeder") : L("Film"), forSegment: i)
            source.setToolTip(s.title, forSegment: i)
            source.setWidth(0, forSegment: i)
        }
        resolutions = Array(Set(scanner?.resolutions.filter { (50...4800).contains($0) } ?? ScanSettings.resolutions)).sorted()
        if resolutions.isEmpty { resolutions = ScanSettings.resolutions }
        resolution.removeAllItems()
        for r in resolutions { resolution.addItem(withTitle: L("%d dpi", r)) }
        let enabled = scanner != nil
        for c in [source, color, resolution, paper, preset] as [NSControl] { c.isEnabled = enabled || c === preset || c === color }
        supportsDuplex = scanner?.supportsDuplex ?? false
        capabilitiesKnown = scanner != nil && capabilitiesReady
        duplex.isEnabled = capabilitiesKnown && supportsDuplex
        apply(settings)
    }

    func apply(_ s: ScanSettings) {
        var s = s.normalized
        if !sources.contains(s.source), let source = sources.first { s.source = source }
        if s.source != settings.source { capabilitiesKnown = false }
        resolution.isEnabled = capabilitiesKnown
        duplex.isEnabled = capabilitiesKnown && supportsDuplex
        if capabilitiesKnown { s.resolution = resolutions.min { abs($0 - s.resolution) < abs($1 - s.resolution) } ?? s.resolution }
        if s.source != .feeder || (capabilitiesKnown && !supportsDuplex) { s.duplex = false }
        settings = s
        source.selectedSegment = sources.firstIndex(of: s.source) ?? 0
        duplex.state = s.duplex ? .on : .off
        mainGrid?.row(at: 2).isHidden = s.source != .feeder
        color.selectedSegment = s.colorMode.rawValue
        let r = resolutions.min { abs($0 - s.resolution) < abs($1 - s.resolution) } ?? s.resolution
        resolution.selectItem(at: resolutions.firstIndex(of: r) ?? 0)
        paper.selectItem(at: s.paper.rawValue)
        mainGrid?.row(at: 6).isHidden = s.paper != .custom
        customW.doubleValue = (s.customWidth * 2.54 * 10).rounded() / 10
        customH.doubleValue = (s.customHeight * 2.54 * 10).rounded() / 10
        crop.state = s.autoCrop ? .on : .off
        rotate.state = s.autoRotate ? .on : .off
        blank.state = s.removeBlank ? .on : .off
        photos.state = s.detectPhotos ? .on : .off
        photos.isHidden = s.source != .flatbed
        descreen.state = s.descreen ? .on : .off
        enhancement.selectItem(at: s.enhancement.rawValue)
        depth.selectItem(at: s.bitDepth == 16 ? 1 : 0)
        brightness.doubleValue = s.brightness
        contrast.doubleValue = s.contrast
        threshold.doubleValue = s.threshold
        threshold.isEnabled = s.colorMode == .bw
        hint.stringValue = s.source == .feeder ? L("Every page in the feeder is scanned and added to the document.")
            : L("Each scan adds pages to the document. Drag on the overview to scan only a part.")
    }

    private func read() -> ScanSettings {
        var s = settings
        if sources.indices.contains(source.selectedSegment) { s.source = sources[source.selectedSegment] }
        let currentCapabilities = capabilitiesKnown && s.source == settings.source
        if currentCapabilities { s.duplex = supportsDuplex && duplex.state == .on }
        s.colorMode = ColorMode(rawValue: color.selectedSegment) ?? .color
        if currentCapabilities, resolutions.indices.contains(resolution.indexOfSelectedItem) { s.resolution = resolutions[resolution.indexOfSelectedItem] }
        s.paper = PaperSize(rawValue: paper.indexOfSelectedItem) ?? .auto
        if customW.doubleValue > 0 { s.customWidth = customW.doubleValue / 2.54 }
        if customH.doubleValue > 0 { s.customHeight = customH.doubleValue / 2.54 }
        s.autoCrop = crop.state == .on
        s.deskew = s.autoCrop
        s.autoRotate = rotate.state == .on
        s.removeBlank = blank.state == .on
        s.detectPhotos = photos.state == .on
        s.descreen = descreen.state == .on
        s.enhancement = Enhancement(rawValue: enhancement.indexOfSelectedItem) ?? .document
        s.bitDepth = depth.indexOfSelectedItem == 1 ? 16 : 8
        s.brightness = abs(brightness.doubleValue) < 0.04 ? 0 : brightness.doubleValue
        s.contrast = abs(contrast.doubleValue) < 0.04 ? 0 : contrast.doubleValue
        s.threshold = abs(threshold.doubleValue - 0.5) < 0.03 ? 0.5 : threshold.doubleValue
        return s.normalized
    }

    var hasValidInput: Bool {
        guard PaperSize(rawValue: paper.indexOfSelectedItem) == .custom else { return true }
        return [customW, customH].allSatisfy { field in
            guard let formatter = field.formatter as? NumberFormatter,
                  let number = formatter.number(from: field.stringValue)?.doubleValue else { return false }
            return number.isFinite && (1...60).contains(number)
        }
    }

    func controlTextDidChange(_ notification: Notification) { onChange?(settings) }
    func controlTextDidEndEditing(_ notification: Notification) { changed() }

    func commitPendingInput() -> Bool {
        guard hasValidInput else { return false }
        guard view.window?.makeFirstResponder(nil) != false else { return false }
        changed()
        return hasValidInput
    }

    @objc private func changed() {
        guard hasValidInput else { onChange?(settings); return }
        let s = read()
        guard s != settings else { return }
        apply(s)
        Store.settings = settings
        markCustom()
        onChange?(settings)
    }

    @objc private func presetChanged() {
        let i = preset.indexOfSelectedItem
        guard i < Store.allPresets.count else { return }
        onPreset?(i)
    }

    /// Applies a preset, keeping the chosen source (a preset is about the original, not the scanner part).
    func choosePreset(_ i: Int) {
        guard Store.allPresets.indices.contains(i) else { return }
        var s = Store.allPresets[i].settings
        s.source = settings.source
        s.duplex = settings.duplex
        Store.presetIndex = i
        preset.selectItem(at: i)
        apply(s)
        Store.settings = settings
        onChange?(settings)
    }

    @objc private func toggleMore() {
        advanced.isHidden = more.state == .off
    }

    @objc private func toggleMoreLabel() {
        more.state = more.state == .on ? .off : .on
        toggleMore()
    }

    func showMore(_ on: Bool) {
        more.state = on ? .on : .off
        toggleMore()
    }
}
