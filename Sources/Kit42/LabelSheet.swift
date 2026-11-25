import AppKit
import PDFKit

/// A snapshot supplied by an app; source row numbers remain attached through filtering and copies.
public struct LabelSource {
    public var title: String
    public var text: String?
    public var pdfData: Data?
    public var headers: [String]
    public var rows: [[String]]
    public var rowNumbers: [Int]
    public var visible: [Bool]
    public var firstRowIsHeader: Bool
    public var isTable: Bool { !headers.isEmpty }
    public init(title: String, text: String) {
        self.title = title; self.text = text; pdfData = nil; headers = []; rows = []; rowNumbers = []; visible = []; firstRowIsHeader = false
    }
    public init(title: String, pdfData: Data) {
        self.init(title: title, text: ""); text = nil; self.pdfData = pdfData
    }
    public init(title: String, headers: [String], rows: [[String]], rowNumbers: [Int] = [], visible: [Bool] = [], firstRowIsHeader: Bool = false) {
        self.init(title: title, text: ""); text = nil; self.headers = headers; self.rows = rows
        self.rowNumbers = rowNumbers; self.visible = visible; self.firstRowIsHeader = firstRowIsHeader
    }
    public func columnTitles(firstRowIsHeader: Bool) -> [String] {
        headers.enumerated().map { i, fallback in
            let value = firstRowIsHeader && rows.first?.indices.contains(i) == true ? rows[0][i] : fallback
            return value.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? L("Column %d", i + 1) : value
        }
    }
}

public struct LabelStock: Codable, Equatable {
    public var id: String
    public var name: String
    public var width: Double
    public var height: Double
    public var columns: Int
    public var rows: Int
    public var marginLeft: Double
    public var marginTop: Double
    public var gapX: Double
    public var gapY: Double
    public var source: String
    public var note: String
    public static func custom(width: Double, height: Double, gapX: Double = 0, gapY: Double = 0) throws -> LabelStock {
        guard width.isFinite, height.isFinite, gapX.isFinite, gapY.isFinite,
              width >= 5, height >= 5, width <= 210, height <= 297, gapX >= 0, gapY >= 0, gapX <= 50, gapY <= 50 else {
            throw LabelSheetError.message(L("Enter a label size between 5 mm and the A4 page size."))
        }
        let columns = Int(floor((210 + gapX) / (width + gapX)))
        let rows = Int(floor((297 + gapY) / (height + gapY)))
        return LabelStock(id: "custom", name: L("Custom"), width: width, height: height, columns: columns, rows: rows,
                          marginLeft: (210 - Double(columns) * width - Double(columns - 1) * gapX) / 2,
                          marginTop: (297 - Double(rows) * height - Double(rows - 1) * gapY) / 2,
                          gapX: gapX, gapY: gapY, source: "", note: "")
    }
    public static var catalog: [LabelStock] = {
        let bundle = Bundle.main.resourceURL.flatMap { Bundle(url: $0.appendingPathComponent("Suite42_Kit42.bundle")) } ?? Bundle.module
        guard let url = bundle.url(forResource: "formats", withExtension: "json", subdirectory: "Labels"),
              let data = try? Data(contentsOf: url), let result = try? JSONDecoder().decode([LabelStock].self, from: data) else { return [] }
        return result.filter { (try? $0.validate()) != nil }.sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
    }()
    public var capacity: Int { columns * rows }
    public func validate() throws {
        guard (1...100).contains(columns), (1...100).contains(rows), capacity <= 5000,
              [width, height, marginLeft, marginTop, gapX, gapY].allSatisfy({ $0.isFinite && $0 >= 0 }), width >= 5, height >= 5,
              marginLeft + Double(columns) * width + Double(columns - 1) * gapX <= 210.01,
              marginTop + Double(rows) * height + Double(rows - 1) * gapY <= 297.01 else {
            throw LabelSheetError.message(L("The label layout does not fit on A4."))
        }
    }
}

