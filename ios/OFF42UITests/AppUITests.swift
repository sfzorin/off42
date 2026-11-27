import UIKit
import XCTest

/// End-to-end tests on an iPhone and an iPad simulator (CI only): every fixture format opens in its viewer, files are
/// converted through the real Save as flow and the written bytes are checked with the engine, the generators draw
/// and save, and the open file survives rotation, dark mode and Arabic (right to left). App Store screenshots are
/// written to OFF42_SCREENSHOTS (TEST_RUNNER_OFF42_SCREENSHOTS for xcodebuild) and attached to the test results.
///
/// The system save dialog is the one part replaced: with OFF42_UITEST_EXPORT_DIR set the app writes what the user
/// would save into that folder (AppModel.output) instead of presenting fileExporter; `testSystemPickersAppear` checks
/// that the real open and save dialogs come up.
final class AppUITests: XCTestCase {
    private var app: XCUIApplication!
    private var exports: URL!

    private var isPad: Bool { UIDevice.current.userInterfaceIdiom == .pad }
    private var device: String { isPad ? "ipad" : "iphone" }

    override func setUpWithError() throws {
        continueAfterFailure = false
        exports = FileManager.default.temporaryDirectory.appendingPathComponent("exports-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: exports, withIntermediateDirectories: true)
        // iPad screenshots are taken in landscape (2752 × 2064 on a 13-inch iPad), where all three columns show.
        XCUIDevice.shared.orientation = isPad ? .landscapeLeft : .portrait
    }

    override func tearDownWithError() throws {
        app?.terminate()
        try? FileManager.default.removeItem(at: exports)
    }

    // MARK: - Helpers

    private func fixture(_ name: String) -> URL {
        Bundle(for: Self.self).url(forResource: name, withExtension: nil, subdirectory: "fixtures")!
    }

    private func launch(open files: [String] = [], start: String? = nil, language: String = "en", locale: String = "en_US",
                        exportStub: Bool = true) {
        app = XCUIApplication()
        if exportStub { app.launchEnvironment["OFF42_UITEST_EXPORT_DIR"] = exports.path }
        if !files.isEmpty { app.launchEnvironment["OFF42_UITEST_OPEN"] = files.map { fixture($0).path }.joined(separator: "\n") }
        if let start { app.launchEnvironment["OFF42_UITEST_START"] = start }
        app.launchArguments += ["-AppleLanguages", "(\(language))", "-AppleLocale", locale]
        app.launch()
        if let last = files.last { waitForTitle(last) }
    }

    private func element(_ identifier: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: identifier).firstMatch
    }

    @discardableResult
    private func wait(_ identifier: String, timeout: TimeInterval = 30, file: StaticString = #filePath, line: UInt = #line) -> XCUIElement {
        let found = element(identifier)
        XCTAssertTrue(found.waitForExistence(timeout: timeout), "\(identifier) did not appear", file: file, line: line)
        return found
    }

    private func waitUntil(_ timeout: TimeInterval = 30, _ message: String, _ condition: () -> Bool,
                           file: StaticString = #filePath, line: UInt = #line) {
        let end = Date().addingTimeInterval(timeout)
        while Date() < end {
            if condition() { return }
            RunLoop.current.run(until: Date().addingTimeInterval(0.25))
        }
        XCTFail(message, file: file, line: line)
    }

    /// The open file's name in the navigation bar.
    private func waitForTitle(_ name: String, file: StaticString = #filePath, line: UInt = #line) {
        let title = app.navigationBars.staticTexts[name]
        XCTAssertTrue(title.waitForExistence(timeout: 30), "\(name) is not the open file", file: file, line: line)
    }

    /// Goes back one level in the iPhone navigation stack.
    private func back() {
        // On iPad every column is shown; the first bar button there is the sidebar toggle, not Back.
        guard !isPad else { return }
        let button = app.navigationBars.buttons.element(boundBy: 0)
        if button.exists && button.isHittable { button.tap() }
    }

    /// Taps a row of the file or tool list, going back on iPhone when the list is not shown.
    private func tapRow(_ identifier: String) {
        let row = element(identifier)
        for _ in 0..<3 where !(row.exists && row.isHittable) {
            back()
            _ = row.waitForExistence(timeout: 3)
        }
        XCTAssertTrue(row.waitForExistence(timeout: 10), "\(identifier) not in the list")
        row.tap()
    }

