import AppKit
import Kit42
import UniformTypeIdentifiers

/// Pasteboard used by copy/paste (a private one in test runs, so tests never touch the user's clipboard).
enum Clipboard {
    static var board: NSPasteboard {
        ProcessInfo.processInfo.environment["K42_PASTEBOARD"] != nil ? NSPasteboard(name: NSPasteboard.Name("com.sfzorin.ppt42.test")) : .general
    }
    static let objectsType = NSPasteboard.PasteboardType("com.sfzorin.ppt42.objects")
    static let slidesType = NSPasteboard.PasteboardType("com.sfzorin.ppt42.slides")
    /// Objects and slides live in memory; the pasteboard carries a token (and a picture / text for other apps).
    static var objects: (token: String, items: [SlideObject], media: [String: Data])?
    static var slides: (token: String, items: [Slide], media: [String: Data])?

    /// A different document can already use image1.png for different bytes.
    static func importMedia(_ source: [String: Data], into deck: inout Deck) -> [String: String] {
        var names: [String: String] = [:]
        for (name, data) in source.sorted(by: { $0.key < $1.key }) {
            names[name] = deck.addMedia(data, ext: (name as NSString).pathExtension)
        }
        return names
    }

    static func remap(_ object: SlideObject, media names: [String: String]) -> SlideObject {
        var object = object
        if let name = object.media { object.media = names[name] ?? name }
        if let name = object.clip?.media { object.clip?.media = names[name] ?? name }
        if case .picture(let name) = object.fill { object.fill = .picture(names[name] ?? name) }
        object.children = object.children.map { remap($0, media: names) }
        return object
    }
}

extension DeckWindowController {
    // MARK: - Committing

    /// Writes an edited version of the displayed slide back (master view: into the master).
    func commit(slide s: Slide, name: String) {
        if mode == .master, let li = editedLayout {
            doc?.change(name) { deck in
                for o in s.objects {
                    guard let i = deck.layouts[li].placeholders.firstIndex(where: { $0.id == o.id }) else { continue }
                    deck.layouts[li].placeholders[i].frame = o.frame
                    deck.layouts[li].placeholders[i].rotation = o.rotation
                    deck.layouts[li].placeholders[i].fill = o.fill
                    deck.layouts[li].placeholders[i].line = o.line
                    deck.layouts[li].placeholders[i].shadow = o.shadow
                    deck.layouts[li].placeholders[i].opacity = o.opacity
                    deck.layouts[li].placeholders[i].adjust = o.adjust
                    if let t = o.text, var lt = deck.layouts[li].placeholders[i].text {
                        lt.anchor = t.anchor; lt.insets = t.insets; lt.autofit = t.autofit; lt.listStyle = t.listStyle
                        deck.layouts[li].placeholders[i].text = lt
                    }
                }
            }
            return
        }
        if mode == .master {
            doc?.change(name) { deck in
                for o in s.objects {
                    guard let i = deck.master.placeholders.firstIndex(where: { $0.id == o.id }) else { continue }
                    deck.master.placeholders[i].frame = o.frame
                    deck.master.placeholders[i].rotation = o.rotation
                    deck.master.placeholders[i].fill = o.fill
                    deck.master.placeholders[i].line = o.line
                    deck.master.placeholders[i].shadow = o.shadow
                    deck.master.placeholders[i].opacity = o.opacity
                    deck.master.placeholders[i].adjust = o.adjust
                    if let t = o.text, var mt = deck.master.placeholders[i].text {
                        mt.anchor = t.anchor; mt.insets = t.insets; mt.autofit = t.autofit; mt.listStyle = t.listStyle
                        deck.master.placeholders[i].text = mt
                    }
                }
            }
            return
        }
        guard deck.slides.indices.contains(current) else { return }
        let i = current
        doc?.change(name) { $0.slides[i] = s }
    }

    func changeSlide(_ name: String, _ body: (inout Slide) -> Void) {
        var s = displayedSlide
        body(&s)
        commit(slide: s, name: name)
    }

    /// Changes every selected object (and only those).
    func changeSelected(_ name: String, _ body: (inout SlideObject) -> Void) {
        let ids = selection
        changeSlide(name) { s in for id in ids { s.update(id, body) } }
    }

    // MARK: - Inserting

    func nextID(_ s: Slide) -> Int { s.maxID + 1 }

    func insert(_ objects: [SlideObject], media: [String: Data] = [:], name: String = L("Insert")) {
        commitEditing()
        guard currentSlide != nil, !objects.isEmpty else { return }
        var merged = deck
        let mediaNames = Clipboard.importMedia(media, into: &merged)
        var s = displayedSlide
        var ids: [Int] = []
        for source in objects {
            var o = Clipboard.remap(source, media: mediaNames)
            o.id = nextID(s)
            var next = o.id + 1
            func renumber(_ list: inout [SlideObject]) { for k in list.indices { list[k].id = next; next += 1; renumber(&list[k].children) } }
            renumber(&o.children)
            s.objects.append(o)
            ids.append(o.id)
        }
        if media.isEmpty { commit(slide: s, name: name) }
        else {
            let slide = current
            doc?.change(name) { d in d.media = merged.media; d.slides[slide] = s }
        }
        selection = ids
    }

    func insertTextBox(in r: CGRect, autoHeight: Bool) {
        var o = SlideObject(id: 0, kind: .textBox, frame: r)
        var t = TextBody()
        t.autofit = .resize
        t.wrap = true
        o.text = t
        if autoHeight { o.frame.size.height = 40 }
        insert([o], name: L("Insert Text Box"))
        if let id = selection.first, let obj = displayedSlide.object(id) { canvas.editor.begin(obj, at: nil) }
    }

    func insertShape(_ prst: String, in r: CGRect) {
        if prst == "line" || prst == "arrow" {
            var o = SlideObject(id: 0, kind: .line, frame: .zero)
            o.geometry = prst == "arrow" ? "straightConnector1" : "line"
            o.setLine(from: CGPoint(x: r.minX, y: r.midY), to: CGPoint(x: r.maxX, y: r.midY))
            o.line = LineStyle(color: PColor(scheme: "tx1"), width: 2, tail: prst == "arrow" ? .triangle : .none)
            insert([o], name: L("Insert Shape"))
            return
        }
        var o = SlideObject(id: 0, kind: .shape, frame: r)
        o.geometry = prst
        o.fill = .solid(PColor(scheme: "accent1"))
        o.line = LineStyle(color: PColor(scheme: "accent1", mods: [.init(name: "shade", val: 50000)]), width: 1)
        var t = TextBody()
        t.anchor = .middle
        var p = Paragraph()
        p.props.align = .center
        p.endProps = RunProps(color: PColor(scheme: "bg1"))
        t.paragraphs = [p]
        t.listStyle[0] = LevelStyle(align: .center, run: RunProps(color: PColor(scheme: "bg1")))
        o.text = t
        insert([o], name: L("Insert Shape"))
    }

