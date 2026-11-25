import AppKit
import Kit42

extension SelfTest {
    static func technicalFiles() -> Int {
        var failures = 0
        func check(_ name: String, _ condition: Bool) {
            print(condition ? "ok  " : "FAIL", "technical files: " + name)
            if !condition { failures += 1 }
        }
        do {
            let recipe = TechnicalElement(codeType: .qr, value: "0000042", widthMM: 25, caption: false)
            let render = try recipe.render()
            var wb = Workbook(); wb.sheets = [Sheet(name: "Technical")]
            let image = SheetImage(anchor: CellRef("C7")!, offset: CGPoint(x: 2.5, y: 3.75), size: render.sizePoints, data: render.pngData)
            wb.sheets[0].images = [image]
            for (name, data) in [("xlsx", XLSX.write(wb)), ("ods", ODS.write(wb))] {
                let back = try SpreadsheetFile.read(data)
                let restored = back.sheets[0].images.first
                check(name + " raw PNG survives", restored?.data == render.pngData)
                check(name + " recipe survives", restored.flatMap { TechnicalElement.fromPNG($0.data) } == recipe)
                let geometry = restored.map { abs($0.size.width - image.size.width) <= 1.0 / 12700 && abs($0.size.height - image.size.height) <= 1.0 / 12700 } ?? false
                check(name + " anchor and dimensions survive to one EMU", restored?.anchor == image.anchor && restored?.offset == image.offset && geometry)
                check(name + " image-only print area", back.sheets[0].printableRange?.contains(image.anchor) == true && back.sheets[0].usedRange == nil)
            }
            wb.sheets[0].merges = [CellRange("B6:D8")!]
            let merged = try ODS.read(ODS.write(wb))
            check("covered-cell image survives ODS", merged.sheets[0].images.first?.anchor == image.anchor && merged.sheets[0].images.first?.data == render.pngData)
        } catch {
            print("FAIL technical files:", error.localizedDescription)
            failures += 1
        }
        return failures
    }
}
