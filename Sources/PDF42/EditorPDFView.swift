import Kit42
import AppKit
import PDFKit

/// PDFView with annotation tools: drawing, shapes, markup, selection, moving and resizing.
final class EditorPDFView: PDFView {
    weak var editor: EditorWindowController?

    var tool: Tool = .select {
        didSet {
            if labelRegionSelection { editor?.updateSignatureBanner() }
            labelRegionSelection = false
            cancelInteraction()
            commitTextEdit()
            if tool != .select { selectAnnotation(nil) }
            clearSelection()
            if (oldValue == .link) != (tool == .link) { updateLinkBorders() }
        }
    }
    var color: NSColor = .systemRed
    /// Uses only the crop tool's temporary drag preview, never a crop mutation.
    var labelRegionSelection = false
    /// In-place editor of page text (Edit Text tool), while open.
    var inlineEditor: InlineTextEditor?
    var hasPendingTextEdit: Bool { inlineEditor?.hasPendingChanges == true }
    func commitTextEdit() { inlineEditor?.end(commit: true) }
    func cancelTextEdit() { inlineEditor?.end(commit: false) }
    var lineWidth: CGFloat = 2
    /// Dashed frames around links (View › Show Link Borders; always while the Link tool is active).
    var showsLinkBorders = false { didSet { updateLinkBorders() } }

    private(set) var selectedAnnotation: PDFAnnotation?
    private(set) var selectedPage: PDFPage?
    private var selectionFrame: PDFAnnotation?
    private var selectionHandle: PDFAnnotation?

    private enum Drag { case none, move, resize, create }
    private var drag: Drag = .none
    private var dragPage: PDFPage?
    private var dragStart: CGPoint = .zero
    private var dragOriginalBounds: CGRect = .zero
    private var mouseHandled = false
    private var tempAnnotation: PDFAnnotation?
    private var inkPoints: [CGPoint] = []

    /// An unfinished mouse gesture is a preview, not a document mutation.
    func cancelInteraction() {
        if let page = dragPage {
            if drag == .move || drag == .resize { selectedAnnotation?.bounds = dragOriginalBounds }
            if let tempAnnotation { page.removeAnnotation(tempAnnotation) }
            refreshMarker()
            annotationsChanged(on: page)
        }
        dragPage = nil
        drag = .none
        mouseHandled = false
        tempAnnotation = nil
        inkPoints = []
    }

    override var acceptsFirstResponder: Bool { true }

    // MARK: - Selection

    private var handleSize: CGFloat { 9 / max(scaleFactor, 0.1) }

    func selectAnnotation(_ annotation: PDFAnnotation?, on page: PDFPage? = nil) {
        for marker in [selectionFrame, selectionHandle] {
            if let marker, let p = marker.page { p.removeAnnotation(marker) }
        }
        selectionFrame = nil
        selectionHandle = nil
        selectedAnnotation = annotation
        selectedPage = annotation == nil ? nil : (page ?? annotation?.page)
        editor?.annotationSelectionChanged()
        guard let annotation, let page = selectedPage else { return }

        let frame = PDFAnnotation(bounds: .zero, forType: .square, withProperties: nil)
        let border = PDFBorder()
        border.lineWidth = 1
        border.style = .dashed
        border.dashPattern = [4, 3]
        frame.border = border
        frame.color = .controlAccentColor
        frame.shouldPrint = false
        page.addAnnotation(frame)
        selectionFrame = frame

        if annotation.isResizable {
            let handle = PDFAnnotation(bounds: .zero, forType: .square, withProperties: nil)
            let hb = PDFBorder()
            hb.lineWidth = 1
            handle.border = hb
            handle.color = .white
            handle.interiorColor = .controlAccentColor
            handle.shouldPrint = false
            page.addAnnotation(handle)
            selectionHandle = handle
        }
        refreshMarker()
    }

    func refreshMarker() {
        guard let annotation = selectedAnnotation else { return }
        let b = annotation.bounds
        selectionFrame?.bounds = b.insetBy(dx: -3, dy: -3)
        let s = handleSize
        selectionHandle?.bounds = CGRect(x: b.maxX + 3 - s / 2, y: b.minY - 3 - s / 2, width: s, height: s)
        if let page = selectedPage { annotationsChanged(on: page) }
    }

    private func isMarker(_ a: PDFAnnotation) -> Bool { a === selectionFrame || a === selectionHandle }