    func insertImages(_ datas: [Data], at point: CGPoint?) {
        commitEditing()
        guard currentSlide != nil else { return }
        var d = deck
        var objects: [SlideObject] = []
        var offset: CGFloat = 0
        // An empty picture placeholder takes the first picture.
        let placeholder = point == nil ? displayedSlide.objects.first { $0.placeholder?.type == "pic" && $0.media == nil } : nil
        for data in datas {
            guard let img = NSImage(data: data) else { continue }
            let name = d.addMedia(data, ext: imageExtension(data))
            var size = img.size
            let maxW = deck.size.width * 0.6, maxH = deck.size.height * 0.6
            let k = min(1, maxW / max(1, size.width), maxH / max(1, size.height))
            size = CGSize(width: size.width * k, height: size.height * k)
            let c = point ?? CGPoint(x: deck.size.width / 2, y: deck.size.height / 2)
            var o = SlideObject(id: 0, kind: .picture, frame: CGRect(x: c.x - size.width / 2 + offset, y: c.y - size.height / 2 + offset, width: size.width, height: size.height))
            o.media = name
            o.name = L("Picture")
            if objects.isEmpty, let ph = placeholder {
                // Fit into the placeholder, keeping proportions.
                let f = ph.frame
                let s = min(f.width / max(1, img.size.width), f.height / max(1, img.size.height))
                let w = img.size.width * s, h = img.size.height * s
                o.frame = CGRect(x: f.midX - w / 2, y: f.midY - h / 2, width: w, height: h)
                o.placeholder = ph.placeholder
                o.id = ph.id
            }
            objects.append(o)
            offset += 20
        }
        guard !objects.isEmpty else { return }
        let i = current
        var s = displayedSlide
        var ids: [Int] = []
        for var o in objects {
            if let ph = placeholder, o.id == ph.id, let k = s.objects.firstIndex(where: { $0.id == ph.id }) {
                s.objects[k] = o
            } else {
                o.id = s.maxID + 1
                s.objects.append(o)
            }
            ids.append(o.id)
        }
        let media = d.media
        doc?.change(L("Insert Image")) { deck in
            deck.media = media
            deck.slides[i] = s
        }
        selection = ids
    }

    // MARK: - Insert actions

    @objc func insertTextBoxAction(_ sender: Any?) {
        canvas.insertTool = .textBox
        window?.makeFirstResponder(canvas)
    }

    @objc func insertShapeMenuAction(_ sender: Any?) {
        let menu = NSMenu()
        for item in Shapes.menuItems(action: #selector(insertShapeAction(_:)), target: nil) { menu.addItem(item) }
        menu.popUp(positioning: nil, at: NSEvent.mouseLocation, in: nil)
    }

    @objc func insertShapeAction(_ sender: Any?) {
        let tag = commandTag(of: sender)
        guard Shapes.gallery.indices.contains(tag) else { return }
        let prst = Shapes.gallery[tag].prst
        // From a menu: place in the middle (drag on the canvas to draw it instead).
        if sender is NSMenuItem {
            let c = CGPoint(x: deck.size.width / 2, y: deck.size.height / 2)
            if prst == "line" || prst == "arrow" {
                insertShape(prst, in: CGRect(x: c.x - 100, y: c.y, width: 200, height: 0))
            } else {
                insertShape(prst, in: CGRect(x: c.x - 90, y: c.y - 60, width: 180, height: 120))
            }
        } else {
            canvas.insertTool = prst == "line" ? .line(arrow: false) : prst == "arrow" ? .line(arrow: true) : .shape(prst)
        }
    }

    @objc func insertImageAction(_ sender: Any?) {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.image]
        panel.allowsMultipleSelection = true
        panel.message = L("Choose images to insert")
        guard let window else { return }
        panel.beginSheetModal(for: window) { [weak self] r in
            guard r == .OK else { return }
            self?.insertImages(panel.urls.compactMap { try? Data(contentsOf: $0) }, at: nil)
        }
    }

    @objc func insertTableAction(_ sender: Any?) {
        let rows = NSTextField(string: "3"), cols = NSTextField(string: "3")
        let alert = NSAlert.form(L("Insert Table"), ok: L("Insert"), rows: [(L("Rows:"), rows), (L("Columns:"), cols)])
        guard let window else { return }
        alert.beginSheetModal(for: window) { [weak self] r in
            guard r == .alertFirstButtonReturn, let self else { return }
            let nr = min(30, max(1, rows.integerValue)), nc = min(15, max(1, cols.integerValue))
            let w = min(self.deck.size.width * 0.8, CGFloat(nc) * 160)
            var t = TableModel(rows: nr, columns: nc, width: w, rowHeight: 36)
            for r in 0..<nr { for c in 0..<nc { t.cells[r][c].text.anchor = .middle } }
            var o = SlideObject(id: 0, kind: .table, frame: CGRect(x: (self.deck.size.width - w) / 2, y: max(120, (self.deck.size.height - t.size.height) / 2), width: w, height: t.size.height))
            o.table = t
            self.insert([o], name: L("Insert Table"))
        }
    }

    @objc func showStampsAction(_ sender: Any?) {
        let popover = NSPopover()
        let vc = StampLibraryViewController { [weak self] cg in
            let rep = NSBitmapImageRep(cgImage: cg)
            // Its physical size (written into the PNG), whatever resolution it was drawn at.
            let aspect = CGFloat(cg.width) / CGFloat(max(1, cg.height))
            let width = MarkPlacement.standard(aspect: aspect).widthPoints
            rep.size = NSSize(width: width, height: width / aspect)
            if let png = rep.representation(using: .png, properties: [:]) { self?.insertImages([png], at: nil) }
        }
        vc.popover = popover
        popover.contentViewController = vc
        popover.behavior = .transient
        popover.show(relativeTo: canvas.slideRect.insetBy(dx: canvas.slideRect.width / 2 - 10, dy: 0), of: canvas, preferredEdge: .minY)
    }