    /// Shows a section's list: on iPhone back to the sections and into it, on iPad from the sidebar.
    private func showSection(_ section: String, file: StaticString = #filePath, line: UInt = #line) {
        let row = element("section-\(section)")
        for _ in 0..<4 where !(row.exists && row.isHittable) {
            back()
            _ = row.waitForExistence(timeout: 3)
        }
        XCTAssertTrue(row.exists && row.isHittable, "section \(section) not reachable", file: file, line: line)
        row.tap()
    }

    private func section(of name: String) -> String {
        switch (name as NSString).pathExtension {
        case "xlsx", "ods", "csv", "xls": return "spreadsheets"
        case "pptx", "odp": return "presentations"
        case "pdf": return "pdf"
        default: return "documents"
        }
    }

    private func select(file name: String) {
        let row = element("file-\(name)")
        if !(row.exists && row.isHittable) {
            back()
            if !row.waitForExistence(timeout: 3) || !row.isHittable { showSection(section(of: name)) }
        }
        XCTAssertTrue(row.waitForExistence(timeout: 10), "file-\(name) not in the list")
        row.tap()
        waitForTitle(name)
    }

    /// Scrolls until the element can be tapped (forms are lazy lists).
    /// Scrolls the form that holds `anchor` (an element visible now) until the element can be tapped. The drag is
    /// made at the anchor's column, so on iPad it scrolls the detail column, not the lists beside it.
    private func reveal(_ identifier: String, from anchor: String? = nil, file: StaticString = #filePath,
                        line: UInt = #line) -> XCUIElement {
        let target = element(identifier)
        let window = app.windows.firstMatch.frame
        var x = window.midX
        if let anchor {
            let frame = element(anchor).frame
            if !frame.isEmpty { x = frame.midX }
        }
        let origin = app.coordinate(withNormalizedOffset: CGVector(dx: 0, dy: 0))
        for _ in 0..<12 where !(target.exists && target.isHittable) {
            let start = origin.withOffset(CGVector(dx: x, dy: window.midY + window.height * 0.2))
            let end = origin.withOffset(CGVector(dx: x, dy: window.midY - window.height * 0.2))
            start.press(forDuration: 0.05, thenDragTo: end)
            RunLoop.current.run(until: Date().addingTimeInterval(0.4))
        }
        XCTAssertTrue(target.exists && target.isHittable, "\(identifier) cannot be reached", file: file, line: line)
        return target
    }

    private func dismissKeyboard() {
        let done = element("keyboard-done")
        if done.exists && done.isHittable { done.tap() }
    }

