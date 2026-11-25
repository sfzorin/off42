import AppKit
import Kit42

/// Scanner ▾ · Overview · Scan · iPhone ·  —  · Rotate · Delete · Enhance · OCR · Save · Share
final class ScanToolbar: NSObject, NSSharingServicePickerDelegate {
    weak var owner: ScanWindowController?
    private(set) var controller: ToolbarController!
    let scanners = NSPopUpButton(frame: NSRect(x: 0, y: 0, width: 220, height: 26), pullsDown: false)
    let phoneButton = NSButton()
    let share = NSButton()

    var toolbar: NSToolbar { controller.toolbar }

    init(owner: ScanWindowController) {
        self.owner = owner
        super.init()
        scanners.toolTip = L("Scanner")
        scanners.translatesAutoresizingMaskIntoConstraints = false
        scanners.widthAnchor.constraint(equalToConstant: 220).isActive = true
        scanners.heightAnchor.constraint(equalToConstant: 24).isActive = true
        scanners.setContentHuggingPriority(.required, for: .vertical)
        (scanners.cell as? NSPopUpButtonCell)?.lineBreakMode = .byTruncatingTail

        phoneButton.bezelStyle = .toolbar
        phoneButton.image = NSImage(systemSymbolName: "iphone", accessibilityDescription: Cmd.scanWithPhone.shortLabel)
        phoneButton.imagePosition = .imageOnly
        phoneButton.toolTip = Cmd.scanWithPhone.fullToolTip
        phoneButton.target = owner
        phoneButton.action = #selector(ScanWindowController.scanWithPhoneAction(_:))
        phoneButton.sendAction(on: .leftMouseDown)

        share.bezelStyle = .toolbar
        share.image = NSImage(systemSymbolName: "square.and.arrow.up", accessibilityDescription: L("Share"))
        share.imagePosition = .imageOnly
        share.toolTip = L("Share")
        share.target = self
        share.action = #selector(shareClicked(_:))
        share.sendAction(on: .leftMouseDown)

        controller = ToolbarController(identifier: "SCN42.toolbar", elements: [
            .view(id: "scanner", label: L("Scanner"), scanners),
            .command(Cmd.scan),
            .view(id: "iphone", label: L("iPhone"), phoneButton),
            .flexibleSpace,
            .group(id: "rotate", label: L("Rotate"), [Cmd.rotateLeft, Cmd.rotateRight]),
            .command(Cmd.recognize),
            .command(Cmd.save),
            .view(id: "share", label: L("Share"), share),
        ], optional: [
            // Overview is the inspector's secondary button; page deletion and enhancement are in the Page menu.
            .command(Cmd.overview), .command(Cmd.deletePage), .command(Cmd.enhance), .command(Cmd.stamps),
        ])
    }

    func reloadScanners(_ list: [ScannerBackend], selected: ScannerBackend?) {
        scanners.removeAllItems()
        if list.isEmpty {
            scanners.addItem(withTitle: DeviceManager.shared.finishedLocalSearch ? L("No scanner found") : L("Looking for scanners…"))
            scanners.lastItem?.isEnabled = false
            scanners.autoenablesItems = false
        }
        for s in list {
            let item = NSMenuItem(title: s.name, action: #selector(ScanWindowController.selectScannerAction(_:)), keyEquivalent: "")
            item.representedObject = s
            item.target = owner
            item.toolTip = s.connection
            item.image = Theme42.symbol(s.connection == L("Virtual") ? "scanner.fill" : "scanner", size: 13)
            scanners.menu?.addItem(item)
        }
        if let selected, let i = list.firstIndex(where: { $0 === selected }) { scanners.selectItem(at: i) }
    }

    @objc private func shareClicked(_ sender: NSButton) {
        guard let items = owner?.shareItems(), !items.isEmpty else { return }
        let picker = NSSharingServicePicker(items: items)
        picker.delegate = self
        picker.show(relativeTo: sender.bounds, of: sender, preferredEdge: .minY)
    }
}
