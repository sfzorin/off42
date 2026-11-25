import AppKit
import Kit42
import PDFKit
import UniformTypeIdentifiers

extension ImageWindowController {
    func auditFiles() async -> Bool {
        guard let window else { print("FAIL IMG42 files: no window"); return false }
        var count = 0, failures = 0
        func check(_ condition: Bool, _ name: String) { count += 1; if !condition { failures += 1 }; print(condition ? "PASS" : "FAIL", "IMG files UI:", name) }
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("IMG42-file-ui-" + UUID().uuidString)
        let savedURLs = urls, savedURL = url
        let oldGroups = undo.groupsByEvent; undo.groupsByEvent = false
        func pump(_ seconds: Double = 0.06) {
            let deadline = Date(timeIntervalSinceNow: seconds)
            while Date() < deadline {
                if let event = NSApp.nextEvent(matching: .any, until: deadline, inMode: .default, dequeue: true) { NSApp.sendEvent(event) }
            }
        }
        func views(_ view: NSView) -> [NSView] { [view] + view.subviews.flatMap(views) }
        func event(_ body: () throws -> Void) rethrows { undo.beginUndoGrouping(); defer { undo.endUndoGrouping() }; try body() }
        defer {
            canvas.markup.cancelTextEditing()
            undo.removeAllActions(); undo.groupsByEvent = oldGroups
            if let savedURL { setURLsKeepingCurrent(savedURLs, current: savedURL); adoptSavedURL(savedURL, loaded: ImageLoader.load(savedURL)) }
            try? FileManager.default.removeItem(at: folder)
        }
        do {
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            let first = folder.appendingPathComponent("source.png"), other = folder.appendingPathComponent("existing.png")
            guard let white = ImageLoader.render(size: CGSize(width: 320, height: 240), { ctx in ctx.setFillColor(CGColor(gray: 1, alpha: 1)); ctx.fill(CGRect(x: 0, y: 0, width: 320, height: 240)) }) else { throw CocoaError(.fileWriteUnknown) }
            try ImageWriter.write(white, to: first, type: "public.png", description: .some("Saved caption"), dpi: 144)
            try ImageWriter.write(white, to: other, type: "public.png")
            func reset() {
                canvas.markup.cancelTextEditing(); adoptSavedURL(first, loaded: ImageLoader.load(first))
                undo.removeAllActions(); markupBar.show(true); canvas.zoom(to: 1)
                NSApp.activate(ignoringOtherApps: true); window.makeKeyAndOrderFront(nil); pump()
            }
            func beginText(_ text: String) -> Bool {
                undo.beginUndoGrouping(); defer { undo.endUndoGrouping() }
                markupBar.setTool(.text); window.layoutIfNeeded()
                let point = canvas.markup.convert(CGPoint(x: 30, y: 85), to: nil)
                let down = NSEvent.mouseEvent(with: .leftMouseDown, location: point, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
                    windowNumber: window.windowNumber, context: nil, eventNumber: 1, clickCount: 1, pressure: 1)!
                canvas.markup.mouseDown(with: down)
                guard let field = canvas.markup.subviews.compactMap({ $0 as? NSTextField }).first,
                      let editor = field.currentEditor() as? NSTextView else { return false }
                editor.selectAll(nil); editor.insertText(text, replacementRange: editor.selectedRange())
                return canvas.markup.isEditingText
            }
            func hasInk(_ image: CGImage?) -> Bool {
                guard let image else { return false }
                guard let context = CGContext(data: nil, width: image.width, height: image.height, bitsPerComponent: 8,
                                              bytesPerRow: image.width * 4, space: CGColorSpaceCreateDeviceRGB(),
                                              bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue), let data = context.data else { return false }
                context.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
                let bytes = data.bindMemory(to: UInt8.self, capacity: image.width * image.height * 4)
                return stride(from: 8, to: image.height - 8, by: 2).contains { y in
                    stride(from: 8, to: image.width - 8, by: 2).contains { x in
                        let i = (y * image.width + x) * 4
                        return bytes[i] < 180 || bytes[i + 1] < 180 || bytes[i + 2] < 180
                    }
                }
            }
            reset()
            check(!isDirty && imageDPI == 144 && imageDescription == "Saved caption", "opening fixture establishes pixels/DPI/description baseline")
            check(beginText("VISIBLE SAVE"), "native text editor contains a pending Save draft")
            check(isDirty && canvas.markup.marks.isEmpty, "uncommitted text participates in dirty state")
            var saved = false
            event { save(confirmed: true) { saved = $0 } }
            check(saved && !isDirty && !canvas.markup.isEditingText, "Save commits the draft and establishes a clean baseline")
            check(hasInk(ImageLoader.load(first).image), "saved/reopened image contains the visible text pixels")

            try ImageWriter.write(white, to: first, type: "public.png", description: .some("Saved caption"), dpi: 144)
            reset(); let originalBytes = try Data(contentsOf: first)
            event { check(beginText("EXPORT DRAFT"), "native export draft opens") }
            let copy = folder.appendingPathComponent("export.png")
            try event { try writeExport(to: copy, type: "public.png", saveAs: false) }
            check(url == first && isDirty && canvas.markup.marks.contains { $0.text == "EXPORT DRAFT" }, "Export copy retains the source URL and editable marks")
            check((try Data(contentsOf: first)) == originalBytes && hasInk(ImageLoader.load(copy).image), "Export writes the visible draft without replacing the original")
            let elsewhere = folder.appendingPathComponent("elsewhere"); try FileManager.default.createDirectory(at: elsewhere, withIntermediateDirectories: true)
            let saveAs = elsewhere.appendingPathComponent("saved.png")
            imageDescription = "New baseline"; imageDPI = 300
            try writeExport(to: saveAs, type: "public.png", saveAs: true)
            check(url == saveAs && window.representedURL == saveAs && !isDirty, "Save As in another folder switches the current file and clean baseline")
            check(imageDPI == 300 && imageDescription == "New baseline" && hasInk(base), "Save As reloads exported pixels and metadata")
            check((try Data(contentsOf: first)) == originalBytes, "Save As leaves original source bytes unchanged")
            setURLsKeepingCurrent(urls + [other], current: saveAs)
            adjustments[.brightness] = 15
            try writeExport(to: other, type: "public.png", saveAs: true)
            check(url == other && urls.filter { $0 == other }.count == 1 && !isDirty, "Save As over an existing set member adopts it without duplicate entries")

            reset(); check(beginText("CANCELLED SAVE"), "pending draft opens before Cancel")
            var cancelled: Bool?
            event { save(confirmed: false) { cancelled = $0 } }; pump()
            if let sheet = window.attachedSheet, let content = sheet.contentView,
               let button = views(content).compactMap({ $0 as? NSButton }).first(where: { $0.title == L("Cancel") }) {
                button.performClick(nil); pump()
                check(cancelled == false && isDirty, "native Save confirmation Cancel keeps the editing session")
                check((try Data(contentsOf: first)) == originalBytes, "cancelled Save does not modify the file")
            } else { check(false, "Save confirmation exposes a native Cancel button") }

            reset(); check(beginText("FAILED REVERT DRAFT"), "draft exists before failed Revert")
            try Data("broken image".utf8).write(to: first)
            do { try revertSavedImage(); check(false, "invalid Revert must fail") }
            catch { check(canvas.markup.isEditingText && isDirty && base != nil, "failed Revert preserves the native draft and visible model") }
            try originalBytes.write(to: first)
            try event { try revertSavedImage() }
            check(!isDirty && !canvas.markup.isEditingText && canvas.markup.marks.isEmpty, "successful Revert discards visible pending text")
            undo.undo()
            check(canvas.markup.marks.contains { $0.text == "FAILED REVERT DRAFT" } && isDirty, "Revert Undo restores the committed text draft")
            undo.redo(); check(canvas.markup.marks.isEmpty && !isDirty, "Revert Redo returns to the saved image")

            reset(); imageDescription = "Retained edit"; imageDPI = 200
            let sentinel = Data("KEEP OUTPUT".utf8), failed = folder.appendingPathComponent("failure.png"); try sentinel.write(to: failed)
            do { try writeExport(to: failed, type: "com.invalid.codec", saveAs: true); check(false, "unsupported Save As must fail") }
            catch { check(url == first && isDirty && imageDescription == "Retained edit" && imageDPI == 200 && (try? Data(contentsOf: failed)) == sentinel,
                          "failed Save As preserves URL, edits, DPI and existing destination") }

            reset(); check(beginText("SHARE VISIBLE TEXT"), "native draft opens before Share")
            var items: [Any] = []; event { items = shareItems() }
            check(!canvas.markup.isEditingText && items.first is NSImage && isDirty, "Share commits the current text and shares edited pixels")
            if let image = items.first as? NSImage { var rect = CGRect(origin: .zero, size: image.size); check(hasInk(image.cgImage(forProposedRect: &rect, context: nil, hints: nil)), "Share image contains the current text pixels") }
            else { check(false, "Share returns an image") }

            reset(); check(beginText("COPY VISIBLE TEXT"), "native draft opens before Copy")
            let pasteboard = NSPasteboard.withUniqueName()
            event { copyImage(to: pasteboard) }
            let copied = NSImage(pasteboard: pasteboard); var copiedRect = CGRect(origin: .zero, size: copied?.size ?? .zero)
            check(!canvas.markup.isEditingText && hasInk(copied?.cgImage(forProposedRect: &copiedRect, context: nil, hints: nil)), "Copy includes pending text using an isolated native pasteboard")
            pasteboard.releaseGlobally()

            reset(); check(beginText("PRINT VISIBLE TEXT"), "native draft opens before Print")
            let printURL = folder.appendingPathComponent("printed.pdf")
            var printOperation: NSPrintOperation?
            event { printOperation = imagePrintOperation() }
            if let operation = printOperation {
                operation.printInfo.jobDisposition = .save
                operation.printInfo.dictionary()[NSPrintInfo.AttributeKey.jobSavingURL] = printURL
                operation.showsPrintPanel = false; operation.showsProgressPanel = false
                let printed = operation.run(), document = PDFDocument(url: printURL)
                check(printed && document?.pageCount == 1 && !canvas.markup.isEditingText, "native Print writes a PDF and commits the visible draft")
                if let page = document?.page(at: 0) {
                    let thumbnail = page.thumbnail(of: CGSize(width: 320, height: 400), for: .mediaBox)
                    var rect = CGRect(origin: .zero, size: thumbnail.size)
                    let pixels = thumbnail.cgImage(forProposedRect: &rect, context: nil, hints: nil)
                    check(hasInk(pixels), "printed PDF contains the edited image rather than a blank original")
                } else { check(false, "printed PDF page opens") }
            } else { check(false, "native Print operation exists") }

            reset()
            let panel = NSSavePanel()
            panel.nameFieldStringValue = "accessory.png"
            let options = ExportOptions(sourceType: .png, hasAlpha: false, panel: panel)
            panel.accessoryView = options.view
            if let grid = views(options.view).compactMap({ $0 as? NSGridView }).first,
               let format = grid.cell(atColumnIndex: 1, rowIndex: 0).contentView as? NSPopUpButton,
               let size = grid.cell(atColumnIndex: 1, rowIndex: 2).contentView as? NSPopUpButton,
               let quality = views(options.view).compactMap({ $0 as? NSSlider }).first {
                for type in [UTType.png, .jpeg, .tiff] {
                    guard let index = options.types.firstIndex(of: type) else { check(false, "Save As offers " + type.identifier); continue }
                    format.selectItem(at: index)
                    if let action = format.action { check(NSApp.sendAction(action, to: format.target, from: format), "native Save As format action: " + type.identifier) }
                    check(options.type == type && panel.allowedContentTypes == [type]
                        && panel.nameFieldStringValue == "accessory." + (type.preferredFilenameExtension ?? ""),
                          "Save As format updates encoding, extension and allowed file type: " + type.identifier)
                    check(quality.isHidden == (type != .jpeg), "Save As exposes quality only for a lossy format: " + type.identifier)
                }
                format.selectItem(at: options.types.firstIndex(of: .jpeg)!)
                if let action = format.action { _ = NSApp.sendAction(action, to: format.target, from: format) }
                quality.doubleValue = 0.63
                if let action = quality.action { check(NSApp.sendAction(action, to: quality.target, from: quality), "native Save As quality action") }
                check(abs(options.quality - 0.63) < 0.001
                    && views(options.view).compactMap({ $0 as? NSTextField }).contains { $0.stringValue == "63 %" },
                      "Save As quality slider updates the encoded value and visible percentage")
                let expectedSizes = [CGSize(width: 320, height: 240), CGSize(width: 240, height: 180),
                                     CGSize(width: 160, height: 120), CGSize(width: 80, height: 60)]
                for index in expectedSizes.indices {
                    size.selectItem(at: index)
                    let resized = options.resized(white)
                    check(resized?.width == Int(expectedSizes[index].width) && resized?.height == Int(expectedSizes[index].height),
                          "Save As size preset changes the rendered pixels: " + size.titleOfSelectedItem!)
                }
                if let wide = ImageLoader.render(size: CGSize(width: 5000, height: 10), { $0.setFillColor(CGColor(gray: 1, alpha: 1)); $0.fill(CGRect(x: 0, y: 0, width: 5000, height: 10)) }) {
                    for (index, expected) in [(4, CGSize(width: 3840, height: 8)), (5, CGSize(width: 2048, height: 4)), (6, CGSize(width: 1024, height: 2))] {
                        size.selectItem(at: index)
                        let resized = options.resized(wide)
                        check(resized?.width == Int(expected.width) && resized?.height == Int(expected.height), "Save As fit preset bounds real output: " + size.titleOfSelectedItem!)
                        check(options.resized(white) === white, "Save As fit preset never upscales a smaller original")
                    }
                } else { check(false, "Save As fit-size fixture") }
                size.selectItem(at: 2)
                let accessoryOutput = folder.appendingPathComponent("accessory-output.jpg")
                try writeExport(to: accessoryOutput, type: options.type.identifier, quality: options.quality, saveAs: true, resize: options.resized)
                let exported = ImageLoader.load(accessoryOutput)
                check(url == accessoryOutput && !isDirty && exported.typeIdentifier == UTType.jpeg.identifier
                    && exported.image?.width == 160 && exported.image?.height == 120,
                      "Save As uses the native accessory's selected JPEG format and size in the adopted file")
            } else { check(false, "Save As native format, quality and size controls exist") }

            // Only this UUID-named fixture enters the user's Trash. The final Undo restores it, and the
            // cleanup below touches that exact filename/byte sequence only if a failed assertion leaves it there.
            reset()
            let trashFixture = folder.appendingPathComponent("IMG42-audit-trash-" + UUID().uuidString + ".png")
            try ImageWriter.write(white, to: trashFixture, type: "public.png", description: .some("Disposable trash fixture"))
            let fixtureBytes = try Data(contentsOf: trashFixture), neighbourBytes = try Data(contentsOf: other)
            let trashDirectory = try FileManager.default.url(for: .trashDirectory, in: .userDomainMask, appropriateFor: trashFixture, create: true)
            let trashedFixture = trashDirectory.appendingPathComponent(trashFixture.lastPathComponent)
            guard !FileManager.default.fileExists(atPath: trashedFixture.path) else { throw CocoaError(.fileWriteFileExists) }
            defer {
                if (try? Data(contentsOf: trashedFixture)) == fixtureBytes { try? FileManager.default.removeItem(at: trashedFixture) }
            }
            setURLsKeepingCurrent([trashFixture, other], current: trashFixture)
            adoptSavedURL(trashFixture, loaded: ImageLoader.load(trashFixture)); undo.removeAllActions()
            let fixtureSet = Set(urls)
            func trashState(_ label: String) async {
                check(await UITest.wait { self.loaded?.url == other && self.url == other && self.base != nil }, label + " loads the surviving neighbour")
                check(!FileManager.default.fileExists(atPath: trashFixture.path) && Set(urls) == [other]
                    && (try? Data(contentsOf: other)) == neighbourBytes && (try? Data(contentsOf: trashedFixture)) == fixtureBytes,
                      label + " removes only the captured file and preserves neighbour/Trash bytes")
            }
            func restoredState(_ label: String) async {
                check(await UITest.wait { self.loaded?.url == trashFixture && self.url == trashFixture && self.base != nil }, label + " restores the original current URL after loading")
                check(Set(urls) == fixtureSet && urls.count == 2 && (try? Data(contentsOf: trashFixture)) == fixtureBytes
                    && (try? Data(contentsOf: other)) == neighbourBytes && !FileManager.default.fileExists(atPath: trashedFixture.path),
                      label + " restores exact bytes and the original image set without duplicates")
            }
            event { check(NSApp.sendAction(#selector(trashAction(_:)), to: self, from: nil), "native Move to Trash action dispatches") }
            await trashState("Trash")
            check(undo.canUndo, "Trash registers file Undo")
            undo.undo(); await restoredState("Undo Trash")
            check(undo.canRedo, "Undo Trash registers Redo")
            undo.redo(); await trashState("Redo Trash")
            undo.undo(); await restoredState("second Undo Trash")
            go(to: urls.firstIndex(of: other)!)
            check(await UITest.wait { self.loaded?.url == other && self.url == other }, "navigation after Undo opens the neighbour")
            let neighbourBase = base
            undo.redo()
            await trashState("Redo Trash after navigation")
            check(base === neighbourBase && loaded?.url == other, "Redo of another file keeps the displayed neighbour model")
            undo.undo(); await restoredState("Undo after navigation")

            // Visit B before A so B is definitely retained in the neighbouring-image cache.
            let cacheA = folder.appendingPathComponent("cache-source.png"), cacheB = folder.appendingPathComponent("cache-destination.png")
            try ImageWriter.write(ImageFileSelfTest.fixture(), to: cacheA, type: UTType.png.identifier)
            try ImageWriter.write(white, to: cacheB, type: UTType.png.identifier)
            let sourceBytes = try Data(contentsOf: cacheA)
            setSet([cacheA, cacheB], current: cacheB); undo.removeAllActions()
            check(await UITest.wait { self.loaded?.url == cacheB && self.base != nil }, "existing Export destination is loaded into the neighbour cache")
            let cachedDestination = base
            go(to: urls.firstIndex(of: cacheA)!)
            check(await UITest.wait { self.loaded?.url == cacheA && self.base?.width == 96 }, "Export source loads while the old destination stays cached")
            event { var value = Adjustments(); value[.brightness] = 10; applyAdjustments(value, name: "Audit export edit") }
            let sourceBase = base, sourceAdjustments = adjustments
            try writeExport(to: cacheB, type: UTType.png.identifier, saveAs: false)
            check(url == cacheA && base === sourceBase && adjustments == sourceAdjustments && isDirty
                && (try? Data(contentsOf: cacheA)) == sourceBytes,
                  "Export over an existing member preserves the source pixels, editable adjustments and file bytes")
            let writtenDestination = ImageLoader.load(cacheB).image
            check(writtenDestination?.width == 96 && writtenDestination?.height == 64
                && writtenDestination.map(ImagePixelAudit.pixels) != cachedDestination.map(ImagePixelAudit.pixels),
                  "Export replaces the existing member with the new dimensions and rendered pixels on disk")
            go(to: urls.firstIndex(of: cacheB)!)
            if await UITest.wait({ window.attachedSheet != nil }), let sheet = window.attachedSheet, let content = sheet.contentView,
               let discard = views(content).compactMap({ $0 as? NSButton }).first(where: { $0.title == L("Don't Save") }) {
                check(true, "leaving the exported source still asks about its unsaved edits")
                discard.performClick(nil)
            } else { check(false, "leaving the exported source still asks about its unsaved edits") }
            check(await UITest.wait { self.loaded?.url == cacheB && self.base?.width == 96 && self.base?.height == 64 },
                  "navigation after Export reloads the replaced neighbour dimensions")
            check(base !== cachedDestination && base.map(ImagePixelAudit.pixels) == writtenDestination.map(ImagePixelAudit.pixels),
                  "navigation after Export displays the newly saved pixels instead of the stale cached image")
            check((try? Data(contentsOf: cacheA)) == sourceBytes, "discarding the source edit leaves its original file untouched")

            let orientationA = folder.appendingPathComponent("orientation-a.jpg"), orientationB = folder.appendingPathComponent("orientation-b.jpg")
            for operation in [Orientation.Op.rotateRight, .flipHorizontal] {
                try ImageWriter.write(ImageFileSelfTest.fixture(), to: orientationA, type: UTType.jpeg.identifier, quality: 1)
                try ImageWriter.write(ImagePixelAudit.fixture(width: 135, height: 91), to: orientationB, type: UTType.jpeg.identifier, quality: 1)
                let neighbourBytes = try Data(contentsOf: orientationB)
                setSet([orientationA, orientationB], current: orientationA); undo.removeAllActions()
                check(await UITest.wait { self.loaded?.url == orientationA && self.base?.width == 96 && self.base?.height == 64 },
                      "lossless transform fixture opens clean JPEG A: \(operation)")
                guard let original = base else { throw CocoaError(.fileReadCorruptFile) }
                let originalPixels = ImagePixelAudit.pixels(original)
                let expected = Orientation.apply(operation.orientation, to: original)
                let expectedPixels = ImagePixelAudit.pixels(expected)
                let action = operation == .rotateRight ? #selector(rotateRightAction(_:)) : #selector(flipHorizontalAction(_:))
                event { check(NSApp.sendAction(action, to: self, from: nil), "native lossless transform action: \(operation)") }
                let transformed = ImageLoader.load(orientationA)
                check(!isDirty && transformed.orientation == operation.orientation
                    && transformed.image?.width == expected.width && transformed.image?.height == expected.height
                    && transformed.image.map(ImagePixelAudit.pixels) == expectedPixels,
                      "lossless transform writes JPEG A's orientation and displayed pixels: \(operation)")
                go(to: urls.firstIndex(of: orientationB)!)
                check(await UITest.wait { self.loaded?.url == orientationB && self.base?.width == 135 && self.base?.height == 91 },
                      "navigation opens distinct JPEG B before file Undo: \(operation)")
                let unchangedBase = base
                func neighbourUnchanged(_ name: String) {
                    let disk = ImageLoader.load(orientationB)
                    check(url == orientationB && loaded?.url == orientationB && base === unchangedBase && !isDirty
                        && disk.orientation == .up && disk.image?.width == 135 && disk.image?.height == 91
                        && (try? Data(contentsOf: orientationB)) == neighbourBytes,
                          name + " keeps JPEG B's URL, displayed model and exact file bytes: \(operation)")
                }
                undo.undo()
                let undone = ImageLoader.load(orientationA)
                check(undone.orientation == .up && undone.image?.width == 96 && undone.image?.height == 64
                    && undone.image.map(ImagePixelAudit.pixels) == originalPixels,
                      "file Undo targets JPEG A after navigation: \(operation)")
                neighbourUnchanged("Undo")
                check(undo.canRedo, "file Undo preserves Redo after navigation: \(operation)")
                undo.redo()
                let redone = ImageLoader.load(orientationA)
                check(redone.orientation == operation.orientation && redone.image?.width == expected.width && redone.image?.height == expected.height
                    && redone.image.map(ImagePixelAudit.pixels) == expectedPixels,
                      "file Redo targets JPEG A after navigation: \(operation)")
                neighbourUnchanged("Redo")
                go(to: urls.firstIndex(of: orientationA)!)
                check(await UITest.wait { self.loaded?.url == orientationA && self.loaded?.orientation == operation.orientation },
                      "returning to JPEG A reloads its redone orientation: \(operation)")
                check(base?.width == expected.width && base?.height == expected.height
                    && base.map(ImagePixelAudit.pixels) == expectedPixels
                    && (try? Data(contentsOf: orientationB)) == neighbourBytes,
                      "returning to JPEG A shows its transformed pixels while JPEG B stays untouched: \(operation)")
            }
        } catch { check(false, "fixture error: " + error.localizedDescription) }
        print("IMG42 file UI regressions: \(count - failures)/\(count) passed")
        return failures == 0
    }
}
