import XCTest
@testable import OFF42

/// The String Catalog compiled into the app: every key in every one of the 20 languages, with the same format
/// arguments as English (a mismatch would crash String(format:)).
final class LocalizationTests: XCTestCase {
    static let languages = ["en", "ar", "de", "es", "fil", "fr", "hu", "id", "it", "ja", "ko", "ms", "pl", "pt", "ru", "th",
                            "tr", "vi", "zh-Hans", "zh-Hant"]

    private func table(_ language: String) throws -> [String: String] {
        let bundle = Bundle(for: AppModel.self)
        let url = try XCTUnwrap(bundle.url(forResource: "Localizable", withExtension: "strings", subdirectory: nil,
                                           localization: language), "Localizable.strings for \(language)")
        let data = try Data(contentsOf: url)
        return try XCTUnwrap(try PropertyListSerialization.propertyList(from: data, format: nil) as? [String: String], language)
    }

    private func arguments(_ text: String) -> [String] {
        let regex = try! NSRegularExpression(pattern: #"%(\d+\$)?(@|lld|d)"#)
        let range = NSRange(text.startIndex..., in: text)
        return regex.matches(in: text, range: range).map { (text as NSString).substring(with: $0.range) }.sorted()
    }

    func testEveryLanguageHasEveryStringWithTheSameArguments() throws {
        let english = try table("en")
        XCTAssertGreaterThan(english.count, 90)
        for key in ["open_file", "section_documents", "doc_stats", "pdf_info", "slide_n", "done", "privacy_note"] {
            XCTAssertNotNil(english[key], key)
        }
        for language in Self.languages {
            let strings = try table(language)
            XCTAssertEqual(Set(strings.keys), Set(english.keys), language)
            for (key, text) in strings {
                XCTAssertEqual(arguments(text), arguments(english[key] ?? ""), "\(language) \(key): \(text)")
            }
        }
        XCTAssertEqual(try table("ar")["section_documents"], "المستندات")
        XCTAssertEqual(try table("ja")["open_file"], "ファイルを開く")
    }

    func testFormattedStrings() {
        XCTAssertEqual(tr("doc_stats", "3", "20", "2"), "3 words · 20 characters · 2 paragraphs")
        XCTAssertEqual(tr("labels_status", 5, 1), "5 labels on 1 page(s).")
        XCTAssertEqual(tr("pdf_page", 2, 9), "Page 2 of 9")
        XCTAssertEqual(tr("slide_n", 4), "Slide 4")
    }
}
