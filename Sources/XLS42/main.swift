import AppKit
import Kit42

Localization.register(rows: xls42Strings)

#if APP_STORE
// The sandboxed App Store variant: shared library location, disabled features and its own strings.
if CommandLine.arguments.contains("--selftest"), !AppStoreEdition.selfCheck() { exit(1) }
#endif

// --sign-document / --verify-document: Office and OpenDocument digital signatures (Kit42/DocumentSignatureCLI.swift).
if let code = DocumentSignatureCLI.run(CommandLine.arguments) { exit(code) }

if CommandLine.arguments.contains("--selftest") { exit(SelfTest.run()) }
// XLS42 --lo-compare [dir] [soffice path] [verbose]: function values against LibreOffice (headless, private profile)
if let i = CommandLine.arguments.firstIndex(of: "--lo-compare") {
    let rest = CommandLine.arguments.dropFirst(i + 1)
    let dir = rest.first { $0.hasPrefix("/") && !$0.hasSuffix("soffice") } ?? NSTemporaryDirectory() + "xls42-lo-compare"
    let soffice = rest.first { $0.hasSuffix("soffice") } ?? "/Applications/LibreOffice.app/Contents/MacOS/soffice"
    exit(SelfTest.loCompare(dir: dir, soffice: soffice))
}
// XLS42 --roundtrip in.xlsx out.xlsx: read, recalculate, write (for testing compatibility)
if let i = CommandLine.arguments.firstIndex(of: "--roundtrip"), CommandLine.arguments.count > i + 2 {
    let args = CommandLine.arguments
    var wb = try SpreadsheetFile.read(Data(contentsOf: URL(fileURLWithPath: args[i + 1])), typeName: URL(fileURLWithPath: args[i + 1]).pathExtension)
    wb = Calculator.recalculate(wb)
    for s in wb.sheets {
        print("[\(s.name)] cells: \(s.cells.count), formulas: \(s.cells.values.filter(\.isFormula).count), merges: \(s.merges.count)")
        for ref in s.cells.keys.sorted().prefix(40) {
            let c = s.cells[ref]!
            print("  \(ref.a1) \(c.input.prefix(50)) → \(NumberFormat.format(c.value, c.style.numberFormat, date1904: wb.date1904).text)")
        }
    }
    try XLSX.write(wb).write(to: URL(fileURLWithPath: args[i + 2]))
    exit(0)
}

// XLS42 --info file: summary of structure (for format tests)
if let i = CommandLine.arguments.firstIndex(of: "--info"), CommandLine.arguments.count > i + 1 {
    let wb = Calculator.recalculate(try SpreadsheetFile.read(Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[i + 1])), typeName: URL(fileURLWithPath: CommandLine.arguments[i + 1]).pathExtension))
    for s in wb.sheets {
        let formulas = s.cells.values.filter(\.isFormula).count
        let errors = s.cells.filter { $0.value.isFormula && $0.value.value.isError }.map { "\($0.key.a1)=\($0.value.input)→\($0.value.value.plain)" }
        print("[\(s.name)] cells \(s.cells.count) formulas \(formulas) merges \(s.merges.count) frozen \(s.frozenRows)x\(s.frozenCols) widths \(s.columnWidths.count) comments \(s.comments.count) printArea \(s.printArea?.a1 ?? "-") bold \(s.cells.values.filter(\.style.bold).count) fills \(s.cells.values.filter { $0.style.fill != nil }.count) errors \(errors.prefix(5))")
        for v in s.validations { print("  validation \(v.ranges.map(\.a1).joined(separator: " ")): \(v.kind.rawValue) \(v.op) \(v.formula1) \(v.formula2) style \(v.errorStyle) \"\(v.errorMessage)\"") }
        if !s.rowLevels.isEmpty || !s.colLevels.isEmpty {
            print("  outline rows \(s.rowLevels.sorted { $0.key < $1.key }.map { "\($0.key + 1):\($0.value)" }) hidden \(s.outlineHiddenRows.sorted().map { $0 + 1 })"
                  + " cols \(s.colLevels.sorted { $0.key < $1.key }.map { "\(CellRef.columnName($0.key)):\($0.value)" }) hidden \(s.outlineHiddenCols.sorted().map(CellRef.columnName))")
        }
        if let p = s.pivot {
            print("  pivot \(p.name) source \(p.sourceSheet)!\(p.source.a1) row \(p.rowField) col \(p.colField.map(String.init) ?? "-") value \(p.valueField) \(p.aggregate) at \(p.anchor.a1) location \(p.location?.a1 ?? "-") fields \(p.fieldNames)")
        }
        if let m = s.solver { print("  " + SolverFiles.describe(m)) }
        if let sp = s.split {
            print("  split x \(Value.formatGeneral(Double(sp.x)))px y \(Value.formatGeneral(Double(sp.y)))px top-left \(sp.topLeft.a1) bottom-right \(sp.bottomRight.a1) active \(sp.activePane)")
        }
        let styled = Dictionary(grouping: s.cells.filter { $0.value.style.styleName != nil }, by: { $0.value.style.styleName! })
        if !styled.isEmpty {
            print("  styles " + styled.keys.sorted().map { "\($0): \(ODS.ranges(styled[$0]!.map(\.key)).map(\.a1).joined(separator: ","))" }.joined(separator: "; "))
        }
        if !s.arrays.isEmpty {
            print("  arrays " + s.arrays.sorted { $0.key < $1.key }.map { "{\(s.cells[$0.key]?.input ?? "")} \($0.value.a1)" }.joined(separator: "; "))
        }
        if let p = s.protection {
            print("  protected hash \(p.passwordHash ?? "-") sha512 \(p.sha512Hash != nil) ods \(p.odsKey != nil); unlocked \(s.cells.filter { !$0.value.style.locked }.keys.sorted().map(\.a1))")
        }
    }
    print("names", wb.names)
    if !wb.cellStyles.isEmpty { print("cell styles", wb.cellStyles.keys.sorted()) }
    exit(0)
}

