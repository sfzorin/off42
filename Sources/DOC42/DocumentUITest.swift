import AppKit
import Kit42
import PDFKit

extension DocWindowController {
    /// Native document/model regressions, invoked by K42_SCRIPT=auditdocument.
    /// Fixtures have their own documents and Undo managers; the user's editor is untouched.
    func auditDocumentLifecycle() -> Bool {
        var failures = 0, checks = 0
        func check(_ ok: Bool, _ name: String) {
            checks += 1
            if !ok { failures += 1 }
            print("\(ok ? "PASS" : "FAIL") DOC document: \(name)")
        }
        func fixture(_ text: String, _ body: (TextDocument, DocWindowController, UndoManager) throws -> Void) rethrows {
            let document = TextDocument()
            document.storage.setAttributedString(NSAttributedString(string: text, attributes: Styles.normal.attributes))
            document.makeWindowControllers()
            let editor = document.editor!, undo = document.undoManager!
            undo.groupsByEvent = false
            editor.paged.layoutAll()
            defer {
                editor.headerEditor?.onCommit = nil
                editor.headerEditor?.commit()
                undo.removeAllActions()
                document.updateChangeCount(.changeCleared)
                document.close()
                editor.paged.detach()
            }
            try body(document, editor, undo)
        }
        func grouped(_ undo: UndoManager, _ body: () throws -> Void) rethrows {
            undo.beginUndoGrouping()
            defer { undo.endUndoGrouping() }
            try body()
        }
        func near(_ a: CGFloat, _ b: CGFloat) -> Bool { abs(a - b) < 0.2 }
        do {
            let directory = FileManager.default.temporaryDirectory.appendingPathComponent("doc42-epub-lifecycle-" + UUID().uuidString)
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
            defer { try? FileManager.default.removeItem(at: directory) }
            let source = directory.appendingPathComponent("source.epub")
            let docx = directory.appendingPathComponent("saved.docx")
            let exported = directory.appendingPathComponent("exported.epub")
            let bytes = try EPUBFormat.write(NSAttributedString(string: "EPUB source 世界"))
            try bytes.write(to: source)
            var opened: TextDocument?, openError: Error?, completed = false
            NSDocumentController.shared.openDocument(withContentsOf: source, display: true) { document, _, error in
                opened = document as? TextDocument; openError = error; completed = true
            }
            let deadline = Date().addingTimeInterval(10)
            while !completed && Date() < deadline { RunLoop.current.run(until: Date().addingTimeInterval(0.01)) }
            guard let imported = opened, openError == nil, let importedEditor = imported.editor else {
                throw openError ?? NSError(domain: "DOC42 test", code: 1)
            }
            defer { imported.updateChangeCount(.changeCleared); imported.close(); importedEditor.paged.detach() }
            check(imported.fileURL == nil && imported.fileType == "org.openxmlformats.wordprocessingml.document" && imported.isDocumentEdited,
                  "EPUB opens as an edited untitled DOCX, not its source")
            importedEditor.textView.setSelectedRange(NSRange(location: imported.storage.length, length: 0))
            importedEditor.textView.insertText(" Edited", replacementRange: importedEditor.textView.selectedRange())
            var saved = false, saveError: Error?
            imported.save(to: docx, ofType: "org.openxmlformats.wordprocessingml.document", for: .saveAsOperation) { error in
                saveError = error; saved = true
            }
            let saveDeadline = Date().addingTimeInterval(10)
            while !saved && Date() < saveDeadline { RunLoop.current.run(until: Date().addingTimeInterval(0.01)) }
            check(saved && saveError == nil && imported.fileURL == docx && (try? Data(contentsOf: source)) == bytes,
                  "native Save As writes DOCX and preserves source EPUB")
            let reopened = TextDocument()
            try reopened.read(from: docx, ofType: "org.openxmlformats.wordprocessingml.document")
            check(reopened.storage.string.contains("EPUB source 世界") && reopened.storage.string.contains("Edited"),
                  "saved DOCX reopens with imported and edited text")
            try imported.epubData().write(to: exported)
            let exportedText = try EPUBFormat.read(Data(contentsOf: exported)).string
            check((try? Data(contentsOf: source)) == bytes && exportedText.contains("Edited"),
                  "explicit EPUB export reopens without modifying source")
        } catch { check(false, "EPUB lifecycle failed: \(error)") }
        do {
            try fixture("Page setup and document lifecycle\nРусский и Türkçe текст.") { document, editor, undo in
                let original = document.setup
                grouped(undo) { editor.setMargins(left: 91, right: 63) }
                editor.paged.layoutAll()
                check(near(document.setup.margins.left, 91) && near(editor.paged.cards[0].setup.margins.right, 63), "horizontal margins update model and page")
                check(document.isDocumentEdited, "page setup marks document edited")
                for round in 1...3 {
                    undo.undo()
                    check(document.setup == original, "page setup Undo \(round)")
                    undo.redo()
                    check(near(document.setup.margins.left, 91), "page setup Redo \(round)")
                }
                undo.undo()
                undo.removeAllActions()
                check(Self.pageMeasurement(Self.pageMeasurementText(72), original: 72) == 72 && Self.pageMeasurement(Self.pageMeasurementText(36), original: 36) == 36,
                      "unchanged rounded page measurements retain exact points")
                check(Self.pageMeasurement("2,75", original: 72).map { near($0, 2.75 * DocRuler.cm) } == true && Self.pageMeasurement("inf", original: 72) == nil,
                      "edited page measurements accept decimal comma and reject infinity")
                let invalid: [(String, (inout PageSetup) -> Void)] = [
                    ("infinite margin", { $0.margins.left = .infinity }),
                    ("NaN margin", { $0.margins.top = .nan }),
                    ("negative margin", { $0.margins.right = -1 }),
                    ("margins consume page", { $0.margins.left = 500; $0.margins.right = 500 }),
                    ("columns consume page", { $0.columns = 3; $0.columnSpacing = 400 }),
                ]
                for (name, change) in invalid {
                    check(!editor.changeSetup("Invalid", change) && document.setup == original && !undo.canUndo, "reject \(name) without mutation or Undo")
                }
                grouped(undo) { editor.changeSetup("Custom paper") { $0.size = NSSize(width: 700, height: 900); $0.columns = 2 } }
                let printSetup = document.setup
                editor.setViewMode(.web)
                editor.paged.layoutAll()
                check(editor.currentSetup == printSetup && editor.paged.setup(at: 0).size != printSetup.size, "web layout keeps printed page setup for editing")
                editor.textView.setSelectedRange(NSRange(location: 0, length: 0))
                grouped(undo) { editor.insertSectionBreakAction(nil) }
                check(editor.paged.sections.first.map { printSetup.with($0.section).sameGeometry(printSetup) } == true, "section inserted in web mode retains print geometry")
                editor.setViewMode(.single)
                editor.paged.layoutAll()
                editor.textView.setSelectedRange(NSRange(location: document.storage.length, length: 0))
                let caret = editor.textView.selectedRange()
                grouped(undo) { editor.setVerticalMargins(top: 90, bottom: 60, at: 0) }
                check(editor.paged.sections.first?.section.margins.top == 90 && document.setup.margins.top == printSetup.margins.top && editor.textView.selectedRange() == caret,
                      "vertical margin targets visible section without moving caret")
                undo.undo()
                check(editor.paged.sections.first?.section.margins.top == printSetup.margins.top, "section margin Undo")
                // Native Undo selects the section-break attribute range it restored.
                // Put the body caret back in the other section before testing header-targeted commands.
                editor.textView.setSelectedRange(caret)
                grouped(undo) { editor.setVerticalMargins(top: 90, bottom: 60, at: 0) }
                editor.paged.layoutAll()
                editor.beginHeaderEdit(editor.paged.cards[0], header: true)
                if editor.currentSetup.margins.top != 90 || editor.textView.selectedRange() != caret {
                    print("DOC header section details: top=\(editor.currentSetup.margins.top), bodyCaret=\(editor.textView.selectedRange()), expectedCaret=\(caret), location=\(editor.pageSetupLocation), cardSection=\(editor.headerEditor?.card.sectionIndex ?? -1), sections=\(editor.paged.sections.map { ($0.index, $0.section.margins.top) })")
                    for card in editor.paged.cards {
                        let ranges = card.textViews.compactMap { tv -> String? in
                            guard let container = tv.textContainer else { return nil }
                            let glyphs = editor.paged.layoutManager.glyphRange(for: container)
                            let chars = editor.paged.layoutManager.characterRange(forGlyphRange: glyphs, actualGlyphRange: nil)
                            return "glyphs=\(glyphs), chars=\(chars)"
                        }
                        print("DOC card \(card.number): top=\(card.setup.margins.top), section=\(card.sectionIndex), \(ranges)")
                    }
                }
                check(editor.currentSetup.margins.top == 90 && editor.textView.selectedRange() == caret,
                      "page-setup menu follows active header section without moving body caret")
                let preset = NSMenuItem(); preset.tag = 1
                grouped(undo) { editor.marginPresetAction(preset) }
                check(near(editor.paged.sections.first?.section.margins.top ?? 0, 2.54 * DocRuler.cm) && document.setup == printSetup && editor.textView.selectedRange() == caret,
                      "margin preset targets active header section only")
                editor.headerEditor?.commit()

                // Save/reopen all rich formats that claim to preserve page geometry.
                for (type, label) in [(NSAttributedString.DocumentType.officeOpenXML, "docx"), (.rtf, "rtf"), (.openDocument, "odt")] {
                    let source = TextDocument()
                    source.storage.setAttributedString(NSAttributedString(string: "Saved body Русский Türkçe", attributes: Styles.normal.attributes))
                    source.setup.size = NSSize(width: 612, height: 792)
                    source.setup.margins = NSEdgeInsets(top: 54, left: 63, bottom: 72, right: 81)
                    source.setup.header = "Saved header"
                    source.setup.footer = "Saved footer"
                    source.setup.pageNumbers = false
                    let reopened = TextDocument()
                    let encoded = try source.exportData(type)
                    try reopened.read(from: encoded, ofType: label)
                    let preserved = reopened.storage.string.contains("Saved body Русский Türkçe") && near(reopened.setup.size.width, 612) && near(reopened.setup.size.height, 792) && near(reopened.setup.margins.left, 63) && near(reopened.setup.margins.right, 81) && near(reopened.setup.margins.top, 54) && near(reopened.setup.margins.bottom, 72)
                    if !preserved {
                        print("DOC \(label) round trip details: text=\(reopened.storage.string.debugDescription), size=\(reopened.setup.size), margins=\(reopened.setup.margins)")
                        if label == "rtf" { print("DOC RTF prefix: \(String(decoding: encoded.prefix(1200), as: UTF8.self))") }
                    }
                    check(preserved, "\(label) saves and reopens body and page geometry")
                    check(HeaderFooter.plain(reopened.setup.effectiveHeaderFooter.header).0 == "Saved header" && HeaderFooter.plain(reopened.setup.effectiveHeaderFooter.footer).0 == "Saved footer", "\(label) preserves header and footer")
                }
            }

            try fixture("Body remains separate from the header") { document, editor, undo in
                document.updateChangeCount(.changeCleared)
                editor.beginHeaderEdit(editor.paged.cards[0], header: true)
                editor.headerEditor?.commit()
                check(!document.isDocumentEdited && !undo.canUndo, "opening and leaving unchanged header stays clean")
                editor.beginHeaderEdit(editor.paged.cards[0], header: true)
                let pending = editor.headerEditor!
                pending.textView.insertText("Unsaved header Русский", replacementRange: NSRange(location: 0, length: 0))
                check(document.isDocumentEdited && document.setup.hf == nil, "pending header is reported as unsaved before model commit")
                var data = Data()
                try grouped(undo) { data = try document.data(ofType: "docx") }
                let reopened = TextDocument()
                try reopened.read(from: data, ofType: "docx")
                check(editor.headerEditor == nil && HeaderFooter.plain(reopened.setup.effectiveHeaderFooter.header).0 == "Unsaved header Русский", "Save commits and round-trips pending header")
                undo.undo()
                check(document.setup.hf == nil, "Undo saved header edit")
                undo.redo()
                check(HeaderFooter.plain(document.setup.effectiveHeaderFooter.header).0 == "Unsaved header Русский", "Redo header edit")
                undo.undo()
                check(document.setup.hf == nil, "second Undo header edit")
                undo.removeAllActions()

                let hf = DocHeaderFooter()
                hf.differentFirst = true
                let first = NSMutableAttributedString(string: "First-page header ", attributes: HeaderFooter.attributes(width: 400))
                first.append(Fields.reference(DocField(.page), attributes: HeaderFooter.attributes(width: 400)))
                hf.firstHeader = first
                document.setup.hf = hf
                check(editor.hasPageNumbers, "page-number menu detects first-page fields")
                grouped(undo) { editor.togglePageNumbersAction(nil) }
                check(!editor.hasPageNumbers && document.setup.hf?.firstHeader?.string.contains("First-page header") == true, "removing page numbers retains first-page-only header text")

                // Revert reuses read(from:ofType:) on an existing document.
                let saved = TextDocument()
                saved.storage.setAttributedString(NSAttributedString(string: "Saved version", attributes: Styles.normal.attributes))
                let savedData = try saved.exportData(.officeOpenXML)
                document.styles.replace(DocStyle(id: "UnsavedStyle", name: "Unsaved style"))
                document.background = .red
                document.watermark = "Unsaved watermark"
                document.trackChanges = true
                document.hyphenate = true
                editor.beginHeaderEdit(editor.paged.cards[0], header: true)
                editor.headerEditor?.textView.insertText("Discard on Revert", replacementRange: NSRange(location: 0, length: 0))
                try document.read(from: savedData, ofType: "docx")
                check(document.styles["UnsavedStyle"] == nil && document.background == nil && document.watermark.isEmpty && !document.trackChanges && !document.hyphenate && editor.headerEditor == nil,
                      "Revert replaces unsaved styles/metadata and discards pending header")
                document.setup.margins.left = 150
                document.watermark = "Stale"
                try document.read(from: Data("Reverted plain text".utf8), ofType: "txt")
                check(document.setup == PageSetup() && document.watermark.isEmpty && document.storage.string == "Reverted plain text", "plain-text read resets unsupported prior setup and metadata")
            }

            try fixture("Live editor before export\n") { document, editor, _ in
                let fieldAt = document.storage.length
                let liveField = DocField(.pageCount)
                document.storage.append(Fields.reference(liveField, attributes: Styles.normal.attributes))
                editor.paged.layoutAll()
                let liveText = document.storage.string
                let liveResult = liveField.result
                let liveManagerCount = document.storage.layoutManagers.count
                let offscreen = document.offscreenPages()
                let copiedField = offscreen.layoutManager.textStorage?.attribute(.docField, at: fieldAt, effectiveRange: nil) as? DocField
                check(offscreen.layoutManager.textStorage !== document.storage && document.storage.layoutManagers.count == liveManagerCount,
                      "offscreen layout leaves live layout-manager count unchanged while alive")
                check(copiedField != nil && copiedField !== liveField && liveField.result == liveResult,
                      "offscreen field has an independent result cache")
                copiedField?.result = "Offscreen-only value"
                check(liveField.result == liveResult && document.storage.string == liveText,
                      "offscreen field mutation cannot affect live text or cached value")
                offscreen.detach()
                _ = try document.exportData(.officeOpenXML)
                check(document.storage.layoutManagers.count == liveManagerCount && document.storage.string == liveText && liveField.result == liveResult,
                      "DOCX export preserves live storage, layout managers and field results")
                editor.paged.layoutAll()
                let lm = editor.paged.layoutManager
                var lastCharacter = 0, lastGlyph = 0
                for container in lm.textContainers {
                    let glyphs = lm.glyphRange(for: container)
                    lastGlyph = max(lastGlyph, NSMaxRange(glyphs))
                    lastCharacter = max(lastCharacter, NSMaxRange(lm.characterRange(forGlyphRange: glyphs, actualGlyphRange: nil)))
                }
                check(lastCharacter >= document.storage.length && lastGlyph >= lm.numberOfGlyphs && document.storage.string == liveText,
                      "live editor still lays out complete text after export")
            }

            fixture("Title: ") { document, editor, _ in
                document.docInfo.title = "5"
                var attributes = Styles.normal.attributes
                attributes[.font] = NSFont.monospacedSystemFont(ofSize: 12, weight: .regular)
                let field = DocField(.title), at = document.storage.length
                document.storage.append(Fields.reference(field, attributes: attributes))
                document.storage.append(NSAttributedString(string: " — text after the field", attributes: attributes))
                editor.paged.layoutAll()
                guard let attachment = document.storage.attribute(.attachment, at: at, effectiveRange: nil) as? NSTextAttachment,
                      let cell = attachment.attachmentCell as? FieldCell, let originalWidth = cell.laidOutWidth else {
                    check(false, "field-width fixture was measured"); return
                }
                document.docInfo.title = "7"
                check(!editor.paged.refreshFields(schedule: false) && field.result == "7" && cell.laidOutWidth == originalWidth,
                      "equal-width field update refreshes value without invalidating layout")
                document.docInfo.title = "123456789"
                // A draw can evaluate the new value before refresh; the measured frame still has the old width.
                field.result = document.docInfo.title
                check(editor.paged.refreshFields(schedule: false), "field width growth is detected even after its value cache changed")
                editor.paged.layoutAll()
                let lm = editor.paged.layoutManager
                let end = lm.textContainers.map { NSMaxRange(lm.characterRange(forGlyphRange: lm.glyphRange(for: $0), actualGlyphRange: nil)) }.max() ?? 0
                check((cell.laidOutWidth ?? 0) > originalWidth && end >= document.storage.length,
                      "wider field is measured and following text remains completely laid out")
                check(!editor.paged.refreshFields(schedule: false) && field.result == "123456789",
                      "settled field does not trigger another layout invalidation")
            }

            fixture("") { document, editor, _ in
                document.docInfo.title = "7"
                var attributes = Styles.normal.attributes
                attributes[.font] = NSFont.monospacedSystemFont(ofSize: 12, weight: .regular)
                let field = DocField(.title)
                let content = NSMutableAttributedString(attributedString: Fields.reference(field, attributes: attributes))
                content.append(NSAttributedString(string: "\n", attributes: attributes))
                let model = TableModel(rows: [[TableModel.Cell(content: content, props: CellProps()),
                                               TableModel.Cell(content: NSAttributedString(string: "Other cell\n", attributes: attributes), props: CellProps())]], widths: [50, 50])
                document.storage.setAttributedString(model.render())
                document.storage.append(NSAttributedString(string: "After the table.", attributes: attributes))
                editor.paged.layoutAll()
                document.docInfo.title = "123456789"
                let resized = editor.paged.refreshFields(schedule: false)
                editor.paged.layoutAll()
                let lm = editor.paged.layoutManager
                let end = lm.textContainers.map { NSMaxRange(lm.characterRange(forGlyphRange: lm.glyphRange(for: $0), actualGlyphRange: nil)) }.max() ?? 0
                check(resized && field.result == "123456789" && end >= document.storage.length && !editor.paged.refreshFields(schedule: false),
                      "table field width growth settles without losing text after the table")
            }

            var options = PDFOptions()
            check(try options.pageSet(count: 5) == nil, "empty PDF range means all pages")
            for (spec, expected) in [("1-3, 5", Set([0, 1, 2, 4])), ("-2;4-", Set([0, 1, 3, 4])), ("1-9223372036854775807", Set(0..<5))] {
                options.pages = spec
                check(try options.pageSet(count: 5) == expected, "PDF range \(spec) is bounded to real pages")
            }
            for spec in ["abc", "999", "3-1", "0", "-", "1,", "1-NaN", "1-9223372036854775808"] {
                options.pages = spec
                do { _ = try options.pageSet(count: 5); check(false, "reject PDF range \(spec)") }
                catch { check(true, "reject PDF range \(spec)") }
            }
            let document = TextDocument()
            document.storage.setAttributedString(NSAttributedString(string: "Page One\u{000C}Page Two\u{000C}Page Three", attributes: Styles.normal.attributes))
            options.pages = "2"
            let pdf = PDFDocument(data: try document.pdfData(options))
            check(pdf?.pageCount == 1 && pdf?.string?.contains("Page Two") == true && pdf?.string?.contains("Page One") == false, "PDF export includes only selected page text")
        } catch {
            check(false, "unexpected error: \(error)")
        }
        print("DOC document UI regressions: \(checks - failures)/\(checks) passed")
        return failures == 0
    }
}
