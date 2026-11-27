import AppKit
import Kit42
import PDFKit

/// Focused format/output regressions, called by --selftest and the native lifecycle audit.
enum PPTFilesAudit {
    static func fixture() -> Deck {
        var deck = Deck.makeDefault()
        deck.slides = []
        for (index, text) in ["VISIBLE-FIRST", "HIDDEN-MIDDLE", "VISIBLE-LAST"].enumerated() {
            var slide = Slide(layout: deck.layoutIndex(type: "blank") ?? 0)
            var object = SlideObject(id: 2, kind: .textBox, frame: CGRect(x: 40, y: 40, width: 500, height: 100))
            object.text = TextBody(paragraphs: [Paragraph(text, run: RunProps(size: 28, color: PColor(rgb: 0x123456)))])
            object.text?.paragraphs[0].runs[0].props.bold = true
            object.line = LineStyle(color: PColor(rgb: 0x2244AA), width: 3, dash: .dash)
            slide.objects = [object]
            slide.hidden = index == 1
            slide.notes = "  Notes \(index + 1)\twith CR\rand LF\nlast line  "
            deck.slides.append(slide)
        }
        var table = TableModel(rows: 2, columns: 2, width: 300)
        table.firstRow = false; table.bandRows = false; table.borderColor = PColor(rgb: 0xCC3311); table.borderWidth = 2.5
        table.cells[0][0].text.paragraphs = [Paragraph("A")]
        table.cells[0][1].text.paragraphs = [Paragraph("B")]
        table.cells[1][0].text.paragraphs = [Paragraph("C")]
        table.cells[1][1].text.paragraphs = [Paragraph("D")]
        var object = SlideObject(id: 3, kind: .table, frame: CGRect(x: 50, y: 200, width: 300, height: 56))
        object.table = table
        deck.slides[0].objects.append(object)
        return deck
    }

