import AppKit
import Kit42

extension DocWindowController {
    /// Development aid: `K42_SCRIPT="sel 10 5; comment Text; note Text; section; landscape; cols 2; replace a b; undo; save /tmp/x.docx"`
    /// drives editor commands before a K42_SNAPSHOT (the same code paths as the menus, undo included).
    func runTestScript(_ script: String) {
        let storage = textView.textStorage!
        for step in script.split(separator: ";").map({ $0.trimmingCharacters(in: .whitespaces) }) where !step.isEmpty {
            let parts = step.split(separator: " ", maxSplits: 1).map(String.init)
            let arg = parts.count > 1 ? parts[1] : ""
            let nums = arg.split(separator: " ").compactMap { Int($0) }
            textView.breakUndoCoalescing()
            switch parts[0] {
            case "sel": textView.setSelectedRange(NSRange(location: nums[0], length: nums.count > 1 ? nums[1] : 0))
            case "find":
                let r = (storage.string as NSString).range(of: arg)
                if r.location != NSNotFound { textView.setSelectedRange(r) }
            case "end": textView.setSelectedRange(NSRange(location: storage.length, length: 0))
            case "type": textView.insertText(arg, replacementRange: textView.selectedRange())
            case "bs": for _ in 0..<max(1, nums.first ?? 1) { textView.deleteBackward(nil) }
            case "del": for _ in 0..<max(1, nums.first ?? 1) { textView.deleteForward(nil) }
            case "track": toggleTrackChangesAction(nil)
            case "markup": toggleMarkupAction(nil)
            case "nextchange": nextChangeAction(nil)
            case "accept": acceptChangeAction(nil)
            case "reject": rejectChangeAction(nil)
            case "acceptall": acceptAllChangesAction(nil)
            case "rejectall": rejectAllChangesAction(nil)
            case "dumptext": print("TEXT:", storage.string.debugDescription, "CHANGES:", Revisions.changes(in: storage).map { "\($0.kind.rawValue)@\($0.range.location)+\($0.range.length)" })
            case "comment":
                newCommentAction(nil)
                if let c = commentAtCursor { setCommentText(c, arg) }
            case "note": insertFootnote(arg, edit: false)
            case "section": insertSectionBreakAction(nil)
            case "landscape", "portrait":
                let item = NSMenuItem(); item.tag = parts[0] == "landscape" ? 1 : 0
                orientationAction(item)
            case "cols":
                let item = NSMenuItem(); item.tag = (nums.first ?? 1) - 1
                columnsAction(item)
            case "replace":
                let fr = arg.split(separator: "|").map(String.init)
                let n = TextSearch.replaceAll(storage, find: fr[0], with: fr.count > 1 ? fr[1] : "", FindOptions(), textView: textView)
                print("replaced", n)
            case "undo": doc?.undoManager?.undo()
            case "redo": doc?.undoManager?.redo()
            case "toc": insertTOCAction(nil)
            case "paint":   // paint <from> <to> <len>
                textView.setSelectedRange(NSRange(location: nums[0], length: 1))
                formatPainterAction(nil)
                textView.setSelectedRange(NSRange(location: nums[1], length: nums[2]))
                textViewMouseUp(textView)
            case "link":
                let a: [NSAttributedString.Key: Any] = [.link: linkValue(arg), .foregroundColor: Self.linkColor, .underlineStyle: 1]
                if textView.shouldChangeText(in: textView.selectedRange(), replacementString: nil) {
                    storage.addAttributes(a, range: textView.selectedRange()); textView.didChangeText()
                }
            case "stats": print(TextStats.of(storage, NSRange(location: 0, length: storage.length), layout: paged.layoutManager), "pages", paged.cards.count)
            case "hidecomments": toggleCommentsAction(nil)
            case "ruler": toggleRulerAction(nil)
            case "auditpanels": if !auditPanelPreferences() { exit(1) }
            case "auditediting": if !auditEditing() { exit(1) }
            case "audittechnical": if !auditTechnicalElements() { exit(1) }
            case "auditrulers": if !auditRulers() { exit(1) }
            case "auditdocument": if !auditDocumentLifecycle() { exit(1) }
            case "auditpagination": if !auditPagination() { exit(1) }
            case "editnote": if let n = Notes.all(in: storage).first?.1 { editNote(n) }
            case "findbar": finder.performAction(.showReplaceInterface)
            case "findnext": finder.performAction(.nextMatch)
            case "statistics": statisticsAction(nil)
            case "special": specialCharacterAction(nil)
            case "updatetoc": updateTOCAction(nil)
            case "scroll": paged.scrollRangeToVisible(NSRange(location: nums[0], length: 0))
            case "save":
                if let data = try? doc?.exportData(TextDocument.documentType(for: URL(fileURLWithPath: arg).pathExtension)) {
                    try? data.write(to: URL(fileURLWithPath: arg))
                }
            default:
                if !runWriterStep(parts[0], arg, nums) { print("unknown step", step) }
            }
            paged.layoutAll()
        }
        layoutComments()
        // The snapshot quits the app: no "save changes?" sheet.
        if ProcessInfo.processInfo.environment["K42_SNAPSHOT_KEEP"] == nil {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) { [weak self] in self?.doc?.updateChangeCount(.changeCleared) }   // after the undo group closes
        }
    }
}

