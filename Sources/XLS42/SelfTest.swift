import Kit42
import Foundation

/// `XLS42 --selftest`: formula engine and number format checks.
enum SelfTest {
    static func run() -> Int32 {
        var wb = Workbook()
        func set(_ a1: String, _ input: String) {
            var c = Cell()
            c.input = input
            c.value = input.hasPrefix("=") ? .empty : XLSX.constant(input)
            wb.sheets[0][CellRef(a1)!] = c
        }
        set("A1", "10"); set("A2", "20"); set("A3", "30"); set("A4", "текст"); set("A5", "")
        set("B1", "яблоко"); set("B2", "груша"); set("B3", "яблоко")
        set("C1", "1"); set("C2", "2"); set("C3", "3")
        set("D1", "15.03.2024")
        let cases: [(String, String)] = [
            ("=SUM(A1:A5)", "60"), ("=СУММ(A1:A3;5)", "65"), ("=AVERAGE(A1:A4)", "20"), ("=1+2*3^2", "19"),
            ("=-2^2", "4"), ("=10/0", "#DIV/0!"), ("=IF(A1>5,\"big\",\"small\")", "big"),
            ("=SUMIF(B1:B3,\"яблоко\",A1:A3)", "40"), ("=COUNTIF(A1:A4,\">15\")", "2"), ("=COUNTIF(B1:B3,\"яб*\")", "2"),
            ("=VLOOKUP(\"груша\",B1:C3,2,FALSE)", "2"), ("=INDEX(A1:A3,MATCH(30,A1:A3,0))", "30"),
            ("=XLOOKUP(\"груша\",B1:B3,A1:A3)", "20"), ("=CONCAT(\"a\",A1,\"b\")", "a10b"), ("=A1&\"%\"", "10%"),
            ("=ROUND(2.675,2)", "2.68"), ("=TEXT(1234.5,\"#,##0.00\")", NumberFormat.format(.number(1234.5), "#,##0.00").text), ("=YEAR(D1)", "2024"),
            ("=MONTH(D1)+DAY(D1)", "18"), ("=DATEDIF(DATE(2020,1,15),DATE(2024,3,1),\"Y\")", "4"),
            ("=IFERROR(1/0,\"err\")", "err"), ("=AND(A1>5,A2>5)", "TRUE"), ("=LEFT(\"Привет\",3)", "При"),
            ("=SUBSTITUTE(\"a-b-c\",\"-\",\"+\")", "a+b+c"), ("=SUMPRODUCT(A1:A3,C1:C3)", "140"),
            ("=COUNTA(A1:A5)", "4"), ("=MAX(A1:A3)-MIN(A1:A3)", "20"), ("=A1:A3*2", "20"), ("=SUMIFS(A1:A3,B1:B3,\"яблоко\",C1:C3,\">1\")", "30"),
            ("=PMT(0.01,12,1000)", "-88.84878868"), ("=MEDIAN(1,3,2,4)", "2.5"), ("=ISNUMBER(A4)", "FALSE"), ("=E1", "0"),
            ("=SUM(A:A)", "60"), ("=RANK(20,A1:A3)", "2"), ("=EOMONTH(D1,1)", "45412"), ("=10%*A2", "2"),
        ]
        for (i, c) in cases.enumerated() { set("Z\(i + 1)", c.0) }
        wb = Calculator.recalculate(wb)
        var failed = 0
        for (i, c) in cases.enumerated() {
            let v = wb.sheets[0][CellRef("Z\(i + 1)")!].value
            var got = v.plain
            if case .bool(let b) = v { got = b ? "TRUE" : "FALSE" }
            if case .number(let d) = v { got = String(format: "%.10g", d) }
            let ok = got == c.1
            if !ok { failed += 1 }
            print(ok ? "ok  " : "FAIL", c.0, "→", got, ok ? "" : "(expected \(c.1))")
        }
        let formats: [(Double, String, String)] = [
            (1234.5, "#,##0.00", "1\u{00A0}234,50"), (0.256, "0.0%", "25,6%"), (-5, "0.00;[Red]-0.00", "-5,00"),
            (45366, "dd.mm.yyyy", "15.03.2024"), (45366.5, "dd.mm.yyyy hh:mm", "15.03.2024 12:00"), (1500, "#,##0 \"₽\"", "1\u{00A0}500 ₽"),
            (1234567, "0.00E+00", "1,23E+06"), (0, "0.00", "0,00"), (3.14159, "General", "3,14159"), (0.5, "h:mm", "12:00"),
            (42, "[$$-409]#,##0.00", "$42,00"),
        ]
        for f in formats {
            let got = NumberFormat.format(.number(f.0), f.1).text
            let ok = NumberFormat.decimal != "," || got == f.2
            if !ok { failed += 1 }
            print(ok ? "ok  " : "FAIL", f.1, "→", got, ok ? "" : "(expected \(f.2))")
        }
        // Reference shifting
        let shifted = Formula.shifted("=SUM($A1:B2)+C$3", rows: 2, cols: 1)
        if shifted != "=SUM($A3:C4)+D$3" { failed += 1 }
        print(shifted == "=SUM($A3:C4)+D$3" ? "ok  " : "FAIL", "shift →", shifted)
        let inserted = Formula.adjusted("=SUM(A1:A10)+B5", sheet: "Лист1", formulaSheet: "Лист1", rows: true, at: 2, count: 3)
        if inserted != "=SUM(A1:A13)+B8" { failed += 1 }
        print(inserted == "=SUM(A1:A13)+B8" ? "ok  " : "FAIL", "insert rows →", inserted)
        failed += formulaReferences()
        failed += features()
        failed += functions()
        failed += dataTools()
        failed += pivotFiles()
        failed += colors()
        failed += deepFiles()
        failed += deepFormulas()
        failed += deepSheetReferences()
        failed += technicalFiles()
        print(failed == 0 ? "ALL PASSED" : "\(failed) FAILED")
        return failed == 0 ? 0 : 1
    }

