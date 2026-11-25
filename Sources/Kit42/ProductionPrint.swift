import AppKit
import PDFKit
import UniformTypeIdentifiers

public struct PrintCorrection: Codable, Equatable {
    public var offsetX: Double = 0
    public var offsetY: Double = 0
    public var scaleX: Double = 1
    public var scaleY: Double = 1
    public init(offsetX: Double = 0, offsetY: Double = 0, scaleX: Double = 1, scaleY: Double = 1) {
        self.offsetX = offsetX; self.offsetY = offsetY; self.scaleX = scaleX; self.scaleY = scaleY
    }
    public var isValid: Bool {
        [offsetX, offsetY, scaleX, scaleY].allSatisfy(\.isFinite) && abs(offsetX) <= 20 && abs(offsetY) <= 20 && (0.95...1.05).contains(scaleX) && (0.95...1.05).contains(scaleY)
    }
    public static func measured(horizontal: Double, vertical: Double) throws -> PrintCorrection {
        guard horizontal.isFinite, vertical.isFinite, horizontal > 0, vertical > 0 else { throw LabelSheetError.message(L("Enter positive measured lengths.")) }
        let value = PrintCorrection(scaleX: 100 / horizontal, scaleY: 100 / vertical)
        guard value.isValid else { throw LabelSheetError.message(L("The measured correction must stay between 95% and 105%.")) }
        return value
    }
}

