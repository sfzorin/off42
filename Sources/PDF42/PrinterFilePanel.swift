import AppKit
import PDFKit
import Kit42

enum PrinterFilePanel {
    static func load(_ url: URL) throws -> Data {
        let size = try url.resourceValues(forKeys: [.fileSizeKey]).fileSize
        guard let size, (1...10_000_000).contains(size) else { throw PrinterJobError.limit }
        let data = try Data(contentsOf: url, options: .mappedIfSafe)
        guard data.count <= 10_000_000 else { throw PrinterJobError.limit }
        return data
    }
    static func choose() -> (String, String)? {
        let format = NSPopUpButton(frame: .zero, pullsDown: false)
        format.addItems(withTitles: ["ZPL — 8 dots/mm", "TSPL — 8 dots/mm", "EZPL — 8 dots/mm", "SBPL — 8 dots/mm", "Brother QL-800 — 300 dpi"])
        let stock = NSPopUpButton(frame: .zero, pullsDown: false)
        stock.addItems(withTitles: ["62 mm continuous", "62 × 29 mm", "62 × 100 mm"])
        let stack = NSStackView(views: [NSTextField(labelWithString: L("Format and printer:")), format, NSTextField(labelWithString: L("Brother media:")), stock])
        stack.orientation = .vertical; stack.alignment = .leading; stack.spacing = 8; stack.frame = NSRect(x: 0, y: 0, width: 300, height: 110)
        let alert = NSAlert(); alert.messageText = L("Printer file format")
        alert.informativeText = L("Printer files contain flattened black and white pages. SBPL and Brother jobs may include padding. Choose the matching printer and media.")
        alert.accessoryView = stack; alert.addButton(withTitle: L("Continue")); alert.addButton(withTitle: L("Cancel"))
        guard alert.runModal() == .alertFirstButtonReturn else { return nil }
        return (["zpl","tspl","ezpl","sbpl","brother"][format.indexOfSelectedItem], ["continuous","62x29","62x100"][stock.indexOfSelectedItem])
    }
}
extension AppDelegate {
    @objc func importPrinterFileAction(_ sender: Any?) {
        guard let (name, stock) = PrinterFilePanel.choose() else { return }
        let panel = NSOpenPanel(); panel.allowsMultipleSelection = false
        panel.message = L("Choose a printer file to import as a new PDF")
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            let (format, profile) = try PDFPrinterFiles.settings(name, stock)
            let data = try PDFPrinterFiles.toPDF(PrinterFilePanel.load(url), format: format, profile: profile)
            guard let pdf = PDFDocument(data: data) else { throw PrinterJobError.invalid }
            let doc = PDFDoc(pdf: pdf); NSDocumentController.shared.addDocument(doc); doc.makeWindowControllers(); doc.showWindows(); doc.updateChangeCount(.changeDone)
        } catch { NSApp.presentError(error) }
    }
}
extension EditorWindowController {
    @objc func exportPrinterFileAction(_ sender: Any?) {
        guard let (name, stock) = PrinterFilePanel.choose(), let window else { return }
        let panel = NSSavePanel(); panel.nameFieldStringValue = "label." + (name == "brother" ? "bin" : name)
        panel.message = L("Export flattened printer file at the selected printer resolution")
        panel.beginSheetModal(for: window) { [self] response in
            guard response == .OK, let url = panel.url else { return }
            do {
                if let source = pdfDoc?.fileURL, source.standardizedFileURL == url.standardizedFileURL { throw PrinterJobError.invalid }
                prepareForSave(); let snapshot = try exportSnapshot()
                let (format, profile) = try PDFPrinterFiles.settings(name, stock)
                let data = try PDFPrinterFiles.fromPDF(snapshot, format: format, profile: profile)
                try data.write(to: url, options: .atomic)
            } catch { presentError(error) }
        }
    }
}
