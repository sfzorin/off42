import AppKit

enum ScanDocumentAudit {
    static func run() -> Int {
        var count = 0, failures = 0
        func check(_ value: Bool, _ name: String) {
            count += 1; if !value { failures += 1 }
            print(value ? "PASS" : "FAIL", "SCN document:", name)
        }
        func page(_ n: Int) -> ScanPage {
            var bitmap = Bitmap(width: 80 + n * 10, height: 120, channels: 1, fill: 245)
            for y in 12..<35 { for x in 8..<30 { bitmap.data[y * bitmap.width + x] = UInt8(n * 30) } }
            let image = bitmap.cgImage()!
            var options = ProcessOptions(); options.crop = false; options.deskew = false; options.autoRotate = false; options.enhancement = .none
            var report = ProcessReport(); report.outputSize = CGSize(width: image.width, height: image.height); report.ink = 0.1
            return ScanPage(base: image, image: image, dpi: 144, options: options, report: report)
        }
        let doc = ScanDocument(), a = page(0), b = page(1), c = page(2)
        var selected = [UUID]()
        doc.captureSelection = { selected }; doc.restoreSelection = { selected = $0 }
        doc.append([])
        check(doc.pages.isEmpty && !doc.undo.canUndo && !doc.isDirty && doc.revision == 0, "empty append has no Undo or dirty state")
        doc.append([a, b, c]); selected = [b.id]
        check(doc.pages.map(\.id) == [a.id, b.id, c.id], "append retains page order")
        doc.markSaved(revision: doc.revision); doc.undo.removeAllActions()
        let savedRevision = doc.revision
        doc.append([b]); doc.move([], to: 0); doc.move(IndexSet(integer: 999), to: 0)
        doc.modify([], name: "Empty") { $0.rotate(clockwise: 90) }
        doc.modify([b], name: "Unchanged") { _ in }
        doc.setOrder(doc.pages, name: "Unchanged")
        check(doc.revision == savedRevision && !doc.isDirty && !doc.undo.canUndo, "duplicate/invalid/unchanged mutations remain no-ops")
        doc.move(IndexSet(integer: 1), to: 3)
        check(doc.pages.map(\.id) == [a.id, c.id, b.id], "move uses pre-removal destination coordinates")
        doc.undo.undo()
        check(doc.pages.map(\.id) == [a.id, b.id, c.id] && selected == [b.id] && !doc.isDirty, "move Undo restores order, selection and saved state")
        doc.undo.redo()
        check(doc.pages.map(\.id) == [a.id, c.id, b.id] && doc.isDirty, "move Redo restores the changed order")
        doc.undo.undo(); doc.undo.removeAllActions()

        doc.remove(ids: [a.id, c.id]); selected = [b.id]
        check(doc.pages.map(\.id) == [b.id], "noncontiguous removal retains the other page")
        doc.undo.undo()
        check(doc.pages.map(\.id) == [a.id, b.id, c.id] && !doc.isDirty, "noncontiguous removal Undo restores every original position")
        doc.undo.redo(); check(doc.pages.map(\.id) == [b.id], "removal Redo removes the same page identities")
        doc.undo.undo(); doc.undo.removeAllActions()
        let missing = page(3), revision = doc.revision
        doc.replace(missing, with: [page(4)])
        check(doc.revision == revision && doc.pages.count == 3 && !doc.undo.canUndo, "late rescan cannot append a replacement for a removed target")

        let text = [TextLine(text: "Original text", box: CGRect(x: 0.1, y: 0.2, width: 0.6, height: 0.1))]
        doc.setText(b, text); doc.markSaved(revision: doc.revision)
        let before = b.snapshot, contentRevision = b.contentRevision
        doc.modify([b, b], name: "Rotate") { $0.rotate(clockwise: 90) }
        check(b.image.width == before.image.height && b.image.height == before.image.width, "duplicate targets rotate only once")
        check(b.text == nil && b.report.outputSize == CGSize(width: b.image.width, height: b.image.height), "rotation invalidates OCR and updates geometry report")
        check(b.contentRevision != contentRevision, "pixel mutation changes its content revision")
        doc.undo.undo()
        check(b.snapshot.hasSameContent(as: before) && !doc.isDirty, "rotation Undo restores exact pixels/options/DPI/report/text and clean state")
        check(b.contentRevision != contentRevision, "Undo to identical pixels still invalidates an old worker")
        let undoRevision = doc.revision
        check(!doc.setText(b, [TextLine(text: "Stale", box: .zero)], expectedContentRevision: contentRevision), "OCR from a pre-Undo generation is rejected")
        check(doc.revision == undoRevision && b.text == text, "rejected OCR changes neither document nor prior text")
        doc.undo.redo(); check(b.text == nil && doc.isDirty, "rotation Redo returns to transformed content")
        doc.undo.undo(); doc.undo.removeAllActions()
        doc.modify([b], name: "Complete state") { p in p.dpi = 600; p.report.blank = true; p.options.contrast = 20 }
        doc.undo.undo()
        check(b.snapshot.hasSameContent(as: before), "Undo restores DPI and processing diagnostics as well as pixels")
        doc.undo.redo(); check(b.dpi == 600 && b.report.blank && b.options.contrast == 20, "Redo restores complete metadata state")
        doc.undo.undo(); doc.undo.removeAllActions()
        let copy = b.copy()
        check(copy.id != b.id && copy.snapshot.hasSameContent(as: b.snapshot), "split copy has independent identity and complete content")
        copy.rotate(clockwise: 90)
        check(b.snapshot.hasSameContent(as: before) && copy.image.width != b.image.width, "editing a split copy cannot mutate the original page")
        let revisionBeforeNoopOCR = doc.revision
        doc.setText(b, text)
        check(doc.revision == revisionBeforeNoopOCR && !doc.isDirty, "identical OCR results do not dirty a saved page")
        let removedRevision = b.contentRevision
        doc.remove(ids: [b.id]); doc.markSaved(revision: doc.revision)
        check(!doc.setText(b, text, expectedContentRevision: removedRevision) && !doc.isDirty, "removed pages cannot receive a late OCR result")
        doc.undo.undo(); doc.markSaved(revision: doc.revision); doc.undo.removeAllActions()
        doc.move(IndexSet(integer: 1), to: doc.pages.count)
        let newText = [TextLine(text: "New OCR while reordered", box: CGRect(x: 0, y: 0, width: 1, height: 0.1))]
        doc.setText(b, newText)
        doc.undo.undo()
        check(doc.pages.map(\.id) == [a.id, b.id, c.id] && b.text == newText && doc.isDirty,
              "Undo of ordering cannot mark newly recognized unsaved text as clean")
        doc.markSaved(revision: doc.revision); doc.undo.removeAllActions()
        doc.move(IndexSet(integer: 1), to: doc.pages.count)
        let saving = doc.captureSavedContents(doc.pages)
        doc.undo.undo(); check(!doc.isDirty, "Undo before a pending save returns to the previous disk baseline")
        doc.markSaved(contents: saving)
        check(doc.isDirty && doc.pages.map(\.id) == [a.id, b.id, c.id], "a newer save completion invalidates the previous clean baseline after Undo")
        doc.undo.redo()
        check(!doc.isDirty && doc.pages.map(\.id) == [a.id, c.id, b.id], "Redo to the actual saved snapshot becomes clean")
        print("SCN42 document regressions: \(count - failures)/\(count) passed")
        return failures
    }
}