/// Calibration belongs to a print operation. PDF export and document geometry never read these keys.
public enum ProductionPrint {
    private static let prefix = "Suite42.PrintCalibration."
    public static func correction(in info: NSPrintInfo?) -> PrintCorrection? {
        guard let info, (info.dictionary()[prefix + "enabled"] as? NSNumber)?.boolValue == true else { return nil }
        let d = info.dictionary()
        let value = PrintCorrection(offsetX: (d[prefix + "x"] as? NSNumber)?.doubleValue ?? 0,
                                    offsetY: (d[prefix + "y"] as? NSNumber)?.doubleValue ?? 0,
                                    scaleX: (d[prefix + "sx"] as? NSNumber)?.doubleValue ?? 1,
                                    scaleY: (d[prefix + "sy"] as? NSNumber)?.doubleValue ?? 1)
        return value.isValid ? value : nil
    }
    public static func setCorrection(_ value: PrintCorrection?, in info: NSPrintInfo) {
        let d = info.dictionary(); d[prefix + "enabled"] = value?.isValid == true
        guard let value, value.isValid else { return }
        d[prefix + "x"] = value.offsetX; d[prefix + "y"] = value.offsetY; d[prefix + "sx"] = value.scaleX; d[prefix + "sy"] = value.scaleY
    }
    public static func concatenate(in context: CGContext, pageRect: CGRect, flipped: Bool) {
        guard let value = correction(in: NSPrintOperation.current?.printInfo) else { return }
        apply(value, in: context, pageRect: pageRect, flipped: flipped)
    }
    public static func apply(_ value: PrintCorrection, in context: CGContext, pageRect: CGRect, flipped: Bool) {
        guard value.isValid else { return }
        let mm = 72.0 / 25.4
        let origin = CGPoint(x: pageRect.minX, y: flipped ? pageRect.minY : pageRect.maxY)
        context.translateBy(x: origin.x + value.offsetX * mm, y: origin.y + (flipped ? 1 : -1) * value.offsetY * mm)
        context.scaleBy(x: value.scaleX, y: value.scaleY)
        context.translateBy(x: -origin.x, y: -origin.y)
    }
    public static func install(on operation: NSPrintOperation) {
        operation.printPanel.addAccessoryController(PrintCalibrationAccessory(info: operation.printInfo))
    }
    public static func operation(pdfData: Data, printInfo: NSPrintInfo, autoFit: Bool = false) throws -> NSPrintOperation {
        guard pdfData.count <= 128_000_000, let pdf = PDFDocument(data: pdfData), pdf.pageCount > 0, pdf.pageCount <= 5000 else {
            throw LabelSheetError.message(L("Could not prepare the PDF for printing."))
        }
        // Honor print flags in a private snapshot without changing the open document.
        for index in 0..<pdf.pageCount {
            pdf.page(at: index)?.annotations.forEach { $0.shouldDisplay = $0.shouldPrint }
        }
        let info = printInfo.copy() as! NSPrintInfo
        let view = ProductionPDFPrintView(pdf: pdf, info: info, autoFit: autoFit)
        info.horizontalPagination = .clip; info.verticalPagination = .clip
        info.leftMargin = 0; info.rightMargin = 0; info.topMargin = 0; info.bottomMargin = 0
        info.isHorizontallyCentered = false; info.isVerticallyCentered = false
        let op = NSPrintOperation(view: view, printInfo: info)
        view.info = op.printInfo
        op.printPanel.options.formUnion([.showsPaperSize, .showsOrientation, .showsScaling, .showsPreview, .showsPageRange])
        install(on: op)
        return op
    }
    public static func calibrationPDF() throws -> Data {
        let bytes = NSMutableData(); var box = CGRect(x: 0, y: 0, width: 210 * 72 / 25.4, height: 297 * 72 / 25.4)
        guard let consumer = CGDataConsumer(data: bytes), let ctx = CGContext(consumer: consumer, mediaBox: &box, nil) else { throw LabelSheetError.message(L("Could not create calibration sheet.")) }
        ctx.beginPDFPage(nil); ctx.setStrokeColor(CGColor(gray: 0, alpha: 1)); ctx.setLineWidth(0.5)
        let mm = 72.0 / 25.4, x = 30 * mm, y = box.height - 60 * mm
        ctx.move(to: CGPoint(x: x, y: y)); ctx.addLine(to: CGPoint(x: x + 100 * mm, y: y))
        ctx.move(to: CGPoint(x: x, y: y)); ctx.addLine(to: CGPoint(x: x, y: y - 100 * mm))
        for n in 0...10 {
            let p = Double(n) * 10 * mm
            ctx.move(to: CGPoint(x: x + p, y: y - 3)); ctx.addLine(to: CGPoint(x: x + p, y: y + 3))
            ctx.move(to: CGPoint(x: x - 3, y: y - p)); ctx.addLine(to: CGPoint(x: x + 3, y: y - p))
        }; ctx.strokePath()
        NSGraphicsContext.saveGraphicsState(); NSGraphicsContext.current = NSGraphicsContext(cgContext: ctx, flipped: false)
        let attrs: [NSAttributedString.Key: Any] = [.font: NSFont.systemFont(ofSize: 12), .foregroundColor: NSColor.black]
        (L("Calibration sheet — print at 100% with calibration disabled.") as NSString).draw(in: CGRect(x: 30 * mm, y: box.height - 35 * mm, width: 155 * mm, height: 30), withAttributes: attrs)
        ("100 mm" as NSString).draw(at: CGPoint(x: x + 40 * mm, y: y + 10), withAttributes: attrs)
        ("100 mm" as NSString).draw(at: CGPoint(x: x + 10, y: y - 50 * mm), withAttributes: attrs)
        NSGraphicsContext.restoreGraphicsState(); ctx.endPDFPage(); ctx.closePDF(); return bytes as Data
    }
}

