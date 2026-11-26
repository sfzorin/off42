import AppKit
import Kit42

@objc(SheetDocument)
final class SheetDocument: NSDocument {
    var workbook = Workbook()

    override init() {
        super.init()
        workbook.sheets = [Sheet(name: L("Sheet%d", 1))]
    }

    override class var autosavesInPlace: Bool { false }

    var editor: SheetWindowController? { windowControllers.first as? SheetWindowController }

    override var isDocumentEdited: Bool {
        if super.isDocumentEdited { return true }
        guard let editor, let pending = editor.grid.pendingEdit else { return false }
        return pending.text != editor.editText(at: pending.cell)
    }

    override func makeWindowControllers() {
        let controller = SheetWindowController()
        addWindowController(controller)
        if let root = controller.window?.contentView { signatureState.install(in: root) }
    }

    override func read(from url: URL, ofType typeName: String) throws {
        try super.read(from: url, ofType: typeName)
        signatureState.load(url)
    }

    // MARK: Digital signatures (Office/OpenDocument XMLDSig, Kit42)

    lazy var signatureState = DocumentSignatureState(document: self)

    override func updateChangeCount(_ change: NSDocument.ChangeType) {
        super.updateChangeCount(change)
        if change == .changeDone { signatureState.didEdit() }
    }

    override func save(_ sender: Any?) {
        guard signatureState.confirmSave() else { return }
        super.save(sender)
    }

    override func save(to url: URL, ofType typeName: String, for saveOperation: NSDocument.SaveOperationType,
                       completionHandler: @escaping (Error?) -> Void) {
        super.save(to: url, ofType: typeName, for: saveOperation) { [weak self] error in
            if error == nil, saveOperation == .saveOperation || saveOperation == .saveAsOperation { self?.signatureState.didSave() }
            completionHandler(error)
        }
    }

    @objc func signDocumentAction(_ sender: Any?) { signatureState.sign() }
    @objc func showSignaturesAction(_ sender: Any?) { signatureState.showDetails() }

    static func isCSV(_ typeName: String) -> Bool {
        let t = typeName.lowercased()
        return t.contains("separated-values") || t.contains("csv") || t.contains("tsv")
    }

    override func read(from data: Data, ofType typeName: String) throws {
        let restored = Calculator.recalculate(try SpreadsheetFile.read(data, typeName: typeName))
        // Failed reads leave the live editor intact; successful Revert discards its draft.
        editor?.grid.cancelEdit()
        workbook = restored
        if let editor {
            editor.workbookDidChange()
            editor.grid.select(CellRef(row: 0, col: 0))
            editor.restoreSplit()
        }
    }

    override func data(ofType typeName: String) throws -> Data {
        editor?.grid.commitEdit(move: nil)
        if editor?.grid.isEditing == true {
            throw NSError(domain: "XLS42", code: 1, userInfo: [NSLocalizedDescriptionKey: editor?.lastAlertMessage ?? L("Invalid value")])
        }
        editor?.storeSplit()
        if Self.isCSV(typeName) {
            return try XLSX.writeCSV(workbook.sheets[editor?.sheetIndex ?? 0], date1904: workbook.date1904)
        }
        if typeName.contains("opendocument") || typeName.lowercased().hasSuffix("ods") { return ODS.write(workbook) }
        return XLSX.write(workbook)
    }

    /// Applies an undoable change to the workbook, recalculates and refreshes the window.
    func change(_ name: String, recalc: Bool = true, _ body: (inout Workbook) -> Void) {
        let old = workbook
        body(&workbook)
        // Formatting-only changes don't need a recalculation (big sheets stay fast).
        if recalc { workbook = Calculator.recalculate(workbook) }
        undoManager?.beginUndoGrouping()
        undoManager?.registerUndo(withTarget: self) { $0.restore(old, name: name) }
        undoManager?.setActionName(name)
        undoManager?.endUndoGrouping()
        editor?.workbookDidChange()
    }

    private func restore(_ wb: Workbook, name: String) {
        let current = workbook
        workbook = wb
        undoManager?.registerUndo(withTarget: self) { $0.restore(current, name: name) }
        undoManager?.setActionName(name)
        editor?.workbookDidChange()
    }

    /// Non-undoable live update (column resize preview).
    func preview(_ body: (inout Workbook) -> Void) {
        body(&workbook)
        editor?.grid.rebuildLayout()
    }

    override func printOperation(withSettings printSettings: [NSPrintInfo.AttributeKey: Any]) throws -> NSPrintOperation {
        editor?.grid.commitEdit(move: nil)
        if editor?.grid.isEditing == true { throw NSError(domain: "XLS42", code: 1, userInfo: [NSLocalizedDescriptionKey: editor?.lastAlertMessage ?? L("Invalid value")]) }
        let options = SheetExport.Options.saved
        if options.scope == .selection, editor == nil { options.scope = .sheet }
        let info = SheetExport.printInfo(from: printInfo)
        let view = SheetPrintView(workbook: workbook, activeSheet: editor?.sheetIndex ?? 0,
                                  selection: editor?.printSelection, options: options)
        view.prepare(info)
        let op = NSPrintOperation(view: view, printInfo: info)
        op.jobTitle = displayName
        op.printPanel.options.formUnion([.showsPaperSize, .showsOrientation, .showsScaling, .showsPreview, .showsPageSetupAccessory])
        op.printPanel.addAccessoryController(SheetPrintAccessory(options: options))
        ProductionPrint.install(on: op)
        return op
    }

}
