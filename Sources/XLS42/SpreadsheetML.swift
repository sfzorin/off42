import AppKit
import Foundation

/// Excel 2003 XML Spreadsheet (not the ZIP-based Office Open XML format).
/// Unsupported formulas are rejected rather than silently changing references.
enum SpreadsheetML {
    enum Error: LocalizedError {
        case invalid(String)
        var errorDescription: String? { if case .invalid(let s) = self { return "SpreadsheetML: " + s }; return nil }
    }
    static let namespace = "urn:schemas-microsoft-com:office:spreadsheet"
    static func isWorkbook(_ data: Data) -> Bool {
        guard data.count <= 100_000_000, let text = String(data: data, encoding: .utf8), !text.uppercased().contains("<!DOCTYPE"), !text.uppercased().contains("<!ENTITY") else { return false }
        let probe = RootProbe(), parser = XMLParser(data: data)
        parser.shouldProcessNamespaces = true
        parser.shouldResolveExternalEntities = false
        parser.delegate = probe
        _ = parser.parse()
        return probe.found
    }
    private final class RootProbe: NSObject, XMLParserDelegate {
        var found = false
        func parser(_ parser: XMLParser, didStartElement name: String, namespaceURI: String?, qualifiedName: String?, attributes: [String: String]) {
            found = name == "Workbook" && namespaceURI == SpreadsheetML.namespace
            parser.abortParsing()
        }
    }
    static func read(_ data: Data) throws -> Workbook {
        guard data.count <= 100_000_000 else { throw Error.invalid("File is too large") }
        // Reject declarations before parsing, including internal entities. XMLParser must never resolve external entities.
        guard let text = String(data: data, encoding: .utf8), !text.uppercased().contains("<!DOCTYPE"), !text.uppercased().contains("<!ENTITY") else { throw Error.invalid("Unsupported encoding or XML declaration") }
        let reader = Reader(), parser = XMLParser(data: data)
        parser.shouldProcessNamespaces = true
        parser.shouldReportNamespacePrefixes = true
        parser.shouldResolveExternalEntities = false
        parser.delegate = reader
        guard parser.parse(), reader.failure == nil, reader.root, !reader.book.sheets.isEmpty else {
            throw Error.invalid(reader.failure ?? "Invalid workbook XML")
        }
        return reader.book
    }
    static func write(_ book: Workbook) throws -> Data {
        func esc(_ s: String) -> String { s.replacingOccurrences(of: "&", with: "&amp;").replacingOccurrences(of: "<", with: "&lt;").replacingOccurrences(of: ">", with: "&gt;").replacingOccurrences(of: "\"", with: "&quot;").replacingOccurrences(of: "'", with: "&apos;") }
        guard !book.date1904 else { throw Error.invalid("1904 date system is not supported") }
        guard !book.sheets.isEmpty else { throw Error.invalid("Workbook has no worksheets") }
        var styles: [CellStyle: String] = [:]
        for sheet in book.sheets { for cell in sheet.cells.values where cell.style != .default { if styles[cell.style] == nil { styles[cell.style] = "s\(styles.count + 1)" } } }
        var out = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Workbook xmlns=\"\(namespace)\" xmlns:ss=\"\(namespace)\"><Styles>"
        for (style, id) in styles.sorted(by: { $0.value < $1.value }) {
            out += "<Style ss:ID=\"\(id)\"><Font ss:Bold=\"\(style.bold ? 1 : 0)\" ss:Italic=\"\(style.italic ? 1 : 0)\" ss:Size=\"\(style.fontSize)\""
            if let name = style.fontName { out += " ss:FontName=\"\(esc(name))\"" }
            if style.underline { out += " ss:Underline=\"Single\"" }
            if let c = style.textColor { out += " ss:Color=\"#\(XLSX.argb(c).suffix(6))\"" }
            out += "/>"
            if let c = style.fill { out += "<Interior ss:Color=\"#\(XLSX.argb(c).suffix(6))\" ss:Pattern=\"Solid\"/>" }
            out += "<NumberFormat ss:Format=\"\(esc(style.numberFormat))\"/><Alignment ss:WrapText=\"\(style.wrap ? 1 : 0)\""
            if let h = style.hAlign { out += " ss:Horizontal=\"\(esc(h.capitalized))\"" }
            out += "/></Style>"
        }
        out += "</Styles>"
        for sheet in book.sheets {
            out += "<Worksheet ss:Name=\"\(esc(sheet.name))\"><Table>"
            for (col, width) in sheet.columnWidths.sorted(by: { $0.key < $1.key }) { guard col >= 0 && col <= RefPart.maxCol else { throw Error.invalid("Column out of range") }; out += "<Column ss:Index=\"\(col + 1)\" ss:Width=\"\(width)\"/>" }
            let rows = Dictionary(grouping: Set(sheet.cells.keys).union(sheet.merges.map(\.start)), by: \.row)
            for row in Set(rows.keys).union(sheet.rowHeights.keys).sorted() {
                guard row >= 0 && row <= RefPart.maxRow else { throw Error.invalid("Row out of range") }
                out += "<Row ss:Index=\"\(row + 1)\""
                if let height = sheet.rowHeights[row] { out += " ss:Height=\"\(height)\"" }
                out += ">"
                for ref in (rows[row] ?? []).sorted() {
                    guard ref.col >= 0 && ref.col <= RefPart.maxCol else { throw Error.invalid("Column out of range") }
                    let cell = sheet.cells[ref] ?? Cell()

                    out += "<Cell ss:Index=\"\(ref.col + 1)\""
                    if cell.isFormula { out += " ss:Formula=\"\(esc(try formulaToR1C1(cell.input, at: ref)))\"" }
                    if let id = styles[cell.style] { out += " ss:StyleID=\"\(id)\"" }
                    if let m = sheet.merges.first(where: { $0.start == ref }) { out += " ss:MergeAcross=\"\(m.end.col - ref.col)\" ss:MergeDown=\"\(m.end.row - ref.row)\"" }
                    out += ">"
                    switch cell.value {
                    case .empty: break
                    case .number(let n): guard n.isFinite else { throw Error.invalid("Non-finite number") }; out += "<Data ss:Type=\"Number\">\(Value.numberLiteral(n))</Data>"
                    case .string(let s): out += "<Data ss:Type=\"String\">\(esc(s))</Data>"
                    case .bool(let b): out += "<Data ss:Type=\"Boolean\">\(b ? 1 : 0)</Data>"
                    case .error(let e): out += "<Data ss:Type=\"Error\">\(esc(e))</Data>"
                    }
                    out += "</Cell>"
                }
                out += "</Row>"
            }
            out += "</Table></Worksheet>"
        }
        out += "</Workbook>"
        return Data(out.utf8)
    }

