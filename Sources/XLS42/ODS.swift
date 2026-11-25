import AppKit
import Kit42

/// OpenDocument Spreadsheet (.ods, LibreOffice/OpenOffice): reading and writing.
/// Values, formulas (OpenFormula ↔ Excel syntax), cell styles, number formats, column widths,
/// row heights, merges, freeze panes, comments, print ranges and named ranges.
enum ODS {
    static func xmlEscape(_ text: String) -> String { XLSX.xmlEscape(text) }

    // MARK: - Reading

    struct StyleInfo {
        var style = CellStyle()
        var columnWidth: CGFloat?
        var rowHeight: CGFloat?
        var dataStyle: String?
        var parent: String?
        var element: XMLElement?
    }

    static func read(_ data: Data) throws -> Workbook {
        let zip = try ZipArchive(data: data)
        func xml(_ path: String) -> XMLElement? {
            guard let d = zip[path] else { return nil }
            return (try? XMLDocument(data: d, options: [.nodePreserveWhitespace]))?.rootElement()
        }
        guard let content = xml("content.xml"), let spreadsheet = content.child("body")?.child("spreadsheet") else { throw ZipError.corrupt }
        let stylesRoot = xml("styles.xml")

        // Repetition counts are dimensions, never signed offsets. Reject invalid
        // documents before constructing Swift ranges or overflowing coordinates.
        func count(_ element: XMLElement, _ attribute: String, maximum: Int) throws -> Int {
            guard maximum > 0 else { throw ZipError.corrupt }
            guard let raw = element.attr(attribute) else { return 1 }
            guard let value = Int(raw), value > 0, value <= maximum else { throw ZipError.corrupt }
            return value
        }
        let maxRows = 1_048_576, maxCols = 16_384
        var expandedCells = 0

        // Styles: common (styles.xml) then automatic (content.xml); number styles from both.
        var numberFormats: [String: String] = [:]
        var styles: [String: StyleInfo] = [:]
        var commonCellStyles: [String] = []
        for container in [stylesRoot?.child("styles"), stylesRoot?.child("automatic-styles"), content.child("automatic-styles")] {
            guard let container else { continue }
            for e in (container.children ?? []).compactMap({ $0 as? XMLElement }) {
                if let name = e.attr("name"), let code = numberFormat(e) { numberFormats[name] = code }
                if e.localName == "style", let name = e.attr("name") {
                    styles[name] = styleInfo(e)
                    // Named (common) cell styles: their name stays on the cells (Format › Cell Styles).
                    if container === stylesRoot?.child("styles"), e.attr("family") == "table-cell", name != "Default" {
                        styles[name]?.style.styleName = CellStyles.internalName(e.attr("display-name") ?? name)
                        commonCellStyles.append(name)
                    }
                }
                if e.localName == "default-style", e.attr("family") == "table-cell" { styles["__default"] = styleInfo(e) }
            }
        }
        // Table styles with table:display="false" hide their sheet.
        var hiddenTableStyles = Set<String>(), tabColors: [String: NSColor] = [:]
        for container in [stylesRoot?.child("automatic-styles"), content.child("automatic-styles")] {
            for e in container?.children("style") ?? [] where e.attr("family") == "table" {
                guard let n = e.attr("name"), let p = e.child("table-properties") else { continue }
                if p.attr("display") == "false" { hiddenTableStyles.insert(n) }
                if let c = color(p.attr("tab-color")) { tabColors[n] = c }
            }
        }
        func resolved(_ name: String?) -> CellStyle {
            var chain: [StyleInfo] = []
            var n = name
            var seen = Set<String>()
            while let k = n, let s = styles[k], !seen.contains(k) { seen.insert(k); chain.insert(s, at: 0); n = s.parent }
            var out = styles["__default"]?.style ?? CellStyle()
            for s in chain {
                if let element = s.element { out = styleInfo(element, base: out).style }
                if let name = s.style.styleName { out.styleName = name }
            }
            if let ds = chain.last(where: { $0.dataStyle != nil })?.dataStyle, let code = numberFormats[ds] { out.numberFormat = code }
            return out
        }
        var styleCache: [String: CellStyle] = [:]
        func cellStyle(_ name: String?) -> CellStyle {
            let key = name ?? ""
            if let s = styleCache[key] { return s }
            let s = resolved(name)
            styleCache[key] = s
            return s
        }

        var wb = Workbook()
        wb.sheets = []
        wb.date1904 = spreadsheet.child("calculation-settings")?.child("null-date")?.attr("date-value") == "1904-01-01"
        for name in commonCellStyles {
            let s = cellStyle(name)
            if let n = s.styleName, wb.cellStyles[n] == nil { wb.cellStyles[n] = s }
        }
        wb.structureProtected = spreadsheet.attr("structure-protected") == "true"
        let validationRules = readValidations(spreadsheet)
        for table in spreadsheet.children("table") {
            var sheet = Sheet(name: table.attr("name") ?? "Sheet\(wb.sheets.count + 1)")
            if let st = table.attr("style-name") {
                sheet.hidden = hiddenTableStyles.contains(st)
                sheet.tabColor = tabColors[st]
            }
            if table.attr("protected") == "true" {
                // LibreOffice may chain two digests (legacy Excel hash, then SHA-1).
                let digests = [table.attr("protection-key-digest-algorithm") ?? sha1Digest, table.attr("protection-key-digest-algorithm-2")]
                sheet.protection = SheetProtection(odsKey: table.attr("protection-key"), odsDigest: digests.compactMap { $0 }.joined(separator: " "))
                if digests[1] == nil, digests[0] == legacyExcelDigest, let raw = table.attr("protection-key").flatMap({ Data(base64Encoded: $0) }), raw.count == 2 {
                    sheet.protection?.passwordHash = String(format: "%02X%02X", raw[0], raw[1])
                }
            }
            var validationCells: [String: [CellRef]] = [:]
            var matrixRanges: [CellRange] = []
            var col = 0
            var columnDefault: [Int: String] = [:]
            for (c, level) in outlined(table, "table-column", group: "table-column-group") {
                let n = try count(c, "number-columns-repeated", maximum: maxCols - col)
                let w = c.attr("style-name").flatMap { styles[$0]?.columnWidth }
                let hidden = c.attr("visibility") == "collapse"
                let defaultStyle = c.attr("default-cell-style-name").flatMap { $0 == "Default" ? nil : $0 }
                for k in 0..<n {
                    if level > 0 { sheet.colLevels[col + k] = min(level, Sheet.maxOutlineLevel) }
                    if hidden && level > 0 {
                        sheet.outlineHiddenCols.insert(col + k)
                        if let w, abs(w - sheet.defaultColumnWidth) > 0.5 { sheet.columnWidths[col + k] = w }
                    }
                    else if hidden { sheet.columnWidths[col + k] = 0 }
                    else if let w, abs(w - sheet.defaultColumnWidth) > 0.5 { sheet.columnWidths[col + k] = w }
                    if let defaultStyle { columnDefault[col + k] = defaultStyle }
                }
                col += n
            }
            var row = 0
            for (r, level) in outlined(table, "table-row", group: "table-row-group") {
                let repeatRows = try count(r, "number-rows-repeated", maximum: maxRows - row)
                let cells = r.children ?? []
                // Even an otherwise empty row can contain invalid repetition
                // counts. Validate before skipping it, including covered cells.
                var declaredCol = 0
                for case let cell as XMLElement in cells where cell.localName == "table-cell" || cell.localName == "covered-table-cell" {
                    let n = try count(cell, "number-columns-repeated", maximum: maxCols - declaredCol)
                    if cell.localName == "table-cell" {
                        for attribute in ["number-columns-spanned", "number-matrix-columns-spanned"] {
                            _ = try count(cell, attribute, maximum: maxCols - declaredCol - n + 1)
                        }
                        for attribute in ["number-rows-spanned", "number-matrix-rows-spanned"] {
                            _ = try count(cell, attribute, maximum: maxRows - row - repeatRows + 1)
                        }
                    }
                    declaredCol += n
                }
                let rowHeight = r.attr("style-name").flatMap { styles[$0]?.rowHeight }
                let collapsed = r.attr("visibility") == "collapse"
                if r.attr("visibility") == "filter" { for k in 0..<repeatRows { sheet.filterHidden.insert(row + k) } }
                if level > 0 {
                    for k in 0..<repeatRows {
                        sheet.rowLevels[row + k] = min(level, Sheet.maxOutlineLevel)
                        if collapsed { sheet.outlineHiddenRows.insert(row + k) }
                    }
                }
                let hasContent = cells.contains { ($0 as? XMLElement).map {
                    $0.attr("value-type") != nil || $0.attr("formula") != nil || $0.attr("style-name") != nil || $0.attr("number-columns-spanned") != nil || $0.attr("number-rows-spanned") != nil || $0.attr("content-validation-name") != nil || !($0.children ?? []).isEmpty } ?? false }
                if !hasContent {
                    if collapsed && level == 0 { for k in 0..<repeatRows { sheet.rowHeights[row + k] = 0 } }
                    else if let rowHeight, abs(rowHeight - sheet.defaultRowHeight) > 0.6 {
                        for k in 0..<repeatRows { sheet.rowHeights[row + k] = rowHeight }
                    }
                    row += repeatRows
                    continue
                }
                for k in 0..<repeatRows {
                    let rr = row + k
                    if collapsed && level == 0 { sheet.rowHeights[rr] = 0 }
                    else if let rowHeight, abs(rowHeight - sheet.defaultRowHeight) > 0.6 { sheet.rowHeights[rr] = rowHeight }
                    var c = 0
                    for case let cellXML as XMLElement in cells {
                        guard cellXML.localName == "table-cell" || cellXML.localName == "covered-table-cell" else { continue }
                        let repeatCols = try count(cellXML, "number-columns-repeated", maximum: maxCols - c)
                        guard cellXML.localName == "table-cell" else {
                            for kc in 0..<repeatCols { sheet.images += try readImages(cellXML, anchor: CellRef(row: rr, col: c + kc), zip: zip) }
                            guard sheet.images.count <= 10_000 else { throw ZipError.corrupt }
                            c += repeatCols; continue
                        }
                        if let name = cellXML.attr("content-validation-name"), validationRules[name] != nil {
                            for kc in 0..<repeatCols { validationCells[name, default: []].append(CellRef(row: rr, col: c + kc)) }
                        }
                        let (value, formula) = cellValue(cellXML, date1904: wb.date1904)
                        let comment = cellXML.child("annotation").map { a in
                            a.children("p").map(\.allTextODF).joined(separator: "\n")
                        }
                        let explicit = cellXML.attr("style-name") ?? r.attr("default-cell-style-name")
                        // Blank formatted cells are meaningful: number formats and
                        // unlocked input areas must survive a save/reopen too.
                        let isEmpty = value.isEmpty && formula == nil && comment == nil
                        for kc in 0..<repeatCols {
                            let ref = CellRef(row: rr, col: c + kc)
                            sheet.images += try readImages(cellXML, anchor: ref, zip: zip)
                            guard sheet.images.count <= 10_000 else { throw ZipError.corrupt }
                            let style = cellStyle(explicit ?? columnDefault[c + kc])
                            if isEmpty && style == cellStyle(nil) { continue }
                            expandedCells += 1
                            guard expandedCells <= 1_000_000 else { throw ZipError.corrupt }
                            var cell = Cell()
                            cell.style = style
                            cell.value = value
                            if let formula {
                                cell.input = formula
                                if cellXML.attr("number-matrix-columns-spanned") != nil || cellXML.attr("number-matrix-rows-spanned") != nil {
                                    let cs = try count(cellXML, "number-matrix-columns-spanned", maximum: maxCols - ref.col)
                                    let rs = try count(cellXML, "number-matrix-rows-spanned", maximum: maxRows - ref.row)
                                    cell.array = true
                                    let range = CellRange(ref, CellRef(row: ref.row + max(1, rs) - 1, col: ref.col + max(1, cs) - 1))
                                    if cs * rs > 1 { matrixRanges.append(range) }
                                    // ODF matrices are fixed ranges, like Excel's Ctrl+Shift+Enter formulas
                                    // (dynamic-array functions keep spilling to their natural size).
                                    if !Calculator.isDynamic(formula) { sheet.arrays[ref] = range }
                                }
                            } else {
                                cell.input = XLSX.inputText(for: value, style: style, date1904: wb.date1904)
                                if case .string(let s) = value, XLSX.constant(s) != .string(s) || s.hasPrefix("=") { cell.input = "'" + s }
                            }
                            sheet[ref] = cell
                            if let comment { sheet.comments[ref] = comment }
                            if let a = cellXML.children("p").lazy.compactMap({ $0.child("a") }).first, var href = a.attr("href") {
                                if href.hasPrefix("#") { href = "#" + Self.placeFromODF(String(href.dropFirst())) }
                                sheet.hyperlinks[ref] = href
                            }
                        }
                        let colSpan = try count(cellXML, "number-columns-spanned", maximum: maxCols - c)
                        let rowSpan = try count(cellXML, "number-rows-spanned", maximum: maxRows - rr)
                        if colSpan > 1 || rowSpan > 1 {
                            sheet.merges.append(CellRange(CellRef(row: rr, col: c), CellRef(row: rr + rowSpan - 1, col: c + colSpan - 1)))
                        }
                        c += repeatCols
                    }
                }
                row += repeatRows
            }
            for r in matrixRanges {
                for ref in sheet.cells.keys.filter({ $0 != r.start && r.contains($0) }) {
                    if var c = sheet.cells[ref], !c.isFormula { c.input = ""; sheet.cells[ref] = c }
                }
            }
            matrixRanges = []
            for (name, refs) in validationCells.sorted(by: { $0.key < $1.key }) {
                guard var v = validationRules[name] else { continue }
                v.ranges = ranges(refs)
                sheet.validations.append(v)
            }
            // Sheet-local names: the Solver model (LibreOffice keeps Excel's hidden solver_… names here).
            var solverNames: [String: String] = [:]
            for case let e as XMLElement in table.child("named-expressions")?.children ?? [] {
                guard let name = e.attr("name"), SolverFiles.isSolverName(name), let text = solverText(e) else { continue }
                solverNames[name] = text
            }
            if !solverNames.isEmpty { sheet.solver = SolverFiles.model(solverNames, sheet: sheet.name) }
            if let ranges = table.attr("print-ranges") {
                sheet.printArea = rangeFromODF(addresses(ranges).first ?? "").flatMap { CellRange($0.range.replacingOccurrences(of: "$", with: "")) }
            }
            wb.sheets.append(sheet)
        }
        // Named ranges
        var globalSolver: [String: String] = [:]
        for case let n as XMLElement in spreadsheet.child("named-expressions")?.children ?? [] {
            if let name = n.attr("name"), SolverFiles.isSolverName(name) { if let t = solverText(n) { globalSolver[name] = t }; continue }
            guard let name = n.attr("name") else { continue }
            if n.localName == "named-range", let addr = n.attr("cell-range-address"), let r = rangeFromODF(addr) {
                wb.names[name] = (r.sheet.map { Formula.sheetPrefix($0) } ?? "") + r.range
            } else if n.localName == "named-expression", let expression = n.attr("expression") {
                wb.names[name] = String(excelFormula(expression).dropFirst())
            }
        }
        if let opt = globalSolver["solver_opt"], let bang = opt.lastIndex(of: "!") {
            var name = String(opt[..<bang])
            if name.hasPrefix("'") { name = String(name.dropFirst().dropLast()).replacingOccurrences(of: "''", with: "'") }
            if let si = wb.sheetIndex(named: name), wb.sheets[si].solver == nil { wb.sheets[si].solver = SolverFiles.model(globalSolver, sheet: name) }
        }
        readDatabaseRanges(spreadsheet, &wb)
        PivotFiles.readODS(spreadsheet, &wb)
        // Freeze panes and gridlines from settings.xml
        if let settings = xml("settings.xml") {
            for map in descendantsNamed(settings, "config-item-map-named") where map.attr("name") == "Tables" {
                for entry in map.children("config-item-map-entry") {
                    guard let name = entry.attr("name"), let i = wb.sheetIndex(named: name) else { continue }
                    var items: [String: String] = [:]
                    for item in entry.children("config-item") { if let k = item.attr("name") { items[k] = item.stringValue } }
                    if items["HorizontalSplitMode"] == "2" { wb.sheets[i].frozenCols = Int(items["HorizontalSplitPosition"] ?? "0") ?? 0 }
                    if items["VerticalSplitMode"] == "2" { wb.sheets[i].frozenRows = Int(items["VerticalSplitPosition"] ?? "0") ?? 0 }
                    // Split window (mode 1): positions in pixels, panes' first column/row in Position….
                    let hx = items["HorizontalSplitMode"] == "1" ? Double(items["HorizontalSplitPosition"] ?? "") ?? 0 : 0
                    let vy = items["VerticalSplitMode"] == "1" ? Double(items["VerticalSplitPosition"] ?? "") ?? 0 : 0
                    if wb.sheets[i].frozenRows == 0, wb.sheets[i].frozenCols == 0, hx > 0 || vy > 0 {
                        func n(_ k: String) -> Int { max(0, Int(items[k] ?? "") ?? 0) }
                        let pane = Int(items["ActiveSplitRange"] ?? "").flatMap { $0 >= 0 && $0 < 4 ? SheetSplit.odsPanes[$0] : nil }
                        wb.sheets[i].split = SheetSplit.from(x: hx, y: vy, topLeft: CellRef(row: vy > 0 ? n("PositionTop") : n("PositionBottom"), col: n("PositionLeft")),
                                                             paneTopLeft: CellRef(row: n("PositionBottom"), col: hx > 0 ? n("PositionRight") : n("PositionLeft")), activePane: pane)
                    }
                    if items["ShowGrid"] == "false" { wb.sheets[i].showGridlines = false }
                    if let tab = items["TabColor"].flatMap(Int.init), tab >= 0, tab <= 0xFFFFFF {
                        let rgb = tab & 0xFFFFFF
                        wb.sheets[i].tabColor = NSColor(srgbRed: CGFloat(rgb >> 16) / 255, green: CGFloat((rgb >> 8) & 0xFF) / 255, blue: CGFloat(rgb & 0xFF) / 255, alpha: 1)
                    }
                }
            }
        }
        if wb.sheets.isEmpty { wb.sheets = [Sheet(name: "Sheet1")] }
        return wb
    }