private final class ProductionPDFPrintView: NSView {
    let pdf: PDFDocument
    var info: NSPrintInfo
    let autoFit: Bool
    init(pdf: PDFDocument, info: NSPrintInfo, autoFit: Bool) {
        self.pdf = pdf; self.info = info; self.autoFit = autoFit
        super.init(frame: CGRect(origin: .zero, size: info.paperSize))
    }
    required init?(coder: NSCoder) { nil }
    override func knowsPageRange(_ range: NSRangePointer) -> Bool {
        let size = info.paperSize
        setFrameSize(CGSize(width: size.width, height: size.height * CGFloat(pdf.pageCount)))
        range.pointee = NSRange(location: 1, length: pdf.pageCount); return true
    }
    override func locationOfPrintRect(_ rect: NSRect) -> NSPoint { .zero }
    override func rectForPage(_ page: Int) -> NSRect { CGRect(x: 0, y: CGFloat(page - 1) * info.paperSize.height, width: info.paperSize.width, height: info.paperSize.height) }
    override func draw(_ dirtyRect: NSRect) {
        guard let ctx = NSGraphicsContext.current?.cgContext else { return }
        for index in 1...pdf.pageCount {
            let rect = rectForPage(index)
            guard dirtyRect.intersects(rect), let page = pdf.page(at: index - 1), let reference = page.pageRef else { continue }
            ctx.saveGState(); ctx.clip(to: rect); ctx.setFillColor(CGColor(gray: 1, alpha: 1)); ctx.fill(rect)
            ProductionPrint.concatenate(in: ctx, pageRect: rect, flipped: false)
            var size = reference.getBoxRect(.cropBox).size
            if abs(reference.rotationAngle) % 180 == 90 { size = CGSize(width: size.height, height: size.width) }
            guard size.width > 0, size.height > 0 else { ctx.restoreGState(); continue }
            let factor = autoFit ? min(1, rect.width / size.width, rect.height / size.height) : 1
            let target = CGRect(x: rect.midX - size.width * factor / 2, y: rect.midY - size.height * factor / 2, width: size.width * factor, height: size.height * factor)
            let rotation = page.rotation
            let transform = reference.getDrawingTransform(.cropBox, rect: target, rotate: 0, preserveAspectRatio: true)
            ctx.clip(to: target)
            ctx.concatenate(transform)
            page.rotation = 0
            page.draw(with: .mediaBox, to: ctx)
            page.rotation = rotation
            ctx.restoreGState()
        }
    }
}