    /// Rewrite reference tokens while leaving quoted strings and sheet names untouched.
    private static func rewrite(_ formula: String, pattern: String, convert: (String) throws -> String) throws -> String {
        let chars = Array(formula), regex = try NSRegularExpression(pattern: "^(?:" + pattern + ")", options: [.caseInsensitive])
        var output = "", i = 0
        while i < chars.count {
            let c = chars[i]
            if c == "\"" || c == "'" {
                let quote = c; output.append(c); i += 1; var closed = false
                while i < chars.count {
                    let ch = chars[i]; output.append(ch); i += 1
                    if ch == quote {
                        if i < chars.count && chars[i] == quote { output.append(chars[i]); i += 1 } else { closed = true; break }
                    }
                }
                guard closed else { throw Error.invalid("Unterminated formula quote") }
                continue
            }
            let previous = i > 0 ? chars[i - 1] : " "
            if !(previous.isLetter || previous.isNumber || previous == "_" || previous == "." || previous == "$") {
                let rest = String(chars[i...]), ns = rest as NSString
                if let match = regex.firstMatch(in: rest, range: NSRange(location: 0, length: ns.length)) {
                    let token = ns.substring(with: match.range), count = token.count, end = i + count
                    let following = end < chars.count ? chars[end] : " "
                    if !(following.isLetter || following.isNumber || following == "_" || following == "." || following == "!" || following == "(") {
                        output += try convert(token); i = end; continue
                    }
                }
            }
            output.append(c); i += 1
        }
        return output
    }
    private static func validate(_ expression: Expr) throws {
        switch expression {
        case .name: throw Error.invalid("Named or external formula references are unsupported")
        case .ref(let sheet, let part):
            if part.wholeColumn || part.wholeRow || (sheet?.contains("[") ?? false) || (sheet?.contains(":") ?? false) { throw Error.invalid("Unsupported formula reference") }
        case .range(let sheet, let a, let b):
            if a.wholeColumn || a.wholeRow || b.wholeColumn || b.wholeRow || (sheet?.contains("[") ?? false) || (sheet?.contains(":") ?? false) { throw Error.invalid("Unsupported formula range") }
        case .unary(_, let x), .percent(let x): try validate(x)
        case .binary(_, let l, let r): try validate(l); try validate(r)
        case .call(_, let args): for arg in args { try validate(arg) }
        default: break
        }
    }
    static func formulaToA1(_ formula: String, at origin: CellRef) throws -> String {
        guard formula.hasPrefix("=") else { throw Error.invalid("Invalid formula") }
        let result = try rewrite(formula, pattern: #"R(?:\[-?\d+\]|\d+)?C(?:\[-?\d+\]|\d+)?"#) { token in
            let u = token.uppercased(), c = u.firstIndex(of: "C")!
            let r = String(u[u.index(after: u.startIndex)..<c]), col = String(u[u.index(after: c)...])
            func axis(_ part: String, _ base: Int, _ max: Int) throws -> (Int, Bool) {
                let absolute = !part.isEmpty && !part.hasPrefix("[")
                let offset = part.isEmpty ? 0 : Int(part.trimmingCharacters(in: CharacterSet(charactersIn: "[]")))
                guard let n = offset else { throw Error.invalid("Invalid R1C1 reference") }
                let v = absolute ? n - 1 : base + n
                guard v >= 0 && v <= max else { throw Error.invalid("R1C1 reference outside worksheet") }
                return (v, absolute)
            }
            let (row, ar) = try axis(r, origin.row, RefPart.maxRow), (column, ac) = try axis(col, origin.col, RefPart.maxCol)
            return (ac ? "$" : "") + CellRef.columnName(column) + (ar ? "$" : "") + String(row + 1)
        }
        let expression = try Formula.parse(result); try validate(expression)
        return "=" + Formula.serialize(expression)
    }
    static func formulaToR1C1(_ formula: String, at origin: CellRef) throws -> String {
        let expression = try Formula.parse(formula); try validate(expression)
        let canonical = "=" + Formula.serialize(expression, forFile: true)
        return try rewrite(canonical, pattern: #"\$?[A-Z]{1,3}\$?\d+"#) { token in
            let chars = Array(token); var i = 0
            let ac = chars[i] == "$"; if ac { i += 1 }; let start = i
            while i < chars.count && chars[i].isLetter { i += 1 }
            guard let column = CellRef.columnIndex(Substring(String(chars[start..<i]))) else { throw Error.invalid("Invalid A1 reference") }
            let ar = i < chars.count && chars[i] == "$"; if ar { i += 1 }
            guard let n = Int(String(chars[i...])), n > 0, n <= RefPart.maxRow + 1 else { throw Error.invalid("Invalid A1 reference") }
            func axis(_ value: Int, _ base: Int, _ absolute: Bool) -> String { absolute ? String(value + 1) : (value == base ? "" : "[\(value - base)]") }
            return "R" + axis(n - 1, origin.row, ar) + "C" + axis(column, origin.col, ac)
        }
    }

