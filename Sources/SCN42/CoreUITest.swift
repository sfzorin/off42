import AppKit
import ImageIO
import PDFKit
import Kit42
import UniformTypeIdentifiers

extension ScanWindowController {
    @MainActor func auditCore(folder: URL) async -> Bool {
        guard let window else { return false }
        var count = 0, failures = 0
        func check(_ value: Bool, _ name: String) { count += 1; if !value { failures += 1 }; print(value ? "PASS" : "FAIL", "SCN42 core:", name) }
        func action(_ control: NSControl) { if let selector = control.action { check(control.sendAction(selector, to: control.target), "native control action dispatches") } }
        func views(_ v: NSView) -> [NSView] { [v] + v.subviews.flatMap(views) }
        func commands(_ elements: [MenuElement]) -> [Command] {
            elements.flatMap { element -> [Command] in
                switch element { case .command(let c): return [c]; case .submenu(_, let items): return commands(items); default: return [] }
            }
        }
        let device = AuditScanner()
        print("SCN42 core setup: preparing device")
        choose(device)
        print("SCN42 core setup: device ready")
        window.makeFirstResponder(preview.canvas)
        // Cmd.menuBar() attaches a single shared Preset menu; inspect the live menu without rebuilding it.
        let inventory: [Command] = [Cmd.scan, Cmd.overview, Cmd.cancelScan, Cmd.detectPhotos, Cmd.clearAreas,
            Cmd.scanWithPhone, Cmd.scanAndPrint, Cmd.savePreset, Cmd.deletePreset, Cmd.importFiles, Cmd.save, Cmd.export,
            Cmd.exportText, Cmd.mail, Cmd.copyText, Cmd.pagesPanel, Cmd.settingsPanel, Cmd.historyPanel, Cmd.showGlass,
            Cmd.zoomIn, Cmd.zoomOut, Cmd.actualSize, Cmd.zoomToFit, Cmd.rotateLeft, Cmd.rotateRight, Cmd.deletePage,
            Cmd.moveUp, Cmd.moveDown, Cmd.rescan, Cmd.insertHere, Cmd.split, Cmd.recognize, Cmd.enhance,
            Cmd.removeBlank, Cmd.straighten, Cmd.stamps, Cmd.welcome] + Cmd.enhancements + Cmd.colorModes
        let missing = inventory.filter { NSApp.target(forAction: $0.action, to: nil, from: $0.menuItem()) == nil }
        if !missing.isEmpty { print("SCN42 missing responders:", missing.map(\.title)) }
        check(missing.isEmpty, "all declared menu commands resolve responders (wiring only)")
        let toolbar = (window.toolbar?.items ?? []).flatMap { ($0 as? NSToolbarItemGroup)?.subitems ?? [$0] }
        check(toolbar.allSatisfy { item in item.action.map { NSApp.target(forAction: $0, to: item.target, from: item) != nil } ?? true }, "toolbar command responders exist")
        print("SCN42 command inventory:", inventory.count, "menu declarations,", toolbar.count, "toolbar items")
        check(!toolbarImpl.share.isEnabled && !validate(#selector(saveDocument(_:))), "empty document disables Save and Share")
        check(canScan && settingsPanel.scanButton.isEnabled, "ready scanner enables the real Scan button")

        let p = settingsPanel
        let storedSettings = Store.settings
        var feeder = storedSettings; feeder.source = .feeder; feeder.duplex = true
        Store.settings = feeder
        let restored = SettingsPanelController(); _ = restored.view
        check(restored.settings.source == .feeder && restored.settings.duplex, "saved duplex survives opening a panel before device discovery")
        device.prepared = .feeder; restored.update(scanner: device)
        check(restored.settings.duplex && restored.duplex.state == .on && restored.duplex.isEnabled, "supported feeder restores saved duplex after capabilities arrive")
        Store.settings = storedSettings; device.prepared = .flatbed
        for i in Preset.builtIn.indices {
            p.preset.selectItem(at: i); action(p.preset)
            let expected = Preset.builtIn[i].settings
            check(settings.colorMode == expected.colorMode && settings.enhancement == expected.enhancement
                  && settings.resolution == [75, 173, 300].min(by: { abs($0 - expected.resolution) < abs($1 - expected.resolution) }),
                  "preset applies supported settings: " + Preset.builtIn[i].name)
        }
        check(p.resolution.itemTitles.contains(L("%d dpi", 173)), "nonstandard supported scanner resolutions remain selectable")
        p.resolution.selectItem(at: 1); action(p.resolution)
        check(settings.resolution == 173, "displayed resolution equals the request model")
        p.source.selectedSegment = 1; action(p.source)
        check(device.prepared == .feeder && p.duplex.isEnabled && settings.source == .feeder, "choosing Feeder prepares its capabilities before the first scan")
        p.duplex.performClick(nil)
        check(settings.duplex, "duplex checkbox updates the request model")
        p.source.selectedSegment = 0; action(p.source)
        check(settings.source == .flatbed && !p.duplex.isEnabled && !settings.duplex, "flatbed does not retain an unsupported duplex request")
        device.resolutions = [75, 300]; p.update(scanner: device)
        device.deferPreparation = true
        var desired = settings; desired.source = .feeder; desired.resolution = 600; desired.duplex = true
        p.apply(desired); settingsChanged()
        check(preparingSource && settings.resolution == 600 && settings.duplex && !p.resolution.isEnabled,
              "pending source preparation preserves requested DPI and duplex without using old capabilities")
        p.brightness.doubleValue = 0.25; action(p.brightness)
        check(settings.resolution == 600 && settings.duplex,
              "editing another setting during preparation cannot copy a stale resolution picker into the request")
        desired = settings; desired.source = .flatbed; desired.resolution = 173
        p.apply(desired); settingsChanged()
        device.resolutions = [300, 600]; device.finishPreparation()
        check(preparingSource && settings.source == .flatbed && settings.resolution == 173 && !p.resolution.isEnabled,
              "late capabilities for a previous source cannot clamp the newly chosen source settings")
        device.resolutions = [75, 173, 300]; device.finishPreparation(); device.deferPreparation = false
        check(!preparingSource && preparedSource == .flatbed && settings.resolution == 173 && p.resolution.isEnabled && canScan,
              "only the matching source capabilities enable scanning and the resolution picker")
        p.color.selectedSegment = ColorMode.bw.rawValue; action(p.color)
        check(settings.colorMode == .bw && p.threshold.isEnabled, "B&W enables its threshold control")
        p.threshold.doubleValue = 0.7; action(p.threshold)
        check(abs(settings.threshold - 0.7) < 0.001, "threshold control changes processing settings")
        p.brightness.doubleValue = 0.32; action(p.brightness)
        p.contrast.doubleValue = -0.28; action(p.contrast)
        check(abs(settings.brightness - 0.32) < 0.001 && abs(settings.contrast + 0.28) < 0.001, "brightness and contrast reach processing settings")
        p.depth.selectItem(at: 1); action(p.depth)
        check(settings.bitDepth == 16, "bit depth control changes acquisition settings")
        for (button, read, name) in [(p.crop, { self.settings.autoCrop && self.settings.deskew }, "Crop/Deskew"),
                                     (p.rotate, { self.settings.autoRotate }, "Auto Rotate"),
                                     (p.blank, { self.settings.removeBlank }, "Remove Blank"),
                                     (p.photos, { self.settings.detectPhotos }, "Separate Photos"),
                                     (p.descreen, { self.settings.descreen }, "Descreen")] {
            let before = read(); button.performClick(nil); check(read() != before, name + " checkbox changes settings")
        }
        p.more.performClick(nil); check(!p.advanced.isHidden, "More Options disclosure exposes controls")
        p.more.performClick(nil); check(p.advanced.isHidden, "More Options disclosure collapses controls")
        for paper in PaperSize.allCases { p.paper.selectItem(at: paper.rawValue); action(p.paper); check(settings.paper == paper, "paper selection: " + paper.title) }
        p.customW.stringValue = "12"; p.customH.stringValue = "18"; action(p.customW)
        check(abs(settings.customWidth * 2.54 - 12) < 0.01 && abs(settings.customHeight * 2.54 - 18) < 0.01, "custom centimetres become correct scanner inches")
        p.customW.stringValue = "0"; p.controlTextDidChange(Notification(name: NSControl.textDidChangeNotification))
        check(!p.hasValidInput && !canScan && !p.scanButton.isEnabled, "invalid custom size disables scanning without substituting the old size")
        p.paper.selectItem(at: PaperSize.a4.rawValue); action(p.paper)
        check(settings.paper == .a4 && canScan, "a standard paper size can replace invalid custom input")
        var bad = settings; bad.brightness = .infinity; bad.contrast = .nan; bad.customWidth = -5; bad.resolution = Int.max
        p.apply(bad)
        check(settings.brightness == 0 && settings.contrast == 0 && settings.customWidth > 0 && settings.resolution == 300,
              "invalid persisted numeric settings cannot enter requests or filters")
        var plain = ScanSettings(); plain.autoCrop = false; plain.deskew = false; plain.autoRotate = false; plain.enhancement = .none
        p.apply(plain)

        let oldPresets = Store.userPresets, oldPresetIndex = Store.presetIndex
        savePresetAction(nil)
        if await UITest.wait({ window.attachedSheet != nil }), let sheet = window.attachedSheet, let content = sheet.contentView,
           let field = views(content).compactMap({ $0 as? NSTextField }).first(where: { $0.isEditable }),
           let button = views(content).compactMap({ $0 as? NSButton }).first(where: { $0.title == L("Save") }) {
            if let editor = field.currentEditor() as? NSTextView {
                editor.selectAll(nil); editor.insertText("Audit custom preset", replacementRange: editor.selectedRange())
            } else { field.stringValue = "Audit custom preset" }
            button.performClick(nil)
            check(await UITest.wait { Store.userPresets.count == oldPresets.count + 1 }, "Save Preset sheet persists the chosen name and settings")
            check(Store.userPresets.last?.name == "Audit custom preset" && Cmd.presetsMenu.items.last?.title == "Audit custom preset", "saved preset appears in both menu and settings")
            deletePresetAction(nil)
            check(Store.userPresets.count == oldPresets.count && Store.presetIndex == -1, "Delete Preset removes only the custom preset")
        } else { check(false, "native Save Preset controls exist") }
        Store.userPresets = oldPresets; Store.presetIndex = oldPresetIndex; presetsChanged()

        guard let small = Bitmap(width: 320, height: 240, channels: 1, fill: 200).cgImage(),
              let tall = Bitmap(width: 240, height: 480, channels: 1, fill: 80).cgImage() else { return false }
        doc.append([ScanPage(base: small, image: small, dpi: 100, options: plain.processing, report: ProcessReport())])
        pageList.select([0]); window.layoutIfNeeded(); refreshPreview()
        actualSizeAction(nil)
        check(await UITest.wait { abs(self.preview.canvas.frame.width * self.preview.scroll.magnification * window.backingScaleFactor - 320) < 2 }, "Actual Size maps one image pixel to one display pixel even for a small scan")
        let actual = preview.scroll.magnification
        zoomOutAction(nil); check(await UITest.wait { self.preview.scroll.magnification < actual }, "Zoom Out after Actual Size reduces the image")
        zoomInAction(nil); check(await UITest.wait { self.preview.scroll.magnification > actual * 0.81 }, "Zoom In enlarges the image")
        zoomToFitAction(nil); check(await UITest.wait { abs(self.preview.scroll.magnification - 1) < 0.001 }, "Fit restores the complete page")
        let zoom = preview.scroll.magnification; preview.zoom(by: .nan); check(preview.scroll.magnification == zoom, "nonfinite zoom is ignored")
        showHistoryAction(nil); check(!inspectorItem.isCollapsed && inspector.showsHistory, "History command switches the inspector")
        showSettingsAction(nil); check(!inspectorItem.isCollapsed && !inspector.showsHistory, "Settings command restores scan controls")
        let wasCollapsed = sidebarItem.isCollapsed; togglePagesAction(nil)
        check(await UITest.wait { self.sidebarItem.isCollapsed != wasCollapsed }, "Pages command toggles the sidebar")
        togglePagesAction(nil); _ = await UITest.wait { self.sidebarItem.isCollapsed == wasCollapsed }
        overview = small; showGlassAction(nil); check(showingGlass && preview.canvas.editsAreas, "Show Glass enables scan area editing")
        showGlassAction(nil); check(!showingGlass && !preview.canvas.editsAreas, "Show Pages disables glass area editing")
        areas = [CGRect(x: -0.2, y: 0.1, width: 0.5, height: 2), CGRect(x: CGFloat.nan, y: 0, width: 1, height: 1)]
        check(scanAreas(plain) == [CGRect(x: 0, y: 0.1, width: 0.3, height: 0.9)], "scan areas are clipped and nonfinite regions rejected")
        clearAreasAction(nil); check(areas.isEmpty && Store.lastAreas.isEmpty, "Clear Areas clears persisted regions")
        busyText = "audit"; updateStatus()
        check(!canScan && !toolbarImpl.share.isEnabled && !toolbarImpl.phoneButton.isEnabled && !toolbarImpl.scanners.isEnabled, "busy operations disable overlapping custom actions")
        check(!windowShouldClose(window), "Close preserves pending background work")
        check(AppDelegate.shared?.applicationShouldTerminate(NSApp) == .terminateCancel, "Quit cannot discard pending work")
        busyText = nil; updateStatus()

        do {
            let tiff = folder.appendingPathComponent("two-pages.tiff")
            guard let destination = CGImageDestinationCreateWithURL(tiff as CFURL, UTType.tiff.identifier as CFString, 2, nil) else { throw CocoaError(.fileWriteUnknown) }
            CGImageDestinationAddImage(destination, small, [kCGImagePropertyDPIWidth: 100, kCGImagePropertyDPIHeight: 100] as CFDictionary)
            CGImageDestinationAddImage(destination, tall, [kCGImagePropertyDPIWidth: 200, kCGImagePropertyDPIHeight: 200] as CFDictionary)
            guard CGImageDestinationFinalize(destination) else { throw CocoaError(.fileWriteUnknown) }
            let imported = try ContinuityImport.loadChecked([tiff])
            check(imported.count == 2 && imported[0].0.image.width == 320 && imported[1].0.image.height == 480, "TIFF import retains every page in order")
            check(imported.map { $0.0.dpi } == [100, 200], "TIFF import retains per-page physical resolution")
            let broken = folder.appendingPathComponent("broken.png"); try Data("broken".utf8).write(to: broken)
            do { _ = try ContinuityImport.loadChecked([tiff, broken]); check(false, "mixed import must report an unreadable input") }
            catch { check(true, "mixed import reports an unreadable input before adding partial pages") }
            let pdf = PDFDocument(); let page = PDFPage(image: NSImage(cgImage: small, size: CGSize(width: 320, height: 240)))!
            page.rotation = 90; pdf.insert(page, at: 0)
            let rendered = try ContinuityImport.rasterizeChecked(pdf)
            let physical = CGSize(width: Double(rendered[0].image.width) / rendered[0].dpi * 72, height: Double(rendered[0].image.height) / rendered[0].dpi * 72)
            check(rendered.count == 1 && abs(physical.width - 240) < 1 && abs(physical.height - 320) < 1, "rotated PDF import preserves orientation and physical page size")
            let before = pages.count
            importFiles([tiff], at: 0)
            check(await UITest.wait({ self.pendingProcessing == 0 }, seconds: 15) && pages.count == before + 2, "native import processes every TIFF page")
            check(pages[0].image.width == 320 && pages[1].image.height == 480, "drop-before-first insertion retains source page order")
        } catch { check(false, "import fixture: \(error)") }
        let oldWindows = AppDelegate.shared?.windows.count ?? 0
        AppDelegate.shared?.newScanAction(nil)
        check((AppDelegate.shared?.windows.count ?? 0) == oldWindows + 1, "New Scan always opens a new empty document")
        if let new = AppDelegate.shared?.windows.last, new !== self { new.window?.close() }
        check((AppDelegate.shared?.windows.count ?? 0) == oldWindows, "closed scan windows are removed from app lifecycle state")
        print("SCN42 core regressions: \(count - failures)/\(count) passed")
        return failures == 0
    }
}
