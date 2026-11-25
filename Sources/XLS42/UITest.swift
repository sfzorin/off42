import AppKit
import Kit42

/// `XLS42 --uitest`: drives a real window through the controller and checks the results.
enum UITest {
    static func run() {
        setbuf(stdout, nil)
        let doc = SheetDocument()
        NSDocumentController.shared.addDocument(doc)
        doc.makeWindowControllers()
        doc.showWindows()
        guard let w = doc.editor else { print("FAIL no window"); exit(1) }
        doc.undoManager?.groupsByEvent = false   // one undo step per change, as with separate user actions
        // The tests copy and paste: keep the user's clipboard and put it back afterwards.
        let pasteboard = SheetWindowController.pasteboard
        let savedClipboard: [[(NSPasteboard.PasteboardType, Data)]] = (pasteboard.pasteboardItems ?? []).map { item in
            item.types.compactMap { t in item.data(forType: t).map { (t, $0) } }
        }
        func restoreClipboard() {
            pasteboard.clearContents()
            let items = savedClipboard.map { pairs -> NSPasteboardItem in
                let item = NSPasteboardItem()
                for (t, d) in pairs { item.setData(d, forType: t) }
                return item
            }
            if !items.isEmpty { pasteboard.writeObjects(items) }
        }
        var failed = 0
        func check(_ name: String, _ ok: Bool, _ detail: @autoclosure () -> String = "") {
            print(ok ? "ok  " : "FAIL", name, ok ? "" : detail())
            if !ok { failed += 1 }
        }
        func cell(_ a1: String) -> Cell { w.sheet[CellRef(a1)!] }
        func select(_ a1: String, to b1: String? = nil) {
            w.grid.select(CellRef(a1)!)
            if let b1 { w.grid.select(CellRef(b1)!, extend: true) }
        }

        w.setInput("10", at: CellRef("A1")!)
        w.setInput("20", at: CellRef("A2")!)
        w.setInput("=сумм(A1:A2)", at: CellRef("A3")!)
        check("formula with Russian name", cell("A3").value == .number(30) && cell("A3").input == "=SUM(A1:A2)", "\(cell("A3"))")
        w.setInput("15%", at: CellRef("C1")!)
        check("percent input", cell("C1").value == .number(0.15) && cell("C1").style.numberFormat == "0%")
        w.setInput("31.12.2024", at: CellRef("C2")!)
        check("date input", cell("C2").style.numberFormat == "dd.mm.yyyy" && NumberFormat.format(cell("C2").value, "dd.mm.yyyy").text == "31.12.2024")

        select("A3"); w.copy(nil); select("B3"); w.paste(nil)
        check("paste shifts formula", cell("B3").input == "=SUM(B1:B2)", cell("B3").input)

        select("A1"); w.insertRowsAction(nil)
        check("insert row adjusts refs", cell("A4").input == "=SUM(A2:A3)" && cell("A4").value == .number(30), cell("A4").input)
        doc.undoManager?.undo()
        check("undo insert", cell("A3").input == "=SUM(A1:A2)", cell("A3").input)

        select("A1"); w.toggleBold(nil)
        check("bold", cell("A1").style.bold)
        doc.undoManager?.undo()
        check("undo bold", !cell("A1").style.bold)

        w.freezeTopRowAction(nil)
        check("freeze top row", w.sheet.frozenRows == 1 && w.sheet.frozenCols == 0)
        select("B2"); w.freezeAction(nil)   // toggles off
        check("unfreeze", w.sheet.frozenRows == 0)
        w.freezeAction(nil)
        check("freeze at B2", w.sheet.frozenRows == 1 && w.sheet.frozenCols == 1)

        w.setInput("3", at: CellRef("D1")!); w.setInput("1", at: CellRef("D2")!); w.setInput("2", at: CellRef("D3")!)
        select("D1", to: "D3"); w.sortAscending(nil)
        check("sort", [cell("D1").value, cell("D2").value, cell("D3").value] == [.number(1), .number(2), .number(3)])

        w.setInput("=D1*2", at: CellRef("E1")!)
        select("E1", to: "E3"); w.fillDown(nil)
        check("fill down", cell("E3").input == "=D3*2" && cell("E3").value == .number(6), cell("E3").input)

        select("A5", to: "C5"); w.mergeAction(nil)
        check("merge", w.sheet.merges.contains(CellRange(CellRef("A5")!, CellRef("C5")!)))

        let sourceSheet = w.sheet.name.replacingOccurrences(of: "'", with: "''")
        w.addSheet(nil)
        w.setInput("='\(sourceSheet)'!A3*2", at: CellRef("A1")!)
        check("cross-sheet ref (new sheet)", cell("A1").value == .number(60), "\(cell("A1").value)")

        w.selectSheet(0)
        // Fill handle: series and text increments
        w.setInput("1", at: CellRef("G1")!); w.setInput("3", at: CellRef("G2")!)
        w.autoFill(from: CellRange(CellRef("G1")!, CellRef("G2")!), to: CellRange(CellRef("G1")!, CellRef("G5")!))
        check("fill series", cell("G5").value == .number(9), "\(cell("G5").value)")
        w.setInput("Item 1", at: CellRef("H1")!)
        w.autoFill(from: CellRange(CellRef("H1")!, CellRef("H1")!), to: CellRange(CellRef("H1")!, CellRef("H3")!))
        check("fill text increment", cell("H3").value == .string("Item 3"), "\(cell("H3").value)")

        // Hide rows
        select("A7", to: "A8"); w.hideRowsAction(nil)
        check("hide rows", w.sheet.isRowHidden(6) && w.sheet.isRowHidden(7))
        select("A6", to: "A9"); w.unhideRowsAction(nil)
        check("unhide rows", !w.sheet.isRowHidden(6))

        // Filter
        w.setInput("x", at: CellRef("J1")!); w.setInput("a", at: CellRef("J2")!); w.setInput("b", at: CellRef("J3")!); w.setInput("a", at: CellRef("J4")!)
        select("J1", to: "J4"); w.toggleFilterAction(nil)
        w.applyFilter(column: 9, allowed: ["a"])
        check("filter hides rows", w.sheet.filterHidden == [2], "\(w.sheet.filterHidden)")

        // Names, comments, rules, chart, print area
        let sheetName = w.sheet.name
        doc.change("names") { $0.names["Rate"] = "\(sheetName)!$A$1" }
        w.setInput("=Rate*3", at: CellRef("K1")!)
        check("defined name", cell("K1").value == .number(30), "\(cell("K1").value)")
        doc.change("comment") { $0.sheets[0].comments[CellRef("A1")!] = "note" }
        doc.change("rule") { $0.sheets[0].conditionalFormats.append(ConditionalFormat(range: CellRange("A1:A3")!, kind: .cellIs(op: "greaterThan", values: ["15"]), fill: .red)) }
        select("D1", to: "E3"); w.insertChartAction(nil)
        check("chart inserted", w.sheet.charts.count == 1 && w.sheet.charts[0].series.count >= 1)
        select("A1", to: "E5"); w.setPrintAreaAction(nil)
        check("print area", w.sheet.printArea == CellRange("A1:E5"))

        let pdf = SheetExport.pdf(doc.workbook, activeSheet: 0, selection: nil, options: .init(), base: NSPrintInfo.shared)
        check("PDF export", pdf.count > 1000 && String(decoding: pdf.prefix(5), as: UTF8.self) == "%PDF-")
        let xlsx = XLSX.write(doc.workbook)
        let back = try? XLSX.read(xlsx)
        check("xlsx round trip", back?.sheets.count == 2 && back?.sheets[0].frozenRows == 1 && back?.sheets[0].merges.count == 1)
        let s0 = back?.sheets[0]
        check("xlsx extras round trip", s0?.comments.count == 1 && s0?.conditionalFormats.count == 1 && s0?.charts.count == 1
              && s0?.autoFilter != nil && s0?.printArea == CellRange("A1:E5") && back?.names["Rate"] != nil,
              "comments \(s0?.comments.count ?? -1) cf \(s0?.conditionalFormats.count ?? -1) charts \(s0?.charts.count ?? -1) filter \(String(describing: s0?.autoFilter)) area \(String(describing: s0?.printArea))")

        failed += features(w, doc)
        failed += data(w, doc)
        failed += filesAndArrays(w, doc)
        failed += interactions(w, doc)
        failed += deepEditing(w, doc)
        failed += deepCommon(w, doc)
        failed += deepSheetReferences(w, doc)
        failed += technicalLabels(w, doc)
        restoreClipboard()
        print(failed == 0 ? "ALL PASSED" : "\(failed) FAILED")
        exit(failed == 0 ? 0 : 1)
    }
}
