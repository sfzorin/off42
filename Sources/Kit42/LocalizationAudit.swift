import AppKit

/// Opt-in native checks for the isolated welcome-window localization audit.
enum LocalizationAudit {
    static func runIfRequested(in window: NSWindow?) -> Bool {
        guard let path = ProcessInfo.processInfo.environment["K42_LOCALIZATION_AUDIT"] else { return true }
        var failures: [String] = []
        func check(_ condition: Bool, _ message: String) {
            if !condition { failures.append(message) }
        }
        func descendants(_ view: NSView) -> [NSView] {
            [view] + view.subviews.flatMap(descendants)
        }
        let views = window?.contentView.map(descendants) ?? []
        let picker = views.compactMap { $0 as? LanguagePicker }.first
        let codes = picker?.itemArray.compactMap { $0.representedObject as? String } ?? []
        check(Bundle.main.infoDictionary?["CFBundleLocalizations"] as? [String] == Language.allCases.map(\.rawValue),
              "App metadata must declare the same 20 languages")
        check(codes == Language.allCases.map(\.rawValue), "Native picker must contain all 20 language codes in order")
        check(picker?.selectedItem?.representedObject as? String == Language.current.rawValue,
              "Native picker selection must match the process language")
        check(picker?.itemArray.map(\.title) == Language.allCases.map(\.nativeName),
              "Native picker must show the native language names")
        let menus = NSApp.mainMenu?.items.compactMap { $0.submenu?.title } ?? []
        check(menus.contains(L("File")), "Native menu must contain the translated File menu")
        let open = L("Open…")
        check(Language.current == .en || open != "Open…", "The selected translation catalog must load")
        var labels: [String] = []
        for view in views where !view.isHiddenOrHasHiddenAncestor {
            if let field = view as? NSTextField, !field.stringValue.isEmpty {
                labels.append(field.stringValue)
                check(field.bounds.width + 1 >= field.intrinsicContentSize.width,
                      "Clipped welcome label: \(field.stringValue)")
            } else if let button = view as? NSButton, !(button is NSPopUpButton), !button.title.isEmpty {
                labels.append(button.title)
                check(button.bounds.width + 1 >= button.intrinsicContentSize.width,
                      "Clipped welcome action: \(button.title)")
            }
        }
        let report: [String: Any] = [
            "language": Language.current.rawValue,
            "picker_codes": codes,
            "menu_titles": menus,
            "visible_labels": labels,
            "open_translation": open,
            "failures": failures,
        ]
        do {
            let data = try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys])
            try data.write(to: URL(fileURLWithPath: path))
        } catch {
            failures.append("Cannot save native audit report: \(error)")
        }
        print("Localization UI: \(failures.isEmpty ? "PASS" : "FAIL") \(Language.current.rawValue)")
        failures.forEach { print($0) }
        return failures.isEmpty
    }
}