    private func replaceText(_ identifier: String, with text: String) {
        let field = wait(identifier)
        // Tap at the end of the text so the deletes remove all of it.
        field.coordinate(withNormalizedOffset: CGVector(dx: 0.97, dy: 0.5)).tap()
        let current = (field.value as? String) ?? ""
        if !current.isEmpty { field.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: current.count + 2)) }
        field.typeText(text)
        waitUntil(10, "\(identifier) is not \(text)") { (field.value as? String) == text }
    }

    /// The screen as the user sees it: XCUIScreen returns the portrait buffer, so landscape shots are turned upright.
    private func upright(_ shot: XCUIScreenshot) -> UIImage {
        let image = shot.image
        guard let cg = image.cgImage, XCUIDevice.shared.orientation.isLandscape, cg.width < cg.height else { return image }
        let width = CGFloat(cg.width), height = CGFloat(cg.height)
        let format = UIGraphicsImageRendererFormat()
        format.scale = 1
        let clockwise = XCUIDevice.shared.orientation == .landscapeRight
        return UIGraphicsImageRenderer(size: CGSize(width: height, height: width), format: format).image { context in
            let c = context.cgContext
            if clockwise {
                c.translateBy(x: height, y: 0)
                c.rotate(by: .pi / 2)
            } else {
                c.translateBy(x: 0, y: width)
                c.rotate(by: -.pi / 2)
            }
            UIImage(cgImage: cg).draw(in: CGRect(x: 0, y: 0, width: width, height: height))
        }
    }

    /// A menu item, by identifier when SwiftUI passes it to the menu, else by its label.
    private func menuItem(_ identifier: String, label: String) -> XCUIElement {
        let byID = app.buttons[identifier]
        if byID.waitForExistence(timeout: 3) { return byID }
        let byLabel = app.buttons[label]
        XCTAssertTrue(byLabel.waitForExistence(timeout: 5), "menu item \(label) missing")
        return byLabel
    }

    private func saveAs(_ format: String) {
        wait("save-menu").tap()
        menuItem("save-\(format)", label: format.uppercased()).tap()
    }

    /// Waits for the app to write a file into the export folder and returns its bytes.
    private func written(_ name: String, file: StaticString = #filePath, line: UInt = #line) -> Data {
        let url = exports.appendingPathComponent(name)
        waitUntil(30, "\(name) was not written", { FileManager.default.fileExists(atPath: url.path) }, file: file, line: line)
        RunLoop.current.run(until: Date().addingTimeInterval(0.3))
        return (try? Data(contentsOf: url)) ?? Data()
    }

    private func written(prefix: String, ext: String) -> (String, Data) {
        var found: URL?
        waitUntil(30, "no \(prefix)*.\(ext) written") {
            found = (try? FileManager.default.contentsOfDirectory(at: exports, includingPropertiesForKeys: nil))?
                .first { $0.lastPathComponent.hasPrefix(prefix) && $0.pathExtension == ext }
            return found != nil
        }
        RunLoop.current.run(until: Date().addingTimeInterval(0.3))
        return (found?.lastPathComponent ?? "", (try? Data(contentsOf: found!)) ?? Data())
    }

    private func screenshot(_ name: String) {
        RunLoop.current.run(until: Date().addingTimeInterval(1))
        let image = upright(XCUIScreen.main.screenshot())
        let attachment = XCTAttachment(image: image)
        attachment.name = "\(device)-\(name)"
        attachment.lifetime = .keepAlways
        add(attachment)
        if let folder = ProcessInfo.processInfo.environment["OFF42_SCREENSHOTS"] {
            let url = URL(fileURLWithPath: folder, isDirectory: true)
            try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
            try? image.pngData()?.write(to: url.appendingPathComponent("\(device)-\(name).png"))
        }
    }

    private let png = Data([0x89, 0x50, 0x4E, 0x47])

    // MARK: - Tests

    func testDocumentsOpenInEveryFormat() {
        let names = ["sample.md", "sample.odt", "sample.rtf", "sample.html", "sample.txt", "sample.docx"]
        launch(open: names)
        for name in names {
            select(file: name)
            wait("doc-stats")
            let page = app.webViews["doc-preview"]
            XCTAssertTrue(page.waitForExistence(timeout: 20), name)
            let expected = name == "sample.txt" ? "Plain text notes" : "Quarterly"
            let text = page.staticTexts.containing(NSPredicate(format: "label CONTAINS %@", expected)).firstMatch
            XCTAssertTrue(text.waitForExistence(timeout: 20), "\(name): \(expected) not shown")
        }
        XCTAssertTrue((element("doc-stats").label).contains("words"))
        screenshot("1-document")
    }

    func testSpreadsheetsShowTheirCells() {
        let names = ["sample.ods", "sample.csv", "sample.xls", "sample.xlsx"]
        launch(open: names)
        for name in names {
            select(file: name)
            let cell = wait("cell-0-0")
            waitUntil(20, "\(name): A1 is not Item") { (cell.value as? String) == "Item" }
            XCTAssertEqual(element("sheet-note").exists, name == "sample.xls", name)
            XCTAssertTrue(element("sheet-tab-0").exists, name)
        }
        // The formula total of the CSV fixture is calculated by the engine.
        screenshot("2-spreadsheet")
    }

    func testPresentationsShowTheirSlides() {
        launch(open: ["sample.odp", "sample.pptx"])
        for name in ["sample.odp", "sample.pptx"] {
            select(file: name)
            wait("slide-0")
            wait("slide-canvas")
        }
        let second = element("slide-1")
        if second.exists {
            second.tap()
            waitUntil(10, "slide 2 is not selected") { element("slide-1").isSelected }
            element("slide-0").tap()
        }
        screenshot("3-presentation")
    }

    func testPdfPagesRenderWithTheEngineInfo() {
        launch(open: ["sample.pdf"])
        wait("pdf-view")
        let label = wait("pdf-page-label")
        XCTAssertEqual(label.label, "Page 1 of 1")
        screenshot("4-pdf")
        wait("pdf-info-button").tap()
        wait("pdf-info-pages")
        XCTAssertTrue(app.staticTexts["Pages"].exists)
        wait("done").tap()
    }

    func testSaveAsConvertsThroughTheSaveFlow() {
        launch(open: ["sample.md", "sample.docx", "sample.csv", "sample.pptx", "sample.xls"])

        select(file: "sample.md")
        saveAs("docx")
        let docx = written("sample.docx")
        XCTAssertTrue(((try? documentHtml(bytes: docx, from: "docx")) ?? "").contains("Quarterly"))

        // A Word file keeps package parts that ODT cannot carry: the app asks first.
        select(file: "sample.docx")
        saveAs("odt")
        let convert = app.alerts.buttons["Convert"].firstMatch
        XCTAssertTrue(convert.waitForExistence(timeout: 20))
        convert.tap()
        let odt = written("sample.odt")
        XCTAssertTrue(((try? documentHtml(bytes: odt, from: "odt")) ?? "").contains("Quarterly"))

        select(file: "sample.csv")
        saveAs("ods")
        let ods = written("sample.ods")
        let book = try? spreadsheetOpen(bytes: ods, kind: "ods", maxRows: 100, maxCols: 100)
        XCTAssertTrue(book?.sheets.first?.cells.contains { $0.text == "Item" } == true)

        select(file: "sample.pptx")
        saveAs("odp")
        let odp = written("sample.odp")
        XCTAssertEqual((try? presentationOutline(bytes: odp, from: "odp"))?.count,
                       (try? presentationOutline(bytes: Data(contentsOf: fixture("sample.pptx")), from: "pptx"))?.count)

        // XLS is read-only: its values export as CSV.
        select(file: "sample.xls")
        wait("save-menu").tap()
        menuItem("values-csv", label: "Sheet values as CSV").tap()
        let (name, csv) = written(prefix: "sample - ", ext: "csv")
        XCTAssertTrue(String(decoding: csv, as: UTF8.self).hasPrefix("Item,"), name)
        wait("notice")
    }

    func testCodesLabelSheetsAndMarks() {
        launch(start: "labels/codes")
        let preview = wait("code-preview")
        replaceText("code-value", with: "OFF42-0001")
        dismissKeyboard()
        XCTAssertTrue(preview.waitForExistence(timeout: 10))
        screenshot("5-codes")
        reveal("code-save-png", from: "code-preview").tap()
        let code = written("qr.png")
        XCTAssertEqual(code.prefix(4), png)
        XCTAssertNotNil(UIImage(data: code))

        tapRow("tool-labelSheet")
        reveal("make-sheet", from: "stock-picker").tap()
        wait("sheet-status")
        reveal("sheet-save", from: "make-sheet").tap()
        let sheet = written("labels.pdf")
        XCTAssertGreaterThanOrEqual((try? pdfInfo(bytes: sheet))?.pages ?? 0, 1)

        tapRow("tool-marks")
        let search = app.searchFields.firstMatch
        XCTAssertTrue(search.waitForExistence(timeout: 20))
        search.tap()
        search.typeText("CE")
        let mark = wait("mark-ce")
        if app.keyboards.count > 0, app.keyboards.buttons["search"].exists { app.keyboards.buttons["search"].tap() }
        mark.tap()
        reveal("mark-save-png", from: "done").tap()
        let markPNG = written("ce.png")
        XCTAssertEqual(markPNG.prefix(4), png)
    }

    func testSignaturesAndStamps() {
        launch(start: "signatures/signature")
        let name = wait("sig-name")
        name.tap()
        name.typeText("Anna Smith\n")
        reveal("sig-generate", from: "sig-name").tap()
        // The variants follow the form: scroll to the first one.
        let save = reveal("signature-save-0", from: "sig-generate")
        wait("signature-image-0")
        screenshot("6-signatures")
        save.tap()
        let signature = written("signature-Anna-Smith.png")
        XCTAssertEqual(signature.prefix(4), png)
        XCTAssertNotNil(UIImage(data: signature))

        tapRow("tool-stamp")
        wait("stamp-preview")
        screenshot("7-stamp")
        wait("stamp-save").tap()
        let (_, stamp) = written(prefix: "stamp-", ext: "png")
        XCTAssertEqual(stamp.prefix(4), png)
    }

    func testOpenFileSurvivesRotation() {
        launch(open: ["sample.xlsx"])
        wait("cell-0-0")
        let turned: UIDeviceOrientation = isPad ? .portrait : .landscapeLeft
        XCUIDevice.shared.orientation = turned
        waitForTitle("sample.xlsx")
        let cell = wait("cell-0-0")
        XCTAssertEqual(cell.value as? String, "Item")
        XCUIDevice.shared.orientation = isPad ? .landscapeLeft : .portrait
        waitForTitle("sample.xlsx")
        XCTAssertEqual(wait("cell-0-0").value as? String, "Item")
    }

    /// Runs in a separate xcodebuild invocation after CI switched the simulator to dark mode
    /// (`xcrun simctl ui <device> appearance dark`, TEST_RUNNER_OFF42_SYSTEM_APPEARANCE=dark); the light run checks
    /// the opposite. Either way the app must follow the system: its bars are drawn dark or light.
    func testAppearanceFollowsTheSystem() {
        let dark = ProcessInfo.processInfo.environment["OFF42_SYSTEM_APPEARANCE"] == "dark"
        launch(open: ["sample.pptx"])
        wait("slide-canvas")
        RunLoop.current.run(until: Date().addingTimeInterval(1))
        // The navigation bar above the slide and the canvas around it.
        let image = upright(XCUIScreen.main.screenshot())
        let brightness = averageBrightness(image, rows: 0.07...0.10)
        if dark {
            XCTAssertLessThan(brightness, 0.35, "bar brightness \(brightness) in dark mode")
            screenshot("8-dark")
        } else {
            XCTAssertGreaterThan(brightness, 0.6, "bar brightness \(brightness) in light mode")
        }
    }

    func testArabicIsRightToLeft() {
        launch(start: "documents", language: "ar", locale: "ar_SA")
        let open = wait("open-file")
        XCTAssertEqual(open.label, "فتح ملف")
        let window = app.windows.firstMatch.frame
        // Right to left: the sidebar is on the right on iPad; on iPhone the large title starts at the right edge.
        let anchor = isPad ? app.staticTexts["OFF42"].firstMatch : app.navigationBars.staticTexts["المستندات"].firstMatch
        XCTAssertTrue(anchor.waitForExistence(timeout: 20), "Arabic title missing")
        XCTAssertGreaterThan(anchor.frame.midX, window.midX, "layout is not right to left")
        screenshot("9-arabic")
    }

    func testNavigationAndSystemPickersAppear() {
        launch(exportStub: false)
        if !isPad { wait("section-documents").tap() }
        wait("open-file").tap()
        // The system document picker (its own process) shows a Cancel button.
        let cancel = app.buttons["Cancel"].firstMatch
        XCTAssertTrue(cancel.waitForExistence(timeout: 30), "document picker did not appear")
        cancel.tap()
        XCTAssertTrue(element("open-file").waitForExistence(timeout: 20))
        waitUntil(20, "picker still shown") { element("open-file").isHittable }

        // Through the sections to a generator.
        showSection("presentations")
        wait("open-file")
        showSection("labels")
        tapRow("tool-codes")
        wait("code-preview")

        // The real save dialog (fileExporter) comes up for Save PNG.
        reveal("code-save-png", from: "code-preview").tap()
        // Its look differs between iOS versions (a Cancel button, or a location title over a file name bar), so any
        // of them counts; the suggested file name "qr" shows in the newer layout.
        waitUntil(30, "save dialog did not appear") {
            app.buttons["Cancel"].exists || app.buttons["Move"].exists || app.buttons["Save"].exists
                || app.staticTexts["On My iPhone"].exists || app.staticTexts["On My iPad"].exists
        }
    }

    // MARK: - Pixels

    private func averageBrightness(_ image: UIImage, rows: ClosedRange<CGFloat>) -> CGFloat {
        guard let cgImage = image.cgImage else { return 1 }
        let width = cgImage.width, height = cgImage.height
        let context = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4,
                                space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
        context.draw(cgImage, in: CGRect(x: 0, y: 0, width: width, height: height))
        let pixels = context.data!.assumingMemoryBound(to: UInt8.self)
        var total: CGFloat = 0
        var count: CGFloat = 0
        // The bitmap's first row in memory is the top of the image.
        let top = Int(CGFloat(height) * rows.lowerBound), bottom = Int(CGFloat(height) * rows.upperBound)
        for row in top..<bottom {
            for x in stride(from: 0, to: width, by: 8) {
                let i = (row * width + x) * 4
                total += (CGFloat(pixels[i]) * 0.299 + CGFloat(pixels[i + 1]) * 0.587 + CGFloat(pixels[i + 2]) * 0.114) / 255
                count += 1
            }
        }
        return count > 0 ? total / count : 1
    }
}
