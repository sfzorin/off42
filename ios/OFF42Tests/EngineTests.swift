import PDFKit
import UniformTypeIdentifiers
import XCTest
@testable import OFF42

/// The app's engine layer against the real off42 engine (the same fixtures as the Android tests,
/// android/app/src/androidTest/assets/fixtures).
final class EngineTests: XCTestCase {
    private func fixture(_ name: String) throws -> Data {
        let url = try XCTUnwrap(Bundle(for: Self.self).url(forResource: name, withExtension: nil, subdirectory: "fixtures"),
                                "fixture \(name)")
        return try Data(contentsOf: url)
    }

    private func load(_ name: String) throws -> OpenedFile {
        let ext = Files.engineExtension(name: name, type: nil)
        let kind = try XCTUnwrap(fileKind(name: ext), name)
        return OpenedFile.load(id: 1, name: name, ext: ext, kind: kind, bytes: try fixture(name))
    }

    func testEveryFixtureOpensInItsViewer() throws {
        for name in ["sample.docx", "sample.odt", "sample.rtf", "sample.md", "sample.html", "sample.txt"] {
            let file = try load(name)
            XCTAssertNil(file.error, name)
            guard case let .document(html, info)? = file.content else { return XCTFail("\(name): not a document") }
            XCTAssertGreaterThan(info.words, 3, name)
            XCTAssertTrue(html.contains(name == "sample.txt" ? "Plain text notes" : "Quarterly"), name)
        }
        for name in ["sample.xlsx", "sample.ods", "sample.csv", "sample.xls"] {
            let file = try load(name)
            guard case let .spreadsheet(book)? = file.content else { return XCTFail("\(name): \(file.error ?? "")") }
            XCTAssertEqual(book.readOnly, name == "sample.xls", name)
            XCTAssertEqual(Sheets.grid(book.sheets[0])[0][0], "Item", name)
        }
        for name in ["sample.pptx", "sample.odp"] {
            let file = try load(name)
            guard case let .presentation(slides)? = file.content else { return XCTFail("\(name): \(file.error ?? "")") }
            XCTAssertFalse(slides.isEmpty, name)
            XCTAssertFalse(SlideCanvas.title(of: slides[0]).isEmpty, name)
        }
        let pdf = try load("sample.pdf")
        guard case let .pdf(info)? = pdf.content else { return XCTFail(pdf.error ?? "pdf") }
        XCTAssertFalse(info.encrypted)
        XCTAssertGreaterThan(info.pages, 0)
        // PDFKit, which draws the pages, agrees with the engine.
        XCTAssertEqual(PDFDocument(data: pdf.bytes)?.pageCount, Int(info.pages))
    }

    func testBrokenFilesReportTheEngineMessage() throws {
        let file = OpenedFile.load(id: 1, name: "broken.docx", ext: "docx", kind: .document, bytes: Data("not a zip".utf8))
        XCTAssertNil(file.content)
        XCTAssertFalse((file.error ?? "").isEmpty)
        XCTAssertFalse(file.error!.contains("Off42Error"), "the message, not the enum: \(file.error!)")
    }

    func testConversionsKeepTheContent() throws {
        let docx = try documentConvert(bytes: try fixture("sample.md"), from: "md", to: "docx", dropPackageParts: false)
        XCTAssertTrue(try documentHtml(bytes: docx, from: "docx").contains("Quarterly"))

        XCTAssertThrowsError(try documentConvert(bytes: try fixture("sample.docx"), from: "docx", to: "odt", dropPackageParts: false)) {
            guard case Off42Error.PackagePartsWouldBeLost = $0 else { return XCTFail("\($0)") }
        }
        let odt = try documentConvert(bytes: try fixture("sample.docx"), from: "docx", to: "odt", dropPackageParts: true)
        XCTAssertTrue(try documentHtml(bytes: odt, from: "odt").contains("Quarterly"))

        let ods = try spreadsheetConvert(bytes: try fixture("sample.csv"), from: "csv", to: "ods")
        XCTAssertTrue(try spreadsheetOpen(bytes: ods, kind: "ods", maxRows: 10, maxCols: 10).sheets[0].cells.contains { $0.text == "Item" })

        let pptx = try fixture("sample.pptx")
        let odp = try presentationConvert(bytes: pptx, from: "pptx", to: "odp")
        XCTAssertEqual(try presentationOutline(bytes: odp, from: "odp").count, try presentationOutline(bytes: pptx, from: "pptx").count)

        let csv = String(decoding: try spreadsheetValuesCsv(bytes: try fixture("sample.xls"), from: "xls", sheet: 0), as: UTF8.self)
        XCTAssertTrue(csv.hasPrefix("Item,"), csv)
    }