    /// Style name as an NCName, encoded like LibreOffice ("Heading 1" → "Heading_20_1").
    static func styleNCName(_ name: String) -> String {
        var out = ""
        for (i, ch) in CellStyles.odfName(name).unicodeScalars.enumerated() {
            let ok = CharacterSet.letters.contains(ch) || ch == "_" || (i > 0 && (CharacterSet.decimalDigits.contains(ch) || ch == "-" || ch == "."))
            out += ok ? String(ch) : String(format: "_%x_", ch.value)
        }
        return out
    }

    /// ODF space-separated address lists allow spaces inside quoted sheet names.
    static func addresses(_ text: String) -> [String] {
        var parts: [String] = [], part = "", quoted = false
        for character in text {
            if character == "'" { quoted.toggle() }
            if character == " " && !quoted {
                if !part.isEmpty { parts.append(part); part = "" }
            } else { part.append(character) }
        }
        if !part.isEmpty { parts.append(part) }
        return parts
    }

    /// A solver_… name as Excel text: "Sheet1!$A$1:$B$2" for named ranges (several joined by ","), the expression otherwise.
    static func solverText(_ e: XMLElement) -> String? {
        if e.localName == "named-range", let addr = e.attr("cell-range-address") {
            // Several ranges are separated by spaces (outside quoted sheet names).
            var pieces: [String] = [], cur = "", quoted = false
            for ch in addr {
                if ch == "'" { quoted.toggle() }
                if ch == " " && !quoted { if !cur.isEmpty { pieces.append(cur) }; cur = ""; continue }
                cur.append(ch)
            }
            if !cur.isEmpty { pieces.append(cur) }
            let parts = pieces.compactMap { rangeFromODF($0) }.map { r in
                (r.sheet.map { Formula.sheetPrefix($0) } ?? "") + r.range
            }
            return parts.isEmpty ? nil : parts.joined(separator: ",")
        }
        guard var t = e.attr("expression") else { return nil }
        for prefix in ["of:", "oooc:"] where t.hasPrefix(prefix) { t.removeFirst(prefix.count) }
        if t.hasPrefix("=") { t.removeFirst() }
        return t
    }

