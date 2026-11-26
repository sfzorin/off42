import AppKit

protocol RecentDocumentsSource: AnyObject {
    var recentDocumentURLs: [URL] { get }
    func clearRecentDocuments(_ sender: Any?)
    func openDocument(withContentsOf url: URL, display displayDocument: Bool,
                      completionHandler: @escaping (NSDocument?, Bool, Error?) -> Void)
}

extension NSDocumentController: RecentDocumentsSource {}

/// Programmatically created menus need an explicit recent-files data source. Opening through
/// NSDocumentController also respects IMG42's and SCN42's custom document controllers.
final class RecentDocumentsMenu: NSObject, NSMenuDelegate, NSMenuItemValidation {
    static let shared = RecentDocumentsMenu()
    private let controller: () -> RecentDocumentsSource

    init(controller: @escaping () -> RecentDocumentsSource = { NSDocumentController.shared }) {
        self.controller = controller
        super.init()
    }

    func makeMenu() -> NSMenu {
        let menu = NSMenu(title: L("Open Recent"))
        menu.identifier = NSUserInterfaceItemIdentifier("Suite42.recentDocuments")
        menu.delegate = self
        menuNeedsUpdate(menu)
        return menu
    }

    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        let urls = controller().recentDocumentURLs
        #if APP_STORE
        SecurityScopedRecents.remember(urls)
        #endif
        for url in urls {
            let item = NSMenuItem(title: url.lastPathComponent, action: #selector(openRecent(_:)), keyEquivalent: "")
            item.target = self
            item.representedObject = url
            item.toolTip = url.path
            let icon = NSWorkspace.shared.icon(forFile: url.path)
            icon.size = NSSize(width: 16, height: 16)
            item.image = icon
            menu.addItem(item)
        }
        if !urls.isEmpty { menu.addItem(.separator()) }
        let clear = NSMenuItem(title: L("Clear Menu"), action: #selector(clearRecent(_:)), keyEquivalent: "")
        clear.target = self
        clear.isEnabled = !urls.isEmpty
        menu.addItem(clear)
    }

    @objc private func openRecent(_ sender: NSMenuItem) {
        guard let url = sender.representedObject as? URL else { return }
        #if APP_STORE
        SecurityScopedRecents.beginAccess(url)
        #endif
        controller().openDocument(withContentsOf: url, display: true) { _, _, error in
            if let error { NSApp.presentError(error) }
        }
    }

    @objc private func clearRecent(_ sender: NSMenuItem) {
        controller().clearRecentDocuments(sender)
    }

    func validateMenuItem(_ item: NSMenuItem) -> Bool {
        if item.action == #selector(clearRecent(_:)) { return !controller().recentDocumentURLs.isEmpty }
        return item.representedObject is URL
    }
}

extension DebugSnapshot {
    /// Run inside a launched app's UI test. The in-memory controller leaves the user's history alone.
    public static func auditRecentMenu() -> Bool {
        let controller = RecentMenuTestSource()
        let source = RecentDocumentsMenu(controller: { controller })
        let menu = source.makeMenu()
        var failed = 0
        func check(_ name: String, _ ok: Bool) {
            print("\(ok ? "PASS" : "FAIL") recent menu: \(name)")
            if !ok { failed += 1 }
        }
        menu.update()
        check("empty history disables Clear", menu.items.count == 1 && menu.items[0].isEnabled == false)
        let first = URL(fileURLWithPath: "/tmp/Suite42 first/document.txt")
        let second = URL(fileURLWithPath: "/tmp/Suite42 second/document.txt")
        controller.urls = [first, second]
        source.menuNeedsUpdate(menu)
        menu.update()
        check("items follow recent-file order", menu.items.compactMap { $0.representedObject as? URL } == [first, second])
        check("paths remain distinguishable", menu.items.count >= 2 && menu.items[0].toolTip == first.path && menu.items[1].toolTip == second.path)
        check("recent entries are enabled", menu.items.count >= 2 && menu.items[0].isEnabled && menu.items[1].isEnabled)
        if menu.items.count >= 2 {
            let item = menu.items[1]
            if let action = item.action { NSApp.sendAction(action, to: item.target, from: item) }
        }
        check("Open routes to the selected URL", controller.opened == second && controller.displayed)
        if let clear = menu.items.last, let action = clear.action { NSApp.sendAction(action, to: clear.target, from: clear) }
        source.menuNeedsUpdate(menu)
        menu.update()
        check("Clear refreshes the menu", controller.urls.isEmpty && menu.items.count == 1 && !menu.items[0].isEnabled)
        controller.urls = [second]
        source.menuNeedsUpdate(menu)
        check("later documents appear on next opening", menu.items.compactMap { $0.representedObject as? URL } == [second])
        let actual = NSApp.mainMenu?.items.flatMap { $0.submenu?.items ?? [] }.compactMap(\.submenu)
            .first { $0.identifier?.rawValue == "Suite42.recentDocuments" }
        if let actual {
            actual.delegate?.menuNeedsUpdate?(actual)
            actual.update()
            check("main menu uses the current controller history",
                  actual.items.compactMap { $0.representedObject as? URL } == NSDocumentController.shared.recentDocumentURLs)
        } else { check("main menu uses the current controller history", false) }
        print("Recent menu UI regressions: \(8 - failed)/8 passed")
        return failed == 0
    }
}

// NSDocumentController.init() may return its existing singleton, even when invoked on a subclass.
// A separate source avoids mutating the application's real document controller or recent history.
private final class RecentMenuTestSource: RecentDocumentsSource {
    var urls: [URL] = []
    var opened: URL?
    var displayed = false
    var recentDocumentURLs: [URL] { urls }
    func clearRecentDocuments(_ sender: Any?) { urls = [] }
    func openDocument(withContentsOf url: URL, display displayDocument: Bool,
                               completionHandler: @escaping (NSDocument?, Bool, Error?) -> Void) {
        opened = url
        displayed = displayDocument
        completionHandler(nil, false, nil)
    }
}
