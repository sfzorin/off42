import AppKit
import Kit42

extension ImageWindowController {
    /// Mouse/key, native field-editor, style-control and rendered-pixel regressions in a disposable window.
    func auditMarkup() async -> Bool {
        guard let window, let image = ImageLoader.render(size: CGSize(width: 480, height: 360), { context in
            context.setFillColor(NSColor.white.cgColor); context.fill(CGRect(x: 0, y: 0, width: 480, height: 360))
        }) else { print("FAIL IMG42 markup: fixture"); return false }
        let markup = canvas.markup
        let saved = state, savedOriginal = originalBase, savedDescription = originalDescription, savedDPI = originalDPI
        let oldGroups = undo.groupsByEvent, oldStyle = markupBar.style, oldZoom = canvas.magnification
        let oldZoomMode = canvas.zoomMode, oldTool = markup.tool, oldShown = markupBar.strip.isShown
        undo.groupsByEvent = false
        var count = 0, failures = 0
        func check(_ condition: Bool, _ name: String) {
            count += 1; if !condition { failures += 1 }
            print(condition ? "PASS" : "FAIL", "IMG42 markup:", name); fflush(stdout)
        }
        func close(_ a: CGFloat, _ b: CGFloat) -> Bool { abs(a - b) < 1e-7 }
        func rect(_ a: CGRect, _ b: CGRect) -> Bool { close(a.minX, b.minX) && close(a.minY, b.minY) && close(a.width, b.width) && close(a.height, b.height) }
        func event(_ body: () -> Void) { undo.beginUndoGrouping(); body(); undo.endUndoGrouping() }
        func choose(_ tool: MarkTool) {
            let item = NSMenuItem(); item.tag = tool.rawValue
            _ = NSApp.sendAction(#selector(selectToolAction(_:)), to: self, from: item)
        }
        func reset() {
            markup.cancelInteraction(); markup.cancelTextEditing()
            base = image; originalBase = image; displayed = image; adjustments = Adjustments(); autoFilters = nil
            imageDescription = nil; originalDescription = nil; imageDPI = 72; originalDPI = 72
            markup.marks = []; markup.selectedID = nil; markup.pixelSelection = nil; markup.hideMarks = false; markup.aspect = .free
            markupBar.style = MarkStyle(); markupBar.show(true); choose(.select)
            canvas.setImage(image, size: CGSize(width: 480, height: 360), resetZoom: true)
            canvas.zoom(to: 1); window.makeKeyAndOrderFront(nil); window.makeFirstResponder(markup)
            window.layoutIfNeeded(); undo.removeAllActions(); updateSubtitle()
        }
        defer {
            markup.cancelInteraction(); markup.cancelTextEditing(); undo.removeAllActions()
            restore(saved); originalBase = savedOriginal; originalDescription = savedDescription; originalDPI = savedDPI
            markupBar.style = oldStyle; markupBar.setTool(oldTool); markupBar.show(oldShown)
            canvas.zoom(to: oldZoom); canvas.zoomMode = oldZoomMode; undo.groupsByEvent = oldGroups; updateSubtitle()
        }
        func mouse(_ type: NSEvent.EventType, _ point: CGPoint, flags: NSEvent.ModifierFlags = [], clicks: Int = 1) {
            let event = NSEvent.mouseEvent(with: type, location: markup.convert(point, to: nil), modifierFlags: flags,
                timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber, context: nil,
                eventNumber: 1, clickCount: clicks, pressure: type == .leftMouseUp ? 0 : 1)!
            switch type {
            case .leftMouseDown: markup.mouseDown(with: event)
            case .leftMouseDragged: markup.mouseDragged(with: event)
            default: markup.mouseUp(with: event)
            }
        }
        func drag(_ start: CGPoint, _ end: CGPoint, flags: NSEvent.ModifierFlags = []) {
            mouse(.leftMouseDown, start, flags: flags); mouse(.leftMouseDragged, end, flags: flags); mouse(.leftMouseUp, end, flags: flags)
        }
        func key(_ code: UInt16) {
            let event = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
                windowNumber: window.windowNumber, context: nil, characters: code == 53 ? "\u{1b}" : "\u{7f}",
                charactersIgnoringModifiers: "", isARepeat: false, keyCode: code)!
            markup.keyDown(with: event)
        }
        func menu(_ selector: Selector, tag: Int = 0, grouped: Bool = true) {
            let item = NSMenuItem(); item.tag = tag
            if grouped { event { _ = NSApp.sendAction(selector, to: markupBar, from: item) } }
            else { _ = NSApp.sendAction(selector, to: markupBar, from: item) }
        }
        func bytes(_ image: CGImage) -> [UInt8] {
            guard let context = CGContext(data: nil, width: image.width, height: image.height, bitsPerComponent: 8, bytesPerRow: image.width * 4,
                space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue), let data = context.data else { return [] }
            context.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
            return Array(UnsafeBufferPointer(start: data.assumingMemoryBound(to: UInt8.self), count: image.width * image.height * 4))
        }
        reset()
        for tool in MarkTool.allCases {
            choose(tool); check(markup.tool == tool && markup.markupActive && markupBar.strip.isShown, "tool command selects " + tool.title)
        }
        for tool in MarkTool.allCases where tool.markKind != nil && tool != .text {
            reset(); choose(tool)
            event { drag(CGPoint(x: 90, y: 90), CGPoint(x: 190, y: 150)) }
            check(markup.marks.count == 1 && markup.marks.first?.kind == tool.markKind, "native drag creates " + tool.title)
            let created = markup.marks.first
            undo.undo(); check(markup.marks.isEmpty, tool.title + " creation Undo")
            undo.redo(); check(created.map { saved in markup.marks.first?.hasSameContent(as: saved) == true } == true, tool.title + " creation Redo")
        }
        reset(); choose(.line)
        event { mouse(.leftMouseDown, CGPoint(x: 150, y: 120)); mouse(.leftMouseUp, CGPoint(x: 150, y: 120)) }
        check(markup.marks.first?.points.count == 2 && close((markup.marks.first?.points.last?.x ?? 0) - (markup.marks.first?.points.first?.x ?? 0), 120), "clicking the Line tool places a usable default line")
        reset(); choose(.rectangle)
        mouse(.leftMouseDown, CGPoint(x: 50, y: 50)); mouse(.leftMouseDragged, CGPoint(x: 160, y: 110)); key(53); mouse(.leftMouseUp, CGPoint(x: 160, y: 110))
        check(markup.marks.isEmpty && !undo.canUndo, "Escape discards a drawing preview without Undo")
        mouse(.leftMouseDown, CGPoint(x: 50, y: 50)); mouse(.leftMouseDragged, CGPoint(x: 160, y: 110)); choose(.oval); mouse(.leftMouseUp, CGPoint(x: 160, y: 110))
        check(markup.marks.isEmpty && !undo.canUndo, "tool switch discards the previous unfinished drawing")
        choose(.rectangle); event { drag(CGPoint(x: 70, y: 70), CGPoint(x: 190, y: 150)) }; choose(.select)
        guard let rectangle = markup.marks.first else { check(false, "rectangle fixture"); return false }
        markup.selectedID = rectangle.id; undo.removeAllActions()
        event { drag(CGPoint(x: 110, y: 70), CGPoint(x: 135, y: 100)) }
        check(rect(markup.marks[0].rect, rectangle.rect.offsetBy(dx: 25, dy: 30)), "Select moves the selected shape")
        undo.undo(); check(rect(markup.marks[0].rect, rectangle.rect), "move Undo restores the original geometry")
        undo.redo(); check(rect(markup.marks[0].rect, rectangle.rect.offsetBy(dx: 25, dy: 30)), "move Redo restores the moved geometry")
        let moved = markup.marks[0]; markup.selectedID = moved.id; undo.removeAllActions()
        mouse(.leftMouseDown, CGPoint(x: moved.rect.midX, y: moved.rect.minY)); mouse(.leftMouseDragged, CGPoint(x: moved.rect.midX + 30, y: moved.rect.minY + 20)); key(53)
        check(markup.marks[0].hasSameContent(as: moved) && !undo.canUndo, "Escape rolls a live move back without Undo")
        markup.selectedID = moved.id
        event { drag(CGPoint(x: moved.rect.maxX, y: moved.rect.maxY), CGPoint(x: moved.rect.maxX + 30, y: moved.rect.maxY + 25)) }
        check(close(markup.marks[0].rect.width, moved.rect.width + 30) && close(markup.marks[0].rect.height, moved.rect.height + 25), "corner handle resizes both dimensions")
        undo.undo(); check(markup.marks[0].hasSameContent(as: moved), "resize Undo restores the exact mark")
        undo.redo(); check(close(markup.marks[0].rect.width, moved.rect.width + 30), "resize Redo restores width")
        markup.selectedID = moved.id; undo.removeAllActions()
        let resized = markup.marks[0], corner = CGPoint(x: resized.rect.maxX, y: resized.rect.maxY)
        mouse(.leftMouseDown, corner); mouse(.leftMouseUp, corner)
        check(!undo.canUndo, "clicking a resize handle without moving creates no Undo")
        menu(#selector(MarkupBar.widthChosen(_:)), tag: 3)
        check(markup.marks[0].width == 5, "line-width control changes the selected border")
        undo.undo(); check(markup.marks[0].width == resized.width, "border width Undo")
        undo.redo(); check(markup.marks[0].width == 5, "border width Redo")
        markup.selectedID = moved.id; undo.removeAllActions(); menu(#selector(MarkupBar.widthChosen(_:)), tag: 3, grouped: false)
        check(!undo.canUndo, "choosing the existing width creates no Undo")
        markupBar.fillButton.color = NSColor(srgbRed: 0.1, green: 0.7, blue: 0.2, alpha: 1)
        event { _ = NSApp.sendAction(markupBar.fillButton.action!, to: markupBar, from: markupBar.fillButton) }
        check(markup.marks[0].fill != nil && markupBar.fillButton.color != nil, "native colour control applies fill and reflects selection")
        let filled = Mark.flatten(markup.marks, onto: image), filledBytes = bytes(filled)
        let centre = (Int(markup.marks[0].rect.midY) * filled.width + Int(markup.marks[0].rect.midX)) * 4
        check(filledBytes.count > centre + 3 && Int(filledBytes[centre + 1]) > Int(filledBytes[centre]) + 80, "filled rectangle produces green pixels in saved rendering")
        undo.undo(); check(markup.marks[0].fill == nil, "fill Undo restores transparency")
        check(markupBar.fillButton.color == nil && markupBar.fillButton.isEnabled, "fill Undo refreshes the colour control for the retained selection")
        undo.redo(); check(markup.marks[0].fill != nil, "fill Redo restores colour")
        check(markupBar.fillButton.color != nil && markupBar.fillButton.isEnabled, "fill Redo refreshes the colour control")
        markup.selectedID = moved.id; menu(#selector(MarkupBar.dashChosen(_:)), tag: 1)
        check(markup.marks[0].dashed, "Dash changes the selected border")
        menu(#selector(MarkupBar.shadowChosen(_:))); check(markup.marks[0].shadow, "Shadow changes the selected shape")
        let beforeDelete = markup.marks; event { key(51) }; check(markup.marks.isEmpty, "Delete key removes selected markup")
        undo.undo(); check(markup.marks.count == 1 && markup.marks[0].hasSameContent(as: beforeDelete[0]), "Delete Undo restores full style and geometry")
        undo.redo(); check(markup.marks.isEmpty, "Delete Redo")

        reset(); choose(.selectRect)
        event { drag(CGPoint(x: 50, y: 40), CGPoint(x: 170, y: 120)) }
        check(markup.pixelSelection.map { rect($0.bounds, CGRect(x: 50, y: 40, width: 120, height: 80)) } == true, "rectangle selection follows the native drag")
        event { drag(CGPoint(x: 170, y: 120), CGPoint(x: 200, y: 145)) }
        check(markup.pixelSelection.map { rect($0.bounds, CGRect(x: 50, y: 40, width: 150, height: 105)) } == true, "visible selection corner handles really resize the selection")
        let selection = markup.pixelSelection!
        mouse(.leftMouseDown, CGPoint(x: 200, y: 145)); mouse(.leftMouseDragged, CGPoint(x: 250, y: 190)); key(53)
        check(markup.pixelSelection.map { rect($0.bounds, selection.bounds) } == true, "Escape rolls selection resizing back")
        event { drag(CGPoint(x: 110, y: 80), CGPoint(x: 600, y: 500)) }
        check(markup.pixelSelection.map { rect($0.bounds, CGRect(x: 330, y: 255, width: 150, height: 105)) } == true, "moving selection to image edge preserves its full extent")
        markup.pixelSelection = nil; markup.aspect = .square
        event { drag(CGPoint(x: 430, y: 310), CGPoint(x: 490, y: 330)) }
        check(markup.pixelSelection.map { rect($0.bounds, CGRect(x: 430, y: 310, width: 50, height: 50)) } == true, "aspect constraint stays square when it meets image edges")
        markup.pixelSelection = nil; markup.aspect = .free; choose(.selectEllipse)
        event { drag(CGPoint(x: 80, y: 80), CGPoint(x: 200, y: 160)) }
        check(markup.pixelSelection?.isRectangle == false && markup.pixelSelection?.outline?.contains(CGPoint(x: 140, y: 120)) == true,
              "elliptical selection creates a nonrectangular mask")
        event { drag(CGPoint(x: 82, y: 82), CGPoint(x: 105, y: 105)) }
        check(markup.pixelSelection.map { $0.bounds.width < 30 } == true, "click outside the ellipse starts a new selection rather than moving its bounding box")
        markup.pixelSelection = nil; choose(.selectLasso)
        mouse(.leftMouseDown, CGPoint(x: 60, y: 60)); mouse(.leftMouseDragged, CGPoint(x: 180, y: 70)); mouse(.leftMouseDragged, CGPoint(x: 120, y: 190)); mouse(.leftMouseUp, CGPoint(x: 60, y: 60))
        check(markup.pixelSelection?.outline?.contains(CGPoint(x: 120, y: 100)) == true, "lasso closes the native stroke around selected pixels")
        reset(); choose(.instantAlpha); mouse(.leftMouseDown, CGPoint(x: 20, y: 20)); mouse(.leftMouseUp, CGPoint(x: 20, y: 20))
        let alphaDeadline = Date(timeIntervalSinceNow: 3)
        while markup.pixelSelection == nil && Date() < alphaDeadline { try? await Task.sleep(nanoseconds: 20_000_000) }
        if markup.pixelSelection == nil { print("IMG42 Alpha diagnostic", "canvas", markup.bounds.size, "displayed", displayed?.width as Any, "source identity", markup.baseImage?() === image) }
        check(markup.pixelSelection.map { rect($0.bounds, CGRect(x: 0, y: 0, width: 480, height: 360)) } == true, "Instant Alpha selects the contiguous colour region")
        mouse(.leftMouseDown, CGPoint(x: 20, y: 20)); choose(.select); try? await Task.sleep(nanoseconds: 150_000_000)
        check(markup.pixelSelection == nil, "switching tools prevents an old Instant Alpha result from reappearing")

        reset(); choose(.text)
        mouse(.leftMouseDown, CGPoint(x: 90, y: 100))
        check(markup.isEditingText && !markup.hasPendingTextEdit && !isDirty, "opening an empty text editor stays clean")
        func type(_ value: String) -> Bool {
            guard let field = markup.subviews.compactMap({ $0 as? NSTextField }).first,
                  let editor = field.currentEditor() as? NSTextView else { return false }
            editor.selectAll(nil); editor.insertText(value, replacementRange: editor.selectedRange()); return true
        }
        check(type("  Native text\n") && markup.hasPendingTextEdit && isDirty, "native field typing marks the document dirty")
        event { markup.endTextEditing() }
        check(markup.marks.first?.text == "  Native text\n", "commit reads the native editor and preserves authored whitespace")
        undo.undo(); check(markup.marks.isEmpty, "text creation Undo")
        undo.redo(); check(markup.marks.first?.text == "  Native text\n", "text creation Redo")
        guard let text = markup.marks.first else { check(false, "text fixture"); return false }
        choose(.select); markup.selectedID = text.id; undo.removeAllActions()
        mouse(.leftMouseDown, CGPoint(x: text.rect.midX, y: text.rect.midY), clicks: 2)
        check(markup.isEditingText && !markup.hasPendingTextEdit, "unchanged text re-edit has no pending changes")
        markup.endTextEditing()
        if undo.canUndo || !markup.marks[0].hasSameContent(as: text) { print("IMG42 unchanged text diagnostic", "undo", undo.canUndo, "bounds", markup.marks[0].rect, text.rect, "text", markup.marks[0].text.debugDescription, text.text.debugDescription) }
        check(!undo.canUndo && markup.marks[0].hasSameContent(as: text), "unchanged text re-edit creates no Undo or resize")
        mouse(.leftMouseDown, CGPoint(x: text.rect.midX, y: text.rect.midY), clicks: 2)
        check(type("Discard this"), "native re-edit accepts replacement text")
        if let field = markup.subviews.compactMap({ $0 as? NSTextField }).first, let editor = field.currentEditor() as? NSTextView {
            _ = markup.control(field, textView: editor, doCommandBy: #selector(NSResponder.cancelOperation(_:)))
        }
        if markup.isEditingText || !markup.marks[0].hasSameContent(as: text) || undo.canUndo {
            print("IMG42 text Escape diagnostic", "editing", markup.isEditingText, "undo", undo.canUndo,
                  "actual text", markup.marks[0].text.debugDescription, "original text", text.text.debugDescription)
        }
        check(!markup.isEditingText && markup.marks[0].hasSameContent(as: text) && !undo.canUndo, "Escape cancels a text draft without changing the original")
        markup.selectedID = text.id; menu(#selector(MarkupBar.fontSizeChosen(_:)), tag: 5)
        check(markup.marks[0].fontSize == 32 && markupBar.currentStyle.fontSize == 32, "text-size command changes selected text and control state")
        menu(#selector(MarkupBar.traitChosen(_:)), tag: 0); menu(#selector(MarkupBar.traitChosen(_:)), tag: 1); menu(#selector(MarkupBar.traitChosen(_:)), tag: 2)
        check(markup.marks[0].bold && markup.marks[0].italic && markup.marks[0].underline, "Bold Italic and Underline commands change selected text")
        menu(#selector(MarkupBar.alignChosen(_:)), tag: 2); check(markup.marks[0].alignment == .right, "alignment command changes selected text")
        menu(#selector(MarkupBar.fontChosen(_:)), tag: 2); check(markup.marks[0].fontName == "Georgia", "font-family command changes selected text")
        check(markup.marks[0].font.fontDescriptor.symbolicTraits.contains([.bold, .italic])
              && (markup.marks[0].textAttributes[.underlineStyle] as? Int) == NSUnderlineStyle.single.rawValue,
              "text rendering attributes contain the chosen font traits and underline")
        let ink = bytes(Mark.flatten(markup.marks, onto: image))
        check(stride(from: 0, to: ink.count, by: 4).contains { ink[$0] < 200 }, "styled text survives final pixel rendering")

        reset()
        if let tall = ImageLoader.render(size: CGSize(width: 10, height: 300), { context in context.setFillColor(NSColor.blue.cgColor); context.fill(CGRect(x: 0, y: 0, width: 10, height: 300)) }) {
            event { markup.insertImage(tall) }
            check(markup.selected.map { markup.visibleRect.contains($0.rect) } == true, "a tall signature image fits inside the visible canvas")
            markupBar.selectionChanged()
            check(!markupBar.borderButton.isEnabled && !markupBar.fillButton.isEnabled && !markupBar.textButton.isEnabled,
                  "image marks do not expose ineffective border fill or text controls")
            undo.removeAllActions(); menu(#selector(MarkupBar.widthChosen(_:)), tag: 4, grouped: false)
            check(!undo.canUndo, "unsupported image border action does not create a phantom Undo")
        } else { check(false, "tall image fixture") }
        var ellipse = Mark(kind: .oval, width: 3, rect: CGRect(x: 40, y: 40, width: 120, height: 80))
        check(!ellipse.hitTest(CGPoint(x: 42, y: 42), tolerance: 2) && ellipse.hitTest(CGPoint(x: 100, y: 40), tolerance: 2), "unfilled oval hit testing follows its visible curve")
        ellipse.fill = .blue
        check(ellipse.hitTest(CGPoint(x: 100, y: 80), tolerance: 2), "filled oval selects by its visible interior")
        if let checker = ImageLoader.render(size: CGSize(width: 96, height: 96), { context in
            for y in stride(from: 0, to: 96, by: 4) {
                for x in stride(from: 0, to: 96, by: 4) {
                    context.setFillColor(((x + y) / 4) % 2 == 0 ? NSColor.black.cgColor : NSColor.white.cgColor)
                    context.fill(CGRect(x: x, y: y, width: 4, height: 4))
                }
            }
        }) {
            var loupe = Mark(kind: .loupe, color: .red, width: 2, rect: CGRect(x: 16, y: 16, width: 64, height: 64), zoom: 1)
            let normal = bytes(Mark.flatten([loupe], onto: checker)); loupe.zoom = 3
            check(bytes(Mark.flatten([loupe], onto: checker)) != normal, "Loupe magnification changes actual sampled image pixels")
            let pixelation = Mark(kind: .pixelate, rect: CGRect(x: 16, y: 16, width: 64, height: 64))
            let originalPixels = bytes(checker), pixelated = bytes(Mark.flatten([pixelation], onto: checker))
            check(pixelated != originalPixels && Array(pixelated.prefix(4)) == Array(originalPixels.prefix(4)),
                  "Pixelate changes selected image pixels and preserves pixels outside its region")
        } else { check(false, "effect rendering fixture") }
        if let gradient = ImageLoader.render(size: CGSize(width: 96, height: 96), { context in
            for x in 0..<96 {
                context.setFillColor(NSColor(srgbRed: CGFloat(x) / 95, green: 0.2, blue: CGFloat(95 - x) / 95, alpha: 1).cgColor)
                context.fill(CGRect(x: x, y: 0, width: 1, height: 96))
            }
        }) {
            let outside = Mark(kind: .pixelate, rect: CGRect(x: -16, y: 16, width: 48, height: 64))
            let inside = Mark(kind: .pixelate, rect: CGRect(x: 0, y: 16, width: 32, height: 64))
            check(bytes(Mark.flatten([outside], onto: gradient)) == bytes(Mark.flatten([inside], onto: gradient)),
                  "Pixelate at the image edge samples and draws the same visible rectangle without stretching")
            check(outside.rect == CGRect(x: -16, y: 16, width: 48, height: 64),
                  "clipping the Pixelate rendering preserves editable object bounds")
        } else { check(false, "edge pixelation fixture") }
        var shadow = Mark(kind: .rectangle, color: .blue, width: 5, rect: CGRect(x: 90, y: 90, width: 120, height: 80))
        let noShadow = bytes(Mark.flatten([shadow], onto: image)); shadow.shadow = true
        check(bytes(Mark.flatten([shadow], onto: image)) != noShadow, "Shadow is rendered even for an unfilled shape")
        var arrow = Mark(kind: .line, color: .red, width: 3, points: [CGPoint(x: 90, y: 90), CGPoint(x: 190, y: 150)])
        let plainLine = bytes(Mark.flatten([arrow], onto: image)); arrow.arrowEnd = true; arrow.arrowStart = true
        check(bytes(Mark.flatten([arrow], onto: image)) != plainLine, "arrowhead options change rendered line pixels")
        if let contrast = ImageLoader.render(size: CGSize(width: 96, height: 96), { context in
            context.setFillColor(NSColor.white.cgColor); context.fill(CGRect(x: 0, y: 0, width: 96, height: 96))
            context.setFillColor(NSColor.black.cgColor); context.fill(CGRect(x: 40, y: 40, width: 16, height: 16))
        }) {
            let highlight = Mark(kind: .highlighter, color: .yellow, width: 4, points: [CGPoint(x: 20, y: 48), CGPoint(x: 76, y: 48)])
            let pixels = bytes(Mark.flatten([highlight], onto: contrast))
            let black = (48 * 96 + 48) * 4, yellow = (48 * 96 + 30) * 4
            check(pixels[black] < 5 && pixels[black + 1] < 5 && pixels[black + 2] < 5
                  && pixels[yellow] > 200 && pixels[yellow + 1] > 200 && pixels[yellow + 2] < 200,
                  "Highlight colours white pixels without hiding underlying black content")
        } else { check(false, "highlight rendering fixture") }
        if let context = CGContext(data: nil, width: 17000, height: 2, bitsPerComponent: 8, bytesPerRow: 0,
                                   space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue),
           let oversized = context.makeImage() {
            check(Mark.flattenChecked([shadow], onto: oversized) == nil,
                  "checked output rejects oversized rendering instead of silently discarding markup")
        } else { check(false, "bounded oversized rendering fixture") }
        let priorZoom = canvas.magnification
        for value: CGFloat in [.nan, .infinity, 0, -1] { canvas.zoom(to: value) }
        check(canvas.magnification == priorZoom, "invalid zoom factors do not corrupt canvas magnification")
        print("IMG42 markup regressions: \(count - failures)/\(count) passed"); fflush(stdout)
        return failures == 0
    }
}
