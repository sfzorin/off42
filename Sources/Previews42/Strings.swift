import Foundation
import Kit42

/// Quick Look extensions run sandboxed and cannot read the suite's language choice:
/// they follow the system language order instead. Existing rows are [en, ru, tr];
/// the other interface languages use Kit42's bundled catalogs.
let previews42Strings: [[String]] = [
    ["First %d rows shown.", "Показаны первые %d строк.", "İlk %d satır gösteriliyor."],
    ["Empty document", "Пустой документ", "Boş belge"],
]

enum Strings {
    static let language: Language = Locale.preferredLanguages.compactMap(Language.matching).first ?? .en

    static let table: [String: [String]] = Dictionary(previews42Strings.map { ($0[0], $0) }, uniquingKeysWith: { a, _ in a })

    static func lookup(_ key: String) -> String {
        if let row = table[key] {
            if language == .ru { return row[1] }
            if language == .tr { return row[2] }
        }
        // Explicit lookup deliberately avoids the suite preference and AppleLanguages mutation.
        return Localization.lookup(key, language: language)
    }

    static var firstRows: String { PL("First %d rows shown.") }
}

/// Localized string for the Quick Look extensions.
func PL(_ key: String) -> String { Strings.lookup(key) }