    @objc func insertSlideNumberAction(_ sender: Any?) { insertField(TextRun(text: "\(Renderer.number(of: current, deck: deck))", props: RunProps(), field: "slidenum")) }

    @objc func insertDateAction(_ sender: Any?) { insertField(TextRun(text: TextRender.fieldText("datetime", fallback: "", number: 0), field: "datetime")) }

    private func insertField(_ run: TextRun) {
        if isEditingText { canvas.editor.insert(run); return }
        var o = SlideObject(id: 0, kind: .textBox, frame: CGRect(x: deck.size.width - 240, y: deck.size.height - 50, width: 200, height: 30))
        var p = Paragraph()
        p.runs = [run]
        p.props.align = .right
        o.text = TextBody(paragraphs: [p])
        o.text?.listStyle[0].run = RunProps(size: 12)
        insert([o], name: L("Insert"))
    }

    // MARK: - Slides

    /// New slide after the current one; the layout follows the current slide (title slide → title and content).
    @objc func newSlideAction(_ sender: Any?) {
        if sender is NSMenuItem || sender == nil {
            showLayoutPicker()
            return
        }
        addSlide(layout: defaultNewLayout)
    }

    var defaultNewLayout: Int {
        guard let s = currentSlide else { return deck.layoutIndex(type: "title") ?? 0 }
        let t = deck.layout(of: s)?.type
        if t == "title" { return deck.layoutIndex(type: "obj") ?? s.layout }
        return s.layout
    }

    @objc func newSlideWithLayoutAction(_ sender: Any?) { addSlide(layout: commandTag(of: sender)) }

    func addSlide(layout: Int) {
        commitEditing()
        let at = deck.slides.isEmpty ? 0 : current + 1
        doc?.change(L("New Slide")) { d in d.slides.insert(d.newSlide(layout: layout), at: min(at, d.slides.count)) }
        select(slide: at)
    }

    func showLayoutPicker() {
        let picker = LayoutPicker(deck: deck) { [weak self] i in self?.addSlide(layout: i) }
        let popover = NSPopover()
        popover.contentViewController = picker
        picker.popover = popover
        popover.behavior = .transient
        if let item = window?.toolbar?.items.first(where: { $0.itemIdentifier.rawValue == "newSlide" }), let v = item.view, v.window != nil, window?.toolbar?.isVisible == true {
            popover.show(relativeTo: v.bounds, of: v, preferredEdge: .minY)
        } else {
            popover.show(relativeTo: thumbnails.view.bounds.insetBy(dx: 10, dy: 10), of: thumbnails.view, preferredEdge: .maxX)
        }
    }

    var selectedSlideIndexes: [Int] {
        let set = slideSelection.isEmpty ? IndexSet(integer: current) : slideSelection
        return set.filter { deck.slides.indices.contains($0) }.sorted()
    }

    @objc func duplicateSlideAction(_ sender: Any?) {
        commitEditing()
        let idx = selectedSlideIndexes
        guard let last = idx.last else { return }
        doc?.change(L("Duplicate Slide")) { d in
            let copies = idx.map { i -> Slide in var s = d.slides[i]; s.uid = UUID(); return s }
            d.slides.insert(contentsOf: copies, at: last + 1)
        }
        slideSelection = IndexSet((last + 1)..<(last + 1 + idx.count))
        select(slide: last + 1, extend: true)
        slideSelection = IndexSet((last + 1)..<(last + 1 + idx.count))
        thumbnails.syncSelection()
    }

    @objc func deleteSlideAction(_ sender: Any?) {
        commitEditing()
        let idx = selectedSlideIndexes
        guard !idx.isEmpty else { return }
        doc?.change(L("Delete Slide")) { d in
            for i in idx.reversed() { d.slides.remove(at: i) }
            d.pruneMedia()
        }
        select(slide: min(idx.first ?? 0, deck.slides.count - 1))
    }

    func moveSlides(_ indexes: [Int], to destination: Int) {
        commitEditing()
        let idx = Set(indexes).filter { deck.slides.indices.contains($0) }.sorted()
        guard !idx.isEmpty else { return }
        var dest = destination
        doc?.change(L("Move Slide")) { d in
            let moving = idx.map { d.slides[$0] }
            for i in idx.reversed() { d.slides.remove(at: i) }
            dest -= idx.filter { $0 < destination }.count
            dest = min(max(0, dest), d.slides.count)
            d.slides.insert(contentsOf: moving, at: dest)
        }
        current = dest
        slideSelection = IndexSet(dest..<(dest + idx.count))
        thumbnails.syncSelection()
        canvas.slideChanged()
        notesView.load()
        updateTitle()
    }

    @objc func hideSlideAction(_ sender: Any?) {
        commitEditing()
        let idx = selectedSlideIndexes
        let hide = !(currentSlide?.hidden ?? false)
        doc?.change(hide ? L("Hide Slide") : L("Show Slide")) { d in for i in idx { d.slides[i].hidden = hide } }
    }

    /// Changes the layout: placeholders move to the new layout's places, text is kept.
    @objc func layoutAction(_ sender: Any?) {
        commitEditing()
        let li = commandTag(of: sender)
        let idx = selectedSlideIndexes
        guard deck.layouts.indices.contains(li) else { return }
        doc?.change(L("Change Layout")) { d in
            for i in idx { d.slides[i] = Self.relayout(d.slides[i], to: li, deck: d) }
        }
        canvas.needsDisplay = true
    }

