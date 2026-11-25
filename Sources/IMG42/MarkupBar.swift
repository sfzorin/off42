import AppKit
import Kit42

/// Markup strip under the toolbar, modelled on Preview's markup bar:
/// [↖ Selection▾ Instant Alpha Sketch Draw] [Shapes▾ Text Sign Adjust Size] [Style▾ Border Fill Aa▾ Description]
final class MarkupBar: NSObject, NSMenuDelegate {
    weak var owner: ImageWindowController?
    private(set) var strip: ToolStrip!
    var style = MarkStyle()

    let selectionButton = StripButton(symbol: "rectangle.dashed", toolTip: L("Selection Tools"))
    let shapesButton = StripButton(symbol: "square.on.circle", toolTip: L("Shapes"))
    let styleButton = StripButton(symbol: "line.3.horizontal", toolTip: L("Shape Style"))
    let textButton = StripButton(symbol: "textformat", toolTip: L("Text Style"))
    let borderButton = ColorButton(symbol: "square", toolTip: L("Border colour"), color: .systemRed, allowsNone: true)
    let fillButton = ColorButton(symbol: "square.fill", toolTip: L("Fill colour"), color: nil, allowsNone: true)
    private var lastSelectionTool: MarkTool = .selectRect
    private var lastShapeTool: MarkTool = .rectangle

    static let widths: [CGFloat] = [1, 2, 3, 5, 8, 12]
    static let fonts = ["Helvetica Neue", "Avenir Next", "Georgia", "Times New Roman", "Menlo", "Marker Felt", "Noteworthy", "Snell Roundhand"]
    static let fontSizes: [CGFloat] = [10, 12, 14, 18, 24, 32, 48, 64, 96]
    static let textColors: [(String, NSColor)] = [(L("Black"), .black), (L("White"), .white), (L("Red"), .systemRed),
                                                   (L("Blue"), .systemBlue), (L("Green"), .systemGreen), (L("Yellow"), .systemYellow)]

    init(owner: ImageWindowController) {
        self.owner = owner
        super.init()
        for b in [selectionButton, shapesButton, styleButton, textButton] {
            let menu = NSMenu()
            menu.delegate = self
            b.dropDown = menu
        }
        borderButton.target = self
        borderButton.action = #selector(borderChanged(_:))
        fillButton.target = self
        fillButton.action = #selector(fillChanged(_:))
        strip = ToolStrip([
            .group([.tool(Cmd.tool(.select)), .view(selectionButton), .tool(Cmd.tool(.instantAlpha)), .tool(Cmd.tool(.sketch)), .tool(Cmd.tool(.draw))]),
            .group([.view(shapesButton), .tool(Cmd.tool(.text)), .command(Cmd.stamps), .command(Cmd.adjust), .command(Cmd.resize)]),
            .group([.view(styleButton), .view(borderButton), .view(fillButton), .view(textButton), .command(Cmd.imageDescription)]),
        ])
        rebuildMenus()
    }

    // MARK: Tools

    func setTool(_ tool: MarkTool) {
        guard let owner else { return }
        let markup = owner.canvas.markup
        markup.markupActive = true
        markup.tool = tool
        markup.style = style
        if tool != .select { markup.selectedID = nil }
        strip.select(tag: tool.rawValue)
        if MarkTool.selectionTools.contains(tool) { lastSelectionTool = tool; selectionButton.setSymbol(tool.symbol) }
        if MarkTool.shapeTools.contains(tool) { lastShapeTool = tool; shapesButton.setSymbol(tool.symbol) }
        selectionButton.isOn = MarkTool.selectionTools.contains(tool)
        shapesButton.isOn = MarkTool.shapeTools.contains(tool)
        if tool == .highlighter {
            // The toolbar's Highlight pen is not in the strip: no strip button lights up.
            strip.select(tag: -1)
        }
        if !strip.isShown { strip.isShown = true }
        selectionChanged()
    }

    func show(_ on: Bool) {
        strip.isShown = on
        owner?.canvas.markup.markupActive = on
        if on { setTool(owner?.canvas.markup.tool ?? .select) }
    }