    /// The model's Save as flow: conversion, the package-parts question, and the bytes handed to the save dialog.
    @MainActor
    func testSaveAsAsksBeforeDroppingPackagePartsAndWritesTheConvertedFile() async throws {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        defer { try? FileManager.default.removeItem(at: folder) }
        let model = AppModel(environment: ["OFF42_UITEST_EXPORT_DIR": folder.path])
        let opened = await model.openBytes(name: "sample.docx", type: nil, bytes: try fixture("sample.docx"))
        let file = try XCTUnwrap(opened)
        XCTAssertEqual(model.section, .documents)
        XCTAssertEqual(model.file(in: .documents)?.id, file.id)

        await model.saveAs(file, to: "odt")
        let question = try XCTUnwrap(model.question)
        XCTAssertEqual(question.format, "odt")
        await model.answer(question, convert: true)
        let written = try Data(contentsOf: folder.appendingPathComponent("sample.odt"))
        XCTAssertTrue(try documentHtml(bytes: written, from: "odt").contains("Quarterly"))

        // Without the test folder the bytes wait for the system save dialog.
        let dialog = AppModel(environment: [:])
        let openedCSV = await dialog.openBytes(name: "sample.csv", type: nil, bytes: try fixture("sample.csv"))
        let csv = try XCTUnwrap(openedCSV)
        await dialog.saveAs(csv, to: "xlsx")
        XCTAssertEqual(dialog.pendingExport?.name, "sample.xlsx")
        XCTAssertEqual(dialog.pendingExport?.bytes.prefix(2), Data("PK".utf8))

        let photo = await dialog.openBytes(name: "photo.jpeg", type: nil, bytes: Data([0xFF, 0xD8]))
        XCTAssertNil(photo)
        XCTAssertEqual(dialog.notice?.isError, true)
    }

    func testFileNamesAndTypes() {
        XCTAssertEqual(Files.engineExtension(name: "Report.DOCX", type: nil), "docx")
        XCTAssertEqual(Files.engineExtension(name: "notes.markdown", type: nil), "md")
        XCTAssertEqual(Files.engineExtension(name: "download", type: .pdf), "pdf")
        XCTAssertEqual(Files.engineExtension(name: "table", type: .commaSeparatedText), "csv")
        XCTAssertEqual(Files.contentType(ext: "png"), .png)
        XCTAssertEqual(Files.contentType(ext: "pdf"), .pdf)
        XCTAssertTrue(Files.contentType(ext: "docx").conforms(to: .data))
        XCTAssertEqual(Files.contentType(ext: "docx").preferredFilenameExtension, "docx")
        XCTAssertTrue(Files.pickerTypes(.spreadsheet).contains { $0.preferredFilenameExtension == "xls" })
        XCTAssertEqual(Files.pickerTypes(.pdf), [.pdf])
        XCTAssertEqual(Files.safeName("a/b:c?.csv"), "a_b_c_.csv")
        XCTAssertEqual(OpenedFile(id: 1, name: "x.y.docx", ext: "docx", kind: .document, bytes: Data(), content: nil, error: nil).stem, "x.y")
    }

    func testGridHelpers() {
        XCTAssertEqual(Sheets.columnName(0), "A")
        XCTAssertEqual(Sheets.columnName(25), "Z")
        XCTAssertEqual(Sheets.columnName(26), "AA")
        XCTAssertEqual(Sheets.columnName(701), "ZZ")
        XCTAssertEqual(Sheets.columnName(702), "AAA")
        XCTAssertTrue(Sheets.looksNumeric("-1,234.5"))
        XCTAssertTrue(Sheets.looksNumeric("12%"))
        XCTAssertFalse(Sheets.looksNumeric("Tea"))
        XCTAssertFalse(Sheets.looksNumeric(""))
        let sheet = Sheet(name: "S", rows: 2, cols: 1, truncated: false, cells: [Cell(row: 3, col: 2, text: "x")])
        let grid = Sheets.grid(sheet)
        XCTAssertEqual(grid.count, 4)
        XCTAssertEqual(grid[0].count, 3)
        XCTAssertEqual(grid[3][2], "x")
    }