    static func relayout(_ slide: Slide, to li: Int, deck: Deck) -> Slide {
        var s = slide
        let target = deck.layouts[li]
        s.layout = li
        var used = Set<Int>()
        var next = s.maxID + 1
        var objects: [SlideObject] = []
        for var o in s.objects {
            guard let ph = o.placeholder, !ph.isFooter else { objects.append(o); continue }
            // Same kind of placeholder in the new layout (title ↔ title, body by order).
            let candidates = target.placeholders.enumerated().filter { !used.contains($0.offset) && $0.element.placeholder?.isFooter != true }
            let match = candidates.first { $0.element.placeholder?.isTitle == ph.isTitle && ($0.element.placeholder?.type == ph.type || ph.isTitle || ($0.element.placeholder?.type != "pic" && ph.type != "pic")) }
            if let m = match {
                used.insert(m.offset)
                o.frame = m.element.frame
                o.placeholder = m.element.placeholder
                if var t = o.text, let lt = m.element.text { t.anchor = lt.anchor; t.autofit = lt.autofit; t.insets = lt.insets; o.text = t }
                objects.append(o)
            } else if !(o.text?.isEmpty ?? true) || o.media != nil {
                // No place for it: it stays where it was as an ordinary object.
                o.placeholder = nil
                objects.append(o)
            }
        }
        for (k, ph) in target.placeholders.enumerated() where !used.contains(k) && ph.placeholder?.isFooter != true {
            var o = ph
            o.id = next
            next += 1
            if var t = o.text { t.paragraphs = [Paragraph()]; t.listStyle = ListStyle(); o.text = t }
            objects.append(o)
        }
        s.objects = objects
        return s
    }

    @objc func backgroundAction(_ sender: Any?) { showInspector(true, tab: 0) }

    @objc func transitionAction(_ sender: Any?) {
        commitEditing()
        let kind = TransitionKind.allCases[min(max(0, commandTag(of: sender)), TransitionKind.allCases.count - 1)]
        let idx = selectedSlideIndexes
        doc?.change(L("Transition")) { d in for i in idx { d.slides[i].transition.kind = kind } }
    }

    @objc func slideSizeAction(_ sender: Any?) {
        let size = commandTag(of: sender) == 0 ? CGSize(width: 960, height: 540) : CGSize(width: 720, height: 540)
        guard size != deck.size else { return }
        commitEditing()
        doc?.change(L("Slide Size")) { $0.resize(to: size) }
        canvas.updateFrame()
        thumbnails.reload()
    }

    @objc func themeAction(_ sender: Any?) {
        commitEditing()
        let i = commandTag(of: sender)
        doc?.change(L("Apply Theme")) { $0.apply(theme: i) }
        thumbnails.reload()
    }

    // MARK: - Edit

    @objc func deleteAction(_ sender: Any?) {
        commitEditing()
        if let li = editedLayout {
            if window?.firstResponder === thumbnails.table { deleteLayoutAction(sender); return }
            let ids = Set(selection)
            doc?.change(L("Delete")) { d in d.layouts[li].placeholders.removeAll { ids.contains($0.id) } }
            selection = []
            return
        }
        if mode == .sorter || window?.firstResponder === thumbnails.table { deleteSlideAction(sender); return }
        guard !selection.isEmpty else { return }
        let ids = Set(selection), i = current
        guard deck.slides.indices.contains(i) else { return }
        doc?.change(L("Delete")) { d in
            d.slides[i].objects.removeAll { ids.contains($0.id) }
            d.slides[i].animations.removeAll { ids.contains($0.target) }
            d.pruneMedia()
        }
        selection = []
    }

    @objc func duplicateAction(_ sender: Any?) {
        commitEditing()
        if mode == .sorter || window?.firstResponder === thumbnails.table { duplicateSlideAction(sender); return }
        let objs = selectedObjects.map { o -> SlideObject in var c = o; c.offset(by: CGSize(width: 18, height: 18)); return c }
        insert(objs, name: L("Duplicate"))
    }

    func copyObjects(cut: Bool) {
        commitEditing()
        let objs = selectedObjects
        guard !objs.isEmpty else { return }
        let token = UUID().uuidString
        var media: [String: Data] = [:]
        func collect(_ list: [SlideObject]) { for o in list { if let m = o.media { media[m] = deck.media[m] }; if let m = o.clip?.media { media[m] = deck.media[m] }; if case .picture(let m) = o.fill { media[m] = deck.media[m] }; collect(o.children) } }
        collect(objs)
        Clipboard.objects = (token, objs, media)
        let pb = Clipboard.board
        pb.clearContents()
        pb.setString(token, forType: Clipboard.objectsType)
        let text = objs.compactMap { $0.text?.plainText }.filter { !$0.isEmpty }.joined(separator: "\n")
        if !text.isEmpty { pb.setString(text, forType: .string) }
        if let png = objectsPNG(objs) { pb.setData(png, forType: .png) }
        if cut { deleteAction(nil) }
    }

    /// Picture of objects (for pasting into other apps).
    private func objectsPNG(_ objs: [SlideObject]) -> Data? {
        let union = objs.map { canvas.bounds(of: $0) }.reduce(canvas.bounds(of: objs[0])) { $0.union($1) }.insetBy(dx: -4, dy: -4)
        guard union.width > 1, union.height > 1 else { return nil }
        var s = Slide(layout: 0)
        s.objects = objs
        s.background = Fill.none
        s.showMasterShapes = false
        var d = deck
        d.master.background = Fill.none
        d.master.objects = []
        d.layouts = d.layouts.map { var l = $0; l.objects = []; l.background = nil; return l }
        guard union.width.isFinite, union.height.isFinite else { return nil }
        let scale: CGFloat = min(2, 4096 / max(union.width, union.height))
        guard let ctx = CGContext(data: nil, width: Int(union.width * scale), height: Int(union.height * scale), bitsPerComponent: 8, bytesPerRow: 0,
                                  space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return nil }
        ctx.translateBy(x: 0, y: union.height * scale)
        ctx.scaleBy(x: scale, y: -scale)
        ctx.translateBy(x: -union.minX, y: -union.minY)
        Renderer.draw(s, deck: d, in: ctx)
        guard let cg = ctx.makeImage() else { return nil }
        return NSBitmapImageRep(cgImage: cg).representation(using: .png, properties: [:])
    }

    func pasteObjects() {
        commitEditing()
        let pb = Clipboard.board
        if let token = pb.string(forType: Clipboard.slidesType), let c = Clipboard.slides, c.token == token {
            pasteSlides(c.items, media: c.media)
            return
        }
        if let token = pb.string(forType: Clipboard.objectsType), let c = Clipboard.objects, c.token == token {
            let media = c.media
            var objs = c.items
            // Pasting onto the same spot again offsets the copy.
            if objs.contains(where: { o in displayedSlide.objects.contains { $0.frame == o.frame } }) {
                objs = objs.map { var o = $0; o.offset(by: CGSize(width: 18, height: 18)); return o }
            }
            insert(objs.map { var o = $0; if o.placeholder != nil, displayedSlide.objects.contains(where: { $0.placeholder == o.placeholder }) { o.placeholder = nil }; return o }, media: media, name: L("Paste"))
            return
        }
        let images = canvas.images(from: pb)
        if !images.isEmpty { insertImages(images, at: nil); return }
        if let text = pb.string(forType: .string), !text.isEmpty {
            var o = SlideObject(id: 0, kind: .textBox, frame: CGRect(x: 80, y: 160, width: deck.size.width - 160, height: 60))
            var t = TextBody()
            t.autofit = .resize
            t.paragraphs = text.components(separatedBy: .newlines).map { Paragraph($0) }
            o.text = t
            let h = TextLayout(TextRender.attributed(t, TextContext(deck: deck, slide: currentSlide, object: o)), width: o.frame.width - 14.4).usedHeight + 7.2
            o.frame.size.height = max(40, h)
            insert([o], name: L("Paste"))
        }
    }