    private func hitAnnotation(on page: PDFPage, at point: CGPoint) -> PDFAnnotation? {
        for annotation in page.annotations.reversed() where !isMarker(annotation) {
            if tool.isFormField && !annotation.isKind(.widget) { continue }
            // Form tools pick widgets (move, resize, delete); otherwise PDFView handles them (filling).
            if annotation.isSystemHandled, !(tool.isFormField && annotation.isKind(.widget)), !(tool == .link && annotation.isKind(.link)) { continue }
            if annotation is LinkBorderMarker { continue }
            if let o = annotation as? PageOverlay, !o.allowsPicking { continue }
            if annotation is TemporaryHighlight { continue }
            if annotation.bounds.insetBy(dx: -2, dy: -2).contains(point) { return annotation }
        }
        return nil
    }

    private func hitsHandle(_ point: CGPoint, on page: PDFPage) -> Bool {
        guard let handle = selectionHandle, page === selectedPage else { return false }
        return handle.bounds.insetBy(dx: -handleSize * 0.6, dy: -handleSize * 0.6).contains(point)
    }

    // MARK: - Mouse

    private func locate(_ event: NSEvent) -> (PDFPage, CGPoint)? {
        let viewPoint = convert(event.locationInWindow, from: nil)
        guard let page = page(for: viewPoint, nearest: true) else { return nil }
        return (page, convert(viewPoint, to: page))
    }

    override func mouseMoved(with event: NSEvent) {
        switch tool {
        case .select:
            if let (page, point) = locate(event), hitsHandle(point, on: page) {
                NSCursor.crosshair.set()
            } else {
                super.mouseMoved(with: event)
            }
        case .highlight, .underline, .strikeOut:
            super.mouseMoved(with: event)
        case .editText:
            NSCursor.iBeam.set()
        default:
            NSCursor.crosshair.set()
        }
    }

    override func mouseDown(with event: NSEvent) {
        cancelInteraction()
        window?.makeFirstResponder(self)
        mouseHandled = true
        drag = .none
        guard let (page, point) = locate(event) else {
            mouseHandled = false
            super.mouseDown(with: event)
            return
        }
        dragPage = page
        dragStart = point

        switch tool {
        case .select:
            if hitsHandle(point, on: page), let a = selectedAnnotation {
                drag = .resize
                dragOriginalBounds = a.bounds
                return
            }
            if let annotation = hitAnnotation(on: page, at: point) {
                selectAnnotation(annotation, on: page)
                if event.clickCount == 2 {
                    editor?.editAnnotationText(annotation, on: page)
                    return
                }
                if !annotation.isMarkup {
                    drag = .move
                    dragOriginalBounds = annotation.bounds
                }
                return
            }
            selectAnnotation(nil)
            mouseHandled = false
            super.mouseDown(with: event)

        case .highlight, .underline, .strikeOut:
            mouseHandled = false
            super.mouseDown(with: event)

        case .ink:
            drag = .create
            inkPoints = [point]
            let annotation = PDFAnnotation(bounds: page.bounds(for: .mediaBox), forType: .ink, withProperties: nil)
            style(annotation)
            page.addAnnotation(annotation)
            tempAnnotation = annotation
            updateInkPreview()

        case .rectangle, .oval, .whiteout, .redact:
            drag = .create
            let annotation = PDFAnnotation(bounds: CGRect(origin: point, size: CGSize(width: 1, height: 1)),
                                           forType: tool == .oval ? .circle : .square, withProperties: nil)
            style(annotation)
            page.addAnnotation(annotation)
            tempAnnotation = annotation

        case .text:
            editor?.createTextBox(on: page, at: point)

        case .note:
            editor?.createNote(on: page, at: point)

        case .editText:
            if let edited = hitAnnotation(on: page, at: point) as? EditedTextOverlay {
                editor?.editPageText(edited, on: page)
            } else if let run = textRun(on: page, at: point, word: event.modifierFlags.contains(.option)) {
                editor?.editPageText(run, on: page)
            } else {
                NSSound.beep()
            }

        case .link, .crop:
            if tool == .link, let link = hitAnnotation(on: page, at: point), link.isKind(.link) {
                editor?.editLink(link, on: page)
                return
            }
            drag = .create
            let preview = PDFAnnotation(bounds: CGRect(origin: point, size: CGSize(width: 1, height: 1)), forType: .square, withProperties: nil)
            let border = PDFBorder()
            border.lineWidth = 1
            border.style = .dashed
            border.dashPattern = [3, 2]
            preview.border = border
            preview.color = tool == .crop ? .systemOrange : .systemBlue
            preview.shouldPrint = false
            page.addAnnotation(preview)
            tempAnnotation = preview

        case .textField, .checkBox, .choiceField:
            if hitsHandle(point, on: page), let a = selectedAnnotation {
                drag = .resize
                dragOriginalBounds = a.bounds
                return
            }
            if let widget = hitAnnotation(on: page, at: point), widget.isKind(.widget) {
                selectAnnotation(widget, on: page)
                drag = .move
                dragOriginalBounds = widget.bounds
                return
            }
            selectAnnotation(nil)
            drag = .create
            let preview = PDFAnnotation(bounds: CGRect(origin: point, size: CGSize(width: 1, height: 1)), forType: .square, withProperties: nil)
            let border = PDFBorder()
            border.lineWidth = 1
            border.style = .dashed
            border.dashPattern = [3, 2]
            preview.border = border
            preview.color = .controlAccentColor
            preview.shouldPrint = false
            page.addAnnotation(preview)
            tempAnnotation = preview
        }
    }

