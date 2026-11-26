import AppKit

public indirect enum MenuElement {
    case command(Command)
    case separator
    case submenu(String, [MenuElement])
    case item(NSMenuItem)
}

/// Declarative description of an app's menu bar. Every Suite42 app gets the same skeleton
/// and order; an app only fills in its own sections:
///
///     [App] · Файл · Правка · Вид · Вставка · Формат · <app menus> · Окно · Справка
///
/// Empty `insert` / `format` menus are omitted.
public struct MenuBarSpec {
    public var appName: String
    public var preferences: Command?
    /// File menu: after L("Revert to Saved"), before printing (export, protect, ...).
    public var fileExtras: [MenuElement] = []
    /// Edit menu: after L("Select All").
    public var editExtras: [MenuElement] = []
    /// Edit › Найти submenu (find, find next, find previous, replace).
    public var find: [MenuElement] = []
    /// View menu: app items; Kit42 appends sidebar/toolbar/full-screen items.
    public var view: [MenuElement] = []
    public var insert: [MenuElement] = []
    public var format: [MenuElement] = []
    /// App-specific top-level menus between Формат and Окно, e.g. "Страница", "Таблица".
    public var custom: [(title: String, items: [MenuElement])] = []
    public var help: [MenuElement] = []

    public init(appName: String) { self.appName = appName }
}

public enum MenuBar {
    public static func build(_ spec: MenuBarSpec) -> NSMenu {
        let main = NSMenu()
        let app = spec.appName

        var appItems: [MenuElement] = [
            .item(item(L("About %@", app), #selector(NSApplication.orderFrontStandardAboutPanel(_:)))),
            .separator,
        ]
        if let prefs = spec.preferences { appItems += [.command(prefs), .separator] }
        #if APP_STORE
        AppStoreEdition.start()
        appItems += [.item(AppStoreEdition.importMenuItem()), .separator]
        #endif
        let services = NSMenuItem(title: L("Services"), action: nil, keyEquivalent: "")
        services.submenu = NSMenu(title: L("Services"))
        NSApp.servicesMenu = services.submenu
        appItems += [
            .item(services), .separator,
            .item(item(L("Hide %@", app), #selector(NSApplication.hide(_:)), "h")),
            .item(item(L("Hide Others"), #selector(NSApplication.hideOtherApplications(_:)), "h", [.command, .option])),
            .item(item(L("Show All"), #selector(NSApplication.unhideAllApplications(_:)))),
            .separator,
            .item(item(L("Quit %@", app), #selector(NSApplication.terminate(_:)), "q")),
        ]
        main.addItem(top(app, appItems))

        let recent = NSMenuItem(title: L("Open Recent"), action: nil, keyEquivalent: "")
        recent.submenu = RecentDocumentsMenu.shared.makeMenu()

        var file: [MenuElement] = [
            .item(item(L("New"), #selector(NSDocumentController.newDocument(_:)), "n")),
            .item(item(L("Open…"), #selector(NSDocumentController.openDocument(_:)), "o")),
            .item(recent),
            .separator,
            .item(item(L("Close"), #selector(NSWindow.performClose(_:)), "w")),
            .item(item(L("Save"), #selector(NSDocument.save(_:)), "s")),
            .item(item(L("Save As…"), #selector(NSDocument.saveAs(_:)), "s", [.command, .shift])),
            .item(item(L("Duplicate"), #selector(NSDocument.duplicate(_:)))),
            .item(item(L("Revert to Saved"), #selector(NSDocument.revertToSaved(_:)))),
        ]
        if !spec.fileExtras.isEmpty { file += [.separator] + spec.fileExtras }
        file += [
            .separator,
            .item(item(L("Page Setup…"), #selector(NSDocument.runPageLayout(_:)), "p", [.command, .shift])),
            .item(item(L("Print…"), #selector(NSDocument.printDocument(_:)), "p")),
        ]
        main.addItem(top(L("File"), file))

        var edit: [MenuElement] = [
            .item(item(L("Undo"), Selector(("undo:")), "z")),
            .item(item(L("Redo"), Selector(("redo:")), "z", [.command, .shift])),
            .separator,
            .item(item(L("Cut"), #selector(NSText.cut(_:)), "x")),
            .item(item(L("Copy"), #selector(NSText.copy(_:)), "c")),
            .item(item(L("Paste"), #selector(NSText.paste(_:)), "v")),
            .item(item(L("Select All"), #selector(NSText.selectAll(_:)), "a")),
        ]
        if !spec.editExtras.isEmpty { edit += [.separator] + spec.editExtras }
        if !spec.find.isEmpty { edit += [.separator, .submenu(L("Find"), spec.find)] }
        main.addItem(top(L("Edit"), edit))

        var view = spec.view
        if !view.isEmpty { view.append(.separator) }
        view += [
            .item(item(L("Show Sidebar"), #selector(NSSplitViewController.toggleSidebar(_:)), "s", [.command, .control])),
            .item(item(L("Show Toolbar"), #selector(NSWindow.toggleToolbarShown(_:)), "t", [.command, .option])),
            .item({
                let i = item(L("Show Toolbar Labels"), #selector(ToolbarLabelsToggle.toggleToolbarLabels(_:)))
                i.target = ToolbarLabelsToggle.shared
                return i
            }()),
            .item(item(L("Customize Toolbar…"), #selector(NSWindow.runToolbarCustomizationPalette(_:)))),
            .item(item(L("Hide All Panels"), #selector(PanelHiding.toggleAllPanelsAction(_:)), "\\", [.command, .shift])),
            .separator,
            .item(item(L("Enter Full Screen"), #selector(NSWindow.toggleFullScreen(_:)), "f", [.command, .control])),
        ]
        main.addItem(top(L("View"), view))

        if !spec.insert.isEmpty { main.addItem(top(L("Insert"), spec.insert)) }
        if !spec.format.isEmpty { main.addItem(top(L("Format"), spec.format)) }
        for menu in spec.custom { main.addItem(top(menu.title, menu.items)) }

        let window = top(L("Window"), [
            .item(item(L("Minimize"), #selector(NSWindow.performMiniaturize(_:)), "m")),
            .item(item(L("Zoom"), #selector(NSWindow.performZoom(_:)))),
            .separator,
            .item(item(L("Bring All to Front"), #selector(NSApplication.arrangeInFront(_:)))),
        ])
        main.addItem(window)
        NSApp.windowsMenu = window.submenu

        let help = top(L("Help"), spec.help)
        main.addItem(help)
        NSApp.helpMenu = help.submenu

        return main
    }

    // MARK: - Helpers

    static func item(_ title: String, _ action: Selector?, _ key: String = "",
                     _ mods: NSEvent.ModifierFlags = .command) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: action, keyEquivalent: key)
        item.keyEquivalentModifierMask = mods
        return item
    }

    public static func menu(_ title: String, _ elements: [MenuElement]) -> NSMenu {
        let menu = NSMenu(title: title)
        for element in elements {
            switch element {
            case .command(let command): menu.addItem(command.menuItem())
            case .separator: menu.addItem(.separator())
            case .item(let item): menu.addItem(item)
            case .submenu(let title, let children):
                let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
                item.submenu = self.menu(title, children)
                menu.addItem(item)
            }
        }
        return menu
    }

    static func top(_ title: String, _ elements: [MenuElement]) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
        item.submenu = menu(title, elements)
        return item
    }
}

public extension Array where Element == Command {
    var menuElements: [MenuElement] { map { .command($0) } }
}