    static func run() -> Int {
        var checks = 0, failures = 0
        func check(_ name: String, _ ok: Bool) {
            checks += 1; if !ok { failures += 1 }
            print(ok ? "PASS" : "FAIL", "PPT files:", name)
        }
        let deck = fixture()
        for ext in ["pptx", "odp", "potx", "otp"] {
            do {
                let bytes = try DeckIO.write(deck, ext: ext)
                let result = try DeckIO.read(bytes, ext: ext)
                check(ext + " slide order and hidden state", result.slides.count == 3 && result.slides[1].hidden && !result.slides[0].hidden && result.slides[2].objects.first?.text?.plainText == "VISIBLE-LAST")
                check(ext + " notes preserve whitespace, CR, LF and tabs", result.slides.first?.notes == deck.slides[0].notes)
                let text = result.slides[0].objects.first?.text?.paragraphs.first?.runs.first
                check(ext + " rich text survives", text?.text == "VISIBLE-FIRST" && text?.props.bold == true && text?.props.size == 28)
                let line = result.slides[0].objects.first?.line
                check(ext + " shape border colour width and dash survive", line?.color?.resolved(result.theme) == PColor(rgb: 0x2244AA).resolved(result.theme) && abs((line?.width ?? 0) - 3) < 0.02 && line?.dash == .dash)
                let table = result.slides[0].objects.first(where: { $0.kind == .table })?.table
                check(ext + " table contents and borders survive", table?.cells[1][1].text.plainText == "D" && table?.borderColor?.resolved(result.theme) == PColor(rgb: 0xCC3311).resolved(result.theme) && abs((table?.borderWidth ?? 0) - 2.5) < 0.02)
                check(ext + " slide dimensions survive", abs(result.size.width - deck.size.width) < 0.02 && abs(result.size.height - deck.size.height) < 0.02)
            } catch { check(ext + " round trip: " + error.localizedDescription, false) }
        }
        for ending in ["\r", "\n", "\r\n", "\u{2028}", "\u{0B}"] {
            var special = deck
            let expected = "  before\t" + ending + "after  "
            special.slides[0].objects[0].text?.paragraphs = [Paragraph(expected)]
            do {
                let result = try DeckIO.read(DeckIO.write(special, ext: "odp"), ext: "odp")
                check("ODP preserves authored inline whitespace \(ending.debugDescription)", result.slides[0].objects[0].text?.plainText == expected)
            } catch { check("ODP inline whitespace fixture", false) }
        }
        func rejected(_ name: String, _ data: Data, _ ext: String) {
            do { _ = try DeckIO.read(data, ext: ext); check(name, false) }
            catch { check(name, true) }
        }
        do {
            let original = try DeckIO.write(deck, ext: "pptx")
            var zip = try ZipArchive(data: original)
            zip["ppt/slides/slide2.xml"] = nil
            rejected("missing referenced slide is an error, not silent slide loss", zip.data(), "pptx")
            zip = try ZipArchive(data: original)
            zip["ppt/_rels/presentation.xml.rels"] = Data((zip.string("ppt/_rels/presentation.xml.rels") ?? "").replacingOccurrences(of: "../", with: "../").replacingOccurrences(of: "slides/slide2.xml", with: "missing.xml").utf8)
            rejected("broken slide relationship is rejected", zip.data(), "pptx")
            zip = try ZipArchive(data: original)
            zip["ppt/slides/slide1.xml"] = Data((zip.string("ppt/slides/slide1.xml") ?? "").replacingOccurrences(of: "id=\"2\"", with: "id=\"9223372036854775807\"").utf8)
            rejected("overflowing object identifier is rejected before normalization", zip.data(), "pptx")
            zip = try ZipArchive(data: original)
            zip["ppt/presentation.xml"] = Data("<unrelated/>".utf8)
            rejected("unrelated XML cannot become an empty PPTX", zip.data(), "pptx")
        } catch { check("malformed PPTX fixtures", false) }
        func odf(_ body: String, presentation: Bool = true) -> Data {
            var zip = ZipArchive()
            zip["content.xml"] = Data(("<office:document-content xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\" xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" xmlns:svg=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\"><office:body><office:\(presentation ? "presentation" : "spreadsheet")>" + body + "</office:\(presentation ? "presentation" : "spreadsheet")></office:body></office:document-content>").utf8)
            zip["mimetype"] = Data("application/vnd.oasis.opendocument.presentation".utf8)
            return zip.data()
        }
        let repeated = "<draw:page draw:name=\"Repeated\"><draw:frame svg:x=\"1cm\" svg:y=\"1cm\" svg:width=\"10cm\" svg:height=\"3cm\"><table:table><table:table-column table:number-columns-repeated=\"3\"/><table:table-row table:number-rows-repeated=\"2\"><table:table-cell table:number-columns-repeated=\"3\"><text:p>repeat</text:p></table:table-cell></table:table-row></table:table></draw:frame></draw:page>"
        do {
            let data = odf(repeated), result = try DeckIO.read(data, ext: "unknown")
            let table = result.slides.first?.objects.first(where: { $0.kind == .table })?.table
            check("ODP compressed/non-first mimetype is detected", DeckIO.isODF(data))
            check("ODP table expands repeated rows and cells without loss", table?.rows.count == 2 && table?.columns.count == 3 && table?.cells.flatMap { $0 }.allSatisfy { $0.text.plainText == "repeat" } == true)
        } catch { check("ODP repeats: " + error.localizedDescription, false) }
        rejected("wrong ODF document type is rejected", odf("", presentation: false), "odp")
        rejected("negative repeated text is rejected", odf("<draw:page><draw:frame><draw:text-box><text:p><text:s text:c=\"-1\"/></text:p></draw:text-box></draw:frame></draw:page>"), "odp")
        rejected("unbounded repeated table is rejected", odf(repeated.replacingOccurrences(of: "number-columns-repeated=\"3\"", with: "number-columns-repeated=\"9223372036854775807\"")), "odp")
        rejected("negative table repetition is rejected", odf(repeated.replacingOccurrences(of: "number-rows-repeated=\"2\"", with: "number-rows-repeated=\"-2\"")), "odp")
        for width in [-1, 0, 100_000, Int.max] { check("invalid bitmap width \(width) is rejected", Renderer.image(deck.slides[0], deck: deck, pixelWidth: width) == nil) }
        check("standard 1920px output geometry", Renderer.bitmapSize(deck: deck, pixelWidth: 1920)?.height == 1080)
        var invalidDeck = deck; invalidDeck.size.width = .infinity
        check("nonfinite slide dimensions are rejected", Renderer.image(invalidDeck.slides[0], deck: invalidDeck, pixelWidth: 1920) == nil)
        let folder = URL(fileURLWithPath: NSTemporaryDirectory()).appendingPathComponent("ppt-files-" + UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: folder) }
        do {
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            let destination = folder.appendingPathComponent("Slide 01.png"), original = Data("keep existing file".utf8)
            try original.write(to: destination)
            do { _ = try SlideImages.export(deck, to: folder, jpeg: false, width: -1); check("invalid export throws", false) } catch { check("invalid image export preserves existing file", (try? Data(contentsOf: destination)) == original) }
            let images = try SlideImages.export(deck, to: folder.appendingPathComponent("visible"), jpeg: false, width: 320)
            check("image export omits hidden slides and retains original numbering", images.map(\.lastPathComponent) == ["Slide 01.png", "Slide 03.png"])
            let all = try SlideImages.export(deck, to: folder.appendingPathComponent("all"), jpeg: false, width: 320, includeHidden: true)
            check("image export includes hidden slides when requested", all.count == 3)
            let movie = folder.appendingPathComponent("existing.mp4")
            try original.write(to: movie)
            for seconds in [Double.infinity, 1e300, -1] {
                var options = VideoExport.Options(); options.secondsPerSlide = seconds
                do { try VideoExport.export(deck, to: movie, options: options); check("invalid movie timing throws", false) }
                catch { check("invalid movie timing preserves the existing destination", (try? Data(contentsOf: movie)) == original) }
            }
        } catch { check("image export fixtures: " + error.localizedDescription, false) }
        var pdf = PDFDocument(data: Renderer.pdf(deck))
        check("PDF skips hidden slides and retains visible content", pdf?.pageCount == 2 && pdf?.string?.contains("VISIBLE-LAST") == true && pdf?.string?.contains("HIDDEN-MIDDLE") == false)
        pdf = PDFDocument(data: Renderer.pdf(deck, includeHidden: true))
        check("PDF includes hidden slides when requested", pdf?.pageCount == 3 && pdf?.string?.contains("HIDDEN-MIDDLE") == true)
        pdf = PDFDocument(data: Renderer.pdf(deck, handouts: 2))
        check("handout PDF packs visible slides", pdf?.pageCount == 1 && pdf?.string?.contains("VISIBLE-FIRST") == true && pdf?.string?.contains("VISIBLE-LAST") == true)
        var long = deck; long.slides = [deck.slides[0]]
        long.slides[0].notes = (0..<180).map { "Note line \($0) complete text" }.joined(separator: "\n") + "\nLAST-NOTE-TOKEN"
        let ranges = Renderer.notesPageRanges(long.slides[0], deck: long, page: CGSize(width: 595, height: 842))
        check("note pagination accounts for every UTF-16 character", ranges.first?.location == 0 && ranges.last.map(NSMaxRange) == (long.slides[0].notes as NSString).length && zip(ranges, ranges.dropFirst()).allSatisfy { NSMaxRange($0.0) == $0.1.location })
        pdf = PDFDocument(data: Renderer.pdf(long, notes: true))
        check("long notes PDF contains its first and last lines on continuation pages", (pdf?.pageCount ?? 0) > 1 && pdf?.string?.contains("Note line 0") == true && pdf?.string?.contains("LAST-NOTE-TOKEN") == true)
        long.size = CGSize(width: 300, height: 1200)
        pdf = PDFDocument(data: Renderer.pdf(long, notes: true))
        check("portrait slide cannot consume the notes text region", pdf?.string?.contains("LAST-NOTE-TOKEN") == true)
        print("PPT file/output regressions: \(checks - failures)/\(checks) passed")
        return failures
    }

    static func runUI(_ owner: DeckWindowController) -> Int {
        guard let doc = owner.doc else { print("FAIL PPT lifecycle: no document"); return 1 }
        var checks = 0, failures = 0
        func check(_ name: String, _ ok: Bool) { checks += 1; if !ok { failures += 1 }; print(ok ? "PASS" : "FAIL", "PPT lifecycle:", name) }
        owner.commitEditing(); doc.endSilentEdit()
        let original = doc.deck, undo = doc.undoManager, state = EditState(slide: owner.current, selection: owner.selection)
        let originalMode = owner.mode, originalNotesShown = owner.notesShown
        let originalURL = doc.fileURL, originalType = doc.fileType
        let manager = UndoManager(); manager.groupsByEvent = false; doc.undoManager = manager
        // The harness calls actions synchronously in one run-loop turn. Represent each user event explicitly;
        // production AppKit supplies these groups automatically, including focus-change/end-edit callbacks.
        func event(_ body: () -> Void) {
            manager.beginUndoGrouping()
            body()
            manager.endUndoGrouping()
        }
        defer {
            owner.canvas.editor.cancel(); doc.replaceDeck(original); doc.undoManager = undo
            doc.fileURL = originalURL; doc.fileType = originalType
            doc.onChange?(fixture()); doc.restoreState?(state)
            owner.setMode(originalMode); owner.setNotesShown(originalNotesShown)
        }
        do {
            let source = try DeckIO.write(fixture(), ext: "pptx")
            try doc.read(from: source, ofType: "org.openxmlformats.presentationml.presentation")
            check("unchanged document starts clean", !doc.hasUnsavedChanges)
            guard let object = owner.currentSlide?.objects.first else { check("lifecycle fixture", false); return failures }
            owner.canvas.editor.begin(object, at: nil, selectAll: true)
            check("opening text editor alone does not mark document dirty", !doc.hasUnsavedChanges)
            owner.canvas.editor.textView.insertText("PENDING-SAVED", replacementRange: owner.canvas.editor.textView.selectedRange())
            check("visible pending text marks document dirty", doc.hasUnsavedChanges)
            manager.beginUndoGrouping()
            let saved = try doc.data(ofType: "org.openxmlformats.presentationml.presentation")
            manager.endUndoGrouping()
            let reopened = try DeckIO.read(saved, ext: "pptx")
            check("Save serializes the visible editor draft", reopened.slides[0].objects.first?.text?.plainText == "PENDING-SAVED" && !owner.canvas.editor.isActive)
            manager.undo()
            check("Undo after Save restores prior slide text", doc.deck.slides[0].objects.first?.text?.plainText == "VISIBLE-FIRST")
            manager.redo()
            check("Redo after Save restores committed text", doc.deck.slides[0].objects.first?.text?.plainText == "PENDING-SAVED")
            event { owner.select(slide: 2) }
            owner.canvas.editor.begin(owner.currentSlide!.objects[0], at: nil, selectAll: true)
            owner.canvas.editor.textView.insertText("REVERT-DRAFT", replacementRange: owner.canvas.editor.textView.selectedRange())
            do { try doc.read(from: Data([0, 1, 2]), ofType: "org.openxmlformats.presentationml.presentation"); check("failed Revert throws", false) }
            catch { check("failed Revert preserves draft, selected slide and model", owner.canvas.editor.textView.string == "REVERT-DRAFT" && owner.canvas.editor.isActive && owner.current == 2 && doc.deck.slides.count == 3) }
            var replacement = fixture(); replacement.slides = [replacement.slides[0]]
            try doc.read(from: DeckIO.write(replacement, ext: "pptx"), ofType: "org.openxmlformats.presentationml.presentation")
            check("successful Revert discards draft and resets stale slide selection", !owner.canvas.editor.isActive && owner.current == 0 && doc.deck.slides.count == 1 && owner.selection.isEmpty)
            check("Revert refreshes the canvas and clears stale Undo", owner.displayedSlide.objects.first?.text?.plainText == "VISIBLE-FIRST" && !manager.canUndo && !doc.hasUnsavedChanges)
            manager.beginUndoGrouping(); doc.silentChange { $0.slides[0].notes = "DIRTY-NOTES" }; manager.endUndoGrouping()
            try doc.read(from: source, ofType: "org.openxmlformats.presentationml.presentation")
            doc.endSilentEdit()
            check("Revert discards pending notes undo transaction", !manager.canUndo && doc.deck.slides[0].notes == fixture().slides[0].notes && !doc.hasUnsavedChanges)

            func type(_ text: String, in view: NSTextView) {
                event {
                    owner.window?.makeKeyAndOrderFront(nil)
                    owner.window?.makeFirstResponder(view)
                    let local = view.undoManager
                    local?.beginUndoGrouping()
                    view.insertText(text, replacementRange: NSRange(location: 0, length: (view.string as NSString).length))
                    local?.endUndoGrouping()
                    view.breakUndoCoalescing()
                }
            }
            event { owner.setMode(.normal); owner.setNotesShown(true) }
            type("NATIVE-NOTES-DRAFT", in: owner.notesView)
            let notesUndo = owner.notesView.undoManager
            check("native notes typing updates the model and owns a local Undo", doc.deck.slides[0].notes == "NATIVE-NOTES-DRAFT" && notesUndo?.canUndo == true)
            do { try doc.read(from: Data([0, 1, 2]), ofType: "org.openxmlformats.presentationml.presentation"); check("failed notes Revert throws", false) }
            catch { check("failed Revert preserves native notes text and Undo", owner.notesView.string == "NATIVE-NOTES-DRAFT" && notesUndo?.canUndo == true) }
            try doc.read(from: source, ofType: "org.openxmlformats.presentationml.presentation")
            check("successful Revert clears native notes Undo history", notesUndo?.canUndo == false && owner.notesView.string == fixture().slides[0].notes)
            type("SLIDE-ONE-NOTES", in: owner.notesView)
            event { owner.select(slide: 1) }
            check("switching slides cannot apply the previous notes Undo to another slide", notesUndo?.canUndo == false && owner.notesView.string == fixture().slides[1].notes && doc.deck.slides[0].notes == "SLIDE-ONE-NOTES")

            event { owner.setMode(.notesPage) }
            if let pageText = owner.notesPage.subviews.compactMap({ $0 as? NSTextView }).first {
                type("NOTES-PAGE-DRAFT", in: pageText)
                let pageUndo = pageText.undoManager
                check("native notes-page typing records local Undo", pageUndo?.canUndo == true && doc.deck.slides[1].notes == "NOTES-PAGE-DRAFT")
                event { owner.select(slide: 2) }
                check("notes-page switch clears actions for the previous slide", pageUndo?.canUndo == false && pageText.string == fixture().slides[2].notes)
                type("NOTES-PAGE-REVERT-DRAFT", in: pageText)
                try doc.read(from: source, ofType: "org.openxmlformats.presentationml.presentation")
                let notesPageRestored = pageUndo?.canUndo == false && pageText.string == fixture().slides[0].notes
                if !notesPageRestored {
                    print("PPT lifecycle notes-page Revert state: slide=\(owner.current), canUndo=\(String(describing: pageUndo?.canUndo)), actual=\(String(reflecting: pageText.string)), expected=\(String(reflecting: fixture().slides[0].notes))")
                }
                check("Revert clears the notes-page local Undo stack", notesPageRestored)
            } else { check("native notes-page editor exists", false) }

            var titled = Deck.makeDefault()
            titled.setText(slide: 0, placeholder: "ctrTitle", [Paragraph("ORIGINAL-OUTLINE")])
            let titledData = try DeckIO.write(titled, ext: "pptx")
            try doc.read(from: titledData, ofType: "org.openxmlformats.presentationml.presentation")
            event { owner.setMode(.outline) }
            type("IMMEDIATE-OUTLINE-SAVE", in: owner.outline)
            let outlineUndo = owner.outline.undoManager
            check("unflushed outline draft marks the document dirty", owner.outline.hasPendingChanges && doc.hasUnsavedChanges && outlineUndo?.canUndo == true)
            manager.beginUndoGrouping()
            let outlineSaved = try doc.data(ofType: "org.openxmlformats.presentationml.presentation")
            manager.endUndoGrouping()
            check("Save flushes outline typing without waiting for its timer", try DeckIO.read(outlineSaved, ext: "pptx").slides[0].title == "IMMEDIATE-OUTLINE-SAVE")
            type("DISCARDED-OUTLINE-DRAFT", in: owner.outline)
            try doc.read(from: titledData, ofType: "org.openxmlformats.presentationml.presentation")
            check("Revert cancels delayed outline edits and its local Undo stack", !owner.outline.hasPendingChanges && outlineUndo?.canUndo == false && owner.outline.string == "ORIGINAL-OUTLINE" && doc.deck.slides[0].title == "ORIGINAL-OUTLINE")
            if let keyPath = ProcessInfo.processInfo.environment["K42_KEY_FIXTURE"] {
                let key = URL(fileURLWithPath: keyPath)
                let package = key.deletingLastPathComponent().appendingPathComponent("keynote-package.key", isDirectory: true)
                for input in [key, package] {
                    let before = try Data(contentsOf: input.hasDirectoryPath ? input.appendingPathComponent("Index/Document.iwa") : input)
                    var opened: PresentationDocument?, openError: Error?, openDone = false
                    NSDocumentController.shared.openDocument(withContentsOf: input, display: true) { document, _, error in
                        opened = document as? PresentationDocument; openError = error; openDone = true
                    }
                    let openDeadline = Date(timeIntervalSinceNow: 5)
                    while !openDone && Date() < openDeadline { RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.01)) }
                    guard let imported = opened, let window = imported.controller else { check("Keynote controller opens document: \(String(describing: openError))", false); continue }
                    check("Keynote controller creates untitled imported deck", imported.fileURL == nil && imported.deck.slides.count == 2 && imported.fileType == "org.openxmlformats.presentationml.presentation")
                    guard let text = window.currentSlide?.objects.first else { check("Keynote text object exists", false); imported.close(); continue }
                    window.canvas.editor.begin(text, at: nil, selectAll: true)
                    window.canvas.editor.textView.insertText("KEYNOTE-EDITED", replacementRange: window.canvas.editor.textView.selectedRange())
                    let output = key.deletingLastPathComponent().appendingPathComponent(input.hasDirectoryPath ? "package-save.pptx" : "key-save.pptx")
                    var done = false, saveError: Error?
                    imported.undoManager?.beginUndoGrouping()
                    imported.save(to: output, ofType: "org.openxmlformats.presentationml.presentation", for: .saveAsOperation) { error in saveError = error; done = true }
                    imported.undoManager?.endUndoGrouping()
                    let deadline = Date(timeIntervalSinceNow: 5)
                    while !done && Date() < deadline { RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.01)) }
                    let saved = try? DeckIO.read(Data(contentsOf: output), ext: "pptx")
                    let after = try Data(contentsOf: input.hasDirectoryPath ? input.appendingPathComponent("Index/Document.iwa") : input)
                    check("Keynote native edit and Save As reopens without changing source", done && saveError == nil && before == after && saved?.slides.first?.objects.first?.text?.plainText == "KEYNOTE-EDITED")
                    imported.close()
                }
            } else { check("Keynote lifecycle fixture supplied", false) }
        } catch { check("lifecycle fixture: " + error.localizedDescription, false) }
        print("PPT document lifecycle regressions: \(checks - failures)/\(checks) passed")
        return failures
    }
}