    private static func descendantsNamed(_ e: XMLElement, _ name: String) -> [XMLElement] {
        var out: [XMLElement] = []
        for case let c as XMLElement in e.children ?? [] {
            if c.localName == name { out.append(c) }
            // rows/columns may be grouped in table-header-rows, table-row-group, table-columns…
            else if c.localName != "table-cell" && c.localName != "table-row" { out += descendantsNamed(c, name) }
        }
        return out
    }

    static func cellValue(_ c: XMLElement, date1904: Bool = false) -> (Value, String?) {
        var formula: String? = nil
        if let f = c.attr("formula") { formula = excelFormula(f) }
        let text = c.children("p").map(\.allTextODF).joined(separator: "\n")
        switch c.attr("value-type") {
        case "float", "percentage", "currency":
            return (.number(Double(c.attr("value") ?? "") ?? 0), formula)
        case "date":
            let s = c.attr("date-value") ?? ""
            let parts = s.split(separator: "T")
            let d = parts.first.map(String.init) ?? ""
            let nums = d.split(separator: "-").compactMap { Int($0) }
            var serial = nums.count == 3 ? DateMath.serial(year: nums[0], month: nums[1], day: nums[2]) ?? 0 : 0
            if date1904 { serial -= 1462 }
            if parts.count > 1, let t = DateMath.parseTime(String(parts[1].prefix(8))) { serial += t }
            return (.number(serial), formula)
        case "time":
            return (.number(duration(c.attr("time-value") ?? "")), formula)
        case "boolean":
            return (.bool(c.attr("boolean-value") == "true"), formula)
        case "string":
            return (.string(c.attr("string-value") ?? text), formula)
        default:
            return (text.isEmpty ? .empty : .string(text), formula)
        }
    }

    /// "PT10H30M00S" → fraction of a day.
    static func duration(_ s: String) -> Double {
        var total = 0.0, num = ""
        for ch in s.dropFirst(2) {
            if ch.isNumber || ch == "." { num.append(ch); continue }
            let v = Double(num) ?? 0
            switch ch { case "H": total += v * 3600; case "M": total += v * 60; case "S": total += v; default: break }
            num = ""
        }
        return total / 86400
    }

    /// OpenFormula "of:=SUM([.A1:.B2];['Лист 1'.C3])" → Excel "=SUM(A1:B2;'Лист 1'!C3)".
    /// "Sheet2.A1" / "'My sheet'.B2" → "Sheet2!A1".
    static func placeFromODF(_ s: String) -> String {
        if let r = rangeFromODF(s) { return (r.sheet.map { Formula.sheetPrefix($0) } ?? "") + r.range }
        return s
    }