    private static func formulaReferences() -> Int {
        var failed = 0
        func check(_ name: String, _ ok: Bool) {
            print(ok ? "ok  " : "FAIL", name)
            if !ok { failed += 1 }
        }
        var wb = Workbook()
        wb.sheets[0].name = "Data"
        for (i, value) in [10.0, 20.0, 30.0].enumerated() {
            var cell = Cell(); cell.input = String(value); cell.value = .number(value)
            wb.sheets[0][CellRef(row: i, col: 0)] = cell
        }
        let calc = Calculator(wb), at = CellRef(row: 4, col: 1)
        for formula in ["=SUM(1:3)", "=SUM(1 : 3)", "=SUM($1:3)", "=SUM(Data!1:3)", "=SUM('Data'!$1:$3)"] {
            let value = (try? Formula.parse(formula)).map { calc.scalar(calc.eval($0, sheet: 0, at: at), at: at) }
            check("whole-row reference: \(formula)", value == .number(60))
        }
        for invalid in ["=SUM(A:3)", "=SUM(A1:C)", "=SUM(1:1048577)"] {
            check("invalid range rejected: \(invalid)", (try? Formula.parse(invalid)) == nil)
        }

        let deletions: [(String, Bool, Int, Int, String)] = [
            ("=A2+$B$2+A3", true, 1, -1, "=#REF!+#REF!+A2"),
            ("=SUM(A2:A3)", true, 1, -2, "=SUM(#REF!)"),
            ("=SUM(A2:A5)", true, 1, -2, "=SUM(A2:A3)"),
            ("=SUM(A1:A3)", true, 1, -2, "=SUM(A1:A1)"),
            ("=SUM(A5:A2)", true, 1, -2, "=SUM(A3:A2)"),
            ("=SUM(A3:A1)", true, 1, -2, "=SUM(A1:A1)"),
            ("=B1+$B$2+C1", false, 1, -1, "=#REF!+#REF!+B1"),
            ("=SUM(B1:C2)", false, 1, -2, "=SUM(#REF!)"),
            ("=SUM(A1:D2)", false, 1, -2, "=SUM(A1:B2)"),
            ("=Other!A2+Data!A2", true, 1, -1, "=Other!A2+#REF!"),
            ("=SUM(A:A)+SUM(2:3)", true, 1, -2, "=SUM(A:A)+SUM(#REF!)"),
            ("=A1048576", true, 0, 1, "=#REF!"),
        ]
        for (formula, rows, index, count, expected) in deletions {
            let result = Formula.adjusted(formula, sheet: "Data", formulaSheet: "Data", rows: rows, at: index, count: count)
            check("structural reference change: \(formula) → \(expected)", result == expected)
        }

        // Copying and structural edits serialize the entire expression, including unchanged literals.
        for literal in ["1.23456789012345", "0.000000123456789012345", "1234567890123456"] {
            let original = "=\(literal)+A1"
            let copied = Formula.shifted(original, rows: 1, cols: 0)
            let copiedValue = (try? Formula.parse(copied)).map { calc.scalar(calc.eval($0, sheet: 0, at: at), at: at) }
            check("copy preserves numeric literal \(literal)", copiedValue == .number(Double(literal)! + 20))
            let array = Matrix(rows: 1, cols: 1, values: [.number(Double(literal)!)])
            let arrayText = Formula.arrayText(array, col: ",", row: ";")
            if case .array(let parsed)? = try? Formula.parse("=" + arrayText) {
                check("array literal preserves precision \(literal)", parsed.values == array.values)
            } else { check("array literal parses \(literal)", false) }
        }
        return failed
    }
}
