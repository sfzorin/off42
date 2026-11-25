import AppKit
import Kit42

/// Every SCN42 action, defined once and reused by menus, toolbar and context menus.
enum Cmd {
    typealias W = ScanWindowController
    typealias A = AppDelegate

    // Scan
    static let scan = Command(L("Scan"), #selector(W.scanAction(_:)), symbol: "scanner", shortcut: Shortcut("b"),
                              toolTip: L("Scan and add the pages to the document"), label: L("Scan"))
    static let overview = Command(L("Overview"), #selector(W.overviewAction(_:)), symbol: "viewfinder", shortcut: Shortcut("b", [.command, .shift]),
                                  toolTip: L("Quick preview of the glass to choose what to scan"), label: L("Overview"))
    static let cancelScan = Command(L("Stop Scanning"), #selector(W.cancelScanAction(_:)), symbol: "stop.circle", shortcut: Shortcut("."))
    static let detectPhotos = Command(L("Detect Separate Photos"), #selector(W.detectPhotosAction(_:)), symbol: "rectangle.3.group",
                                      shortcut: Shortcut("d", [.command, .option]), label: L("Detect Photos"))
    static let clearAreas = Command(L("Clear Scan Areas"), #selector(W.clearAreasAction(_:)), symbol: "rectangle.dashed")
    static let scanWithPhone = Command(L("Scan with iPhone or iPad…"), #selector(W.scanWithPhoneAction(_:)), symbol: "iphone",
                                       toolTip: L("Scan documents or take a photo with your iPhone or iPad (Continuity Camera)"), label: L("iPhone"))
    static let scanAndPrint = Command(L("Copy (Scan and Print)…"), #selector(W.scanAndPrintAction(_:)), symbol: "printer.dotmatrix",
                                      shortcut: Shortcut("p", [.command, .option]))
    static let savePreset = Command(L("Save Settings as Preset…"), #selector(W.savePresetAction(_:)), symbol: "plus.square.on.square")
    static let deletePreset = Command(L("Delete Preset"), #selector(W.deletePresetAction(_:)))

    // File
    static let importFiles = Command(L("Import Images or PDF…"), #selector(W.importAction(_:)), symbol: "square.and.arrow.down", shortcut: Shortcut("i", [.command, .shift]))
    static let save = Command(L("Save…"), #selector(W.saveDocument(_:)), symbol: "square.and.arrow.down.on.square", shortcut: Shortcut("s"),
                              toolTip: L("Save the document"), label: L("Save"))
    static let export = Command(L("Export…"), #selector(W.exportAction(_:)), symbol: "square.and.arrow.up.on.square", shortcut: Shortcut("e", [.command, .shift]))
    static let exportText = Command(L("Export Text…"), #selector(W.exportTextAction(_:)), symbol: "doc.plaintext")
    static let mail = Command(L("Send by Mail…"), #selector(W.mailAction(_:)), symbol: "envelope")

    // Edit
    static let copyText = Command(L("Copy Text"), #selector(W.copyTextAction(_:)), symbol: "text.viewfinder", shortcut: Shortcut("c", [.command, .shift]))

    // View
    static let pagesPanel = Command(L("Pages"), #selector(W.togglePagesAction(_:)), symbol: "sidebar.left", shortcut: Shortcut("1", [.command, .option]))
    static let settingsPanel = Command(L("Scan Settings"), #selector(W.showSettingsAction(_:)), symbol: "slider.horizontal.3",
                                       shortcut: Shortcut("2", [.command, .option]), label: L("Settings"))
    static let historyPanel = Command(L("History"), #selector(W.showHistoryAction(_:)), symbol: "clock.arrow.circlepath",
                                      shortcut: Shortcut("3", [.command, .option]), label: L("History"))
    static let showGlass = Command(L("Show Scanner Glass"), #selector(W.showGlassAction(_:)), symbol: "rectangle.on.rectangle", shortcut: Shortcut("g", [.command, .option]))
    static let zoomIn = Command(L("Zoom In"), #selector(W.zoomInAction(_:)), symbol: "plus.magnifyingglass", shortcut: Shortcut("+"))
    static let zoomOut = Command(L("Zoom Out"), #selector(W.zoomOutAction(_:)), symbol: "minus.magnifyingglass", shortcut: Shortcut("-"))
    static let actualSize = Command(L("Actual Size"), #selector(W.actualSizeAction(_:)), symbol: "1.magnifyingglass", shortcut: Shortcut("0"))
    static let zoomToFit = Command(L("Zoom to Fit"), #selector(W.zoomToFitAction(_:)), symbol: "arrow.up.left.and.down.right.magnifyingglass",
                                   shortcut: Shortcut("0", [.command, .option]))

    // Page
    static let rotateLeft = Command(L("Rotate Left"), #selector(W.rotateLeftAction(_:)), symbol: "rotate.left", shortcut: Shortcut("l"))
    static let rotateRight = Command(L("Rotate Right"), #selector(W.rotateRightAction(_:)), symbol: "rotate.right", shortcut: Shortcut("r"))
    static let deletePage = Command(L("Delete Page"), #selector(W.deletePageAction(_:)), symbol: "trash", shortcut: Shortcut(Shortcut.backspace),
                                    label: L("Delete"))
    static let moveUp = Command(L("Move Page Up"), #selector(W.movePageUpAction(_:)), symbol: "arrow.up", shortcut: Shortcut(Shortcut.upArrow, [.command, .option]))
    static let moveDown = Command(L("Move Page Down"), #selector(W.movePageDownAction(_:)), symbol: "arrow.down", shortcut: Shortcut(Shortcut.downArrow, [.command, .option]))
    static let rescan = Command(L("Rescan Page"), #selector(W.rescanPageAction(_:)), symbol: "arrow.clockwise.square", shortcut: Shortcut("b", [.command, .option]))
    static let insertHere = Command(L("Insert Next Scan After This Page"), #selector(W.insertHereAction(_:)), symbol: "text.insert")
    static let split = Command(L("Split Document Here"), #selector(W.splitAction(_:)), symbol: "scissors")

    // Tools
    static let recognize = Command(L("Recognize Text"), #selector(W.recognizeTextAction(_:)), symbol: "text.viewfinder", shortcut: Shortcut("r", [.command, .option]),
                                   toolTip: L("Recognize text: searchable PDF, copy and export text"), label: L("OCR"))
    static let enhance = Command(L("Enhance"), #selector(W.enhanceToggleAction(_:)), symbol: "wand.and.stars", shortcut: Shortcut("e", [.command, .option]),
                                 toolTip: L("Clean up the selected pages: white paper, sharp text (again: undo)"), label: L("Enhance"))
    static let enhancements: [Command] = Enhancement.allCases.map { Command($0.title, #selector(W.enhancementAction(_:)), tag: $0.rawValue + 1) }
    static let colorModes: [Command] = ColorMode.allCases.map { Command($0.title, #selector(W.pageColorAction(_:)), tag: $0.rawValue + 1) }
    static let removeBlank = Command(L("Remove Blank Pages"), #selector(W.removeBlankAction(_:)), symbol: "doc.badge.ellipsis")
    static let stamps = Command(L("Signature or Stamp…"), #selector(W.showStampsAction(_:)), symbol: "signature",
                                shortcut: Shortcut("k", [.command, .shift]), toolTip: L("Print a signature or stamp into the page"), label: L("Stamp"))
    static let straighten = Command(L("Crop and Straighten Again"), #selector(W.restraightenAction(_:)), symbol: "crop.rotate")

    static let welcome = Command(L("Welcome Window"), #selector(A.showWelcomeAction(_:)), shortcut: Shortcut("0", [.command, .shift]))

    /// Continuity Camera fills this item with "Take Photo" / "Scan Documents" for each nearby iPhone or iPad.
    static func importFromDeviceItem() -> NSMenuItem {
        let item = NSMenuItem(title: L("Import from iPhone or iPad"), action: nil, keyEquivalent: "")
        item.identifier = NSMenuItem.importFromDeviceIdentifier
        return item
    }

    static func menuBar() -> MenuBarSpec {
        var spec = MenuBarSpec(appName: "SCN42")
        spec.fileExtras = [.command(importFiles), .item(importFromDeviceItem()), .separator,
                           .command(export), .command(exportText), .command(mail)]
        spec.editExtras = [.command(copyText)]
        spec.view = [.command(pagesPanel), .command(settingsPanel), .command(historyPanel), .separator,
                     .command(showGlass), .separator,
                     .command(zoomIn), .command(zoomOut), .command(actualSize), .command(zoomToFit)]
        spec.custom = [
            (L("Scan"), [.command(scan), .command(overview), .command(cancelScan), .separator,
                         .command(detectPhotos), .command(clearAreas), .separator,
                         .command(scanWithPhone), .command(scanAndPrint), .separator,
                         .item(presetsItem()), .command(savePreset), .command(deletePreset)]),
            (L("Page"), [.command(rotateLeft), .command(rotateRight), .separator,
                         .command(moveUp), .command(moveDown), .command(deletePage), .separator,
                         .command(rescan), .command(insertHere), .command(split)]),
            (L("Tools"), [.command(recognize), .command(enhance), .submenu(L("Clean-up"), enhancements.menuElements),
                          .submenu(L("Colour"), colorModes.menuElements), .command(straighten), .separator, .command(stamps), .separator, .command(removeBlank)]),
        ]
        spec.help = [.command(welcome)]
        return spec
    }

    /// Scan › Preset: rebuilt by the window controller whenever presets change.
    static let presetsMenu = NSMenu(title: L("Preset"))
    static func presetsItem() -> NSMenuItem {
        let item = NSMenuItem(title: L("Preset"), action: nil, keyEquivalent: "")
        item.submenu = presetsMenu
        rebuildPresets()
        return item
    }

    static func rebuildPresets() {
        presetsMenu.removeAllItems()
        for (i, p) in Store.allPresets.enumerated() {
            let item = NSMenuItem(title: p.name, action: #selector(W.presetAction(_:)), keyEquivalent: i < 4 ? "\(i + 1)" : "")
            item.keyEquivalentModifierMask = [.command, .control]
            item.tag = i + 1
            presetsMenu.addItem(item)
        }
    }
}