    func copySlides() {
        commitEditing()
        let idx = selectedSlideIndexes
        guard !idx.isEmpty else { return }
        let slides = idx.map { deck.slides[$0] }
        var media: [String: Data] = [:]
        func collect(_ list: [SlideObject]) { for o in list { if let m = o.media { media[m] = deck.media[m] }; if let m = o.clip?.media { media[m] = deck.media[m] }; if case .picture(let m) = o.fill { media[m] = deck.media[m] }; collect(o.children) } }
        for s in slides { collect(s.objects); if case .picture(let m)? = s.background { media[m] = deck.media[m] } }
        let token = UUID().uuidString
        Clipboard.slides = (token, slides, media)
        let pb = Clipboard.board
        pb.clearContents()
        pb.setString(token, forType: Clipboard.slidesType)
        pb.setString(slides.map(\.title).joined(separator: "\n"), forType: .string)
    }

    func pasteSlides(_ slides: [Slide], media: [String: Data]) {
        commitEditing()
        let at = deck.slides.isEmpty ? 0 : current + 1
        doc?.change(L("Paste Slides")) { d in
            let names = Clipboard.importMedia(media, into: &d)
            let copies = slides.map { s -> Slide in
                var c = s; c.uid = UUID(); c.layout = max(0, min(c.layout, max(0, d.layouts.count - 1)))
                c.objects = c.objects.map { Clipboard.remap($0, media: names) }
                if case .picture(let name)? = c.background { c.background = .picture(names[name] ?? name) }
                return c
            }
            d.slides.insert(contentsOf: copies, at: min(at, d.slides.count))
        }
        select(slide: at)
    }

    // MARK: - Arrange, align, group

    @objc func arrangeAction(_ sender: Any?) {
        commitEditing()
        let tag = commandTag(of: sender)
        let ids = Set(selection)
        changeSlide(L("Arrange")) { s in
            var objs = s.objects
            let sel = objs.filter { ids.contains($0.id) }
            switch tag {
            case 0: objs.removeAll { ids.contains($0.id) }; objs += sel
            case 3: objs.removeAll { ids.contains($0.id) }; objs = sel + objs
            case 1:
                for i in stride(from: objs.count - 2, through: 0, by: -1) where ids.contains(objs[i].id) && !ids.contains(objs[i + 1].id) { objs.swapAt(i, i + 1) }
            default:
                for i in 1..<max(1, objs.count) where ids.contains(objs[i].id) && !ids.contains(objs[i - 1].id) { objs.swapAt(i, i - 1) }
            }
            s.objects = objs
        }
    }

    @objc func alignObjectsAction(_ sender: Any?) {
        commitEditing()
        let tag = commandTag(of: sender)
        let objs = selectedObjects
        guard !objs.isEmpty else { return }
        // One object aligns to the slide, several to their common bounds.
        let ref = objs.count == 1 ? CGRect(origin: .zero, size: deck.size) : objs.map { canvas.bounds(of: $0) }.reduce(canvas.bounds(of: objs[0])) { $0.union($1) }
        changeSlide(L("Align")) { s in
            for o in objs {
                let b = canvas.bounds(of: o)
                var d = CGSize.zero
                switch tag {
                case 0: d.width = ref.minX - b.minX
                case 1: d.width = ref.midX - b.midX
                case 2: d.width = ref.maxX - b.maxX
                case 3: d.height = ref.minY - b.minY
                case 4: d.height = ref.midY - b.midY
                default: d.height = ref.maxY - b.maxY
                }
                s.update(o.id) { $0.offset(by: d) }
            }
        }
    }

    @objc func distributeAction(_ sender: Any?) {
        commitEditing()
        let horizontal = commandTag(of: sender) == 0
        let objs = selectedObjects.sorted { horizontal ? canvas.bounds(of: $0).midX < canvas.bounds(of: $1).midX : canvas.bounds(of: $0).midY < canvas.bounds(of: $1).midY }
        guard objs.count > 2 else { return }
        let bs = objs.map { canvas.bounds(of: $0) }
        let total = horizontal ? bs.last!.maxX - bs.first!.minX : bs.last!.maxY - bs.first!.minY
        let sizes = bs.map { horizontal ? $0.width : $0.height }.reduce(0, +)
        let gap = (total - sizes) / CGFloat(objs.count - 1)
        changeSlide(L("Distribute")) { s in
            var pos = horizontal ? bs.first!.minX : bs.first!.minY
            for (o, b) in zip(objs, bs) {
                let d = pos - (horizontal ? b.minX : b.minY)
                s.update(o.id) { $0.offset(by: horizontal ? CGSize(width: d, height: 0) : CGSize(width: 0, height: d)) }
                pos += (horizontal ? b.width : b.height) + gap
            }
        }
    }

    @objc func groupAction(_ sender: Any?) {
        commitEditing()
        let ids = selection
        guard ids.count > 1 else { return }
        var newID = 0
        changeSlide(L("Group")) { s in
            let members = s.objects.filter { ids.contains($0.id) }
            guard let firstIndex = s.objects.lastIndex(where: { ids.contains($0.id) }) else { return }
            var g = SlideObject(id: s.maxID + 1, kind: .group, frame: members.map { canvas.bounds(of: $0) }.reduce(canvas.bounds(of: members[0])) { $0.union($1) })
            g.name = L("Group")
            g.children = members.map { var m = $0; m.placeholder = nil; return m }
            newID = g.id
            let insertAt = firstIndex - (members.count - 1)
            s.objects.removeAll { ids.contains($0.id) }
            s.objects.insert(g, at: max(0, min(insertAt, s.objects.count)))
        }
        selection = [newID]
    }

