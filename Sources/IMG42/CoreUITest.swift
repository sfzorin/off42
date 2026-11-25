import AppKit
import CoreText
import Kit42

extension ImageWindowController {
    @MainActor func auditCore() async -> Bool {
        guard let window, let initialURL = url else { print("FAIL IMG42 core: missing fixture"); return false }
        var checks = 0, failures = 0
        func check(_ condition: Bool, _ name: String) { checks += 1; if !condition { failures += 1 }; print(condition ? "PASS" : "FAIL", "IMG42 core:", name) }
        func command(_ command: Command) { check(NSApp.sendAction(command.action, to: self, from: command.menuItem()), "native action: " + command.title) }
        func views(_ view: NSView) -> [NSView] { [view] + view.subviews.flatMap(views) }
        func commands(_ elements: [MenuElement]) -> [Command] {
            elements.flatMap { element -> [Command] in
                switch element { case .command(let command): return [command]; case .submenu(_, let children): return commands(children); default: return [] }
            }
        }
        func solid(_ color: NSColor, _ size: CGSize) -> CGImage {
            ImageLoader.render(size: size) { context in context.setFillColor(color.cgColor); context.fill(CGRect(origin: .zero, size: size)) }!
        }
        let folder = initialURL.deletingLastPathComponent()
        let a = folder.appendingPathComponent("a.png"), b = folder.appendingPathComponent("b.png"), bad = folder.appendingPathComponent("c.png")
        do {
            let red = solid(.red, CGSize(width: 320, height: 240)), green = solid(.green, CGSize(width: 640, height: 480))
            try ImageWriter.write(red, to: a, type: "public.png")
            try ImageWriter.write(green, to: b, type: "public.png")
            try Data("broken image".utf8).write(to: bad)
            sortOrder = .name; wraps = false
            setSet([a, b, a], current: a)
            check(await UITest.wait { self.loaded?.url == a && self.base != nil }, "navigation fixture loads")
            check(urls.count == 2, "opening overlapping inputs removes duplicate URLs")
            NSApp.activate(ignoringOtherApps: true); window.makeKeyAndOrderFront(nil); window.makeFirstResponder(canvas.markup)
            check(await UITest.wait { window.isKeyWindow }, "command inventory has a key window")
            if let delegate = AppDelegate.shared {
                let spec = Cmd.menuBar(openWithDelegate: delegate)
                let inventory = commands(spec.fileExtras + spec.editExtras + spec.view + spec.help + spec.custom.flatMap(\.items))
                let missing = inventory.filter { NSApp.target(forAction: $0.action, to: nil, from: $0.menuItem()) == nil }
                if !missing.isEmpty { print("Unwired IMG42 commands:", missing.map(\.title)) }
                check(missing.isEmpty, "every declared IMG42 command resolves a responder (wiring inventory)")
                let items = (window.toolbar?.items ?? []).flatMap { ($0 as? NSToolbarItemGroup)?.subitems ?? [$0] }
                check(items.allSatisfy { item in item.action.map { NSApp.target(forAction: $0, to: item.target, from: item) != nil } ?? true }, "toolbar actions resolve responders")
                print("IMG42 command inventory:", inventory.count, "menu declarations,", items.count, "toolbar items")
            }
            invalidateCache(b)
            command(Cmd.next)
            check(loaded == nil && base == nil && !validate(#selector(saveDocumentAs(_:)), tag: 0), "pending navigation cannot edit or export the previous image under the new URL")
            check(!toolbarImpl.share.isEnabled && !toolbarImpl.highlight.isEnabled, "custom toolbar controls disable while the destination loads")
            check(await UITest.wait { self.loaded?.url == b && self.base?.width == 640 }, "Next opens the selected image")
            check(toolbarImpl.share.isEnabled && toolbarImpl.highlight.isEnabled, "custom toolbar controls enable after decoding")
            command(Cmd.next); check(index == 1, "Next at the last image does not wrap when looping is off")
            command(Cmd.previous); check(await UITest.wait { self.loaded?.url == a }, "Previous returns to the first image")
            toggleWrapAction(nil); command(Cmd.previous); check(await UITest.wait { self.loaded?.url == b }, "Previous wraps when looping is on")
            command(Cmd.first); check(await UITest.wait { self.loaded?.url == a }, "First navigates to the start")
            command(Cmd.last); check(await UITest.wait { self.loaded?.url == b }, "Last navigates to the end")
            invalidateCache(a); invalidateCache(b)
            show(index: 0); show(index: 1)
            check(await UITest.wait { self.loaded?.url == b && self.base?.width == 640 }, "rapid navigation discards obsolete load results")
            for order in SortOrder.allCases {
                let item = NSMenuItem(); item.tag = order.rawValue
                _ = NSApp.sendAction(#selector(sortAction(_:)), to: self, from: item)
                check(sortOrder == order && url == b && loaded?.url == b, "sorting keeps the current image: " + order.title)
            }
            sortOrder = .name; setSet([a, b, bad], current: bad)
            check(await UITest.wait { self.loaded?.url == bad }, "broken-image load completes")
            check(base == nil && loaded?.error != nil && messageView?.isHidden == false, "broken image shows an error without stale pixels")
            command(Cmd.first); check(await UITest.wait { self.loaded?.url == a && self.base != nil }, "navigation recovers after a broken image")
            setSet([a, b], current: a); _ = await UITest.wait { self.loaded?.url == a && self.base != nil }

            command(Cmd.actualSize); check(abs(canvas.magnification - 1) < 0.001, "Actual Size uses 100 percent")
            command(Cmd.zoomIn); let enlarged = canvas.magnification; check(enlarged > 1, "Zoom In enlarges pixels")
            command(Cmd.zoomOut); check(canvas.magnification < enlarged, "Zoom Out reduces pixels")
            command(Cmd.zoomToFit); check(canvas.zoomMode == .fit && canvas.imageFits, "Fit contains the whole image")
            let zoom = canvas.magnification
            canvas.zoom(to: .nan); check(canvas.magnification == zoom, "invalid zoom leaves the canvas unchanged")
            for background in Background.allCases {
                let item = NSMenuItem(); item.tag = background.rawValue
                _ = NSApp.sendAction(#selector(backgroundAction(_:)), to: self, from: item)
                check(self.background == background && canvas.backgroundColor == background.color, "background action: " + background.title)
            }
            let stored = Self.defaults.object(forKey: "panels") as? Int
            setPanels([.toolbar, .info], remember: false)
            check(Self.defaults.object(forKey: "panels") as? Int == stored, "temporary panel restoration does not overwrite saved preferences")
            setPanels([.toolbar, .markup, .filmstrip, .adjust], remember: false)
            let visible = visiblePanels
            toggleAllPanelsAction(nil); check(visiblePanels.isEmpty, "Hide All Panels hides every bar and inspector")
            toggleAllPanelsAction(nil); check(visiblePanels == visible, "Show All Panels restores the exact previous set")
            toggleInspector(infoPanel, forceShow: true)
            if let disclosure = views(infoPanel.view).compactMap({ $0 as? NSButton }).first(where: { $0.bezelStyle == .disclosure }) {
                disclosure.performClick(nil)
                let expanded = disclosure.state == .on
                check(infoPanel.metadataIsExpanded == expanded, "All Metadata button actually expands or collapses its fields")
                infoPanel.show(url: url, loaded: loaded)
                let refreshed = views(infoPanel.view).compactMap { $0 as? NSButton }.first { $0.bezelStyle == .disclosure }
                check(refreshed?.state == (expanded ? .on : .off), "All Metadata disclosure persists and restores its state")
            } else { check(false, "All Metadata disclosure exists") }
            check(InfoPanelController.exposureDescription(0) == nil && InfoPanelController.exposureDescription(.nan) == nil && InfoPanelController.exposureDescription(.infinity) == nil, "invalid EXIF exposure cannot trap while opening Info")
            check(InfoPanelController.exposureDescription(0.008) == "1/125 s" && InfoPanelController.exposureDescription(2) == "2 s", "valid exposure durations retain readable formatting")
            check(InfoPanelController.exposureDescription(Double.leastNonzeroMagnitude) != nil, "extreme positive EXIF exposure avoids integer overflow")
            if let collection = views(filmstrip).compactMap({ $0 as? NSCollectionView }).first {
                filmstrip.collectionView(collection, didSelectItemsAt: [IndexPath(item: 1, section: 0)])
                check(await UITest.wait { self.url == b && self.loaded?.url == b }, "native filmstrip selection changes the image")
            } else { check(false, "native filmstrip collection exists") }

            // Use real timer actions; entering a macOS full-screen Space is outside this fixture.
            wraps = false; go(to: 0); _ = await UITest.wait { self.loaded?.url == a }
            startSlideshow(enterFullScreen: false)
            check(slideshowTimer != nil, "slideshow creates its navigation timer")
            slideshowPaused = true; slideshowTimer?.fire(); check(url == a, "paused slideshow keeps the current image")
            slideshowPaused = false; slideshowTimer?.fire()
            check(await UITest.wait { self.loaded?.url == b }, "slideshow timer advances to the next image")
            slideshowTimer?.fire(); check(slideshowTimer == nil, "non-looping slideshow stops at its last image")

            var animation = LoadedImage(url: a)
            animation.pages = [red, green]; animation.isAnimated = true; animation.frameDelays = [60, 60]
            animation.pixelSize = CGSize(width: 320, height: 240)
            display(animation); advanceAnimationFrame()
            check(base === green && originalBase === green && displayed === green && canvas.imageView.image === green, "animation updates the editing/export model to its visible frame")
            check(!isDirty, "advancing an animation alone stays clean")
            stopAnimation()
            check(finalImage()?.width == 640 && finalImage()?.height == 480, "output uses the visible animation frame rather than frame one")

            let textSize = CGSize(width: 800, height: 300)
            let textImage = ImageLoader.render(size: textSize) { context in
                context.setFillColor(NSColor.white.cgColor); context.fill(CGRect(origin: .zero, size: textSize))
                context.textPosition = CGPoint(x: 40, y: 160)
                let line = CTLineCreateWithAttributedString(NSAttributedString(string: "needle alpha needle", attributes: [.font: NSFont(name: "Helvetica", size: 38)!, .foregroundColor: NSColor.black]))
                CTLineDraw(line, context)
            }!
            var recognized = LoadedImage(url: a); recognized.pages = [textImage]; recognized.pixelSize = textSize
            display(recognized)
            let lines = PixelOps.recognizeText(textImage)
            check(lines.contains { $0.text.localizedCaseInsensitiveContains("needle") }, "Vision recognizes text from the image fixture")
            let session = imageSession
            check(acceptRecognizedText(lines, image: textImage, session: session), "current OCR results enter the image cache")
            toolbarImpl.searchField?.stringValue = "needle"; runSearch("needle")
            check(canvas.markup.searchBoxes.count == 2 && searchIndex == 0, "Search highlights separate occurrences on one line")
            let generation = searchGeneration
            runSearch("")
            check(canvas.markup.searchBoxes.isEmpty && searchIndex == -1, "clearing Search removes its results")
            check(!applySearch("needle", lines: lines, image: textImage, generation: generation) && canvas.markup.searchBoxes.isEmpty, "an obsolete query cannot restore cleared search results")
            imageSession = UUID()
            check(!acceptRecognizedText(lines, image: textImage, session: session), "navigation invalidates an old OCR callback even for a cached bitmap")
            let current = imageSession
            resetEditing(base: red)
            check(!acceptRecognizedText(lines, image: textImage, session: current), "pixel changes invalidate an old OCR callback")
            setSet([], current: nil)
            check(base == nil && loaded == nil && window.title == "IMG42" && !window.isDocumentEdited, "empty image set clears the model, title and dirty state")
            check(!toolbarImpl.share.isEnabled && !toolbarImpl.highlight.isEnabled, "image-only custom controls disable for an empty window")
        } catch { check(false, "fixture error: " + error.localizedDescription) }
        print("IMG42 core regressions: \(checks - failures)/\(checks) passed")
        return failures == 0
    }
}