    static func excelFormula(_ f: String) -> String {
        var s = f
        for prefix in ["of:", "oooc:", "msoxl:"] where s.hasPrefix(prefix) { s.removeFirst(prefix.count) }
        if !s.hasPrefix("=") { s = "=" + s }
        var out = ""
        var inString = false, braces = 0
        var i = s.startIndex
        while i < s.endIndex {
            let ch = s[i]
            if ch == "\"" { inString.toggle(); out.append(ch); i = s.index(after: i); continue }
            // Inline arrays: {1;2|3;4} (columns ";", rows "|") → {1,2;3,4}.
            if !inString {
                if ch == "{" { braces += 1 } else if ch == "}" { braces = max(0, braces - 1) }
                if braces > 0, ch == ";" || ch == "|" { out.append(ch == ";" ? "," : ";"); i = s.index(after: i); continue }
            }
            if !inString, ch == "[", let close = s[i...].firstIndex(of: "]") {
                let inner = String(s[s.index(after: i)..<close])
                if let r = rangeFromODF(inner) { out += (r.sheet.map { Formula.sheetPrefix($0) } ?? "") + r.range } else { out += "#REF!" }
                i = s.index(after: close)
                continue
            }
            out.append(ch)
            i = s.index(after: i)
        }
        // ODF names with Excel meanings: FDIST/FINV are the left-tailed F.DIST/F.INV, LEGACY.… the Excel originals.
        for (odf, excel) in [("FDIST", "F.DIST"), ("FINV", "F.INV"), ("SKEWP", "SKEW.P")] {
            out = out.replacingOccurrences(of: "(?<![A-Za-z0-9._])\(odf)\\(", with: excel + "(", options: [.regularExpression, .caseInsensitive])
        }
        for prefix in ["COM.MICROSOFT.", "ORG.OPENOFFICE.", "ORG.LIBREOFFICE.", "LEGACY.", "_xlfn."] { out = out.replacingOccurrences(of: prefix, with: "") }
        return out
    }

    /// ".A1:.B2", "$'Лист 1'.$A$1", "Sheet1.A1:Sheet1.C6" → (sheet, "A1:B2").
    static func rangeFromODF(_ s: String) -> (sheet: String?, range: String)? {
        func part(_ p: Substring) -> (String?, String) {
            var p = p
            var sheet: String? = nil
            if let dot = p.lastIndex(of: ".") {
                var name = String(p[..<dot])
                if name.hasPrefix("$") { name.removeFirst() }
                if name.hasPrefix("'"), name.hasSuffix("'"), name.count >= 2 {
                    name = String(name.dropFirst().dropLast()).replacingOccurrences(of: "''", with: "'")
                }
                if !name.isEmpty { sheet = name }
                p = p[p.index(after: dot)...]
            }
            return (sheet, String(p))
        }
        let pieces = s.split(separator: ":", maxSplits: 1)
        guard let first = pieces.first else { return nil }
        let (sheet, a) = part(first)
        guard !a.isEmpty else { return nil }
        if pieces.count > 1 {
            let (_, b) = part(pieces[1])
            return (sheet, a + ":" + b)
        }
        return (sheet, a)
    }

    private static func length(_ s: String?) -> CGFloat? {
        guard let s else { return nil }
        let units: [(String, CGFloat)] = [("cm", 28.3465), ("mm", 2.83465), ("in", 72), ("pt", 1), ("pc", 12), ("px", 0.75)]
        for (u, k) in units where s.hasSuffix(u) {
            if let v = Double(s.dropLast(u.count)) { return CGFloat(v) * k }
        }
        return Double(s).map { CGFloat($0) }
    }

    private static func color(_ s: String?) -> NSColor? {
        guard let s, s.hasPrefix("#") else { return nil }
        return XLSX.hexColor(String(s.dropFirst()))
    }

    private static func border(_ s: String?) -> BorderSide? {
        guard let s, s != "none" else { return nil }
        let parts = s.split(separator: " ")
        let width = parts.first.flatMap { length(String($0)) } ?? 0.75
        let style = parts.contains("dashed") ? "dashed" : parts.contains("dotted") ? "dotted" : parts.contains("double") ? "double"
            : width >= 2 ? "thick" : width >= 1.2 ? "medium" : width <= 0.5 ? "hair" : "thin"
        return BorderSide(style: style, color: parts.last.flatMap { color(String($0)) } ?? .black)
    }

    static func styleInfo(_ e: XMLElement, base: CellStyle = .default) -> StyleInfo {
        var info = StyleInfo()
        info.style = base
        info.element = e
        info.parent = e.attr("parent-style-name")
        info.dataStyle = e.attr("data-style-name")
        if let t = e.child("text-properties") {
            if let w = t.attr("font-weight") { info.style.bold = w == "bold" || (Int(w) ?? 400) >= 600 }
            if let i = t.attr("font-style") { info.style.italic = i == "italic" || i == "oblique" }
            if let c = t.attr("color") { info.style.textColor = color(c) }
            if let size = length(t.attr("font-size")) { info.style.fontSize = size }
            if let name = t.attr("font-name") ?? t.attr("font-family") { info.style.fontName = name.trimmingCharacters(in: CharacterSet(charactersIn: "'")) }
            if let u = t.attr("text-underline-style") { info.style.underline = u != "none"; if u == "none" { info.style.doubleUnderline = false } }
            if let u = t.attr("text-underline-type") { info.style.doubleUnderline = u == "double"; if u == "double" { info.style.underline = true } }
            if let l = t.attr("text-line-through-style") { info.style.strike = l != "none" }
        }
        if let p = e.child("table-cell-properties") {
            if let bg = p.attr("background-color") { info.style.fill = color(bg) }
            if let text = p.attr("border") { let all = border(text); (info.style.left, info.style.right, info.style.top, info.style.bottom) = (all, all, all, all) }
            if let b = p.attr("border-left") { info.style.left = border(b) }
            if let b = p.attr("border-right") { info.style.right = border(b) }
            if let b = p.attr("border-top") { info.style.top = border(b) }
            if let b = p.attr("border-bottom") { info.style.bottom = border(b) }
            switch p.attr("vertical-align") { case "top": info.style.vAlign = "top"; case "middle": info.style.vAlign = "center"; case "bottom": info.style.vAlign = "bottom"; default: break }
            if let wrap = p.attr("wrap-option") { info.style.wrap = wrap == "wrap" }
            if let protect = p.attr("cell-protect") {
                info.style.locked = protect != "none" && protect != "hidden-and-printable" && protect != "formula-hidden"
                info.style.hiddenFormula = protect.contains("formula-hidden")
            }
            if let a = p.attr("rotation-angle").flatMap(Double.init), a.isFinite { let r = Int(a.truncatingRemainder(dividingBy: 360).rounded()) % 360; info.style.rotation = r > 90 ? (r >= 270 ? r - 360 : 90) : r }
            if p.attr("direction") == "ttb" { info.style.rotation = 255 }
            if let shrink = p.attr("shrink-to-fit") { info.style.shrink = shrink == "true" }
        }
        if let p = e.child("paragraph-properties") {
            switch p.attr("text-align") {
            case "center": info.style.hAlign = "center"
            case "end", "right": info.style.hAlign = "right"
            case "start", "left": info.style.hAlign = "left"
            case "justify": info.style.hAlign = "justify"
            default: break
            }
        }
        if let c = e.child("table-column-properties") { info.columnWidth = length(c.attr("column-width")) }
        if let r = e.child("table-row-properties") { info.rowHeight = length(r.attr("row-height")) }
        return info
    }

    /// number:number-style / percentage / currency / date / time → Excel format code.
    static func numberFormat(_ e: XMLElement) -> String? {
        guard let kind = e.localName, kind.hasSuffix("-style"), e.prefix == "number" || e.uri?.contains("datastyle") == true else { return nil }
        var code = ""
        for case let p as XMLElement in e.children ?? [] {
            switch p.localName {
            case "number":
                let decimals = min(1000, max(0, Int(p.attr("decimal-places") ?? "0") ?? 0))
                let minInt = min(1000, max(1, Int(p.attr("min-integer-digits") ?? "1") ?? 1))
                var n = p.attr("grouping") == "true" ? "#,##" + String(repeating: "0", count: minInt) : String(repeating: "0", count: minInt)
                if p.attr("grouping") == "true", minInt > 1 { n = "#,##0" }
                if decimals > 0 { n += "." + String(repeating: "0", count: decimals) }
                code += n
            case "scientific-number":
                let decimals = min(1000, max(0, Int(p.attr("decimal-places") ?? "2") ?? 2))
                code += "0" + (decimals > 0 ? "." + String(repeating: "0", count: decimals) : "") + "E+00"
            case "fraction": code += "# ?/?"
            case "text":
                // Whitespace-only text nodes can be dropped by the XML parser: an empty <number:text/> is a space.
                var t = p.stringValue ?? ""
                if t.isEmpty { t = " " }
                code += t == "%" ? "%" : t.isEmpty ? "" : (t.allSatisfy { " ./-:,".contains($0) } ? t : "\"" + t + "\"")
            case "currency-symbol": code += "\"" + (p.stringValue ?? "") + "\""
            case "day": code += p.attr("style") == "long" ? "dd" : "d"
            case "month": code += p.attr("textual") == "true" ? (p.attr("style") == "long" ? "mmmm" : "mmm") : (p.attr("style") == "long" ? "mm" : "m")
            case "year": code += p.attr("style") == "long" ? "yyyy" : "yy"
            case "day-of-week": code += p.attr("style") == "long" ? "dddd" : "ddd"
            case "hours": code += p.attr("style") == "long" ? "hh" : "h"
            case "minutes": code += "mm"
            case "seconds": code += "ss"
            case "am-pm": code += "AM/PM"
            case "text-content": code += "@"
            case "boolean": code += "General"
            default: break
            }
        }
        if kind == "text-style" { return "@" }
        return code.isEmpty ? nil : code
    }