    /// The line of page text under `point` (or just the word with ⌥), nil over empty space.
    private func textRun(on page: PDFPage, at point: CGPoint, word: Bool) -> PDFSelection? {
        guard let s = word ? page.selectionForWord(at: point) : page.selectionForLine(at: point),
              !(s.string ?? "").trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
              s.bounds(for: page).insetBy(dx: -3, dy: -3).contains(point) else { return nil }
        return s
    }

    override func mouseDragged(with event: NSEvent) {
        guard mouseHandled else {
            super.mouseDragged(with: event)
            return
        }
        guard let page = dragPage else { return }
        let point = convert(convert(event.locationInWindow, from: nil), to: page)

        switch drag {
        case .move:
            selectedAnnotation?.bounds = dragOriginalBounds.offsetBy(dx: point.x - dragStart.x, dy: point.y - dragStart.y)
            refreshMarker()
        case .resize:
            guard let a = selectedAnnotation else { return }
            let o = dragOriginalBounds
            var w = max(8, point.x - o.minX)
            var h = max(8, o.maxY - point.y)
            if a.keepsAspectRatio || event.modifierFlags.contains(.shift), o.height > 0 {
                let ratio = o.width / o.height
                if w / h > ratio { h = w / ratio } else { w = h * ratio }
            }
            a.bounds = CGRect(x: o.minX, y: o.maxY - h, width: w, height: h)
            refreshMarker()
        case .create:
            if tool == .ink {
                inkPoints.append(point)
                updateInkPreview()
            } else {
                tempAnnotation?.bounds = rect(from: dragStart, to: point)
                annotationsChanged(on: page)
            }
        case .none:
            break
        }
    }

    override func mouseUp(with event: NSEvent) {
        defer {
            dragPage = nil
            drag = .none
            mouseHandled = false
            tempAnnotation = nil
            inkPoints = []
        }
        guard mouseHandled else {
            super.mouseUp(with: event)
            if tool.isTextMarkup, let selection = currentSelection,
               !(selection.string ?? "").trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                editor?.addMarkup(for: selection, tool: tool)
                clearSelection()
            }
            return
        }
        guard let page = dragPage else { return }