final class PrintCalibrationAccessory: NSViewController, NSPrintPanelAccessorizing {
    private let info: NSPrintInfo
    private var initializing = true
    @objc dynamic var enabled = false { didSet { update() } }
    @objc dynamic var offsetX = 0.0 { didSet { update() } }
    @objc dynamic var offsetY = 0.0 { didSet { update() } }
    @objc dynamic var scaleX = 100.0 { didSet { update() } }
    @objc dynamic var scaleY = 100.0 { didSet { update() } }
    private let measuredX = NSTextField(string: "100")
    private let measuredY = NSTextField(string: "100")
    private let status = NSTextField(wrappingLabelWithString: "")
    init(info: NSPrintInfo) {
        self.info = info
        super.init(nibName: nil, bundle: nil)
        title = L("Calibration")
        if let correction = ProductionPrint.correction(in: info) {
            offsetX = correction.offsetX; offsetY = correction.offsetY
            scaleX = correction.scaleX * 100; scaleY = correction.scaleY * 100; enabled = true
        }
        initializing = false
    }
    required init?(coder: NSCoder) { nil }
    override func loadView() {
        let enabledButton = NSButton(checkboxWithTitle: L("Apply print calibration"), target: nil, action: nil)
        enabledButton.bind(.value, to: self, withKeyPath: "enabled")
        func field(_ key: String, min: Double, max: Double) -> NSTextField {
            let f = NSTextField(); let formatter = NumberFormatter(); formatter.minimum = NSNumber(value: min); formatter.maximum = NSNumber(value: max); formatter.maximumFractionDigits = 4; formatter.numberStyle = .decimal
            f.formatter = formatter; f.bind(.value, to: self, withKeyPath: key, options: [.continuouslyUpdatesValue: true]); f.widthAnchor.constraint(equalToConstant: 90).isActive = true; return f
        }
        let grid = NSGridView(views: [
            [NSTextField(labelWithString: L("Offset X (mm)")), field("offsetX", min: -20, max: 20)],
            [NSTextField(labelWithString: L("Offset Y (mm)")), field("offsetY", min: -20, max: 20)],
            [NSTextField(labelWithString: L("Scale X (%)")), field("scaleX", min: 95, max: 105)],
            [NSTextField(labelWithString: L("Scale Y (%)")), field("scaleY", min: 95, max: 105)]])
        grid.rowSpacing = 6
        let measurement = NSStackView(views: [NSTextField(labelWithString: L("Measured 100 mm: X / Y")), measuredX, measuredY])
        measuredX.widthAnchor.constraint(equalToConstant: 65).isActive = true; measuredY.widthAnchor.constraint(equalToConstant: 65).isActive = true
        func button(_ name: String, _ action: Selector) -> NSButton { let b = NSButton(title: name, target: self, action: action); b.bezelStyle = .rounded; return b }
        let actions = NSStackView(views: [button(L("Calculate"), #selector(calculate)), button(L("Save Test Sheet…"), #selector(testSheet))])
        let profiles = NSStackView(views: [button(L("Save Printer Profile"), #selector(saveProfile)), button(L("Load Printer Profile"), #selector(loadProfile))])
        let stack = NSStackView(views: [enabledButton, grid, measurement, actions, profiles, status]); stack.orientation = .vertical; stack.alignment = .leading; stack.spacing = 10
        let box = NSView(); stack.translatesAutoresizingMaskIntoConstraints = false; box.addSubview(stack)
        NSLayoutConstraint.activate([stack.leadingAnchor.constraint(equalTo: box.leadingAnchor, constant: 16), stack.trailingAnchor.constraint(equalTo: box.trailingAnchor, constant: -16), stack.topAnchor.constraint(equalTo: box.topAnchor, constant: 12), stack.bottomAnchor.constraint(equalTo: box.bottomAnchor, constant: -12), box.widthAnchor.constraint(greaterThanOrEqualToConstant: 390)])
        view = box
    }
    private func update() { guard !initializing else { return }; ProductionPrint.setCorrection(enabled ? PrintCorrection(offsetX: offsetX, offsetY: offsetY, scaleX: scaleX / 100, scaleY: scaleY / 100) : nil, in: info) }
    private var profileKey: String { "PrintCalibration." + info.printer.name + "." + String(format: "%.2f-%.2f", info.paperSize.width, info.paperSize.height) }
    @objc private func saveProfile() {
        let value = PrintCorrection(offsetX: offsetX, offsetY: offsetY, scaleX: scaleX / 100, scaleY: scaleY / 100)
        guard value.isValid, let data = try? JSONEncoder().encode(value) else { return }
        UserDefaults(suiteName: "com.sfzorin.suite42")?.set(data, forKey: profileKey); status.stringValue = L("Saved for %@ and this paper size.", info.printer.name)
    }
    @objc private func loadProfile() {
        guard let data = UserDefaults(suiteName: "com.sfzorin.suite42")?.data(forKey: profileKey), let value = try? JSONDecoder().decode(PrintCorrection.self, from: data), value.isValid else { status.stringValue = L("No profile for this printer and paper size."); return }
        offsetX = value.offsetX; offsetY = value.offsetY; scaleX = value.scaleX * 100; scaleY = value.scaleY * 100; enabled = true; status.stringValue = L("Calibration: %@", info.printer.name)
    }
    @objc private func calculate() {
        do {
            let x = Double(measuredX.stringValue.replacingOccurrences(of: ",", with: ".")) ?? .nan
            let y = Double(measuredY.stringValue.replacingOccurrences(of: ",", with: ".")) ?? .nan
            let value = try PrintCorrection.measured(horizontal: x, vertical: y)
            scaleX = value.scaleX * 100; scaleY = value.scaleY * 100; enabled = true; status.stringValue = L("Print the test sheet again to verify the correction.")
        } catch { status.stringValue = error.localizedDescription }
    }
    @objc private func testSheet() {
        let panel = NSSavePanel(); panel.nameFieldStringValue = "calibration.pdf"; panel.allowedContentTypes = [.pdf]
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do { try ProductionPrint.calibrationPDF().write(to: url, options: .atomic) } catch { status.stringValue = error.localizedDescription }
    }
    func localizedSummaryItems() -> [[NSPrintPanel.AccessorySummaryKey: String]] {
        [[.itemName: L("Calibration"), .itemDescription: enabled ? String(format: "X %.2f%% / Y %.2f%%", scaleX, scaleY) : L("Off")]]
    }
    func keyPathsForValuesAffectingPreview() -> Set<String> { ["enabled", "offsetX", "offsetY", "scaleX", "scaleY"] }
}