extension DocWindowController {
    /// K42_SCRIPT=auditpanels: exercise the actual Hide All Panels action and a newly created editor.
    /// Preferences and the original window's visibility are restored even when a check fails.
    private func auditPanelPreferences() -> Bool {
        guard !Panels.allHidden(in: window) else { print("FAIL DOC panels: start with panels restored"); return false }
        let defaults = UserDefaults.standard
        let keys = ["doc.ruler", "doc.comments"]
        let savedPreferences = keys.map { ($0, defaults.object(forKey: $0)) }
        let original = (!ruler.isHidden, comments.shown)
        defer {
            if Panels.allHidden(in: window) { toggleAllPanelsAction(nil) }
            setRuler(shown: original.0)
            setComments(shown: original.1)
            for (key, value) in savedPreferences {
                if let value { defaults.set(value, forKey: key) } else { defaults.removeObject(forKey: key) }
            }
        }
        var failures = 0
        func check(_ condition: Bool, _ name: String) {
            print("\(condition ? "PASS" : "FAIL") DOC panels: \(name)")
            if !condition { failures += 1 }
        }
        for (rulerShown, commentsShown) in [(true, true), (true, false), (false, true), (false, false)] {
            let label = "ruler=\(rulerShown), comments=\(commentsShown)"
            defaults.set(rulerShown, forKey: keys[0])
            defaults.set(commentsShown, forKey: keys[1])
            setRuler(shown: rulerShown)
            setComments(shown: commentsShown)
            toggleAllPanelsAction(nil)
            check(ruler.isHidden && !comments.shown && Panels.allHidden(in: window), "hidden (\(label))")
            check(defaults.bool(forKey: keys[0]) == rulerShown && defaults.bool(forKey: keys[1]) == commentsShown,
                  "preferences unchanged (\(label))")
            let next = DocWindowController(storage: NSTextStorage())
            check(!next.ruler.isHidden == rulerShown && next.comments.shown == commentsShown, "new editor preferences (\(label))")
            next.close()
            toggleAllPanelsAction(nil)
            check(!ruler.isHidden == rulerShown && comments.shown == commentsShown && !Panels.allHidden(in: window),
                  "restored (\(label))")
        }
        print("DOC panel UI regressions: \(16 - failures)/16 passed")
        return failures == 0
    }
}