    @objc func ungroupAction(_ sender: Any?) {
        commitEditing()
        let ids = selection
        var out: [Int] = []
        changeSlide(L("Ungroup")) { s in
            var objs: [SlideObject] = []
            for o in s.objects {
                if ids.contains(o.id), o.kind == .group {
                    // Group rotation is applied to the members.
                    for var c in o.children {
                        let gc = CGPoint(x: o.frame.midX, y: o.frame.midY), cc = CGPoint(x: c.frame.midX, y: c.frame.midY)
                        let mirrored = CGPoint(x: o.flipH ? 2 * gc.x - cc.x : cc.x, y: o.flipV ? 2 * gc.y - cc.y : cc.y)
                        let p = canvas.rotate(mirrored, around: gc, by: o.rotation * .pi / 180)
                        c.offset(by: CGSize(width: p.x - cc.x, height: p.y - cc.y))
                        let reflected = o.flipH != o.flipV
                        c.rotation = (o.rotation + (reflected ? -c.rotation : c.rotation)).truncatingRemainder(dividingBy: 360)
                        c.flipH = c.flipH != o.flipH; c.flipV = c.flipV != o.flipV
                        objs.append(c)
                        out.append(c.id)
                    }
                } else {
                    objs.append(o)
                }
            }
            s.objects = objs
        }
        selection = out
    }

    // MARK: - Tables

    @objc func tableRowAction(_ sender: Any?) {
        let tag = commandTag(of: sender)
        let row = canvas.editor.cell?.0 ?? (tag == 0 ? 0 : (selectedObjects.first?.table?.rows.count ?? 1) - 1)
        commitEditing()
        changeSelected(L("Table Rows")) { object in
            guard var table = object.table, !table.rows.isEmpty else { return }
            let row = min(max(0, row), table.rows.count - 1)
            table.shift(axisRows: true, at: tag == 1 ? row + 1 : row, insert: tag == 0 || tag == 1)
            object.frame.size.height = table.rows.reduce(0, +)
            object.table = table
        }
    }

    @objc func tableColumnAction(_ sender: Any?) {
        let tag = commandTag(of: sender)
        let column = canvas.editor.cell?.1 ?? (tag == 0 ? 0 : (selectedObjects.first?.table?.columns.count ?? 1) - 1)
        commitEditing()
        changeSelected(L("Table Columns")) { object in
            guard var table = object.table, !table.columns.isEmpty else { return }
            let column = min(max(0, column), table.columns.count - 1)
            table.shift(axisRows: false, at: tag == 1 ? column + 1 : column, insert: tag == 0 || tag == 1)
            table.fit(to: object.frame.size)
            object.table = table
        }
    }

    // MARK: - Text formatting

    /// Effective character formatting of the selection (format bar, menus).
    var currentRunProps: RunProps {
        if isEditingText { return canvas.editor.currentRunProps }
        guard let first = selectedObjects.lazy.compactMap(\.firstTextContainer).first else { return RunProps() }
        let o = first.object, body = first.body
        let p = body.paragraphs.first ?? Paragraph()
        var c = TextContext(deck: deck, slide: mode == .master ? nil : displayedSlide, object: o)
        if let t = o.table, let row = first.row { c.cellDefaults = Renderer.cellDefaults(t, row: row) }
        let style = p.props.over(TextRender.levelStyle(c, level: p.level))
        return (p.runs.first?.props ?? p.endProps).over(style.run)
    }

    var currentLevelStyle: LevelStyle {
        if isEditingText { return canvas.editor.currentLevelStyle }
        guard let first = selectedObjects.lazy.compactMap(\.firstTextContainer).first else { return LevelStyle() }
        let p = first.body.paragraphs.first ?? Paragraph()
        var c = TextContext(deck: deck, slide: mode == .master ? nil : displayedSlide, object: first.object)
        if let table = first.object.table, let row = first.row { c.cellDefaults = Renderer.cellDefaults(table, row: row) }
        return p.props.over(TextRender.levelStyle(c, level: p.level))
    }

    /// Character formatting: the edited text's selection, or all text of the selected objects (master view: the master styles).
    func applyRun(_ name: String, _ f: @escaping (inout RunProps) -> Void) {
        if isEditingText { canvas.editor.applyRun(f); return }
        if let li = editedLayout {
            // A layout's placeholders carry their own level styles.
            let objs = selectedObjects
            doc?.change(name) { d in
                for o in objs {
                    guard let k = d.layouts[li].placeholders.firstIndex(where: { $0.id == o.id }), d.layouts[li].placeholders[k].text != nil else { continue }
                    let shown = MasterView.sampleSlide(deck: d, layout: li)
                    for i in 0..<9 {
                        var r = d.levelStyle(for: d.layouts[li].placeholders[k], on: shown, level: i).run
                        f(&r)
                        d.layouts[li].placeholders[k].text?.listStyle.levels[i].run = r
                    }
                }
            }
            return
        }
        if mode == .master {
            let objs = selectedObjects
            doc?.change(name) { d in
                for o in objs {
                    guard let ph = o.placeholder else { continue }
                    for i in 0..<9 {
                        var r = d.master.style(ph.role)[i].run
                        f(&r)
                        switch ph.role {
                        case .title: d.master.titleStyle.levels[i].run = r
                        case .body: d.master.bodyStyle.levels[i].run = r
                        case .other:
                            if let k = d.master.placeholders.firstIndex(where: { $0.id == o.id }) {
                                var lr = d.master.placeholders[k].text?.listStyle[i].run ?? RunProps()
                                f(&lr)
                                d.master.placeholders[k].text?.listStyle.levels[i].run = lr
                            }
                        }
                    }
                }
            }
            return
        }
        changeSelected(name) { o in
            func apply(_ body: inout TextBody) {
                for p in body.paragraphs.indices {
                    for r in body.paragraphs[p].runs.indices { f(&body.paragraphs[p].runs[r].props) }
                    f(&body.paragraphs[p].endProps)
                }
            }
            o.editTextBodies(apply)
        }
    }

