import AppKit
import Kit42

extension DocWindowController {
    /// Native command regressions on a disposable document, including the actual text-system Undo/Redo.
    /// Invoked with K42_SCRIPT=auditediting; does not edit the user's document or use the clipboard.
    func auditEditing() -> Bool {
        let document = TextDocument()
        document.makeWindowControllers()
        guard let editor = document.editor, let undo = document.undoManager else { return false }
        let storage = document.storage
        let tv = editor.textView
        undo.groupsByEvent = false
        defer {
            undo.beginUndoGrouping()
            editor.headerEditor?.commit()
            undo.endUndoGrouping()
            undo.removeAllActions()
            document.updateChangeCount(.changeCleared)
            editor.close()
        }
        var failures = 0, checks = 0
        func check(_ condition: Bool, _ name: String) {
            checks += 1
            if !condition { failures += 1 }
            print("\(condition ? "PASS" : "FAIL") DOC editing: \(name)")
        }
        check(!editor.runWriterStep("modstyle", "MissingAuditStyle42", []),
              "script rejects a missing style instead of silently accepting the command")
        func reset(_ text: NSAttributedString) {
            editor.headerEditor?.commit()
            storage.setAttributedString(text)
            editor.paged.layoutAll()
            tv.setSelectedRange(NSRange(location: 0, length: storage.length))
            tv.typingAttributes = Styles.normal.attributes
            undo.removeAllActions()
        }
        func plain(_ text: String) { reset(NSAttributedString(string: text, attributes: Styles.normal.attributes)) }
        func action(_ body: () -> Void) {
            tv.breakUndoCoalescing()
            undo.beginUndoGrouping()
            body()
            undo.endUndoGrouping()
        }
        func font(_ index: Int = 0) -> NSFont { storage.attribute(.font, at: index, effectiveRange: nil) as? NSFont ?? Styles.normal.font }
        func bold(_ index: Int = 0) -> Bool { NSFontManager.shared.traits(of: font(index)).contains(.boldFontMask) }
        func tag(_ value: Int) -> NSMenuItem { let item = NSMenuItem(); item.tag = value; return item }

        plain("Alpha\nBeta")
        tv.setSelectedRange(NSRange(location: 0, length: 5))
        action { editor.toggleBold(nil) }
        check(bold() && !bold(6), "bold affects only selected text")
        undo.undo(); check(!bold(), "bold Undo")
        undo.redo(); check(bold(), "bold Redo")
        action { editor.toggleUnderline(nil) }
        check(storage.attribute(.underlineStyle, at: 0, effectiveRange: nil) as? Int == 1, "underline action")
        undo.undo(); check(storage.attribute(.underlineStyle, at: 0, effectiveRange: nil) == nil, "underline Undo")
        action { editor.alignAction(tag(2)) }
        check((storage.attribute(.paragraphStyle, at: 0, effectiveRange: nil) as? NSParagraphStyle)?.alignment == .right &&
              (storage.attribute(.paragraphStyle, at: 6, effectiveRange: nil) as? NSParagraphStyle)?.alignment != .right,
              "paragraph alignment preserves next paragraph")
        undo.undo()
        check((storage.attribute(.paragraphStyle, at: 0, effectiveRange: nil) as? NSParagraphStyle)?.alignment != .right, "alignment Undo")

        plain("cat cats CAT concatenate cat")
        let original = storage.string
        action {
            let count = TextSearch.replaceAll(storage, find: "cat", with: "dog", FindOptions(wholeWords: true), textView: tv)
            check(count == 3 && storage.string == "dog cats dog concatenate dog", "Replace All whole words and case folding")
        }
        undo.undo(); check(storage.string == original, "Replace All Undo restores complete text")
        undo.redo(); check(storage.string == "dog cats dog concatenate dog", "Replace All Redo")
        plain("α1 β22")
        action {
            let count = TextSearch.replaceAll(storage, find: "(\\p{L}+)(\\d+)", with: "$2:$1", FindOptions(regex: true), textView: tv)
            check(count == 2 && storage.string == "1:α 22:β", "regex capture replacement with Unicode")
        }
        undo.undo(); check(storage.string == "α1 β22", "Unicode regex replacement Undo")
        if let re = TextSearch.expression("cat", FindOptions(wholeWords: true)) {
            check(TextSearch.selectedMatch(in: "cats cat", range: NSRange(location: 0, length: 3), expression: re) == nil,
                  "Replace rejects selected prefix of a longer word")
            check(TextSearch.selectedMatch(in: "cats cat", range: NSRange(location: 5, length: 3), expression: re) != nil,
                  "Replace accepts selected complete word")
        }
        if let re = TextSearch.expression("^cat$", FindOptions(regex: true)) {
            check(TextSearch.selectedMatch(in: "a cat b", range: NSRange(location: 2, length: 3), expression: re) == nil,
                  "Replace keeps regex anchors relative to document")
        }
        plain("find me later")
        action {
            let range = NSRange(location: 5, length: 2)
            if editor.finderClient.shouldReplaceCharacters(inRanges: [NSValue(range: range)], with: ["us"]) {
                editor.finderClient.replaceCharacters(in: range, with: "us")
                editor.finderClient.didReplaceCharacters()
            }
        }
        check(storage.string == "find us later", "native find-bar client replacement")
        undo.undo(); check(storage.string == "find me later", "native find-bar replacement Undo")

        let model = TableModel(rows: (0..<2).map { row in (0..<3).map { column in
            TableModel.Cell(content: NSAttributedString(string: "\(row),\(column)\n", attributes: Styles.normal.attributes), props: CellProps())
        } }, widths: [33, 34, 33])
        reset(model.render())
        if let grid = editor.tableGrid, let first = grid.cell(row: 0, col: 0), let second = grid.cell(row: 0, col: 1) {
            tv.setSelectedRange(NSRange(location: storage.length, length: 0))
            let lastBox = editor.selectedCellBox(grid)
            check(lastBox.r0 == 1 && lastBox.r1 == 1 && lastBox.c0 == 2 && lastBox.c1 == 2,
                  "caret at table end addresses last cell")
            tv.setSelectedRange(NSRange(location: first.range.location, length: NSMaxRange(second.range) - first.range.location))
            let box = editor.selectedCellBox(grid)
            check(box.r0 == 0 && box.r1 == 0 && box.c0 == 0 && box.c1 == 1, "table selection excludes next cell at endpoint")
            action { editor.mergeCellsAction(nil) }
            check(editor.tableGrid?.cells.count == 5 && editor.tableGrid?.cell(row: 0, col: 0)?.colSpan == 2,
                  "Merge affects exactly the two selected cells")
            undo.undo(); tv.setSelectedRange(NSRange(location: 0, length: 0))
            check(editor.tableGrid?.cells.count == 6, "Merge Undo restores all cells")
            action { editor.tableRowAction(tag(1)) }
            check(editor.tableGrid?.rows == 3 && editor.tableGrid?.columns == 3, "Insert Row Below")
            undo.undo(); tv.setSelectedRange(NSRange(location: 0, length: 0))
            check(editor.tableGrid?.rows == 2, "Insert Row Undo")
            action { editor.tableColumnAction(tag(1)) }
            check(editor.tableGrid?.columns == 4 && editor.tableGrid?.rows == 2, "Insert Column Right")
            undo.undo(); tv.setSelectedRange(NSRange(location: 0, length: 0))
            check(editor.tableGrid?.columns == 3, "Insert Column Undo")
            action { editor.tableRowAction(tag(2)) }
            check(editor.tableGrid?.rows == 1, "Delete Row")
            undo.undo(); tv.setSelectedRange(NSRange(location: 0, length: 0))
            check(editor.tableGrid?.rows == 2, "Delete Row Undo")
        } else { check(false, "table fixture") }

        reset(model.render())
        tv.setSelectedRange(NSRange(location: 0, length: storage.length))
        action { editor.clearFormattingAction(nil) }
        check(editor.tableGrid?.cells.count == 6 && editor.tableGrid?.columns == 3,
              "Clear Formatting preserves table structure")
        undo.undo(); check(editor.tableGrid?.cells.count == 6, "Clear Formatting table Undo")
        let image = CLI.demoImage("audit", size: NSSize(width: 80, height: 60))
        let rich = NSMutableAttributedString(attributedString: image)
        rich.append(Fields.reference(DocField(.page), attributes: Styles.normal.attributes))
        rich.append(NSAttributedString(string: "link\u{000C}", attributes: Styles.normal.attributes))
        let comment = DocComment(author: "Test", text: "Review")
        let revision = DocRevision(.insertion, author: "Test")
        rich.addAttributes([.docComment: comment, .docRevision: revision, .backgroundColor: NSColor.yellow], range: NSRange(location: 0, length: rich.length))
        rich.addAttribute(.link, value: URL(string: "https://example.com")!, range: NSRange(location: 2, length: 4))
        rich.addAttribute(.docSection, value: PageSetup().section, range: NSRange(location: rich.length - 1, length: 1))
        reset(rich)
        action { editor.clearFormattingAction(nil) }
        check(storage.attribute(.attachment, at: 0, effectiveRange: nil) is NSTextAttachment, "Clear Formatting preserves picture")
        check(storage.attribute(.docField, at: 1, effectiveRange: nil) is DocField && storage.attribute(.attachment, at: 1, effectiveRange: nil) is NSTextAttachment,
              "Clear Formatting preserves field")
        check(storage.attribute(.docComment, at: 0, effectiveRange: nil) as? DocComment === comment &&
              storage.attribute(.docRevision, at: 0, effectiveRange: nil) as? DocRevision === revision,
              "Clear Formatting preserves comment and tracked change")
        check(storage.attribute(.link, at: 2, effectiveRange: nil) != nil && storage.attribute(.docSection, at: storage.length - 1, effectiveRange: nil) is DocSection,
              "Clear Formatting preserves link and section")
        check(storage.attribute(.backgroundColor, at: 0, effectiveRange: nil) == nil, "Clear Formatting removes direct highlight")
        undo.undo(); check(storage.attribute(.backgroundColor, at: 0, effectiveRange: nil) != nil, "Clear Formatting Undo restores direct appearance")

        reset(image)
        let originalImageSize = (storage.attribute(.attachment, at: 0, effectiveRange: nil) as? NSTextAttachment)?.bounds.size
        action { editor.resizeImage(0, to: NSSize(width: 120, height: 90)) }
        check((storage.attribute(.attachment, at: 0, effectiveRange: nil) as? NSTextAttachment)?.bounds.size == NSSize(width: 120, height: 90),
              "picture resize changes attachment geometry")
        undo.undo()
        check((storage.attribute(.attachment, at: 0, effectiveRange: nil) as? NSTextAttachment)?.bounds.size == originalImageSize, "picture resize Undo")
        undo.redo()
        check((storage.attribute(.attachment, at: 0, effectiveRange: nil) as? NSTextAttachment)?.bounds.size == NSSize(width: 120, height: 90), "picture resize Redo")

        plain("One\nTwo")
        action { editor.bulletListAction(nil) }
        let listed = storage.string
        check(listed.contains("\t") && (storage.attribute(.paragraphStyle, at: 0, effectiveRange: nil) as? NSParagraphStyle)?.textLists.count == 1,
              "bullet-list action creates list structure")
        tv.setSelectedRange(NSRange(location: 0, length: storage.length))
        action { editor.clearFormattingAction(nil) }
        check(storage.string == listed && (storage.attribute(.paragraphStyle, at: 0, effectiveRange: nil) as? NSParagraphStyle)?.textLists.count == 1,
              "Clear Formatting preserves list and markers")

        plain("")
        tv.typingAttributes[.font] = NSFont.boldSystemFont(ofSize: 28)
        tv.typingAttributes[.backgroundColor] = NSColor.yellow
        editor.clearFormattingAction(nil)
        check((tv.typingAttributes[.font] as? NSFont)?.pointSize == Styles.normal.font.pointSize && tv.typingAttributes[.backgroundColor] == nil,
              "Clear Formatting with caret resets typing attributes")

        plain("Styled text")
        document.styles.replace(DocStyle(id: "AuditParent", name: "Audit Parent", kind: .character, weight: 9, underline: true, color: .red))
        document.styles.replace(DocStyle(id: "AuditChild", name: "Audit Child", kind: .character, basedOn: "AuditParent", italic: true))
        action { editor.applyCharacterStyle("AuditChild") }
        let traits = NSFontManager.shared.traits(of: font())
        check(traits.contains(.boldFontMask) && traits.contains(.italicFontMask) &&
              storage.attribute(.underlineStyle, at: 0, effectiveRange: nil) as? Int == 1,
              "character style inherits bold and underline plus own italic")
        check((storage.attribute(.foregroundColor, at: 0, effectiveRange: nil) as? NSColor) == .red, "character style inherits colour")
        undo.undo(); check(!bold() && storage.attribute(.docCharStyle, at: 0, effectiveRange: nil) == nil, "character style Undo")
        undo.redo()
        action { editor.applyCharacterStyle(nil) }
        check(!bold() && storage.attribute(.underlineStyle, at: 0, effectiveRange: nil) == nil, "Default Character Style clears style underline")
        undo.undo(); check(bold() && storage.attribute(.underlineStyle, at: 0, effectiveRange: nil) as? Int == 1, "Default Character Style Undo")

        plain("Plain\nDirect\nLink")
        document.styles.replace(DocStyle(id: "AuditParagraph", name: "Audit Paragraph", basedOn: "Normal", underline: true))
        action { editor.applyParagraphStyle("AuditParagraph") }
        check(storage.attribute(.underlineStyle, at: 0, effectiveRange: nil) as? Int == NSUnderlineStyle.single.rawValue,
              "Apply paragraph style adds its underline")
        undo.undo()
        check(storage.attribute(.underlineStyle, at: 0, effectiveRange: nil) == nil, "paragraph style underline Undo")
        undo.redo()
        check(storage.attribute(.underlineStyle, at: 0, effectiveRange: nil) as? Int == NSUnderlineStyle.single.rawValue,
              "paragraph style underline Redo")
        tv.setSelectedRange(NSRange(location: 6, length: 6))
        action { editor.applyAttribute(.underlineStyle, NSUnderlineStyle.double.rawValue) }
        tv.setSelectedRange(NSRange(location: 13, length: 4))
        action { editor.applyAttribute(.link, URL(string: "https://example.com")!) }
        action {
            editor.changeStyles(L("Modify Style"), reapply: "AuditParagraph") { sheet in
                var style = sheet["AuditParagraph"]!
                style.underline = false
                sheet.replace(style)
            }
        }
        check(storage.attribute(.underlineStyle, at: 0, effectiveRange: nil) == nil,
              "Modify paragraph style removes style-owned underline")
        check(storage.attribute(.underlineStyle, at: 6, effectiveRange: nil) as? Int == NSUnderlineStyle.double.rawValue,
              "Modify paragraph style preserves direct double underline")
        check(storage.attribute(.underlineStyle, at: 13, effectiveRange: nil) as? Int == NSUnderlineStyle.single.rawValue,
              "Modify paragraph style preserves hyperlink underline")
        undo.undo()
        check(document.styles["AuditParagraph"]?.underline == true && storage.attribute(.underlineStyle, at: 0, effectiveRange: nil) as? Int == 1,
              "Modify Style Undo restores definition and paragraph underline")
        undo.redo()
        check(document.styles["AuditParagraph"]?.underline == false && storage.attribute(.underlineStyle, at: 0, effectiveRange: nil) == nil &&
              storage.attribute(.underlineStyle, at: 6, effectiveRange: nil) as? Int == NSUnderlineStyle.double.rawValue,
              "Modify Style Redo keeps direct formatting")
        action {
            editor.changeStyles(L("Modify Style"), reapply: "AuditParagraph") { sheet in
                var style = sheet["AuditParagraph"]!
                style.underline = true
                sheet.replace(style)
            }
        }
        check(storage.attribute(.underlineStyle, at: 0, effectiveRange: nil) as? Int == 1 &&
              storage.attribute(.underlineStyle, at: 6, effectiveRange: nil) as? Int == NSUnderlineStyle.double.rawValue,
              "Modify paragraph style adds underline without replacing direct variant")

        plain("Body")
        if let card = editor.paged.cards.first {
            editor.beginHeaderEdit(card, header: true)
            if let header = editor.headerEditor {
                let hv = header.textView
                let insertTable = NSMenuItem(title: "", action: #selector(DocWindowController.insertTableAction(_:)), keyEquivalent: "")
                let boldItem = NSMenuItem(title: "", action: #selector(DocWindowController.toggleBold(_:)), keyEquivalent: "")
                check(!editor.validateMenuItem(insertTable) && editor.validateMenuItem(boldItem),
                      "header menu disables body structures and keeps text formatting enabled")
                let tableTool = NSToolbarItem(itemIdentifier: .init("audit.table"))
                tableTool.action = #selector(DocWindowController.insertTableAction(_:))
                check(!editor.validateToolbarItem(tableTool), "header toolbar disables body table insertion")
                header.undo.groupsByEvent = false
                header.undo.beginUndoGrouping()
                hv.insertText("Header text longer than the body", replacementRange: NSRange(location: 0, length: hv.string.utf16.count))
                header.undo.endUndoGrouping()
                header.undo.removeAllActions()
                hv.setSelectedRange(NSRange(location: 12, length: 6))
                header.undo.beginUndoGrouping(); editor.applyParagraphStyle("Heading1"); header.undo.endUndoGrouping()
                check(hv.textStorage?.attribute(Styles.styleKey, at: 12, effectiveRange: nil) as? String == "Heading1" &&
                      storage.attribute(Styles.styleKey, at: 0, effectiveRange: nil) == nil,
                      "paragraph style edits header without touching shorter body")
                header.undo.undo()
                check(hv.textStorage?.attribute(Styles.styleKey, at: 12, effectiveRange: nil) == nil, "header paragraph style Undo")
                header.undo.beginUndoGrouping(); editor.applyCharacterStyle("Strong"); header.undo.endUndoGrouping()
                check(hv.textStorage?.attribute(.docCharStyle, at: 12, effectiveRange: nil) as? String == "Strong" && !bold(),
                      "character style edits header only")
                header.undo.beginUndoGrouping(); editor.clearFormattingAction(nil); header.undo.endUndoGrouping()
                check(hv.textStorage?.attribute(.docCharStyle, at: 12, effectiveRange: nil) == nil && storage.string == "Body",
                      "Clear Formatting edits header only")
                header.undo.beginUndoGrouping()
                hv.textStorage?.addAttribute(.link, value: URL(string: "https://example.com")!, range: NSRange(location: 12, length: 6))
                hv.setSelectedRange(NSRange(location: 12, length: 6))
                editor.removeLinkAction(nil)
                header.undo.endUndoGrouping()
                check(hv.textStorage?.attribute(.link, at: 12, effectiveRange: nil) == nil && storage.string == "Body",
                      "Remove Link edits header without touching shorter body")
                header.undo.beginUndoGrouping(); editor.applyParagraphBorders(BorderOptions()); header.undo.endUndoGrouping()
                check((hv.textStorage?.attribute(.paragraphStyle, at: 12, effectiveRange: nil) as? NSParagraphStyle)?.textBlocks.count == 1 &&
                      (storage.attribute(.paragraphStyle, at: 0, effectiveRange: nil) as? NSParagraphStyle)?.textBlocks.isEmpty == true,
                      "paragraph border edits longer header without touching body")
                header.undo.undo()
                check((hv.textStorage?.attribute(.paragraphStyle, at: 12, effectiveRange: nil) as? NSParagraphStyle)?.textBlocks.isEmpty == true,
                      "header paragraph border Undo")
                action { editor.setDropCap(3) }
                check(storage.attribute(.docDropCap, at: 0, effectiveRange: nil) == nil,
                      "unsupported header drop cap cannot change body")
                action { header.commit() }
            } else { check(false, "header editor opened") }
        } else { check(false, "header page fixture") }

        plain("A comment on these words")
        tv.setSelectedRange(NSRange(location: 2, length: 7))
        action { editor.newCommentAction(nil) }
        check(editor.allComments().count == 1, "New Comment action")
        if let note = editor.allComments().first?.0 {
            action { editor.setCommentText(note, "Changed") }
            check(note.text == "Changed", "Edit Comment")
            undo.undo(); check(note.text.isEmpty, "Edit Comment Undo")
            action { editor.setResolved(note, true) }
            check(note.resolved, "Resolve Comment")
            undo.undo(); check(!note.resolved, "Resolve Comment Undo")
            action { editor.deleteComment(note) }
            check(editor.allComments().isEmpty, "Delete Comment")
            undo.undo(); check(editor.allComments().count == 1, "Delete Comment Undo")
        }
        for (count, expected) in [("-1", 1), ("999999999", 10_000), ("3", 3)] {
            let xml = "<root xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\"><text:note><text:note-body><text:p>A<text:s text:c=\"\(count)\"/>B</text:p></text:note-body></text:note></root>"
            if let root = (try? XMLDocument(xmlString: xml, options: []))?.rootElement() {
                var extras = DocxStyles.Extras()
                OdtFormat.markAnnotations(root, extras: &extras)
                check(extras.notes["of1"] == "A" + String(repeating: " ", count: expected) + "B", "ODT note repeated-space count \(count)")
            } else { check(false, "ODT note XML fixture") }
        }
        print("DOC editing UI regressions: \(checks - failures)/\(checks) passed")
        return failures == 0
    }
}