extension DocWindowController {
    /// Steps for the Writer-like features (see runTestScript). Returns false for an unknown step.
    func runWriterStep(_ name: String, _ arg: String, _ nums: [Int]) -> Bool {
        let storage = textView.textStorage!
        func kv() -> [String: String] {
            var out: [String: String] = [:]
            for p in arg.split(separator: " ") {
                let a = p.split(separator: "=", maxSplits: 1).map(String.init)
                if a.count == 2 { out[a[0]] = a[1] }
            }
            return out
        }
        func color(_ s: String?) -> NSColor? { s.flatMap { DocxStyles.hexColor($0) } }
        func num(_ s: String?) -> CGFloat? { s.flatMap { Double($0) }.map { CGFloat($0) } }
        switch name {
        case "style": applyParagraphStyle(arg)
        case "charstyle": applyCharacterStyle(arg == "none" ? nil : arg)
        case "newstyle": newStyle(named: arg, character: false)
        case "newcharstyle": newStyle(named: arg, character: true)
        case "updatestyle": updateStyleToMatchSelection(arg)
        case "modstyle":
            let a = kv()
            let id = String(arg.split(separator: " ").first ?? "Normal")
            guard var st = styleSheet[id] else { return true }
            if let v = num(a["size"]) { st.size = v }
            if let c = color(a["color"]) { st.color = c }
            if let b = a["bold"] { st.weight = b == "1" ? 9 : 5 }
            if let i = a["italic"] { st.italic = i == "1" }
            if let v = num(a["before"]) { st.spaceBefore = v }
            if let v = num(a["after"]) { st.spaceAfter = v }
            if let f = a["family"] { st.family = f.replacingOccurrences(of: "_", with: " ") }
            if let al = a["align"] { st.alignment = ["l": .left, "c": .center, "r": .right, "j": .justified][al] ?? .natural }
            changeStyles(L("Modify Style"), reapply: id) { $0.replace(st) }
        case "deletestyle": deleteStyle(arg)
        case "sidebar":
            switch arg {
            case "styles": setSidebar(.styles)
            case "navigator": setSidebar(.navigator)
            default: setSidebar(nil)
            }
        case "charpanel": setSidebar(.styles); sidebar.styles.kind.selectedSegment = 1; sidebar.styles.reload()
        case "statusbar": setStatusBar(shown: arg != "off")
        case "viewmode": setViewMode(PagedView.ViewMode(rawValue: nums.first ?? 0) ?? .single)
        case "zoom": setZoom(CGFloat(Double(arg) ?? 1))
        case "field":
            let kinds = Self.fieldMenuKinds
            if let i = nums.first, kinds.indices.contains(i) { let item = NSMenuItem(); item.tag = i; insertFieldAction(item) }
            else if let k = DocField.Kind(rawValue: arg) { insertField(DocField(k)) }
        case "updatefields": updateFieldsAction(nil)
        case "outline": applyOutlineNumbering(preset: nums.first ?? 0)
        case "list":
            var o = ListOptions()
            let parts = arg.split(separator: " ").map(String.init)
            o.choice = Int(parts.first ?? "5") ?? 5
            if parts.count > 1 { o.prefix = parts[1] == "_" ? "" : parts[1] }
            if parts.count > 2 { o.suffix = parts[2] == "_" ? "" : parts[2] }
            o.restart = parts.contains("restart")
            o.showLevels = parts.contains("levels")
            if let s = parts.first(where: { $0.hasPrefix("start=") }) { o.start = Int(s.dropFirst(6)) ?? 1 }
            applyListOptions(o)
        case "nolist": removeList()
        case "demote": changeListLevel(by: 1, onlyAtStart: false)
        case "promote": changeListLevel(by: -1, onlyAtStart: false)
        case "tab": paged.textView(at: textView.selectedRange().location)?.insertTab(nil)
        case "shifttab": paged.textView(at: textView.selectedRange().location)?.insertBacktab(nil)
        case "newline": textView.insertNewline(nil)
        case "table": insertTable(rows: nums.first ?? 3, columns: nums.count > 1 ? nums[1] : 3, header: nums.count > 2 && nums[2] == 1)
        case "cell":   // cell row col: caret into a cell of the table at the caret
            if let g = tableGrid, nums.count > 1, let c = g.cell(row: nums[0], col: nums[1]) { textView.setSelectedRange(NSRange(location: c.range.location, length: 0)) }
        case "cells":  // cells r0 c0 r1 c1: select from one cell to another
            if let g = tableGrid, nums.count > 3, let a = g.cell(row: nums[0], col: nums[1]), let b = g.cell(row: nums[2], col: nums[3]) {
                textView.setSelectedRange(NSRange(location: a.range.location, length: NSMaxRange(b.range) - 1 - a.range.location))
            }
        case "merge": mergeCellsAction(nil)
        case "split":
            if let g = tableGrid, let me = g.cell(containing: textView.selectedRange().location) { splitCell(g, me, columns: nums.first ?? 0, rows: nums.count > 1 ? nums[1] : 0) }
        case "rowabove", "rowbelow", "delrow":
            let item = NSMenuItem(); item.tag = ["rowabove": 0, "rowbelow": 1, "delrow": 2][name]!; tableRowAction(item)
        case "colleft", "colright", "delcol":
            let item = NSMenuItem(); item.tag = ["colleft": 0, "colright": 1, "delcol": 2][name]!; tableColumnAction(item)
        case "sort": sortTable(column: nums.first ?? 0, descending: arg.contains("desc"), numbers: arg.contains("num"), header: arg.contains("header"))
        case "tableprops":
            let a = kv()
            var p = TableProperties()
            if let w = a["widths"] { p.widths = w.split(separator: ",").compactMap { Double($0) }.map { CGFloat($0) } }
            if let b = a["borders"] { p.borders = BorderPreset(rawValue: ["keep": 0, "all": 1, "outer": 2, "inner": 3, "none": 4][b] ?? 0) ?? .keep }
            if let w = num(a["line"]) { p.lineWidth = w }
            if let c = color(a["linecolor"]) { p.lineColor = c }
            if let v = num(a["padding"]) { p.padding = v }
            if let bg = a["bg"] { p.background = bg == "none" ? .some(nil) : .some(color(bg)) }
            if let v = a["valign"] { p.vertical = v == "middle" ? .middleAlignment : v == "bottom" ? .bottomAlignment : .topAlignment }
            if let h = a["header"].flatMap(Int.init) { p.headerRows = h }
            applyTableProperties(p)
        case "autoformat": applyAutoFormat(nums.first ?? 0)
        case "texttotable": textToTable(separator: arg == "tab" || arg.isEmpty ? "\t" : arg)
        case "tabletotext": tableToTextAction(nil)
        case "formula": insertField(DocField(.formula, argument: arg.isEmpty ? "SUM(ABOVE)" : arg))
        case "selimage": if let i = selectedImageIndex ?? (0..<storage.length).first(where: { storage.attribute(.attachment, at: $0, effectiveRange: nil) is NSTextAttachment && storage.attribute(.docNote, at: $0, effectiveRange: nil) == nil && storage.attribute(.docField, at: $0, effectiveRange: nil) == nil }) { textView.setSelectedRange(NSRange(location: i, length: 1)); selectImage(i) }
        case "wrap": if let i = selectedImageIndex { setImageWrap(i, DocImage.Wrap.allCases[max(0, min(4, nums.first ?? 0))]) }
        case "imgsize": if let i = selectedImageIndex, nums.count > 1 { resizeImage(i, to: NSSize(width: nums[0], height: nums[1])) }
        case "imgprops":
            let a = kv()
            if let i = selectedImageIndex {
                changeImage(i, name: L("Image Properties")) { att, p in
                    if let b = num(a["border"]) { p.borderWidth = b }
                    if let c = color(a["bordercolor"]) { p.borderColor = c }
                    if let c = a["crop"]?.split(separator: ",").compactMap({ Double($0) }), c.count == 4 { p.crop = NSEdgeInsets(top: CGFloat(c[1]), left: CGFloat(c[0]), bottom: CGFloat(c[3]), right: CGFloat(c[2])) }
                    if let x = num(a["x"]) { p.offset.x = x }
                    if let y = num(a["y"]) { p.offset.y = y }
                    _ = att
                }
            }
        case "image":   // image W H: a test picture at the caret
            insert(CLI.demoImage("picture", size: NSSize(width: nums.first ?? 160, height: nums.count > 1 ? nums[1] : 100)))
        case "caption": insertCaption(label: L("Figure"), text: arg)
        case "border":
            var o = BorderOptions()
            o.preset = nums.first ?? 1
            if arg.contains("bg") { o.background = NSColor(srgbRed: 1, green: 0.96, blue: 0.8, alpha: 1) }
            applyParagraphBorders(o)
        case "dropcap": setDropCap((nums.first ?? 0) > 1 ? nums.first : nil)
        case "char":
            let a = kv()
            var o = CharacterOptions()
            if let f = a["family"] { o.family = f.replacingOccurrences(of: "_", with: " ") }
            o.size = num(a["size"])
            if let b = a["bold"] { o.bold = b == "1" }
            if let c = a["caps"] { o.allCaps = c == "1" }
            if let c = a["smallcaps"] { o.smallCaps = c == "1" }
            if let p = a["pos"] { o.position = Int(p) }
            o.kern = num(a["kern"])
            if let u = a["underline"] { o.underline = Self.underlineChoices[Int(u) ?? 0].1 }
            if let st = a["strike"] { o.strike = Int(st) }
            if let c = color(a["color"]) { o.color = c }
            if let h = a["highlight"] { o.highlight = h == "none" ? .some(nil) : .some(color(h)) }
            applyCharacterOptions(o)
        case "watermark": setWatermark(arg)
        case "background": setPageBackground(arg == "none" ? nil : color(arg))
        case "hyphen": toggleHyphenationAction(nil)
        case "editheader", "editfooter":
            if let card = paged.card(at: textView.selectedRange().location) { beginHeaderEdit(card, header: name == "editheader") }
        case "htype": headerEditor?.textView.insertText(arg, replacementRange: headerEditor!.textView.selectedRange())
        case "hfield": if let k = DocField.Kind(rawValue: arg), let hv = headerEditor?.textView { hv.insertText(Fields.reference(DocField(k), attributes: hv.typingAttributes), replacementRange: hv.selectedRange()) }
        case "endheader": headerEditor?.commit(); window?.makeFirstResponder(textView)
        case "hfoptions":
            let a = kv()
            var o = HeaderFooterOptions()
            o.header = a["header"] != "0"
            o.footer = a["footer"] != "0"
            o.headerText = a["htext"]?.replacingOccurrences(of: "_", with: " ")
            o.footerText = a["ftext"]?.replacingOccurrences(of: "_", with: " ")
            o.differentFirst = a["first"] == "1"
            o.differentOddEven = a["evenodd"] == "1"
            o.pageNumbers = Int(a["numbers"] ?? "-1") ?? -1
            o.sectionOnly = a["section"] == "1"
            applyHeaderFooterOptions(o)
        case "pagenumbers": togglePageNumbersAction(nil)
        case "props":
            let parts = arg.split(separator: "|").map(String.init)
            setDocInfo(DocInfo(title: parts.first ?? "", author: parts.count > 1 ? parts[1] : ""))
        case "bookmark": addBookmark(arg)
        case "movechapter": if nums.count > 1 { moveChapter(from: nums[0], to: nums[1]) }
        case "pdf":
            var o = PDFOptions()
            let parts = arg.split(separator: " ").map(String.init)
            if parts.count > 1 { o.pages = parts[1] }
            if let d = try? doc?.pdfData(o) { try? d.write(to: URL(fileURLWithPath: parts[0])) }
        case "dumpnav":
            for g in NavigatorPanel.items(storage, sheet: doc?.styles) where !g.children.isEmpty {
                func show(_ i: NavItem, _ d: Int) { print(String(repeating: "  ", count: d) + "nav \(i.title)"); i.children.forEach { show($0, d + 1) } }
                show(g, 0)
            }
        case "dumpstyle":
            let a = currentAttributes
            print("STYLE", a[Styles.styleKey] as? String ?? "Normal", (a[.font] as? NSFont).map { "\($0.familyName ?? "") \($0.pointSize)" } ?? "", (a[.foregroundColor] as? NSColor).map(CLI.hex) ?? "")
        case "selectall": textView.setSelectedRange(NSRange(location: 0, length: storage.length))
        case "line":   // line N: caret to the start of paragraph N (from 0)
            let ns = storage.string as NSString
            var k = 0, at = 0
            ns.enumerateSubstrings(in: NSRange(location: 0, length: ns.length), options: [.byParagraphs, .substringNotRequired]) { _, _, r, stop in
                if k == (nums.first ?? 0) { at = r.location; stop.pointee = true }
                k += 1
            }
            textView.setSelectedRange(NSRange(location: at, length: 0))
        default: return false
        }
        return true
    }
}