// XLS42 --convert in.(xlsx|xls|ods|csv) out.(xlsx|ods|csv|xml)
if let i = CommandLine.arguments.firstIndex(of: "--convert"), CommandLine.arguments.count > i + 2 {
    let args = CommandLine.arguments
    var wb = Calculator.recalculate(try SpreadsheetFile.read(Data(contentsOf: URL(fileURLWithPath: args[i + 1])), typeName: URL(fileURLWithPath: args[i + 1]).pathExtension))
    if args.contains("freeze") { wb.sheets[0].frozenRows = 3; wb.sheets[0].frozenCols = 1 }
    // "refresh": rebuild every pivot table from its source (as Data › Refresh Pivot Table does).
    if args.contains("refresh") { for i in wb.sheets.indices where wb.sheets[i].pivot != nil { Pivot.refresh(&wb, sheet: i) } }
    let out = URL(fileURLWithPath: args[i + 2])
    let data: Data
    switch out.pathExtension.lowercased() {
    case "xml": data = try SpreadsheetML.write(wb)
    case "ods": data = ODS.write(wb)
    case "csv": data = try SheetExport.csv(wb.sheets[0], delimiter: ",", bom: true, date1904: wb.date1904)
    default: data = XLSX.write(wb)
    }
    try data.write(to: out)
    print("\(out.pathExtension): \(data.count) bytes")
    exit(0)
}

// XLS42 --pivot-demo out.(xlsx|ods): a data sheet with a cross-tab pivot and a one-field pivot (for format checks)
if let i = CommandLine.arguments.firstIndex(of: "--pivot-demo"), CommandLine.arguments.count > i + 1 {
    let out = URL(fileURLWithPath: CommandLine.arguments[i + 1])
    let data = SelfTest.pivotDemo()
    try (out.pathExtension.lowercased() == "ods" ? ODS.write(data) : XLSX.write(data)).write(to: out)
    print("wrote \(out.path)")
    exit(0)
}

// XLS42 --feature-demo out.(xlsx|ods): tab colours, hidden sheet, hyperlinks, cell styles (for format checks)
if let i = CommandLine.arguments.firstIndex(of: "--feature-demo"), CommandLine.arguments.count > i + 1 {
    let out = URL(fileURLWithPath: CommandLine.arguments[i + 1])
    let data = SelfTest.featureDemo()
    try (out.pathExtension.lowercased() == "ods" ? ODS.write(data) : XLSX.write(data)).write(to: out)
    print("wrote \(out.path)")
    exit(0)
}

// XLS42 --model-demo out.(xlsx|ods): solver model, split window, named cell styles, array formulas (for format checks)
if let i = CommandLine.arguments.firstIndex(of: "--model-demo"), CommandLine.arguments.count > i + 1 {
    let out = URL(fileURLWithPath: CommandLine.arguments[i + 1])
    let data = SelfTest.modelDemo()
    try (out.pathExtension.lowercased() == "ods" ? ODS.write(data) : XLSX.write(data)).write(to: out)
    print("wrote \(out.path)")
    exit(0)
}

// XLS42 --export-pdf in.xlsx out.pdf [all|landscape|nogrid|nofit]
if let i = CommandLine.arguments.firstIndex(of: "--export-pdf"), CommandLine.arguments.count > i + 2 {
    let args = CommandLine.arguments
    let wb = Calculator.recalculate(try SpreadsheetFile.read(Data(contentsOf: URL(fileURLWithPath: args[i + 1])), typeName: URL(fileURLWithPath: args[i + 1]).pathExtension))
    var options = SheetExport.Options()
    if args.contains("all") { options.scope = .workbook }
    if args.contains("landscape") { options.orientation = .landscape }
    if args.contains("nogrid") { options.gridlines = false }
    if args.contains("nofit") { options.fitWidth = false }
    let data = SheetExport.pdf(wb, activeSheet: 0, selection: nil, options: options, base: NSPrintInfo.shared)
    try data.write(to: URL(fileURLWithPath: args[i + 2]))
    print("pdf \(data.count) bytes")
    exit(0)
}

let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.setActivationPolicy(.regular)
app.run()
