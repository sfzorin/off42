import AppKit

/// UI language shared by all Suite42 apps. English is the source language: every string in code
/// is English and looked up in the per-language tables registered by Kit42 and each app.
public enum Language: String, CaseIterable {
    // Keep the picker order aligned with emshandbook.com, including both Chinese scripts.
    case en
    case zhHans = "zh-Hans"
    case zhHant = "zh-Hant"
    case es, ar, fr, pt, id, ru, de, ja, vi, tr, fil, ko, th, it, pl, ms, hu

    public var nativeName: String {
        switch self {
        case .en: return "English"
        case .zhHans: return "简体中文"
        case .zhHant: return "繁體中文（台灣）"
        case .es: return "Español"
        case .ar: return "العربية"
        case .fr: return "Français"
        case .pt: return "Português"
        case .id: return "Bahasa Indonesia"
        case .ru: return "Русский"
        case .de: return "Deutsch"
        case .ja: return "日本語"
        case .vi: return "Tiếng Việt"
        case .tr: return "Türkçe"
        case .fil: return "Filipino"
        case .ko: return "한국어"
        case .th: return "ไทย"
        case .it: return "Italiano"
        case .pl: return "Polski"
        case .ms: return "Bahasa Melayu"
        case .hu: return "Magyar"
        }
    }

    public var isRightToLeft: Bool { self == .ar }

    /// Match a system language identifier without treating traditional Chinese as simplified.
    /// Explicit script subtags win over regions; Apple also uses `tl` for Filipino.
    public static func matching(_ identifier: String) -> Language? {
        let parts = identifier.replacingOccurrences(of: "_", with: "-").lowercased().split(separator: "-")
        guard let base = parts.first else { return nil }
        if base == "zh" {
            if parts.contains("hant") { return .zhHant }
            if parts.contains("hans") { return .zhHans }
            return parts.contains(where: { ["tw", "hk", "mo"].contains($0) }) ? .zhHant : .zhHans
        }
        if base == "tl" { return .fil }
        return Language(rawValue: String(base))
    }

    #if APP_STORE
    static let defaults = AppStoreEdition.sharedDefaults
    #else
    static let defaults = UserDefaults(suiteName: "com.sfzorin.suite42") ?? .standard
    #endif

    /// Chosen once at launch; changing it relaunches the app (see `switchLanguage`).
    /// `-Suite42Language <code>` overrides this process only, for isolated UI audits.
    public static let current = selectedLanguage(arguments: ProcessInfo.processInfo.arguments,
                                                 saved: defaults.string(forKey: "language"))

    static func selectedLanguage(arguments: [String], saved: String?) -> Language {
        if let index = arguments.lastIndex(of: "-Suite42Language"), arguments.indices.contains(index + 1),
           let language = matching(arguments[index + 1]) {
            return language
        }
        return saved.flatMap(matching) ?? .en
    }

    /// Saves the choice for every Suite42 app and relaunches this one.
    public static func switchLanguage(to language: Language) {
        guard language != current else { return }
        defaults.set(language.rawValue, forKey: "language")
        defaults.synchronize()
        let path = Bundle.main.bundlePath
        let task = Process()
        task.executableURL = URL(fileURLWithPath: "/bin/sh")
        task.arguments = ["-c", "sleep 0.5; /usr/bin/open \"$1\"", "sh", path]
        try? task.run()
        NSApp.terminate(nil)
    }
}

public enum Localization {
    /// English key → [language: translation]
    private static var tables: [String: [Language: String]] = Dictionary(
        (kit42Strings + productionStrings).map { ($0[0], [Language.ru: $0[1], .tr: $0[2]]) }, uniquingKeysWith: { _, b in b })