    // MARK: Menus

    private func item(_ title: String, _ action: Selector, tag: Int = 0, symbol: String? = nil, on: Bool = false, image: NSImage? = nil) -> NSMenuItem {
        let i = NSMenuItem(title: title, action: action, keyEquivalent: "")
        i.target = self
        i.tag = tag
        i.state = on ? .on : .off
        if let symbol { i.image = Theme42.symbol(symbol, size: 13) }
        if let image { i.image = image }
        return i
    }

    func menuNeedsUpdate(_ menu: NSMenu) { rebuildMenus() }

    private func rebuildMenus() {
        let tool = owner?.canvas.markup.tool ?? .select
        let sel = owner?.canvas.markup.pixelSelection != nil
        let selMark = owner?.canvas.markup.selected
        let s = currentStyle

        // Selection tools
        if let m = selectionButton.dropDown {
            m.removeAllItems()
            for t in MarkTool.selectionTools { m.addItem(item(t.title, #selector(toolChosen(_:)), tag: t.rawValue, symbol: t.symbol, on: t == tool)) }
            m.addItem(.separator())
            let aspect = NSMenuItem(title: L("Aspect Ratio"), action: nil, keyEquivalent: "")
            aspect.submenu = NSMenu()
            for a in CropAspect.allCases {
                aspect.submenu?.addItem(item(a.title, #selector(aspectChosen(_:)), tag: a.rawValue, on: owner?.canvas.markup.aspect == a))
            }
            m.addItem(aspect)
            m.addItem(.separator())
            for c in [Cmd.cropToSelection, Cmd.deleteSelection] {
                let i = c.menuItem()
                i.target = owner
                i.isEnabled = sel
                m.addItem(i)
            }
            let copy = item(L("Copy Selection"), #selector(copySelection(_:)), symbol: "doc.on.doc")
            copy.isEnabled = sel
            m.addItem(copy)
            let inv = Cmd.invertSelection.menuItem()
            inv.target = owner
            m.addItem(inv)
            m.autoenablesItems = false
            inv.isEnabled = sel
        }
        // Shapes
        if let m = shapesButton.dropDown {
            m.removeAllItems()
            for (i, t) in MarkTool.shapeTools.enumerated() {
                if i == 8 { m.addItem(.separator()) }
                m.addItem(item(t.title, #selector(toolChosen(_:)), tag: t.rawValue, symbol: t.symbol, on: t == tool))
            }
        }
        // Shape style
        if let m = styleButton.dropDown {
            m.removeAllItems()
            let width = selMark.map { $0.width * (owner?.canvas.markup.screenScale ?? 1) } ?? s.width
            for (i, w) in Self.widths.enumerated() {
                m.addItem(item("\(Int(w)) pt", #selector(widthChosen(_:)), tag: i, on: abs(width - w) < 0.5, image: Self.lineImage(w)))
            }
            m.addItem(.separator())
            m.addItem(item(L("Solid Line"), #selector(dashChosen(_:)), tag: 0, on: !s.dashed, image: Self.lineImage(2, dashed: false)))
            m.addItem(item(L("Dashed Line"), #selector(dashChosen(_:)), tag: 1, on: s.dashed, image: Self.lineImage(2, dashed: true)))
            m.addItem(.separator())
            m.addItem(item(L("Arrow at Start"), #selector(arrowChosen(_:)), tag: 0, symbol: "arrow.left", on: s.arrowStart))
            m.addItem(item(L("Arrow at End"), #selector(arrowChosen(_:)), tag: 1, symbol: "arrow.right", on: s.arrowEnd))
            m.addItem(.separator())
            m.addItem(item(L("Shadow"), #selector(shadowChosen(_:)), symbol: "shadow", on: s.shadow))
            let loupe = NSMenuItem(title: L("Loupe Magnification"), action: nil, keyEquivalent: "")
            loupe.submenu = NSMenu()
            for z: CGFloat in [1.5, 2, 3, 4, 6] {
                loupe.submenu?.addItem(item(String(format: "%g×", z), #selector(loupeChosen(_:)), tag: Int(z * 10), on: abs((selMark?.zoom ?? s.loupeZoom) - z) < 0.01))
            }
            m.addItem(loupe)
            m.autoenablesItems = false
            for entry in m.items {
                switch entry.action {
                case #selector(widthChosen(_:)): entry.isEnabled = selMark?.supportsStroke ?? true
                case #selector(dashChosen(_:)): entry.isEnabled = selMark?.supportsDash ?? true
                case #selector(arrowChosen(_:)): entry.isEnabled = selMark.map { $0.kind == .line } ?? true
                case #selector(shadowChosen(_:)): entry.isEnabled = selMark?.supportsShadow ?? true
                default: break
                }
            }
            loupe.isEnabled = selMark.map { $0.kind == .loupe } ?? true
        }
        // Text style
        if let m = textButton.dropDown {
            m.removeAllItems()
            let font = NSMenuItem(title: L("Font"), action: nil, keyEquivalent: "")
            font.submenu = NSMenu()
            for (i, f) in Self.fonts.enumerated() {
                let it = item(f, #selector(fontChosen(_:)), tag: i, on: s.fontName == f)
                if let nf = NSFont(name: f, size: 13) { it.attributedTitle = NSAttributedString(string: f, attributes: [.font: nf]) }
                font.submenu?.addItem(it)
            }
            m.addItem(font)
            let size = NSMenuItem(title: L("Size"), action: nil, keyEquivalent: "")
            size.submenu = NSMenu()
            for (i, z) in Self.fontSizes.enumerated() { size.submenu?.addItem(item("\(Int(z)) pt", #selector(fontSizeChosen(_:)), tag: i, on: abs(s.fontSize - z) < 0.5)) }
            m.addItem(size)
            let color = NSMenuItem(title: L("Text Colour"), action: nil, keyEquivalent: "")
            color.submenu = NSMenu()
            for (i, c) in Self.textColors.enumerated() {
                color.submenu?.addItem(item(c.0, #selector(textColorChosen(_:)), tag: i, on: s.textColor == c.1, image: ImageToolbar.swatch(c.1)))
            }
            color.submenu?.addItem(.separator())
            color.submenu?.addItem(item(L("Show Colours"), #selector(showColorPanel(_:)), symbol: "paintpalette"))
            m.addItem(color)
            m.addItem(.separator())
            m.addItem(item(L("Bold"), #selector(traitChosen(_:)), tag: 0, symbol: "bold", on: s.bold))
            m.addItem(item(L("Italic"), #selector(traitChosen(_:)), tag: 1, symbol: "italic", on: s.italic))
            m.addItem(item(L("Underline"), #selector(traitChosen(_:)), tag: 2, symbol: "underline", on: s.underline))
            m.addItem(.separator())
            let aligns: [(String, String, NSTextAlignment)] = [(L("Align Left"), "text.alignleft", .left), (L("Align Centre"), "text.aligncenter", .center),
                                                               (L("Align Right"), "text.alignright", .right)]
            for (i, a) in aligns.enumerated() { m.addItem(item(a.0, #selector(alignChosen(_:)), tag: i, symbol: a.1, on: s.alignment == a.2)) }
        }
    }

    /// Style shown in the controls: the selected mark's, else the one for new marks.
    var currentStyle: MarkStyle {
        guard let m = owner?.canvas.markup.selected else { return style }
        var s = style
        s.stroke = m.color
        s.width = m.width * (owner?.canvas.markup.screenScale ?? 1)
        s.fill = m.fill
        s.dashed = m.dashed
        s.shadow = m.shadow
        s.arrowStart = m.arrowStart
        s.arrowEnd = m.arrowEnd
        s.loupeZoom = m.zoom
        if m.kind == .text {
            s.fontName = m.fontName
            s.bold = m.bold
            s.italic = m.italic
            s.underline = m.underline
            s.alignment = m.alignment
            s.textColor = m.textColor
            s.fontSize = m.fontSize * (owner?.canvas.markup.screenScale ?? 1)
        }
        return s
    }

    func selectionChanged() {
        let s = currentStyle
        borderButton.color = s.stroke == .clear ? nil : s.stroke
        fillButton.color = s.fill
        let mark = owner?.canvas.markup.selected
        borderButton.isEnabled = mark?.supportsStroke ?? true
        fillButton.isEnabled = mark?.supportsFill ?? true
        styleButton.isEnabled = mark.map { $0.supportsStroke || $0.supportsShadow } ?? true
        textButton.isEnabled = mark.map { $0.kind == .text } ?? true
    }

    static func lineImage(_ w: CGFloat, dashed: Bool = false) -> NSImage {
        let img = NSImage(size: NSSize(width: 36, height: 14), flipped: false) { r in
            let p = NSBezierPath()
            p.move(to: NSPoint(x: 2, y: r.midY))
            p.line(to: NSPoint(x: r.maxX - 2, y: r.midY))
            p.lineWidth = min(w, 10)
            if dashed { p.setLineDash([5, 3], count: 2, phase: 0) }
            NSColor.black.setStroke()
            p.stroke()
            return true
        }
        img.isTemplate = true
        return img
    }

    // MARK: Actions

    /// Applies a style change to the new-mark style and to the selected mark.
    private func change(_ name: String, style update: (inout MarkStyle) -> Void, mark: (inout Mark) -> Void) {
        update(&style)
        owner?.canvas.markup.style = style
        owner?.canvas.markup.updateSelected(name, mark)
        selectionChanged()
    }

    @objc func toolChosen(_ sender: NSMenuItem) { setTool(MarkTool(rawValue: sender.tag) ?? .select) }

    @objc func aspectChosen(_ sender: NSMenuItem) {
        owner?.canvas.markup.aspect = CropAspect(rawValue: sender.tag) ?? .free
        if !MarkTool.selectionTools.contains(owner?.canvas.markup.tool ?? .select) { setTool(.selectRect) }
    }

    @objc func copySelection(_ sender: Any?) { owner?.copy(nil) }

    @objc func widthChosen(_ sender: NSMenuItem) {
        guard Self.widths.indices.contains(sender.tag), owner?.canvas.markup.selected?.supportsStroke != false else { return }
        let w = Self.widths[sender.tag]
        let scale = owner?.canvas.markup.screenScale ?? 1
        change(L("Line width"), style: { $0.width = w }, mark: { $0.width = w / scale })
    }

    @objc func dashChosen(_ sender: NSMenuItem) {
        guard owner?.canvas.markup.selected?.supportsDash != false else { return }
        let on = sender.tag == 1
        change(L("Shape Style"), style: { $0.dashed = on }, mark: { $0.dashed = on })
    }

    @objc func arrowChosen(_ sender: NSMenuItem) {
        guard owner?.canvas.markup.selected.map({ $0.kind == .line }) != false else { return }
        let s = currentStyle
        if sender.tag == 0 {
            change(L("Shape Style"), style: { $0.arrowStart = !s.arrowStart }, mark: { $0.arrowStart = !s.arrowStart })
        } else {
            change(L("Shape Style"), style: { $0.arrowEnd = !s.arrowEnd }, mark: { $0.arrowEnd = !s.arrowEnd })
        }
    }

    @objc func shadowChosen(_ sender: NSMenuItem) {
        guard owner?.canvas.markup.selected?.supportsShadow != false else { return }
        let on = !currentStyle.shadow
        change(L("Shadow"), style: { $0.shadow = on }, mark: { $0.shadow = on })
    }

    @objc func loupeChosen(_ sender: NSMenuItem) {
        guard [15, 20, 30, 40, 60].contains(sender.tag), owner?.canvas.markup.selected.map({ $0.kind == .loupe }) != false else { return }
        let z = CGFloat(sender.tag) / 10
        change(L("Loupe Magnification"), style: { $0.loupeZoom = z }, mark: { if $0.kind == .loupe { $0.zoom = z } })
    }

    @objc func borderChanged(_ sender: ColorButton) {
        guard owner?.canvas.markup.selected?.supportsStroke != false else { return }
        let c = sender.color ?? .clear
        change(L("Border Colour"), style: { $0.stroke = c }, mark: { $0.color = c })
    }

    @objc func fillChanged(_ sender: ColorButton) {
        guard owner?.canvas.markup.selected?.supportsFill != false else { return }
        let c = sender.color
        change(L("Fill Colour"), style: { $0.fill = c }, mark: { $0.fill = c })
    }

    @objc func fontChosen(_ sender: NSMenuItem) {
        guard Self.fonts.indices.contains(sender.tag), owner?.canvas.markup.selected.map({ $0.kind == .text }) != false else { return }
        let f = Self.fonts[sender.tag]
        change(L("Font"), style: { $0.fontName = f }, mark: { $0.fontName = f })
    }

    @objc func fontSizeChosen(_ sender: NSMenuItem) {
        guard Self.fontSizes.indices.contains(sender.tag), owner?.canvas.markup.selected.map({ $0.kind == .text }) != false else { return }
        let z = Self.fontSizes[sender.tag]
        let scale = owner?.canvas.markup.screenScale ?? 1
        change(L("Size"), style: { $0.fontSize = z }, mark: { $0.fontSize = z / scale })
    }

    @objc func textColorChosen(_ sender: NSMenuItem) {
        guard Self.textColors.indices.contains(sender.tag), owner?.canvas.markup.selected.map({ $0.kind == .text }) != false else { return }
        let c = Self.textColors[sender.tag].1
        change(L("Text Colour"), style: { $0.textColor = c }, mark: { $0.textColor = c })
    }

    @objc func showColorPanel(_ sender: Any?) {
        let panel = NSColorPanel.shared
        panel.setTarget(self)
        panel.setAction(#selector(colorPanelChanged(_:)))
        panel.color = currentStyle.textColor
        panel.orderFront(nil)
    }

    @objc func colorPanelChanged(_ sender: NSColorPanel) {
        guard owner?.canvas.markup.selected.map({ $0.kind == .text }) != false else { return }
        let c = sender.color
        change(L("Text Colour"), style: { $0.textColor = c }, mark: { $0.textColor = c })
    }

    @objc func traitChosen(_ sender: NSMenuItem) {
        guard (0...2).contains(sender.tag), owner?.canvas.markup.selected.map({ $0.kind == .text }) != false else { return }
        let s = currentStyle
        switch sender.tag {
        case 0: change(L("Bold"), style: { $0.bold = !s.bold }, mark: { $0.bold = !s.bold })
        case 1: change(L("Italic"), style: { $0.italic = !s.italic }, mark: { $0.italic = !s.italic })
        default: change(L("Underline"), style: { $0.underline = !s.underline }, mark: { $0.underline = !s.underline })
        }
    }

    @objc func alignChosen(_ sender: NSMenuItem) {
        guard (0...2).contains(sender.tag), owner?.canvas.markup.selected.map({ $0.kind == .text }) != false else { return }
        let a: NSTextAlignment = [.left, .center, .right][max(0, min(2, sender.tag))]
        change(L("Alignment"), style: { $0.alignment = a }, mark: { $0.alignment = a })
    }

    /// Item titles of every drop-down (headless checks: `K42_DUMP_MENUS=1`).
    func dumpMenus() -> String {
        rebuildMenus()
        func dump(_ menu: NSMenu, _ indent: String) -> [String] {
            menu.items.flatMap { i -> [String] in
                if i.isSeparatorItem { return [indent + "—"] }
                return [indent + (i.state == .on ? "✓ " : "  ") + i.title] + (i.submenu.map { dump($0, indent + "    ") } ?? [])
            }
        }
        return [(L("Selection Tools"), selectionButton), (L("Shapes"), shapesButton), (L("Shape Style"), styleButton), (L("Text Style"), textButton)]
            .map { name, b in "[\(name)]\n" + dump(b.dropDown!, "  ").joined(separator: "\n") }.joined(separator: "\n")
    }
}