    private final class Reader: NSObject, XMLParserDelegate {
        var book = Workbook(), root = false, failure: String?, depth = 0
        var sheet: Sheet?, row = -1, col = -1, column = -1, currentCell = Cell(), dataType = "", content = "", readingData = false
        var styles: [String: CellStyle] = [:], styleID: String?, style = CellStyle.default, mergeAcross = 0
        override init() { super.init(); book.sheets = [] }
        func fail(_ parser: XMLParser, _ reason: String) { failure = reason; parser.abortParsing() }
        // XMLParser supplies qualified attribute names, so resolve their prefixes against
        // the current in-scope bindings rather than assuming the conventional "ss".
        var prefixes: [String: [String]] = [:]
        func parser(_ parser: XMLParser, didStartMappingPrefix prefix: String, toURI namespaceURI: String) {
            prefixes[prefix, default: []].append(namespaceURI)
        }
        func parser(_ parser: XMLParser, didEndMappingPrefix prefix: String) {
            guard prefixes[prefix]?.isEmpty == false else { return }
            prefixes[prefix]?.removeLast()
        }
        func attr(_ a: [String: String], _ key: String) -> String? {
            for (qualified, value) in a {
                let parts = qualified.split(separator: ":", maxSplits: 1, omittingEmptySubsequences: false)
                if parts.count == 2 && parts[1] == key && prefixes[String(parts[0])]?.last == SpreadsheetML.namespace { return value }
            }
            return a[key] // Compatibility with files that omit attribute namespaces.
        }
        func color(_ text: String?) -> NSColor? {
            guard let text, text.count == 7, text.first == "#", let n = UInt32(text.dropFirst(), radix: 16) else { return nil }
            return NSColor(srgbRed: CGFloat((n >> 16) & 255) / 255, green: CGFloat((n >> 8) & 255) / 255, blue: CGFloat(n & 255) / 255, alpha: 1)
        }
        func parser(_ parser: XMLParser, didStartElement elementName: String, namespaceURI: String?, qualifiedName qName: String?, attributes a: [String: String]) {
            depth += 1
            guard depth <= 64 else { fail(parser, "XML nesting limit exceeded"); return }
            if depth == 1 { guard elementName == "Workbook", namespaceURI == SpreadsheetML.namespace else { fail(parser, "Not an Excel 2003 XML workbook"); return }; root = true; return }
            guard namespaceURI == SpreadsheetML.namespace else { return }
            switch elementName {
            case "Style": styleID = attr(a, "ID"); style = .default
            case "Font" where styleID != nil:
                style.bold = attr(a, "Bold") == "1"; style.italic = attr(a, "Italic") == "1"; style.underline = attr(a, "Underline").map { $0 != "None" } ?? false
                style.textColor = color(attr(a, "Color")); style.fontName = attr(a, "FontName"); if let v = attr(a, "Size").flatMap(Double.init), v.isFinite, v > 0, v < 1000 { style.fontSize = v }
            case "Interior" where styleID != nil: if attr(a, "Pattern") == "Solid" { style.fill = color(attr(a, "Color")) }
            case "NumberFormat" where styleID != nil: style.numberFormat = attr(a, "Format") ?? "General"
            case "Alignment" where styleID != nil: style.wrap = attr(a, "WrapText") == "1"; style.hAlign = attr(a, "Horizontal")?.lowercased()
            case "Worksheet":
                guard sheet == nil, book.sheets.count < 1024, let name = attr(a, "Name"), !name.isEmpty else { fail(parser, "Invalid worksheet"); return }; sheet = Sheet(name: name); row = -1; column = -1
            case "Column" where sheet != nil:
                let i = attr(a, "Index").flatMap(Int.init).map { $0 - 1 } ?? column + 1
                let span = attr(a, "Span").flatMap(Int.init) ?? 0
                guard i > column, i <= RefPart.maxCol, span >= 0, span <= RefPart.maxCol - i else { fail(parser, "Invalid column index"); return }
                if let w = attr(a, "Width").flatMap(Double.init), w.isFinite, w >= 0 { for n in i...(i + span) { sheet?.columnWidths[n] = w } }
                column = i + span
            case "Row" where sheet != nil:
                let next = attr(a, "Index").flatMap(Int.init).map { $0 - 1 } ?? row + 1
                guard next > row, next <= RefPart.maxRow else { fail(parser, "Invalid row index"); return }; row = next; col = -1; mergeAcross = 0
                if let h = attr(a, "Height").flatMap(Double.init), h.isFinite, h >= 0 { sheet?.rowHeights[row] = h }
            case "Cell" where sheet != nil && row >= 0:
                let next = attr(a, "Index").flatMap(Int.init).map { $0 - 1 } ?? col + mergeAcross + 1
                guard next > col, next <= RefPart.maxCol else { fail(parser, "Invalid cell index"); return }; col = next
                currentCell = Cell();
                if let formula = attr(a, "Formula") {
                    do { currentCell.input = try SpreadsheetML.formulaToA1(formula, at: CellRef(row: row, col: col)) }
                    catch { fail(parser, error.localizedDescription); return }
                }
                if let id = attr(a, "StyleID") { currentCell.style = styles[id] ?? .default }
                let across = attr(a, "MergeAcross").flatMap(Int.init) ?? 0, down = attr(a, "MergeDown").flatMap(Int.init) ?? 0
                guard across >= 0, down >= 0, across <= RefPart.maxCol - col, down <= RefPart.maxRow - row else { fail(parser, "Invalid merge"); return }
                mergeAcross = across
                if across > 0 || down > 0 { sheet?.merges.append(CellRange(CellRef(row: row, col: col), CellRef(row: row + down, col: col + across))) }
            case "Data" where sheet != nil && row >= 0 && col >= 0: dataType = attr(a, "Type") ?? "String"; content = ""; readingData = true
            default: break
            }
        }
        func parser(_ parser: XMLParser, foundCharacters string: String) { if readingData { content += string; if content.utf8.count > 10_000_000 { fail(parser, "Cell is too large") } } }
        func parser(_ parser: XMLParser, didEndElement elementName: String, namespaceURI: String?, qualifiedName qName: String?) {
            defer { depth -= 1 }; guard namespaceURI == SpreadsheetML.namespace else { return }
            switch elementName {
            case "Style": if let id = styleID { styles[id] = style }; styleID = nil
            case "Data" where readingData:
                readingData = false
                switch dataType {
                case "String": currentCell.value = .string(content); if currentCell.input.isEmpty { currentCell.input = content.hasPrefix("=") || content.hasPrefix("'") ? "'" + content : content }
                case "Number": guard let n = Double(content), n.isFinite else { fail(parser, "Invalid number"); return }; currentCell.value = .number(n); if currentCell.input.isEmpty { currentCell.input = Value.numberLiteral(n) }
                case "Boolean": guard content == "0" || content == "1" else { fail(parser, "Invalid Boolean"); return }; currentCell.value = .bool(content == "1"); if currentCell.input.isEmpty { currentCell.input = content == "1" ? "TRUE" : "FALSE" }
                case "Error": currentCell.value = .error(content); if currentCell.input.isEmpty { currentCell.input = content }
                case "DateTime":
                    let iso = ISO8601DateFormatter(); iso.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
                    let time = content.split(separator: "T", maxSplits: 1).last ?? ""
                    let zoned = content.hasSuffix("Z") || time.contains("+") || time.contains("-") ? content : content + "Z"
                    let d = iso.date(from: zoned) ?? ISO8601DateFormatter().date(from: zoned)
                    guard let d else { fail(parser, "Invalid date"); return }; let raw = d.timeIntervalSince(DateMath.base) / 86400; let n = raw < 61 ? raw - 1 : raw
                    currentCell.value = .number(n); if currentCell.input.isEmpty { currentCell.input = Value.numberLiteral(n) }; if currentCell.style.numberFormat == "General" { currentCell.style.numberFormat = "yyyy-mm-dd hh:mm:ss" }
                default: fail(parser, "Unsupported cell type \(dataType)")
                }
            case "Cell" where sheet != nil && row >= 0 && col >= 0: sheet?.cells[CellRef(row: row, col: col)] = currentCell
            case "Worksheet": if let s = sheet { book.sheets.append(s) }; sheet = nil
            default: break
            }
        }
    }
}
