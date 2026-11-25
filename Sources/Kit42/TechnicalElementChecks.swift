import AppKit
import ImageIO
import Vision

/// Cross-format checks used by the shared audit executable. These decode the
/// emitted artwork using Vision, independently of the bwip-js encoder.
public enum TechnicalElementChecks {
    public static func run() -> [String] {
        var failures: [String] = []
        func check(_ condition: Bool, _ name: String) { if !condition { failures.append(name) } }
        for type in TechnicalCodeType.allCases {
            do {
                let element = TechnicalElement(codeType: type, value: "PCB-000001", widthMM: type == .code128 ? 55 : 20)
                let output = try element.render()
                check(TechnicalElement.fromPNG(output.pngData) == element, "\(type.rawValue): PNG recipe roundtrip")
                check(TechnicalElement.fromDescription(element.encodedDescription) == element, "\(type.rawValue): description roundtrip")
                check(abs(output.sizePoints.width - element.widthMM * 72 / 25.4) < 0.000001, "\(type.rawValue): physical width")
                let source = CGImageSourceCreateWithData(output.pngData as CFData, nil)!
                let png = CGImageSourceCreateImageAtIndex(source, 0, nil)!
                check(try decoded(png).contains(element.value), "\(type.rawValue): Vision PNG decode")
                let provider = CGDataProvider(data: output.pdfData as CFData)!
                guard let document = CGPDFDocument(provider), let page = document.page(at: 1) else {
                    failures.append("\(type.rawValue): PDF page missing"); continue
                }
                let bounds = page.getBoxRect(.mediaBox)
                check(abs(bounds.width - output.sizePoints.width) < 0.01 && abs(bounds.height - output.sizePoints.height) < 0.01,
                      "\(type.rawValue): PDF physical dimensions")
                guard let image = renderPDF(page) else { failures.append("\(type.rawValue): PDF raster failed"); continue }
                check(try decoded(image).contains(element.value), "\(type.rawValue): Vision PDF decode")
            } catch { failures.append("\(type.rawValue): \(error)") }
        }
        for type in [TechnicalCodeType.qr, .dataMatrix, .aztec] {
            do {
                let value = "Плата №42 ^ECI000003"
                let output = try TechnicalElement(codeType: type, value: value, widthMM: 35, caption: false).render()
                let source = CGImageSourceCreateWithData(output.pngData as CFData, nil)!
                check(try decoded(CGImageSourceCreateImageAtIndex(source, 0, nil)!).contains(value), "\(type.rawValue): Unicode decode")
            } catch { failures.append("\(type.rawValue): Unicode \(error)") }
        }
        // Catalogue imported from Lably: 97 artworks, earlier ids kept, conformity marks carry use conditions.
        let catalogue = TechnicalSymbols.all
        check(catalogue.count == 97 && Set(catalogue.map(\.id)).count == 97, "Mark catalogue has 97 unique entries")
        for id in ["protective-earth", "functional-earth", "chassis-ground", "equipotentiality", "dc", "ac", "positive-polarity",
                   "negative-polarity", "esd", "fuse", "high-voltage", "warning", "insulation-class", "fan-cooled",
                   "ce", "eac", "ukca", "iso-9001", "osha-ghs-02-flame", "weee"] {
            check(catalogue.contains { $0.id == id }, "Mark catalogue contains \(id)")
        }
        check(catalogue.first { $0.id == "ce" }.map { !$0.notice.isEmpty && $0.category == "certification" } == true, "CE shows its use conditions")
        check(catalogue.first { $0.id == "iso-9001" }.map { $0.unverified && !$0.notice.isEmpty } == true, "Withheld Lably artwork is insertable with a warning")
        check(catalogue.first { $0.id == "bis" }?.needsNumber == true, "BIS asks for its licence number")
        check(Set(catalogue.map(\.category)).isSubset(of: Set(TechnicalSymbols.categories)), "Every mark belongs to a listed category")
        check(TechnicalSymbols.search("EAEU").contains { $0.id == "eac" } && TechnicalSymbols.search("dc").first?.id == "dc", "Mark search by region and exact id")
        for symbol in TechnicalSymbols.all {
            do {
                let element = TechnicalElement(symbolID: symbol.id)
                let output = try element.render()
                check(TechnicalElement.fromPNG(output.pngData) == element, "\(symbol.id): recipe roundtrip")
                guard let provider = CGDataProvider(data: output.pdfData as CFData), let document = CGPDFDocument(provider),
                      let page = document.page(at: 1), let image = renderPDF(page) else {
                    failures.append("\(symbol.id): missing symbol output"); continue
                }
                check(hasInk(image), "\(symbol.id): nonblank symbol output")
            } catch { failures.append("\(symbol.id): \(error)") }
        }
        let invalid: [TechnicalElement] = [
            .init(value: ""), .init(symbolID: "unknown"), .init(value: "A", widthMM: .nan),
            .init(value: "A", widthMM: .infinity), .init(value: "A", widthMM: 0),
            .init(value: "A", widthMM: 251), .init(value: String(repeating: "a", count: 1025)),
            .init(value: "a\nb"), .init(codeType: .code128, value: "Плата", widthMM: 80),
            .init(value: "PCB-000001", widthMM: 3),
            .init(value: String(repeating: "W", count: 100), widthMM: 25, caption: true),
        ]
        for (index, element) in invalid.enumerated() {
            do { _ = try element.render(); failures.append("Invalid input \(index) accepted") } catch { }
        }
        check(TechnicalElement.fromDescription("not a recipe") == nil, "Malformed recipe rejected")
        check(TechnicalElement.fromPNG(Data("not a PNG".utf8)) == nil, "Malformed PNG rejected")
        return failures
    }