public struct LabelSheetOptions: Codable {
    public var stock: LabelStock
    public var textColumns: [Int] = []
    public var codeColumn: Int? = nil
    public var codeType: TechnicalCodeType = .qr
    public var copies: Int = 1
    public var copiesColumn: Int? = nil
    public var visibleOnly = true
    public var firstRowIsHeader = false
    public var includeFieldNames = true
    public var startSlot = 0
    public init(stock: LabelStock) { self.stock = stock }
}

public enum LabelSheetError: LocalizedError {
    case message(String)
    public var errorDescription: String? { if case let .message(message) = self { return message }; return nil }
}

public struct LabelSheetResult {
    public let data: Data
    public let labelCount: Int
    public let pageCount: Int
    public let sourceRows: [Int]
}

public enum LabelSheetRenderer {
    public static let pointsPerMM = 72.0 / 25.4
    private struct Item { let text: String; let code: TechnicalRender?; let sourceRow: Int; let copies: Int }
    public static func render(source: LabelSource, options: LabelSheetOptions) throws -> LabelSheetResult {
        try options.stock.validate()
        guard (1...5000).contains(options.copies), (0..<options.stock.capacity).contains(options.startSlot),
              source.rows.count <= 5000, source.headers.count <= 200,
              source.rows.reduce(0, { $0 + $1.count }) <= 500_000 else {
            throw LabelSheetError.message(L("Use at most 5,000 labels, 5,000 source rows and 200 columns."))
        }
        guard (source.visible.isEmpty || source.visible.count == source.rows.count),
              (source.rowNumbers.isEmpty || source.rowNumbers.count == source.rows.count),
              source.rowNumbers.allSatisfy({ $0 > 0 }),
              source.rows.allSatisfy({ $0.count <= 200 }) else {
            throw LabelSheetError.message(L("The source row information is inconsistent."))
        }
        var items: [Item] = []
        var totalCopies = 0
        let titles = source.columnTitles(firstRowIsHeader: options.firstRowIsHeader)
        if source.isTable {
            guard !options.textColumns.isEmpty || options.codeColumn != nil,
                  options.textColumns.count <= 6,
                  Set(options.textColumns).count == options.textColumns.count,
                  options.textColumns.allSatisfy({ titles.indices.contains($0) }),
                  options.codeColumn.map({ titles.indices.contains($0) }) ?? true,
                  options.copiesColumn.map({ titles.indices.contains($0) }) ?? true else {
                throw LabelSheetError.message(L("Select up to six text columns and an optional code column."))
            }
            for (index, row) in source.rows.enumerated() {
                if options.firstRowIsHeader && index == 0 { continue }
                if options.visibleOnly && source.visible.indices.contains(index) && !source.visible[index] { continue }
                let number = source.rowNumbers.indices.contains(index) ? source.rowNumbers[index] : index + 1
                func cell(_ column: Int) throws -> String {
                    guard row.indices.contains(column) else { throw LabelSheetError.message(L("Row %d has a missing field.", number)) }
                    return row[column]
                }
                let selectedText = try options.textColumns.map { try cell($0) }
                let text = zip(options.textColumns, selectedText).map { column, value in
                    return options.includeFieldNames ? "\(titles[column]): \(value)" : value
                }.joined(separator: "\n")
                guard options.codeColumn != nil || selectedText.contains(where: { !$0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }) else {
                    throw LabelSheetError.message(L("Row %d has no content in the selected fields.", number))
                }
                let copies: Int
                if let column = options.copiesColumn {
                    let value = try cell(column).trimmingCharacters(in: .whitespacesAndNewlines)
                    guard let count = Int(value), (1...5000).contains(count) else {
                        throw LabelSheetError.message(L("Row %d needs a whole copy count between 1 and 5,000.", number))
                    }; copies = count
                } else { copies = options.copies }
                totalCopies += copies
                guard totalCopies <= 5000 else { throw LabelSheetError.message(L("Select between 1 and 5,000 labels.")) }
                var code: TechnicalRender?
                if let column = options.codeColumn {
                    let value = try cell(column)
                    guard !value.isEmpty else { throw LabelSheetError.message(L("Row %d has an empty code value.", number)) }
                    let width = options.codeType == .code128 ? options.stock.width - 4 : min(22, options.stock.height - 6, options.stock.width * 0.43)
                    do {
                        let rendered = try TechnicalElement(codeType: options.codeType, value: value, widthMM: width, caption: true).render()
                        // Label output uses vectors only; do not retain a large PNG for every source row.
                        code = TechnicalRender(pngData: Data(), pdfData: rendered.pdfData, sizePoints: rendered.sizePoints)
                    }
                    catch { throw LabelSheetError.message(L("Row %d: %@", number, error.localizedDescription)) }
                }
                guard text.utf8.count <= 100_000 else { throw LabelSheetError.message(L("Row %d contains too much text.", number)) }
                items.append(Item(text: text, code: code, sourceRow: number, copies: copies))
            }
        } else {
            guard (source.text?.utf8.count ?? 0) <= 100_000 else { throw LabelSheetError.message(L("The selected text is too long for a label.")) }
            guard source.pdfData != nil || !(source.text ?? "").trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
                throw LabelSheetError.message(L("Select text, artwork or table rows to create labels."))
            }
            items = [Item(text: source.text ?? "", code: nil, sourceRow: 1, copies: options.copies)]
        }
        let count = items.reduce(0) { $0 + $1.copies }
        guard count > 0, count <= 5000 else { throw LabelSheetError.message(L("Select between 1 and 5,000 labels.")) }
        let pages = (count + options.startSlot + options.stock.capacity - 1) / options.stock.capacity
        guard pages <= 500 else { throw LabelSheetError.message(L("The label output is limited to 500 pages.")) }
        var artwork: CGPDFDocument?
        if let data = source.pdfData {
            guard data.count <= 64_000_000, let provider = CGDataProvider(data: data as CFData), let pdf = CGPDFDocument(provider), pdf.numberOfPages == 1 else {
                throw LabelSheetError.message(L("Choose a single PDF page or a page region for the label."))
            }; artwork = pdf
        }
        let mm = pointsPerMM
        let cell = CGRect(x: 0, y: 0, width: options.stock.width * mm, height: options.stock.height * mm)
        // Validate every source row before creating any output. Drawing uses exactly these measured fonts.
        var fonts: [NSFont] = []
        for item in items {
            let rect = textRect(in: cell, code: item.code, codeType: options.codeType)
            if let code = item.code {
                let target = codeRect(in: cell, code: code, codeType: options.codeType)
                guard target.minX >= 0, target.minY >= 0, target.maxX <= cell.maxX, target.maxY <= cell.maxY else {
                    throw LabelSheetError.message(L("Row %d code does not fit on the label.", item.sourceRow))
                }
            }
            if item.text.isEmpty || artwork != nil { fonts.append(.systemFont(ofSize: 10)); continue }
            var fitting: NSFont?
            for step in stride(from: 110, through: 51, by: -1) {
                let font = NSFont.systemFont(ofSize: Double(step) / 10)
                let string = attributed(item.text, font: font)
                let bounds = string.boundingRect(with: CGSize(width: rect.width, height: 100_000), options: [.usesLineFragmentOrigin, .usesFontLeading])
                if rect.width > 0 && rect.height > 0 && ceil(bounds.height) <= floor(rect.height) && ceil(bounds.width) <= ceil(rect.width) { fitting = font; break }
            }
            guard let font = fitting else { throw LabelSheetError.message(L("Row %d does not fit. Choose fewer fields or a larger label.", item.sourceRow)) }
            fonts.append(font)
        }
        let output = NSMutableData()
        guard let consumer = CGDataConsumer(data: output) else { throw LabelSheetError.message(L("Could not create label output.")) }
        var media = CGRect(x: 0, y: 0, width: 210 * mm, height: 297 * mm)
        guard let context = CGContext(consumer: consumer, mediaBox: &media, nil) else { throw LabelSheetError.message(L("Could not create label output.")) }
        var slot = options.startSlot
        context.beginPDFPage(nil)
        var rows: [Int] = []
        for (index, item) in items.enumerated() {
            for _ in 0..<item.copies {
                if slot == options.stock.capacity { context.endPDFPage(); context.beginPDFPage(nil); slot = 0 }
                let col = slot % options.stock.columns, row = slot / options.stock.columns
                let rect = CGRect(x: (options.stock.marginLeft + Double(col) * (options.stock.width + options.stock.gapX)) * mm,
                                  y: media.height - (options.stock.marginTop + Double(row) * (options.stock.height + options.stock.gapY) + options.stock.height) * mm,
                                  width: cell.width, height: cell.height)
                context.saveGState(); context.clip(to: rect)
                context.translateBy(x: rect.minX, y: rect.minY)
                if let page = artwork?.page(at: 1) {
                    context.concatenate(page.getDrawingTransform(.cropBox, rect: cell, rotate: 0, preserveAspectRatio: true)); context.drawPDFPage(page)
                } else {
                    if let code = item.code, let provider = CGDataProvider(data: code.pdfData as CFData), let pdf = CGPDFDocument(provider), let page = pdf.page(at: 1) {
                        let target = codeRect(in: cell, code: code, codeType: options.codeType)
                        guard target.minX >= 0, target.minY >= 0, target.maxX <= cell.maxX, target.maxY <= cell.maxY else {
                            context.restoreGState(); context.endPDFPage(); context.closePDF()
                            throw LabelSheetError.message(L("Row %d code does not fit on the label.", item.sourceRow))
                        }
                        context.saveGState(); context.concatenate(page.getDrawingTransform(.mediaBox, rect: target, rotate: 0, preserveAspectRatio: true)); context.drawPDFPage(page); context.restoreGState()
                    }
                    if !item.text.isEmpty {
                        NSGraphicsContext.saveGraphicsState(); NSGraphicsContext.current = NSGraphicsContext(cgContext: context, flipped: false)
                        attributed(item.text, font: fonts[index]).draw(with: textRect(in: cell, code: item.code, codeType: options.codeType), options: [.usesLineFragmentOrigin, .usesFontLeading])
                        NSGraphicsContext.restoreGraphicsState()
                    }
                }
                context.restoreGState(); rows.append(item.sourceRow); slot += 1
            }
        }
        context.endPDFPage(); context.closePDF()
        guard output.length <= 128_000_000 else { throw LabelSheetError.message(L("The label PDF exceeds the 128 MB output limit.")) }
        return LabelSheetResult(data: output as Data, labelCount: count, pageCount: pages, sourceRows: rows)
    }
    private static func attributed(_ text: String, font: NSFont) -> NSAttributedString {
        let style = NSMutableParagraphStyle(); style.lineBreakMode = .byWordWrapping
        return NSAttributedString(string: text, attributes: [.font: font, .foregroundColor: NSColor.black, .paragraphStyle: style])
    }
    private static func codeRect(in cell: CGRect, code: TechnicalRender, codeType: TechnicalCodeType) -> CGRect {
        let margin = 2 * pointsPerMM
        return CGRect(x: codeType == .code128 ? (cell.width - code.sizePoints.width) / 2 : cell.width - margin - code.sizePoints.width,
                      y: codeType == .code128 ? margin : (cell.height - code.sizePoints.height) / 2,
                      width: code.sizePoints.width, height: code.sizePoints.height)
    }
    private static func textRect(in cell: CGRect, code: TechnicalRender?, codeType: TechnicalCodeType) -> CGRect {
        let margin = 2 * pointsPerMM
        var result = cell.insetBy(dx: margin, dy: margin)
        if let code {
            if codeType == .code128 { result.origin.y += code.sizePoints.height + margin; result.size.height -= code.sizePoints.height + margin }
            else { result.size.width -= code.sizePoints.width + margin }
        }
        return result
    }
}