    func testDocumentPageBlocksScriptsAndNetwork() {
        let page = DocumentPage.html("<!doctype html><html><head><meta charset=\"utf-8\"></head><body><p>Hi</p></body></html>")
        let csp = page.range(of: "Content-Security-Policy")
        let body = page.range(of: "<body>")
        XCTAssertNotNil(csp)
        XCTAssertTrue(page.contains("default-src 'none'"))
        if let csp, let body { XCTAssertLessThan(csp.lowerBound, body.lowerBound) }
        let wrapped = DocumentPage.html("<p>fragment</p>")
        XCTAssertTrue(wrapped.hasPrefix("<!doctype html>"))
        XCTAssertTrue(wrapped.contains("default-src 'none'") && wrapped.contains("<p>fragment</p>"))
    }

    func testGenerators() throws {
        let png = try barcodePng(format: "qr", value: "OFF42-0001", widthMm: 30)
        XCTAssertEqual(png.prefix(4), Data([0x89, 0x50, 0x4E, 0x47]))
        XCTAssertTrue(try barcodeSvg(format: "code128", value: "OFF42", widthMm: 40, caption: true).contains("<svg"))

        let stocks = try labelStocks()
        XCTAssertFalse(stocks.isEmpty)
        let labels = parseLabelLines(text: "Box 1 | BOX-1 | 3\nBox 2 | BOX-2\nShelf A")
        XCTAssertEqual(labels.map(\.copies), [3, 1, 1])
        let sheet = try labelSheetPdf(request: LabelSheetRequest(stock: stocks[0].id, width: 70, height: 37, gapX: 0, gapY: 0,
                                                                 firstSlot: 1, codeFormat: "qr", labels: labels))
        XCTAssertGreaterThanOrEqual(try pdfInfo(bytes: sheet).pages, 1)
        XCTAssertNotNil(PDFDocument(data: sheet)?.page(at: 0))

        let marks = try productMarks(language: "de")
        XCTAssertGreaterThan(marks.count, 50)
        XCTAssertTrue(marks.contains { $0.id == "ce" })
        XCTAssertEqual(try productMarkPng(id: "ce", widthMm: 12).prefix(4), Data([0x89, 0x50, 0x4E, 0x47]))

        let options = try signatureOptions()
        XCTAssertEqual(options.inks.map(\.value), ["blue", "violet", "black", "red", "green"])
        let designs = try signatureDesigns(count: 6, form: 0, seed: 42)
        XCTAssertEqual(designs.count, 6)
        XCTAssertEqual(designs, try signatureDesigns(count: 6, form: 0, seed: 42))
        let signature = try signaturePng(name: "Anna Smith", design: designs[0], impression: 7, ink: "blue", pen: 0, pixelHeight: 130)
        XCTAssertEqual(signature, try signaturePng(name: "Anna Smith", design: designs[0], impression: 7, ink: "blue", pen: 0, pixelHeight: 130))

        let templates = try stampTemplates(language: "en")
        let template = try XCTUnwrap(templates.first)
        XCTAssertEqual(try stampPng(template: template.id, values: [:], ink: "", impression: 3, pixelScale: 0.5).prefix(4),
                       Data([0x89, 0x50, 0x4E, 0x47]))
        XCTAssertNotEqual(Engine.randomSeed(), Engine.randomSeed())
    }

    func testSiteLanguageCodes() {
        XCTAssertEqual(siteLanguage(["ar"]), "ar")
        XCTAssertEqual(siteLanguage(["zh-Hant"]), "zh-Hant")
        XCTAssertEqual(siteLanguage(["zh-Hans"]), "zh-Hans")
        XCTAssertEqual(siteLanguage(["zh-TW"]), "zh-Hant")
        XCTAssertEqual(siteLanguage(["pt-PT"]), "pt")
        XCTAssertEqual(siteLanguage(["en-GB"]), "en")
        XCTAssertEqual(siteLanguage(["fil"]), "fil")
        XCTAssertEqual(siteLanguage(["Base"]), "en")
        XCTAssertEqual(siteLanguage([]), "en")
    }
}
