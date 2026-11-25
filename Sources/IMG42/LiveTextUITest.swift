import AppKit
import CoreText
import Kit42
import VisionKit

extension ImageWindowController {
    /// Real VisionKit analysis and queued native mouse/key gestures. Does not use the system pasteboard.
    func auditLiveText() async -> Bool {
        guard let window else { print("FAIL IMG42 Live Text: no window"); return false }
        var count = 0, failures = 0
        func check(_ condition: Bool, _ name: String) {
            count += 1; if !condition { failures += 1 }
            print(condition ? "PASS" : "FAIL", "IMG42 Live Text:", name); fflush(stdout)
        }
        func wait(_ condition: () -> Bool, seconds: Double = 3) async -> Bool {
            let deadline = Date(timeIntervalSinceNow: seconds)
            while !condition(), Date() < deadline { try? await Task.sleep(nanoseconds: 20_000_000) }
            return condition()
        }
        func focus() async -> Bool {
            NSApp.activate(ignoringOtherApps: true); window.makeKeyAndOrderFront(nil); window.makeMain()
            return await wait({ NSApp.isActive && window.isKeyWindow }, seconds: 1)
        }
        func normalized(_ value: String) -> String {
            value.split(whereSeparator: \.isWhitespace).joined(separator: " ").uppercased()
        }
        check(ImageAnalyzer.isSupported, "VisionKit text analysis is supported on the audit desktop")
        guard ImageAnalyzer.isSupported else { print("IMG42 Live Text regressions: \(count - failures)/\(count) passed"); return false }
        let lines = ["ALPHA BRAVO CHARLIE", "DELTA ECHO FOXTROT", "GOLF HOTEL INDIA"]
        let font = CTFontCreateWithName("Helvetica Neue" as CFString, 32, nil)
        let textLines = lines.map { text in
            CTLineCreateWithAttributedString(NSAttributedString(string: text, attributes: [.font: font, .foregroundColor: NSColor.black]))
        }
        let size = CGSize(width: 600, height: 300), left: CGFloat = 60
        let baselines: [CGFloat] = [220, 140, 60]
        guard let fixture = ImageLoader.render(size: size, { context in
            context.setFillColor(NSColor.white.cgColor); context.fill(CGRect(origin: .zero, size: size))
            for (index, line) in textLines.enumerated() {
                context.textPosition = CGPoint(x: left, y: baselines[index]); CTLineDraw(line, context)
            }
        }) else { check(false, "text fixture renders"); return false }
        func wordPoint(_ word: String, line index: Int) -> CGPoint {
            let range = (lines[index] as NSString).range(of: word)
            let a = CTLineGetOffsetForStringIndex(textLines[index], range.location, nil)
            let b = CTLineGetOffsetForStringIndex(textLines[index], NSMaxRange(range), nil)
            var ascent: CGFloat = 0, descent: CGFloat = 0
            _ = CTLineGetTypographicBounds(textLines[index], &ascent, &descent, nil)
            return CGPoint(x: left + (a + b) / 2, y: size.height - baselines[index] - (ascent - descent) / 2)
        }
        let markup = canvas.markup, live = canvas.liveText
        let saved = state, savedOriginal = originalBase, savedDescription = originalDescription, savedDPI = originalDPI
        let savedURLs = urls, savedURL = url, savedLoaded = loaded, savedSort = sortOrder, savedWrap = wraps
        let oldTool = markup.tool, oldShown = markupBar.strip.isShown, oldZoom = canvas.magnification, oldZoomMode = canvas.zoomMode
        var observedEvents: [String] = []
        let monitor = NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .leftMouseDragged, .leftMouseUp, .keyDown]) { event in
            if event.window === window {
                let detail = event.type == .keyDown ? "key=\(event.keyCode)" : "clicks=\(event.clickCount)"
                observedEvents.append("\(event.type.rawValue):\(detail):\(NSStringFromPoint(event.locationInWindow))")
            }
            return event
        }
        defer {
            if let monitor { NSEvent.removeMonitor(monitor) }
            live.overlay.resetSelection(); markup.cancelInteraction(); markup.cancelTextEditing(); undo.removeAllActions()
            sortOrder = savedSort; wraps = savedWrap
            if let savedURL, let savedLoaded {
                setURLsKeepingCurrent(savedURLs, current: savedURL)
                adoptSavedURL(savedURL, loaded: savedLoaded)
            }
            restore(saved); originalBase = savedOriginal; originalDescription = savedDescription; originalDPI = savedDPI
            markupBar.setTool(oldTool); markupBar.show(oldShown); canvas.zoom(to: oldZoom); canvas.zoomMode = oldZoomMode
            updateSubtitle()
        }
        stopAnimation(); markup.cancelInteraction(); markup.cancelTextEditing()
        base = fixture; originalBase = fixture; displayed = fixture; adjustments = Adjustments(); autoFilters = nil
        imageDescription = nil; originalDescription = nil; imageDPI = 72; originalDPI = 72
        markup.marks = []; markup.selectedID = nil; markup.pixelSelection = nil; markup.hideMarks = false
        markupBar.show(false); undo.removeAllActions()
        canvas.setImage(fixture, size: size, resetZoom: true); canvas.zoom(to: 1)
        window.layoutIfNeeded(); window.makeFirstResponder(markup)
        var analysed = false
        live.analyze(fixture); live.whenAnalysed { analysed = true }
        check(await wait({ analysed }, seconds: 15), "real VisionKit analysis finishes")
        print("IMG42 Live Text transcript:", live.overlay.text.debugDescription)
        let transcript = normalized(live.overlay.text)
        check(live.analysedImage === fixture && lines.allSatisfy { transcript.contains($0) }, "recognition contains every generated text line")
        let bravo = wordPoint("BRAVO", line: 0)
        check(await wait({ live.overlay.hasText(at: bravo) }), "recognised BRAVO is hit-testable at its image coordinates")
        check(await focus(), "mouse gestures target an active key window")
        MouseScript.click(bravo, count: 2, in: canvas.document)
        check(await wait({ normalized(live.overlay.selectedText) == "BRAVO" }), "native double-click selects the recognised word")
        if normalized(live.overlay.selectedText) != "BRAVO" { print("IMG42 Live Text double-click diagnostic:", live.overlay.selectedText.debugDescription, observedEvents) }
        check(markup.marks.isEmpty && markup.pixelSelection == nil && !isDirty, "reading text creates neither markup nor pixel edits")
        MouseScript.key(53, chars: "\u{1b}", in: canvas.document)
        check(await wait({ !live.hasSelection }), "native Escape clears the recognised text selection")
        if live.hasSelection { print("IMG42 Live Text Escape diagnostic:", String(describing: window.firstResponder), observedEvents) }

        _ = await focus()
        observedEvents.removeAll()
        MouseScript.drag([wordPoint("ALPHA", line: 0), wordPoint("CHARLIE", line: 0)], in: canvas.document)
        check(await wait({ normalized(live.overlay.selectedText).contains("BRAVO") && normalized(live.overlay.selectedText).count > 5 }),
              "native drag selects recognised text across words")
        if !normalized(live.overlay.selectedText).contains("BRAVO") { print("IMG42 Live Text drag diagnostic:", live.overlay.selectedText.debugDescription, observedEvents) }
        let all = NSMenuItem(title: "", action: #selector(NSResponder.selectAll(_:)), keyEquivalent: "")
        check(NSApp.sendAction(#selector(NSResponder.selectAll(_:)), to: nil, from: all), "Select All resolves through the native responder chain")
        check(await wait({ lines.allSatisfy { normalized(live.overlay.selectedText).contains($0) } }), "Select All selects every recognised line")
        check(markup.pixelSelection == nil && markup.marks.isEmpty, "Select All on Live Text does not select or modify image pixels")
        MouseScript.key(53, chars: "\u{1b}", in: canvas.document)
        check(await wait({ !live.hasSelection }), "Escape also clears the complete recognised selection")

        let draw = NSMenuItem(); draw.tag = MarkTool.draw.rawValue
        _ = NSApp.sendAction(#selector(selectToolAction(_:)), to: self, from: draw)
        window.layoutIfNeeded(); _ = await focus()
        check(!live.allowsInteraction(bravo) && live.hitTest(live.convert(bravo, to: live.superview)) == nil,
              "Draw disables Live Text interception at recognised text")
        MouseScript.drag([wordPoint("ALPHA", line: 0), wordPoint("CHARLIE", line: 0)], in: canvas.document)
        check(await wait({ markup.marks.count == 1 && markup.marks[0].kind == .pen }), "native drag over text with Draw creates an ink stroke")
        check(!live.hasSelection, "Draw does not silently select text instead of drawing")
        undo.undo(); check(markup.marks.isEmpty, "drawing over Live Text is undoable")

        markupBar.show(false); window.layoutIfNeeded(); _ = await focus()
        MouseScript.click(wordPoint("ECHO", line: 1), count: 2, in: canvas.document)
        check(await wait({ normalized(live.overlay.selectedText) == "ECHO" }), "returning to viewing restores native text selection")

        let navigationFolder = FileManager.default.temporaryDirectory.appendingPathComponent("IMG42-live-text-\(UUID().uuidString)", isDirectory: true)
        defer { try? FileManager.default.removeItem(at: navigationFolder) }
        do {
            try FileManager.default.createDirectory(at: navigationFolder, withIntermediateDirectories: true)
            let first = navigationFolder.appendingPathComponent("a-text.png"), second = navigationFolder.appendingPathComponent("b-green.png")
            let secondImage = ImageLoader.render(size: CGSize(width: 320, height: 240)) { context in
                context.setFillColor(NSColor.green.cgColor); context.fill(CGRect(x: 0, y: 0, width: 320, height: 240))
            }!
            try ImageWriter.write(fixture, to: first, type: "public.png")
            try ImageWriter.write(secondImage, to: second, type: "public.png")
            sortOrder = .name; wraps = false
            setSet([first, second], current: first)
            for (code, characters, name): (UInt16, String, String) in [(49, " ", "Space"), (121, "\u{f72d}", "Page Down")] {
                show(index: 0)
                let loadedFirst = await wait({ self.url == first && self.loaded?.url == first && self.base?.width == 600 })
                canvas.zoom(to: 1); window.layoutIfNeeded(); _ = await focus()
                let recognised = await wait({ live.overlay.hasText(at: bravo) }, seconds: 15)
                observedEvents.removeAll()
                MouseScript.click(bravo, count: 2, in: canvas.document)
                let selected = await wait({ normalized(live.overlay.selectedText) == "BRAVO" })
                let previousImage = base, responder = String(describing: window.firstResponder)
                MouseScript.key(code, chars: characters, in: canvas.document)
                let navigated = await wait({ self.url == second && self.loaded?.url == second && self.base?.width == 320 && self.base?.height == 240 })
                check(loadedFirst && recognised && selected && navigated && base !== previousImage && !live.hasSelection,
                      "native \(name) after Live Text selection changes the URL and displayed image")
                if !navigated {
                    print("IMG42 Live Text navigation diagnostic:", name, responder, "selected=", selected, "url=", String(describing: url), observedEvents)
                }
            }
        } catch { check(false, "Live Text navigation fixtures: \(error)") }
        print("IMG42 Live Text regressions: \(count - failures)/\(count) passed"); fflush(stdout)
        return failures == 0
    }
}