        switch drag {
        case .move, .resize:
            if let a = selectedAnnotation, a.bounds != dragOriginalBounds {
                editor?.didChangeBounds(a, from: dragOriginalBounds, on: page, name: drag == .move ? L("Move") : L("Resize"))
            }
        case .create:
            if let temp = tempAnnotation { page.removeAnnotation(temp) }
            if tool == .ink {
                guard inkPoints.count >= 2 else { return }
                let path = NSBezierPath()
                path.move(to: inkPoints[0])
                inkPoints.dropFirst().forEach { path.line(to: $0) }
                let bounds = path.bounds.insetBy(dx: -lineWidth * 2, dy: -lineWidth * 2)
                let annotation = PDFAnnotation(bounds: bounds, forType: .ink, withProperties: nil)
                style(annotation)
                annotation.add(localPath(inkPoints, origin: bounds.origin))
                editor?.add(annotation, to: page, name: L("Drawing"))
            } else if tool.isFormField {
                guard let temp = tempAnnotation else { return }
                editor?.createFormField(tool.fieldKind, in: temp.bounds, on: page)
            } else if tool == .link {
                guard let temp = tempAnnotation, temp.bounds.width > 3, temp.bounds.height > 3 else { return }
                let rect = temp.bounds
                DispatchQueue.main.async { [weak self] in self?.editor?.createLink(in: rect, on: page) }
            } else if tool == .crop {
                guard let temp = tempAnnotation, temp.bounds.width > 10, temp.bounds.height > 10 else { return }
                let rect = temp.bounds
                let forLabels = labelRegionSelection
                DispatchQueue.main.async { [weak self] in
                    guard let self else { return }
                    if forLabels {
                        guard self.labelRegionSelection else { return }
                        self.editor?.finishLabelRegion(rect, on: page)
                    } else { self.editor?.cropDialog(rect: rect, on: page) }
                }
            } else if tool == .redact {
                guard let temp = tempAnnotation, temp.bounds.width > 2, temp.bounds.height > 2 else { return }
                editor?.add(RedactOverlay(bounds: temp.bounds, color: color), to: page, name: L("Redact"))
            } else {
                guard let temp = tempAnnotation, temp.bounds.width > 4, temp.bounds.height > 4 else { return }
                let annotation = PDFAnnotation(bounds: temp.bounds, forType: tool == .oval ? .circle : .square, withProperties: nil)
                style(annotation)
                editor?.add(annotation, to: page, name: tool.title)
            }
        case .none:
            break
        }
    }

    /// PDFView's own menu (copy, zoom…) plus editing commands at the top.
    override func menu(for event: NSEvent) -> NSMenu? {
        let base = super.menu(for: event) ?? NSMenu()
        if let (page, point) = locate(event), let a = hitAnnotation(on: page, at: point) { selectAnnotation(a, on: page) }
        var items: [Command] = []
        if selectedAnnotation?.technicalElement != nil { items.append(Cmd.editTechnicalElement) }
        if selectedAnnotation != nil { items.append(Cmd.deleteAnnotation) }
        if let sel = currentSelection, !(sel.string ?? "").isEmpty { items += [Cmd.replaceText, Cmd.highlightSelection] }
        items += [Cmd.stamps, Cmd.technicalElement, Cmd.labelRegion, Cmd.tool(.text), Cmd.tool(.note)]
        for (i, c) in items.enumerated() { base.insertItem(c.menuItem(), at: i) }
        base.insertItem(.separator(), at: items.count)
        return base
    }

    override func keyDown(with event: NSEvent) {
        if let scalar = event.charactersIgnoringModifiers?.unicodeScalars.first {
            if selectedAnnotation != nil, scalar.value == 127 || Int(scalar.value) == NSDeleteFunctionKey {
                editor?.deleteSelectedAnnotation(nil)
                return
            }
            if scalar.value == 27 {
                cancelInteraction()
                cancelTextEdit()
                selectAnnotation(nil)
                editor?.setTool(.select)
                return
            }
        }
        super.keyDown(with: event)
    }

    // MARK: - Link borders

    /// Re-creates the dashed frames over links on every page (they are never saved).
    func updateLinkBorders() {
        guard let doc = document else { return }
        let show = showsLinkBorders || tool == .link
        for i in 0..<doc.pageCount {
            guard let page = doc.page(at: i) else { continue }
            let old = page.annotations.filter { $0 is LinkBorderMarker }
            old.forEach { page.removeAnnotation($0) }
            if show {
                for link in page.annotations where link.isKind(.link) { page.addAnnotation(LinkBorderMarker(around: link)) }
            }
            if !old.isEmpty || show { annotationsChanged(on: page) }
        }
    }

    // MARK: - Helpers

    private func style(_ annotation: PDFAnnotation) {
        let border = PDFBorder()
        border.lineWidth = tool == .whiteout ? 0 : lineWidth
        annotation.border = border
        annotation.color = tool == .whiteout ? .white : color
        if tool == .whiteout { annotation.interiorColor = .white }
        annotation.userName = NSFullUserName()
        annotation.modificationDate = Date()
    }

    private func updateInkPreview() {
        guard let annotation = tempAnnotation, let page = dragPage else { return }
        annotation.paths?.forEach { annotation.remove($0) }
        annotation.add(localPath(inkPoints, origin: annotation.bounds.origin))
        annotationsChanged(on: page)
    }

    private func localPath(_ points: [CGPoint], origin: CGPoint) -> NSBezierPath {
        let path = NSBezierPath()
        path.lineCapStyle = .round
        path.lineJoinStyle = .round
        path.lineWidth = lineWidth
        guard let first = points.first else { return path }
        path.move(to: CGPoint(x: first.x - origin.x, y: first.y - origin.y))
        if points.count == 1 { path.line(to: CGPoint(x: first.x - origin.x + 0.1, y: first.y - origin.y)) }
        for p in points.dropFirst() { path.line(to: CGPoint(x: p.x - origin.x, y: p.y - origin.y)) }
        return path
    }

    private func rect(from a: CGPoint, to b: CGPoint) -> CGRect {
        CGRect(x: min(a.x, b.x), y: min(a.y, b.y), width: abs(a.x - b.x), height: abs(a.y - b.y))
    }
}
