import Foundation

public enum LocalizationSelfTest {
    private static func testTwentyLanguagesMatchReferenceOrder() {
        equal(Language.allCases.map(\.rawValue), [
            "en", "zh-Hans", "zh-Hant", "es", "ar", "fr", "pt", "id", "ru", "de",
            "ja", "vi", "tr", "fil", "ko", "th", "it", "pl", "ms", "hu",
        ])
        equal(Set(Language.allCases.map(\.nativeName)).count, 20)
        equal(Language.allCases.filter(\.isRightToLeft), [.ar])
    }

    private static func testRegionalAndScriptIdentifiers() {
        let examples: [String: Language] = [
            "en-GB": .en, "pt_BR": .pt, "es-MX": .es, "id-ID": .id,
            "zh-CN": .zhHans, "zh-SG": .zhHans, "zh-TW": .zhHant,
            "zh-HK": .zhHant, "zh_MO": .zhHant, "ZH-hAnT-US": .zhHant,
            "zh-Hans-TW": .zhHans, "zh-Hant-CN": .zhHant, "zh": .zhHans,
            "fil-PH": .fil, "tl-PH": .fil,
        ]
        for (identifier, language) in examples {
            equal(Language.matching(identifier), language, identifier)
        }
        nilValue(Language.matching("uk-UA"))
        nilValue(Language.matching(""))
    }

    private static func testLaunchOverrideAndSavedPreferenceFallback() {
        equal(Language.selectedLanguage(arguments: ["PDF42", "-Suite42Language", "zh-TW"], saved: "ru"), .zhHant)
        equal(Language.selectedLanguage(arguments: ["PDF42"], saved: "tr"), .tr)
        equal(Language.selectedLanguage(arguments: ["PDF42", "-Suite42Language"], saved: "ru"), .ru)
        equal(Language.selectedLanguage(arguments: ["PDF42", "-Suite42Language", "unsupported"], saved: "de"), .de)
        equal(Language.selectedLanguage(arguments: [], saved: "unknown"), .en)
        equal(Language.selectedLanguage(arguments: [], saved: nil), .en)
    }

    private static func testBundledCatalogsResolveForEveryLanguage() {
        for language in Language.allCases where language != .en {
            let value = Localization.lookup("Open…", language: language)
            expectFalse(value.isEmpty, language.rawValue)
            notEqual(value, "Open…", "Missing bundled translation for \(language.rawValue)")
            let formatted = String(format: Localization.lookup("Style %d of %d", language: language), 3, 12)
            expect(formatted.contains("3"), language.rawValue)
            expect(formatted.contains("12"), language.rawValue)
        }
    }

    private static func testLegacyRowsDoNotEraseOtherTranslations() {
        let spanishOpen = Localization.lookup("Open…", language: .es)
        Localization.register(rows: [["Open…", "Открыть…", "Aç…"]])
        equal(Localization.lookup("Open…", language: .es), spanishOpen)
        let key = "__localization_merge_test__"
        Localization.register([key: [.es: "Prueba", .fr: "Essai"]])
        Localization.register(rows: [[key, "Проверка", "Deneme"]])
        Localization.register([key: [.es: "Prueba nueva"]])
        equal(Localization.lookup(key, language: .es), "Prueba nueva")
        equal(Localization.lookup(key, language: .fr), "Essai")
        equal(Localization.lookup(key, language: .ru), "Проверка")
        equal(Localization.lookup(key, language: .tr), "Deneme")
        equal(Localization.lookup(key, language: .en), key)
    }

    private static func testUnknownAndEmptyTranslationsFallBackToSource() {
        let key = "__localization_unknown_test__"
        for language in Language.allCases {
            equal(Localization.lookup(key, language: language), key)
        }
        Localization.register([key: [.de: ""]])
        equal(Localization.lookup(key, language: .de), key)
    }
    private static var failures = 0
    private static var checks = 0

    public static func run() -> Int32 {
        failures = 0; checks = 0
        testTwentyLanguagesMatchReferenceOrder()
        testRegionalAndScriptIdentifiers()
        testLaunchOverrideAndSavedPreferenceFallback()
        testBundledCatalogsResolveForEveryLanguage()
        testLegacyRowsDoNotEraseOtherTranslations()
        testUnknownAndEmptyTranslationsFallBackToSource()
        print("Localization regressions: \(checks - failures)/\(checks) passed")
        return failures == 0 ? 0 : 1
    }

    private static func expect(_ value: Bool, _ message: String = "", file: StaticString = #fileID, line: UInt = #line) {
        checks += 1
        if !value {
            failures += 1
            print("FAIL \(file):\(line) \(message)")
        }
    }
    private static func expectFalse(_ value: Bool, _ message: String = "", file: StaticString = #fileID, line: UInt = #line) {
        expect(!value, message, file: file, line: line)
    }
    private static func equal<T: Equatable>(_ actual: T, _ expected: T, _ message: String = "", file: StaticString = #fileID, line: UInt = #line) {
        expect(actual == expected, "\(message) expected \(expected), got \(actual)", file: file, line: line)
    }
    private static func notEqual<T: Equatable>(_ actual: T, _ other: T, _ message: String = "", file: StaticString = #fileID, line: UInt = #line) {
        expect(actual != other, message, file: file, line: line)
    }
    private static func nilValue<T>(_ value: T?, _ message: String = "", file: StaticString = #fileID, line: UInt = #line) {
        expect(value == nil, message, file: file, line: line)
    }

}