    // MARK: - Writing

    static func write(_ wb: Workbook) -> Data {
        var cellStyles: [CellStyle: String] = [:]
        var dataStyles: [String: String] = [:]   // Excel code → ODS style name
        var columnStyles: [CGFloat: String] = [:]
        var rowStyles: [CGFloat: String] = [:]
        var auto = ""
        func dataStyle(_ code: String) -> String? {
            guard code != "General" else { return nil }
            if let n = dataStyles[code] { return n }
            let name = "N\(dataStyles.count + 100)"
            guard let xml = odfNumberStyle(code, name: name) else { return nil }
            dataStyles[code] = name
            auto += xml
            return name
        }
        // Named cell styles: common styles in styles.xml (display name = the style's name, LibreOffice's for Accent 1…).
        var commonStyles = "", commonDataStyles: [String: String] = [:]
        var namedStyles = Set<String>()
        for d in CellStyles.definitions(wb) {
            namedStyles.insert(d.name)
            var x = "<style:style style:name=\"\(xmlEscape(styleNCName(d.name)))\" style:display-name=\"\(xmlEscape(CellStyles.odfName(d.name)))\" style:family=\"table-cell\" style:parent-style-name=\"Default\""
            if d.style.numberFormat != "General" {
                let ds = commonDataStyles[d.style.numberFormat] ?? "N\(commonDataStyles.count + 900)"
                if commonDataStyles[d.style.numberFormat] == nil, let xml = odfNumberStyle(d.style.numberFormat, name: ds) {
                    commonDataStyles[d.style.numberFormat] = ds
                    commonStyles = xml + commonStyles
                }
                if commonDataStyles[d.style.numberFormat] != nil { x += " style:data-style-name=\"\(ds)\"" }
            }
            commonStyles += x + ">" + cellProperties(d.style) + "</style:style>"
        }
        func styleName(_ s: CellStyle) -> String? {
            guard s != .default else { return nil }
            if let n = cellStyles[s] { return n }
            let name = "ce\(cellStyles.count + 1)"
            cellStyles[s] = name
            let parent = s.styleName.flatMap { namedStyles.contains($0) ? styleNCName($0) : nil } ?? "Default"
            var x = "<style:style style:name=\"\(name)\" style:family=\"table-cell\" style:parent-style-name=\"\(xmlEscape(parent))\""
            if let ds = dataStyle(s.numberFormat) { x += " style:data-style-name=\"\(ds)\"" }
            x += ">" + cellProperties(s) + "</style:style>"
            auto += x
            return name
        }
        func cellProperties(_ s: CellStyle) -> String {
            var x = "<style:table-cell-properties"
            if s.fill != nil || s.styleName != nil { x += " fo:background-color=\"\(s.fill.map { "#" + hex($0) } ?? "transparent")\"" }
            func b(_ side: BorderSide?) -> String {
                side.map {
                    let pattern = ["double", "dashed", "dotted"].contains($0.style) ? $0.style : "solid"
                    return "\($0.width)pt \(pattern) #\(hex($0.color))"
                } ?? "none"
            }
            if s.left != nil || s.right != nil || s.top != nil || s.bottom != nil || s.styleName != nil {
                x += " fo:border-left=\"\(b(s.left))\" fo:border-right=\"\(b(s.right))\" fo:border-top=\"\(b(s.top))\" fo:border-bottom=\"\(b(s.bottom))\""
            }
            if s.wrap || s.styleName != nil { x += " fo:wrap-option=\"\(s.wrap ? "wrap" : "no-wrap")\"" }
            if !s.locked || s.hiddenFormula || s.styleName != nil { x += " style:cell-protect=\"\(s.hiddenFormula ? (s.locked ? "protected formula-hidden" : "formula-hidden") : (s.locked ? "protected" : "none"))\"" }
            if s.rotation == 255 { x += " style:direction=\"ttb\"" } else if s.rotation != 0 || s.styleName != nil { x += " style:rotation-angle=\"\((s.rotation + 360) % 360)\"" }
            if s.shrink || s.styleName != nil { x += " style:shrink-to-fit=\"\(s.shrink)\"" }
            x += " style:vertical-align=\"\(s.vAlign == "center" ? "middle" : s.vAlign == "top" ? "top" : "bottom")\"/>"
            if let h = s.hAlign { x += "<style:paragraph-properties fo:text-align=\"\(h == "center" ? "center" : h == "right" ? "end" : h == "justify" ? "justify" : "start")\"/>" }
            x += "<style:text-properties fo:font-size=\"\(Value.formatGeneral(Double(s.fontSize)))pt\""
            if let fn = s.fontName { x += " style:font-name=\"\(xmlEscape(fn))\" fo:font-family=\"\(xmlEscape(fn))\"" }
            if s.bold || s.styleName != nil { x += " fo:font-weight=\"\(s.bold ? "bold" : "normal")\"" }
            if s.italic || s.styleName != nil { x += " fo:font-style=\"\(s.italic ? "italic" : "normal")\"" }
            if s.underline || s.doubleUnderline || s.styleName != nil { x += " style:text-underline-style=\"\(s.underline || s.doubleUnderline ? "solid" : "none")\" style:text-underline-width=\"auto\" style:text-underline-color=\"font-color\"" }
            if s.doubleUnderline || s.styleName != nil { x += " style:text-underline-type=\"\(s.doubleUnderline ? "double" : "single")\"" }
            if s.strike || s.styleName != nil { x += " style:text-line-through-style=\"\(s.strike ? "solid" : "none")\"" }
            if let c = s.textColor { x += " fo:color=\"#\(hex(c))\"" }
            return x + "/>"
        }
        func colStyle(_ w: CGFloat) -> String {
            if let n = columnStyles[w] { return n }
            let name = "co\(columnStyles.count + 1)"
            columnStyles[w] = name
            auto += "<style:style style:name=\"\(name)\" style:family=\"table-column\"><style:table-column-properties style:column-width=\"\(String(format: "%.3f", w / 28.3465))cm\"/></style:style>"
            return name
        }
        func rowStyle(_ h: CGFloat) -> String {
            if let n = rowStyles[h] { return n }
            let name = "ro\(rowStyles.count + 1)"
            rowStyles[h] = name
            auto += "<style:style style:name=\"\(name)\" style:family=\"table-row\"><style:table-row-properties style:row-height=\"\(String(format: "%.3f", h / 28.3465))cm\" style:use-optimal-row-height=\"false\"/></style:style>"
            return name
        }

        var body = ""
        var validations = ""
        var imageFiles: [String: Data] = [:]
        for (si, sheet) in wb.sheets.enumerated() {
            var imagesAt: [CellRef: String] = [:]
            for (index, image) in sheet.images.enumerated() {
                let path = "Pictures/sheet\(si + 1)-image\(index + 1).png"
                imageFiles[path] = embeddedImageData(image.data)
                imagesAt[image.anchor, default: ""] += imageXML(image, path: path, index: index + 1)
            }
            var t = "<table:table table:name=\"\(xmlEscape(sheet.name))\""
            if sheet.hidden || sheet.tabColor != nil {
                // Visibility and tab colour (table:tab-color, as LibreOffice writes it).
                auto += "<style:style style:name=\"ta\(si + 1)\" style:family=\"table\"><style:table-properties table:display=\"\(!sheet.hidden)\""
                    + (sheet.tabColor.map { " table:tab-color=\"#\(hex($0).lowercased())\"" } ?? "") + "/></style:style>"
                t += " table:style-name=\"ta\(si + 1)\""
            }
            if let area = sheet.printArea { t += " table:print-ranges=\"\(xmlEscape(odfRange(sheet.name, area)))\"" }
            if let p = sheet.protection {
                t += " table:protected=\"true\""
                if let key = p.odsKey {
                    let digests = (p.odsDigest ?? sha1Digest).split(separator: " ")
                    t += " table:protection-key=\"\(key)\" table:protection-key-digest-algorithm=\"\(digests[0])\""
                    if digests.count > 1 { t += " loext:protection-key-digest-algorithm-2=\"\(digests[1])\"" }
                } else if let h = p.passwordHash, let key = legacyProtectionKey(h) {
                    // From .xlsx: LibreOffice's encoding of the Excel hash keeps the password working.
                    t += " table:protection-key=\"\(key)\" table:protection-key-digest-algorithm=\"\(legacyExcelDigest)\" loext:protection-key-digest-algorithm-2=\"\(sha1Digest)\""
                }
            }
            t += ">"
            // Validation rules, clipped to a sane size (whole-column rules would mean a million rows).
            var validationAt: [CellRef: String] = [:]
            for (vi, v) in sheet.validations.enumerated() {
                let name = "val\(si + 1)_\(vi + 1)"
                validations += validationXML(v, name: name, sheet: sheet.name)
                for range in v.ranges {
                    let clipped = CellRange(range.start, CellRef(row: min(range.end.row, range.start.row + 9_999), col: min(range.end.col, range.start.col + 255)))
                    for ref in clipped.cells { validationAt[ref] = name }
                }
            }
            let used = sheet.usedRange
            let lastCol = max(used?.end.col ?? 0, sheet.columnWidths.keys.max() ?? 0, sheet.colLevels.keys.max() ?? 0, sheet.merges.map { $0.end.col }.max() ?? 0, validationAt.keys.map(\.col).max() ?? 0, imagesAt.keys.map(\.col).max() ?? 0)
            var openCols = 0
            for c in 0...lastCol {
                t += groupTags(sheet, c, rows: false, open: &openCols)
                let w = sheet.width(c)
                let style = colStyle(w == 0 ? (sheet.columnWidths[c].flatMap { $0 > 0 ? $0 : nil } ?? sheet.defaultColumnWidth) : w)
                t += "<table:table-column table:style-name=\"\(style)\"\(w == 0 ? " table:visibility=\"collapse\"" : "")/>"
            }
            t += String(repeating: "</table:table-column-group>", count: openCols)
            let lastRow = max(used?.end.row ?? 0, sheet.rowHeights.keys.max() ?? 0, sheet.rowLevels.keys.max() ?? 0, sheet.merges.map { $0.end.row }.max() ?? 0, validationAt.keys.map(\.row).max() ?? 0, imagesAt.keys.map(\.row).max() ?? 0)
            var openRows = 0
            for r in 0...lastRow {
                t += groupTags(sheet, r, rows: true, open: &openRows)
                let h = sheet.height(r)
                let styleHeight = h == 0 ? (sheet.rowHeights[r].flatMap { $0 > 0 ? $0 : nil } ?? sheet.defaultRowHeight) : h
                let visibility = sheet.filterHidden.contains(r) ? " table:visibility=\"filter\"" : h == 0 ? " table:visibility=\"collapse\"" : ""
                t += "<table:table-row table:style-name=\"\(rowStyle(styleHeight))\"\(visibility)>"
                var empty = 0
                func flushEmpty() { if empty > 0 { t += "<table:table-cell\(empty > 1 ? " table:number-columns-repeated=\"\(empty)\"" : "")/>"; empty = 0 } }
                for c in 0...lastCol {
                    let ref = CellRef(row: r, col: c)
                    if sheet.merges.contains(where: { $0.start != ref && $0.contains(ref) }) {
                        flushEmpty()
                        t += imagesAt[ref].map { "<table:covered-table-cell>\($0)</table:covered-table-cell>" } ?? "<table:covered-table-cell/>"
                        continue
                    }
                    guard let cell = sheet.cells[ref] ?? (imagesAt[ref] != nil || sheet.merges.contains(where: { $0.start == ref }) ? Cell() : nil) else {
                        if let v = validationAt[ref] { flushEmpty(); t += "<table:table-cell table:content-validation-name=\"\(v)\"/>" } else { empty += 1 }
                        continue
                    }
                    flushEmpty()
                    var rendered = cellXML(cell, ref: ref, sheet: sheet, style: styleName(cell.style), validation: validationAt[ref], date1904: wb.date1904)
                    if let images = imagesAt[ref] { rendered = String(rendered.dropLast("</table:table-cell>".count)) + images + "</table:table-cell>" }
                    t += rendered
                }
                t += "</table:table-row>"
            }
            t += String(repeating: "</table:table-row-group>", count: openRows)
            if let m = sheet.solver { t += SolverFiles.odsNames(m, sheet: sheet.name) }
            t += "</table:table>"
            body += t
        }
        if !validations.isEmpty { body = "<table:content-validations>\(validations)</table:content-validations>" + body }
        if !wb.names.isEmpty {
            body += "<table:named-expressions>"
            for (name, def) in wb.names.sorted(by: { $0.key < $1.key }) {
                if let e = try? Formula.parse("=" + def), case .range(let s, let a, let b) = e {
                    body += "<table:named-range table:name=\"\(xmlEscape(name))\" table:cell-range-address=\"\(xmlEscape(odfRef(s, a, b, absoluteSheet: true)))\"/>"
                } else if let e = try? Formula.parse("=" + def), case .ref(let s, let a) = e {
                    body += "<table:named-range table:name=\"\(xmlEscape(name))\" table:cell-range-address=\"\(xmlEscape(odfRef(s, a, nil, absoluteSheet: true)))\"/>"
                } else if let e = try? Formula.parse(def.hasPrefix("=") ? def : "=" + def) {
                    body += "<table:named-expression table:name=\"\(xmlEscape(name))\" table:expression=\"\(xmlEscape("of:=" + odfFormula(e)))\"/>"
                }
            }
            body += "</table:named-expressions>"
        }
        body += databaseRangesXML(wb)
        body += PivotFiles.odsXML(wb)

        let ns = """
        xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" \
        xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" \
        xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:number="urn:oasis:names:tc:opendocument:xmlns:datastyle:1.0" \
        xmlns:of="urn:oasis:names:tc:opendocument:xmlns:of:1.2" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:xlink="http://www.w3.org/1999/xlink" \
        xmlns:config="urn:oasis:names:tc:opendocument:xmlns:config:1.0" xmlns:ooo="http://openoffice.org/2004/office" \
        xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" \
        xmlns:loext="urn:org:documentfoundation:names:experimental:office:xmlns:loext:1.0" office:version="1.3"
        """
        let content = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-content \(ns)><office:automatic-styles>\(auto)</office:automatic-styles><office:body><office:spreadsheet\(wb.structureProtected ? " table:structure-protected=\"true\"" : "")><table:calculation-settings table:case-sensitive=\"false\" table:use-regular-expressions=\"false\" table:use-wildcards=\"true\"><table:null-date table:date-value=\"\(wb.date1904 ? "1904-01-01" : "1899-12-30")\"/></table:calculation-settings>\(body)</office:spreadsheet></office:body></office:document-content>"
        let styles = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-styles \(ns)><office:styles><style:default-style style:family=\"table-cell\"><style:text-properties fo:font-size=\"11pt\" style:font-name=\"Calibri\" fo:font-family=\"Calibri\"/></style:default-style><style:style style:name=\"Default\" style:family=\"table-cell\"/>\(commonStyles)</office:styles></office:document-styles>"
        var tables = ""
        for s in wb.sheets {
            tables += "<config:config-item-map-entry config:name=\"\(xmlEscape(s.name))\">"
            if s.frozenCols > 0 { tables += "<config:config-item config:name=\"HorizontalSplitMode\" config:type=\"short\">2</config:config-item><config:config-item config:name=\"HorizontalSplitPosition\" config:type=\"int\">\(s.frozenCols)</config:config-item><config:config-item config:name=\"PositionRight\" config:type=\"int\">\(s.frozenCols)</config:config-item>" }
            if s.frozenRows > 0 { tables += "<config:config-item config:name=\"VerticalSplitMode\" config:type=\"short\">2</config:config-item><config:config-item config:name=\"VerticalSplitPosition\" config:type=\"int\">\(s.frozenRows)</config:config-item><config:config-item config:name=\"PositionBottom\" config:type=\"int\">\(s.frozenRows)</config:config-item>" }
            if s.frozenRows == 0, s.frozenCols == 0, let sp = s.split, sp.x > 0 || sp.y > 0 {
                func item(_ name: String, _ type: String, _ v: Int) -> String { "<config:config-item config:name=\"\(name)\" config:type=\"\(type)\">\(v)</config:config-item>" }
                let px = sp.pixels, pane = sp.paneTopLeft
                tables += item("HorizontalSplitMode", "short", px.x > 0 ? 1 : 0) + item("VerticalSplitMode", "short", px.y > 0 ? 1 : 0)
                tables += item("HorizontalSplitPosition", "int", px.x) + item("VerticalSplitPosition", "int", px.y)
                tables += item("ActiveSplitRange", "short", SheetSplit.odsPanes.firstIndex(of: sp.normalizedPane) ?? 2)
                tables += item("PositionLeft", "int", sp.topLeft.col) + item("PositionRight", "int", px.x > 0 ? pane.col : 0)
                tables += item("PositionTop", "int", px.y > 0 ? sp.topLeft.row : 0) + item("PositionBottom", "int", pane.row)
            }
            if let tab = s.tabColor, let rgb = Int(XLSX.argb(tab).dropFirst(2), radix: 16) {
                tables += "<config:config-item config:name=\"TabColor\" config:type=\"int\">\(rgb)</config:config-item>"
            }
            tables += "<config:config-item config:name=\"ShowGrid\" config:type=\"boolean\">\(s.showGridlines)</config:config-item></config:config-item-map-entry>"
        }
        let settings = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-settings \(ns)><office:settings><config:config-item-set config:name=\"ooo:view-settings\"><config:config-item-map-indexed config:name=\"Views\"><config:config-item-map-entry><config:config-item config:name=\"ViewId\" config:type=\"string\">view1</config:config-item><config:config-item-map-named config:name=\"Tables\">\(tables)</config:config-item-map-named><config:config-item config:name=\"ActiveTable\" config:type=\"string\">\(xmlEscape(wb.sheets.first { !$0.hidden }?.name ?? wb.sheets[0].name))</config:config-item></config:config-item-map-entry></config:config-item-map-indexed></config:config-item-set></office:settings></office:document-settings>"
        let manifest = """
        <?xml version="1.0" encoding="UTF-8"?>
        <manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.3">
        <manifest:file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.spreadsheet"/>
        <manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/>
        <manifest:file-entry manifest:full-path="styles.xml" manifest:media-type="text/xml"/>
        <manifest:file-entry manifest:full-path="settings.xml" manifest:media-type="text/xml"/>
        \(imageFiles.keys.sorted().map { "<manifest:file-entry manifest:full-path=\"\(xmlEscape($0))\" manifest:media-type=\"image/png\"/>" }.joined())
        </manifest:manifest>
        """
        var zip = ZipArchive()
        zip.storedUncompressed.insert("mimetype")
        zip["mimetype"] = Data("application/vnd.oasis.opendocument.spreadsheet".utf8)
        zip["content.xml"] = Data(content.utf8)
        zip["styles.xml"] = Data(styles.utf8)
        zip["settings.xml"] = Data(settings.utf8)
        zip["META-INF/manifest.xml"] = Data(manifest.utf8)
        for (path, data) in imageFiles { zip[path] = data }
        return zip.data()
    }

    private static func cellXML(_ cell: Cell, ref: CellRef, sheet: Sheet, style: String?, validation: String? = nil, date1904: Bool = false) -> String {
        var x = "<table:table-cell"
        if let style { x += " table:style-name=\"\(style)\"" }
        if let validation { x += " table:content-validation-name=\"\(validation)\"" }
        if let m = sheet.merges.first(where: { $0.start == ref }) {
            x += " table:number-columns-spanned=\"\(m.cols.count)\" table:number-rows-spanned=\"\(m.rows.count)\""
        }
        if cell.isFormula, let e = Calculator.expression(cell.input) {
            x += " table:formula=\"\(xmlEscape("of:=" + odfFormula(e)))\""
            if let spill = sheet.arrays[ref] ?? sheet.spills[ref] {
                x += " table:number-matrix-columns-spanned=\"\(spill.cols.count)\" table:number-matrix-rows-spanned=\"\(spill.rows.count)\""
            } else if cell.array {
                x += " table:number-matrix-columns-spanned=\"1\" table:number-matrix-rows-spanned=\"1\""
            }
        }
        var text = NumberFormat.format(cell.value, cell.style.numberFormat, date1904: date1904).text
        switch cell.value {
        case .number(let d):
            if NumberFormat.isDateFormat(cell.style.numberFormat) {
                let date = DateMath.date(d + (date1904 ? 1462 : 0))
                let f = DateFormatter()
                f.locale = Locale(identifier: "en_US_POSIX")
                f.timeZone = TimeZone(identifier: "UTC")
                f.dateFormat = d == floor(d) ? "yyyy-MM-dd" : "yyyy-MM-dd'T'HH:mm:ss"
                x += " office:value-type=\"date\" office:date-value=\"\(f.string(from: date))\""
            } else if cell.style.numberFormat.contains("%") {
                x += " office:value-type=\"percentage\" office:value=\"\(Value.numberLiteral(d))\""
            } else {
                x += " office:value-type=\"float\" office:value=\"\(Value.numberLiteral(d))\""
            }
        case .bool(let b): x += " office:value-type=\"boolean\" office:boolean-value=\"\(b)\""; text = b ? "TRUE" : "FALSE"
        case .string: x += " office:value-type=\"string\""
        case .error: x += " office:value-type=\"string\""
        case .empty: break
        }
        x += ">"
        if let comment = sheet.comments[ref] {
            x += "<office:annotation>" + comment.split(separator: "\n", omittingEmptySubsequences: false).map { "<text:p>\(xmlEscape(String($0)))</text:p>" }.joined() + "</office:annotation>"
        }
        if let link = sheet.hyperlinks[ref] {
            // Places in the workbook: "#Sheet.A1" in ODF.
            var href = link
            if href.hasPrefix("#") { href = "#" + href.dropFirst().replacingOccurrences(of: "!", with: ".") }
            x += "<text:p><text:a xlink:type=\"simple\" xlink:href=\"\(xmlEscape(href))\">\(xmlEscape(text.isEmpty ? link : text))</text:a></text:p>"
            return x + "</table:table-cell>"
        }
        if !text.isEmpty { x += text.split(separator: "\n", omittingEmptySubsequences: false).map { "<text:p>\(xmlEscape(String($0)))</text:p>" }.joined() }
        return x + "</table:table-cell>"
    }

    private static func hex(_ c: NSColor) -> String { String(XLSX.argb(c).dropFirst(2)) }

    private static func odfSheet(_ name: String, absolute: Bool) -> String {
        let plain = name.allSatisfy { $0.isLetter || $0.isNumber || $0 == "_" }
        return (absolute ? "$" : "") + (plain ? name : "'" + name.replacingOccurrences(of: "'", with: "''") + "'")
    }

    static func odfRef(_ sheet: String?, _ a: RefPart, _ b: RefPart?, absoluteSheet: Bool = false) -> String {
        let prefix = sheet.map { odfSheet($0, absolute: absoluteSheet) } ?? ""
        var s = prefix + "." + a.text
        if let b { s += ":" + (sheet.map { _ in prefix } ?? "") + "." + b.text }
        return s
    }

    static func odfCell(_ sheet: String, _ r: CellRef) -> String {
        "\(odfSheet(sheet, absolute: true)).$\(CellRef.columnName(r.col))$\(r.row + 1)"
    }

    static func odfRange(_ sheet: String, _ r: CellRange) -> String {
        "\(odfSheet(sheet, absolute: true)).$\(CellRef.columnName(r.start.col))$\(r.start.row + 1):\(odfSheet(sheet, absolute: true)).$\(CellRef.columnName(r.end.col))$\(r.end.row + 1)"
    }

    /// OpenFormula names LibreOffice uses for Excel functions (COM.MICROSOFT.…, LEGACY.…, native equivalents).
    static func odfName(_ n: String) -> String {
        switch n {
        case "F.DIST": return "FDIST"
        case "F.INV": return "FINV"
        case "SKEW.P": return "SKEWP"
        case "FDIST", "FINV", "NORMSDIST", "NORMSINV", "TDIST", "CHIDIST", "CHIINV": return "LEGACY." + n
        case "REGEX": return "ORG.LIBREOFFICE.REGEX"
        case "ERROR.TYPE": return n
        default:
            return n.contains(".") || microsoftNames.contains(n) ? "COM.MICROSOFT." + n : n
        }
    }

    static let microsoftNames: Set<String> = [
        "MAXIFS", "MINIFS", "IFS", "SWITCH", "XLOOKUP", "XMATCH", "FILTER", "SORT", "SORTBY", "UNIQUE", "SEQUENCE", "RANDARRAY",
        "LET", "CONCAT", "TEXTJOIN", "CEILING", "FLOOR", "AGGREGATE", "VSTACK", "HSTACK", "TOCOL", "TOROW", "CHOOSECOLS",
        "CHOOSEROWS", "TAKE", "DROP", "EXPAND", "WRAPROWS", "WRAPCOLS", "TEXTBEFORE", "TEXTAFTER", "TEXTSPLIT", "REGEXTEST",
        "REGEXEXTRACT", "REGEXREPLACE", "LAMBDA",
    ]

    /// Expression → OpenFormula text (references in brackets, ";" between arguments).
    static func odfFormula(_ e: Expr) -> String {
        func s(_ e: Expr, _ parent: Int) -> String {
            switch e {
            case .ref(let sh, let p): return "[" + odfRef(sh, p, nil) + "]"
            case .range(let sh, let a, let b): return "[" + odfRef(sh, a, b) + "]"
            case .call(let n, let args): return odfName(n) + "(" + args.map { s($0, 0) }.joined(separator: ";") + ")"
            case .binary(let o, let l, let r):
                let prec = Formula.precedence(o)
                let text = s(l, prec) + o + s(r, prec + 1)
                return prec < parent ? "(" + text + ")" : text
            case .unary(let o, let x): return o + s(x, 6)
            case .percent(let x): return s(x, 7) + "%"
            case .array(let m): return Formula.arrayText(m, col: ";", row: "|")
            default: return Formula.serialize(e)
            }
        }
        return s(e, 0)
    }

    /// Excel format code → <number:…-style>, for the common formats.
    static func odfNumberStyle(_ code: String, name: String) -> String? {
        let ns = "number:"
        if code == "@" { return "<\(ns)text-style style:name=\"\(name)\"><\(ns)text-content/></\(ns)text-style>" }
        if NumberFormat.isDateFormat(code) {
            var parts = ""
            var afterHours = false   // "m" right after hours (h:mm) means minutes
            var i = code.startIndex
            let lower = code.lowercased()
            while i < lower.endIndex {
                let rest = lower[i...]
                let tokens: [(String, String, Bool)] = [
                    ("yyyy", "<\(ns)year \(ns)style=\"long\"/>", false), ("yy", "<\(ns)year/>", false),
                    ("mmmm", "<\(ns)month \(ns)style=\"long\" \(ns)textual=\"true\"/>", false), ("mmm", "<\(ns)month \(ns)textual=\"true\"/>", false),
                    ("dd", "<\(ns)day \(ns)style=\"long\"/>", false), ("d", "<\(ns)day/>", false),
                    ("hh", "<\(ns)hours \(ns)style=\"long\"/>", true), ("h", "<\(ns)hours/>", true),
                    ("ss", "<\(ns)seconds \(ns)style=\"long\"/>", false), ("s", "<\(ns)seconds/>", false),
                ]
                if let t = tokens.first(where: { rest.hasPrefix($0.0) }) {
                    parts += t.1
                    afterHours = t.2
                    i = lower.index(i, offsetBy: t.0.count)
                    continue
                }
                if rest.hasPrefix("m") {
                    let n = rest.hasPrefix("mm") ? 2 : 1
                    let nextIsSeconds = lower[lower.index(i, offsetBy: n)...].drop { !$0.isLetter }.first == "s"
                    if afterHours || nextIsSeconds { parts += "<\(ns)minutes \(ns)style=\"long\"/>" }
                    else { parts += n == 2 ? "<\(ns)month \(ns)style=\"long\"/>" : "<\(ns)month/>" }
                    afterHours = false
                    i = lower.index(i, offsetBy: n)
                    continue
                }
                parts += "<\(ns)text>\(xmlEscape(String(code[i])))</\(ns)text>"
                if code[i] != ":" { afterHours = false }
                i = lower.index(after: i)
            }
            return "<\(ns)date-style style:name=\"\(name)\">\(parts)</\(ns)date-style>"
        }
        let section = String(NumberFormat.splitSections(code).first ?? "")
        let stripped = NumberFormat.strip(section)
        let percent = stripped.contains("%")
        let decimals = stripped.split(separator: ".").dropFirst().first.map { $0.filter { $0 == "0" || $0 == "#" }.count } ?? 0
        let grouping = stripped.contains(",")
        var literal = ""
        if let q1 = section.firstIndex(of: "\""), let q2 = section[section.index(after: q1)...].firstIndex(of: "\"") {
            literal = String(section[section.index(after: q1)..<q2])
        }
        let number = "<\(ns)number \(ns)decimal-places=\"\(decimals)\" \(ns)min-decimal-places=\"\(decimals)\" \(ns)min-integer-digits=\"1\"\(grouping ? " \(ns)grouping=\"true\"" : "")/>"
        if percent { return "<\(ns)percentage-style style:name=\"\(name)\">\(number)<\(ns)text>%</\(ns)text></\(ns)percentage-style>" }
        if !literal.isEmpty {
            let before = section.hasPrefix("\"")
            let sym = "<\(ns)currency-symbol>\(xmlEscape(literal.trimmingCharacters(in: .whitespaces)))</\(ns)currency-symbol>"
            return "<\(ns)currency-style style:name=\"\(name)\">" + (before ? sym + number : number + "<\(ns)text> </\(ns)text>" + sym) + "</\(ns)currency-style>"
        }
        return "<\(ns)number-style style:name=\"\(name)\">\(number)</\(ns)number-style>"
    }
}

extension XMLElement {
    /// Text of an ODF paragraph: spans, <text:s/> spaces, tabs and line breaks.
    var allTextODF: String {
        var out = ""
        for node in children ?? [] {
            if node.kind == .text { out += node.stringValue ?? ""; continue }
            guard let e = node as? XMLElement else { continue }
            switch e.localName {
            case "s": out += String(repeating: " ", count: min(100_000, max(1, Int(e.attr("c") ?? "1") ?? 1)))
            case "tab": out += "\t"
            case "line-break": out += "\n"
            case "annotation": break
            default: out += e.allTextODF
            }
        }
        return out
    }
}