    /// Uses the actual native controls/editor. No document or printer is opened.
    public static func runUI() -> [String] {
        var failures: [String] = []
        func check(_ condition: Bool, _ name: String) { if !condition { failures.append(name) } }
        let fresh = TechnicalElementEditor(initial: nil, initialValue: "PCB-001") { _, _ in }
        check(Double(fresh.width.stringValue) == 20 && fresh.rendered?.0.widthMM == 20 && fresh.rendered?.0.value == "PCB-001" && fresh.alert.buttons[0].isEnabled,
              "Fresh code editor starts at 20 mm with a valid preview and Insert enabled")
        let freshSymbol = TechnicalElementEditor(initial: nil, initialValue: "") { _, _ in }
        check(Double(freshSymbol.width.stringValue) == 20 && freshSymbol.rendered?.0.symbolID != nil && freshSymbol.alert.buttons[0].isEnabled,
              "Fresh symbol editor starts at 20 mm with Insert enabled")
        let element = TechnicalElement(codeType: .qr, value: "PCB-000001", widthMM: 20)
        let editor = TechnicalElementEditor(initial: element, initialValue: "") { _, _ in }
        check(editor.rendered?.0 == element && editor.alert.buttons[0].isEnabled, "Editor restores editable recipe")
        editor.width.stringValue = "nan"; editor.update()
        check(editor.rendered == nil && !editor.alert.buttons[0].isEnabled && !editor.errorLabel.stringValue.isEmpty, "Editor rejects nonfinite width")
        editor.width.stringValue = "20"; editor.value.stringValue = ""; editor.update()
        check(!editor.alert.buttons[0].isEnabled, "Editor rejects empty code")
        editor.value.stringValue = "PCB-000002"; editor.update()
        check(editor.rendered?.0.value == "PCB-000002" && editor.alert.buttons[0].isEnabled, "Editor recovers and regenerates changed value")
        editor.caption.performClick(nil)
        check(editor.rendered?.0.caption == false, "Caption checkbox native action")
        editor.mode.selectedSegment = 0
        _ = editor.mode.sendAction(editor.mode.action, to: editor.mode.target)
        check(editor.rendered?.0.symbolID != nil && editor.codeRows.isHidden, "Symbols native action")
        editor.search.stringValue = "no-such-symbol"
        editor.controlTextDidChange(Notification(name: NSControl.textDidChangeNotification, object: editor.search))
        check(editor.rendered == nil && !editor.alert.buttons[0].isEnabled, "Empty symbol search disables insertion")
        editor.search.stringValue = "dc"
        editor.controlTextDidChange(Notification(name: NSControl.textDidChangeNotification, object: editor.search))
        check(editor.rendered?.0.symbolID == "dc", "Symbol search resolves artwork")
        return failures
    }

    private static func decoded(_ image: CGImage) throws -> [String] {
        let request = VNDetectBarcodesRequest()
        try VNImageRequestHandler(cgImage: image, options: [:]).perform([request])
        return (request.results ?? []).compactMap(\.payloadStringValue)
    }

    private static func renderPDF(_ page: CGPDFPage) -> CGImage? {
        let box = page.getBoxRect(.mediaBox)
        let scale: CGFloat = 300 / 72
        guard let context = CGContext(data: nil, width: Int(ceil(box.width * scale)), height: Int(ceil(box.height * scale)),
                                      bitsPerComponent: 8, bytesPerRow: 0, space: CGColorSpaceCreateDeviceRGB(),
                                      bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return nil }
        context.setFillColor(NSColor.white.cgColor); context.fill(CGRect(x: 0, y: 0, width: context.width, height: context.height))
        context.scaleBy(x: scale, y: scale); context.drawPDFPage(page)
        return context.makeImage()
    }

    private static func hasInk(_ image: CGImage) -> Bool {
        guard let context = CGContext(data: nil, width: 100, height: 100, bitsPerComponent: 8, bytesPerRow: 400,
                                      space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue),
              let data = context.data else { return false }
        context.setFillColor(NSColor.white.cgColor); context.fill(CGRect(x: 0, y: 0, width: 100, height: 100))
        context.draw(image, in: CGRect(x: 0, y: 0, width: 100, height: 100))
        let bytes = data.bindMemory(to: UInt8.self, capacity: 40_000)
        // Ink is any clearly non-white pixel: coloured marks (KC navy, BSMI blue, GHS red) count too.
        let dark = stride(from: 0, to: 40_000, by: 4).filter { Int(bytes[$0]) + Int(bytes[$0 + 1]) + Int(bytes[$0 + 2]) < 450 }.count
        return dark > 5 && dark < 9500
    }
}