    /// Paragraph formatting (level and paragraph style) of the edited paragraphs or all paragraphs of the selection.
    func applyPara(_ name: String, _ f: @escaping (inout Int, inout LevelStyle) -> Void) {
        if isEditingText { canvas.editor.applyPara { level, props, _ in f(&level, &props) }; return }
        if let li = editedLayout {
            let objs = selectedObjects
            doc?.change(name) { d in
                for o in objs {
                    guard let k = d.layouts[li].placeholders.firstIndex(where: { $0.id == o.id }), d.layouts[li].placeholders[k].text != nil else { continue }
                    let shown = MasterView.sampleSlide(deck: d, layout: li)
                    for i in 0..<9 {
                        var level = i
                        var st = d.levelStyle(for: d.layouts[li].placeholders[k], on: shown, level: i)
                        let run = d.layouts[li].placeholders[k].text?.listStyle[i].run ?? RunProps()
                        f(&level, &st)
                        st.run = run
                        d.layouts[li].placeholders[k].text?.listStyle[i] = st
                    }
                }
            }
            return
        }
        if mode == .master {
            let objs = selectedObjects
            doc?.change(name) { d in
                for o in objs {
                    guard let ph = o.placeholder else { continue }
                    for i in 0..<9 {
                        var level = i
                        var s = d.master.style(ph.role)[i]
                        let run = s.run
                        f(&level, &s)
                        s.run = run
                        if ph.role == .title { d.master.titleStyle[i] = s } else if ph.role == .body { d.master.bodyStyle[i] = s }
                    }
                }
            }
            return
        }
        changeSelected(name) { o in
            func apply(_ body: inout TextBody) {
                for p in body.paragraphs.indices {
                    var level = body.paragraphs[p].level
                    f(&level, &body.paragraphs[p].props)
                    body.paragraphs[p].level = max(0, min(8, level))
                }
            }
            o.editTextBodies(apply)
        }
    }

    @objc func boldAction(_ sender: Any?) { let v = !(currentRunProps.bold ?? false); applyRun(L("Bold")) { $0.bold = v } }
    @objc func italicAction(_ sender: Any?) { let v = !(currentRunProps.italic ?? false); applyRun(L("Italic")) { $0.italic = v } }
    @objc func underlineAction(_ sender: Any?) { let v = !(currentRunProps.underline ?? false); applyRun(L("Underline")) { $0.underline = v } }
    @objc func strikeAction(_ sender: Any?) { let v = !(currentRunProps.strike ?? false); applyRun(L("Strikethrough")) { $0.strike = v } }

    @objc func biggerAction(_ sender: Any?) { stepSize(1) }
    @objc func smallerAction(_ sender: Any?) { stepSize(-1) }

    func stepSize(_ dir: Int) {
        let steps: [CGFloat] = [8, 9, 10, 11, 12, 14, 16, 18, 20, 24, 28, 32, 36, 40, 44, 48, 54, 60, 66, 72, 80, 96, 120]
        let cur = currentRunProps.size ?? 18
        let v = dir > 0 ? (steps.first { $0 > cur + 0.1 } ?? cur + 8) : (steps.last { $0 < cur - 0.1 } ?? max(1, cur - 1))
        applyRun(L("Font Size")) { $0.size = v }
    }

    func setFontSize(_ v: CGFloat) { guard v.isFinite, v >= 1, v <= 4000 else { return }; applyRun(L("Font Size")) { $0.size = v } }
    func setFont(_ family: String) { applyRun(L("Font")) { $0.font = family } }
    func setTextColor(_ c: PColor) { applyRun(L("Text Colour")) { $0.color = c } }

    @objc func baselineAction(_ sender: Any?) {
        let tag = commandTag(of: sender)
        let cur = currentRunProps.baseline ?? 0
        let v: CGFloat = (tag > 0 ? cur > 0 : cur < 0) ? 0 : (tag > 0 ? 30 : -25)
        applyRun(tag > 0 ? L("Superscript") : L("Subscript")) { $0.baseline = v }
    }

    @objc func clearFormattingAction(_ sender: Any?) { applyRun(L("Clear Formatting")) { $0 = RunProps() } }

    @objc func alignTextAction(_ sender: Any?) {
        let a = TextAlign.allCases[min(3, max(0, commandTag(of: sender)))]
        applyPara(L("Alignment")) { _, p in p.align = a }
    }

    @objc func anchorAction(_ sender: Any?) {
        let a: Anchor = [.top, .middle, .bottom][min(2, max(0, commandTag(of: sender)))]
        let editedCell = canvas.editor.cell, editedObject = canvas.editor.objectID
        commitEditing()
        changeSelected(L("Vertical Alignment")) { o in
            if o.id == editedObject, let cell = editedCell, var table = o.table,
               table.cells.indices.contains(cell.0), table.cells[cell.0].indices.contains(cell.1) {
                table.cells[cell.0][cell.1].text.anchor = a; o.table = table
            } else { o.editTextBodies { $0.anchor = a } }
        }
    }

    @objc func bulletsAction(_ sender: Any?) {
        let tag = commandTag(of: sender)
        let current = currentLevelStyle.bullet?.kind ?? Bullet.Kind.none
        var bullet: Bullet = tag == 1 ? .disc : tag == 2 ? .numbered : .none
        // Choosing the active kind again switches bullets off.
        if (tag == 1 && current == .char) || (tag == 2 && current == .number) { bullet = .none }
        applyPara(L("Bullets")) { level, p in
            p.bullet = bullet
            if bullet.kind != .none {
                p.marL = 18 + CGFloat(level) * 36 + (bullet.kind == .number ? 9 : 0)
                p.indent = bullet.kind == .number ? -27 : -18
            } else if p.marL != nil {
                p.marL = CGFloat(level) * 36
                p.indent = 0
            }
        }
    }

    @objc func indentAction(_ sender: Any?) {
        let d = commandTag(of: sender) > 0 ? 1 : -1
        applyPara(d > 0 ? L("Increase Indent") : L("Decrease Indent")) { level, p in
            level = max(0, min(8, level + d))
            if let m = p.marL { p.marL = max(0, m + CGFloat(d) * 36) }
        }
    }

    @objc func lineSpacingAction(_ sender: Any?) {
        let values: [CGFloat] = [0.9, 1.0, 1.15, 1.5, 2.0]
        let v = values[min(values.count - 1, max(0, commandTag(of: sender)))]
        applyPara(L("Line Spacing")) { _, p in p.lineSpacing = v }
    }

    // MARK: - Fill and line (inspector, format bar)

    func setFill(_ fill: Fill) {
        if mode == .normal, selection.isEmpty {
            let i = current
            doc?.change(L("Background")) { $0.slides[i].background = fill }
            return
        }
        changeSelected(L("Fill")) { o in if o.kind != .line { o.fill = fill } }
    }