    /// Additional languages are shared by the six apps and Quick Look extensions.
    /// Keep these separate from the legacy rows so registering an app cannot erase a language.
    private static let catalogs: [Language: [String: String]] = {
        // SwiftPM's generated accessor checks the app root and the development build path.
        // Packaged apps/extensions use the standard, code-signed Contents/Resources location.
        let bundle = Bundle.main.resourceURL.flatMap {
            Bundle(url: $0.appendingPathComponent("Suite42_Kit42.bundle"))
        } ?? Bundle.module
        var result: [Language: [String: String]] = [:]
        for language in Language.allCases where language != .en && language != .ru && language != .tr {
            guard let url = bundle.url(forResource: language.rawValue, withExtension: "json", subdirectory: "Translations"),
                  let data = try? Data(contentsOf: url),
                  let catalog = try? JSONDecoder().decode([String: String].self, from: data) else { continue }
            result[language] = catalog
        }
        return result
    }()

    /// Registers translations: `["Open…": [.ru: "Открыть…", .tr: "Aç…"]]`.
    public static func register(_ table: [String: [Language: String]]) {
        _ = systemLanguageApplied
        for (key, translations) in table {
            tables[key, default: [:]].merge(translations) { _, new in new }
        }
    }

    /// System panels (Print, Save, Colors, toolbar items) follow the app's AppleLanguages, not our tables:
    /// make it the chosen language. Runs at the first registration, before AppKit loads its strings.
    private static let systemLanguageApplied: Void = {
        // Use the argument domain rather than persisting audit overrides or changing macOS preferences.
        // AppKit handles natural Arabic text direction; document geometry is never mirrored here.
        var arguments = UserDefaults.standard.volatileDomain(forName: UserDefaults.argumentDomain)
        arguments["AppleLanguages"] = [Language.current.rawValue]
        UserDefaults.standard.setVolatileDomain(arguments, forName: UserDefaults.argumentDomain)
    }()

    /// Compact registration: each row is [en, ru, tr].
    public static func register(rows: [[String]]) {
        _ = systemLanguageApplied
        _ = DebugSnapshot.appearanceHook
        for row in rows where row.count == 3 {
            tables[row[0], default: [:]].merge([.ru: row[1], .tr: row[2]]) { _, new in new }
        }
    }

    public static func lookup(_ key: String) -> String {
        lookup(key, language: .current)
    }

    /// Explicit-language lookup is also used by sandboxed Quick Look, which follows macOS settings.
    public static func lookup(_ key: String, language: Language) -> String {
        guard language != .en else { return key }
        let value = tables[key]?[language] ?? catalogs[language]?[key]
        return value.flatMap { $0.isEmpty ? nil : $0 } ?? key
    }
}

/// Localized UI string. The key is the English text.
public func L(_ key: String) -> String { Localization.lookup(key) }

/// Localized format string: `L("Page %d of %d", 3, 12)`.
public func L(_ key: String, _ args: CVarArg...) -> String {
    String(format: Localization.lookup(key), arguments: args)
}

/// Popup for choosing the UI language (used on welcome screens).
public final class LanguagePicker: NSPopUpButton {
    public init() {
        super.init(frame: .zero, pullsDown: false)
        for language in Language.allCases {
            addItem(withTitle: language.nativeName)
            lastItem?.representedObject = language.rawValue
        }
        selectItem(at: Language.allCases.firstIndex(of: .current) ?? 0)
        controlSize = .small
        font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        target = self
        action = #selector(changed)
        toolTip = L("Interface language")
    }

    required init?(coder: NSCoder) { fatalError("not supported") }

    @objc private func changed() {
        guard let raw = selectedItem?.representedObject as? String, let language = Language(rawValue: raw) else { return }
        if NSDocumentController.shared.documents.contains(where: \.isDocumentEdited) {
            let alert = NSAlert()
            alert.messageText = L("Restart to change the language?")
            alert.informativeText = L("Unsaved documents will ask to be saved.")
            alert.addButton(withTitle: L("Restart"))
            alert.addButton(withTitle: L("Cancel"))
            guard alert.runModal() == .alertFirstButtonReturn else {
                selectItem(at: Language.allCases.firstIndex(of: .current) ?? 0)
                return
            }
        }
        Language.switchLanguage(to: language)
    }
}
