import AppKit
import Kit42
import UniformTypeIdentifiers

/// Keeps the text format, allowed destination type and suggested filename in agreement.
final class TextExportOptions: NSObject {
    let popup = NSPopUpButton(frame: NSRect(x: 0, y: 0, width: 200, height: 26), pullsDown: false)
    let view: NSStackView
    private weak var panel: NSSavePanel?
    var rtf: Bool { popup.indexOfSelectedItem == 1 }

    init(panel: NSSavePanel) {
        self.panel = panel
        popup.addItems(withTitles: [L("Plain text (.txt)"), L("Rich text (.rtf)")])
        view = NSStackView(views: [NSTextField(labelWithString: L("Format:")), popup])
        view.edgeInsets = NSEdgeInsets(top: 8, left: 8, bottom: 8, right: 8)
        super.init()
        popup.target = self; popup.action = #selector(changed)
        changed()
    }

    @objc private func changed() {
        guard let panel else { return }
        panel.allowedContentTypes = [rtf ? .rtf : .plainText]
        let name = (panel.nameFieldStringValue as NSString).deletingPathExtension
        panel.nameFieldStringValue = name + (rtf ? ".rtf" : ".txt")
    }
}