    func setLine(_ body: @escaping (inout LineStyle) -> Void) {
        changeSelected(L("Line")) { o in body(&o.line) }
    }

    // MARK: - View

    @objc func viewModeAction(_ sender: Any?) {
        setMode(ViewMode(rawValue: commandTag(of: sender)) ?? .normal)
    }

    @objc func toggleNotesAction(_ sender: Any?) { setNotesShown(!notesShown) }
    @objc func toggleInspectorAction(_ sender: Any?) { showInspector(!inspectorShown) }
    @objc func toggleFormatBarAction(_ sender: Any?) { formatBar.strip.isShown.toggle() }
    @objc func toggleGuidesAction(_ sender: Any?) { snapToGuides.toggle() }

    @objc func zoomInAction(_ sender: Any?) { canvas.zoom = min(8, (canvas.zoom ?? canvas.fitScale) * 1.25) }
    @objc func zoomOutAction(_ sender: Any?) { canvas.zoom = max(0.1, (canvas.zoom ?? canvas.fitScale) / 1.25) }
    @objc func zoomFitAction(_ sender: Any?) { canvas.zoom = nil }
    @objc func actualSizeAction(_ sender: Any?) { canvas.zoom = 1 }

    // MARK: - Slide show

    @objc func playAction(_ sender: Any?) { startShow(from: 0, presenter: false) }
    @objc func playCurrentAction(_ sender: Any?) { startShow(from: current, presenter: false) }
    @objc func rehearseAction(_ sender: Any?) { startShow(from: current, presenter: true, rehearse: true) }

    @objc func playInWindowAction(_ sender: Any?) { startShow(from: current, presenter: false, windowed: true) }

    func startShow(from index: Int, presenter: Bool, rehearse: Bool = false, windowed: Bool = false) {
        commitEditing()
        doc?.endSilentEdit()
        let show = SlideShowController(owner: self, deck: deck, start: index, rehearse: rehearse, windowed: windowed)
        show.start()
    }

    @objc func showSettingsAction(_ sender: Any?) {
        let presenter = NSButton(checkboxWithTitle: L("Presenter view on the second display"), target: nil, action: nil)
        presenter.state = ShowSettings.presenterView ? .on : .off
        let loop = NSButton(checkboxWithTitle: L("Loop until Esc"), target: nil, action: nil)
        loop.state = ShowSettings.loop ? .on : .off
        let hidden = NSButton(checkboxWithTitle: L("Show hidden slides"), target: nil, action: nil)
        hidden.state = ShowSettings.showHidden ? .on : .off
        let alert = NSAlert.form(L("Slide Show Settings"), ok: L("OK"), rows: [("", presenter), ("", loop), ("", hidden)])
        guard let window else { return }
        alert.beginSheetModal(for: window) { r in
            guard r == .alertFirstButtonReturn else { return }
            ShowSettings.presenterView = presenter.state == .on
            ShowSettings.loop = loop.state == .on
            ShowSettings.showHidden = hidden.state == .on
        }
    }

    // MARK: - Find

    @objc func findAction(_ sender: Any?) { finder.show() }
    @objc func findNextAction(_ sender: Any?) { finder.find(forward: true) }
    @objc func findPreviousAction(_ sender: Any?) { finder.find(forward: false) }

    // MARK: - Export

    @objc func exportPDFAction(_ sender: Any?) { Exporter.pdf(self, notes: false) }
    @objc func exportNotesPDFAction(_ sender: Any?) { Exporter.pdf(self, notes: true) }
    @objc func exportImagesAction(_ sender: Any?) { Exporter.images(self) }
    @objc func exportOutlineAction(_ sender: Any?) { Exporter.outline(self) }
}

/// Slide-show options (not saved in the file).
enum ShowSettings {
    private static let defaults = UserDefaults.standard
    static var presenterView: Bool {
        get { defaults.object(forKey: "PresenterView") as? Bool ?? true }
        set { defaults.set(newValue, forKey: "PresenterView") }
    }
    static var loop: Bool {
        get { defaults.bool(forKey: "LoopShow") }
        set { defaults.set(newValue, forKey: "LoopShow") }
    }
    static var showHidden: Bool {
        get { defaults.bool(forKey: "ShowHidden") }
        set { defaults.set(newValue, forKey: "ShowHidden") }
    }
}

/// Layout gallery for a new slide.
final class LayoutPicker: NSViewController {
    let deck: Deck
    let onPick: (Int) -> Void
    weak var popover: NSPopover?

    init(deck: Deck, onPick: @escaping (Int) -> Void) {
        self.deck = deck
        self.onPick = onPick
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) { fatalError("not supported") }

    override func loadView() {
        let header = NSTextField(labelWithString: L("Choose a Layout"))
        header.font = .systemFont(ofSize: 12, weight: .semibold)
        header.textColor = .secondaryLabelColor
        var rows: [NSView] = []
        var row: [NSView] = []
        for (i, layout) in deck.layouts.enumerated() {
            let b = NSButton(title: Themes.displayName(layout), image: LayoutIcons.icon(layout, deck: deck, width: 104), target: self, action: #selector(pick(_:)))
            b.tag = i
            b.imagePosition = .imageAbove
            b.imageScaling = .scaleNone
            b.bezelStyle = .accessoryBarAction
            b.showsBorderOnlyWhileMouseInside = true
            b.font = .systemFont(ofSize: 11)
            b.translatesAutoresizingMaskIntoConstraints = false
            b.widthAnchor.constraint(equalToConstant: 124).isActive = true
            b.heightAnchor.constraint(equalToConstant: 104 * deck.size.height / deck.size.width + 34).isActive = true
            row.append(b)
            if row.count == 4 { rows.append(NSStackView(views: row)); row = [] }
        }
        if !row.isEmpty { rows.append(NSStackView(views: row)) }
        rows.forEach { ($0 as? NSStackView)?.spacing = 6; ($0 as? NSStackView)?.alignment = .top }
        let stack = NSStackView(views: [header] + rows)
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 8
        stack.edgeInsets = NSEdgeInsets(top: 14, left: 14, bottom: 14, right: 14)
        view = stack
    }

    @objc private func pick(_ sender: NSButton) {
        popover?.performClose(nil)
        onPick(sender.tag)
    }
}
