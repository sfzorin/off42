import Foundation

/// Headless regressions for malformed PDFs and safe attachment extraction.
enum PDFSelfTest {
    static var failures = 0

    static func check(_ condition: Bool, _ message: String = "check", file: StaticString = #filePath, line: UInt = #line) {
        if !condition { failures += 1; print("FAIL \(file):\(line) \(message)") }
    }

    static func equal<T: Equatable>(_ actual: T, _ expected: T, _ message: String = "values differ", file: StaticString = #filePath, line: UInt = #line) {
        check(actual == expected, message, file: file, line: line)
    }

    static func rejects<T>(_ operation: @autoclosure () throws -> T, _ message: String = "expected an error", file: StaticString = #filePath, line: UInt = #line) {
        do { _ = try operation(); check(false, message, file: file, line: line) } catch {}
    }

    static func unwrap<T>(_ value: T?) throws -> T {
        guard let value else { throw NSError(domain: "PDF42.SelfTest", code: 1) }
        return value
    }

    static func testUnterminatedStringsAreRejected() {
        for input in ["<", "<0123", "(", "(text", "(text\\", "(nested(inner)"] {
            var lexer = PDFLexer(bytes: Array(input.utf8), pos: 0)
            rejects(try lexer.parse(), input)
        }
    }

    static func testValidStringsAndNestedObjectsStillParse() throws {
        let input = "<< /Text (outer(inner)\\)end) /Hex <012F> /Array [1 2 0 R /Name] >>"
        var lexer = PDFLexer(bytes: Array(input.utf8), pos: 0)
        let object = try lexer.parse()
        equal(object["Text"]?.serialized, "(outer(inner)\\)end)")
        equal(object["Hex"]?.serialized, "<012F>")
        equal(object["Array"]?.items?.count, 3)
    }

    static func testInvalidLexerOffsetsAreRejected() {
        for offset in [-1, 2, Int.max] {
            var lexer = PDFLexer(bytes: [0x30], pos: offset)
            equal(lexer.word(), "")
            check(!lexer.peek("0"))
            rejects(try lexer.parse())
        }
    }

    static func testExcessiveNestingIsRejected() {
        var lexer = PDFLexer(bytes: Array((String(repeating: "[", count: 1000) + "0" + String(repeating: "]", count: 1000)).utf8), pos: 0)
        rejects(try lexer.parse())
    }

    static func testInvalidCrossReferenceTablesAreRejected() {
        let cases = [
            "startxref\n-1", "startxref\n9223372036854775807",
            "xref\n0 -1\ntrailer\n<<>>\nstartxref\n0",
            "xref\n9223372036854775807 2\n0 0 n\n0 0 n\ntrailer\n<<>>\nstartxref\n0",
            "xref\n0 1\n-1 0 n\ntrailer\n<<>>\nstartxref\n0",
            "xref\n0 1\n9223372036854775807 0 n\ntrailer\n<<>>\nstartxref\n0",
            "xref\n0 0\ntrailer\n<< /Prev -1 >>\nstartxref\n0",
            "xref\n0 0\ntrailer\n<< /Prev 0 >>\nstartxref\n0",
        ]
        for input in cases { rejects(try PDFFileStructure(Data(input.utf8)), input) }
    }

    static func testValidCrossReferenceTableAndObject() throws {
        var input = "%PDF-1.4\n"
        let objectOffset = input.utf8.count
        input += "1 0 obj\n<< /Type /Catalog >>\nendobj\n"
        let xrefOffset = input.utf8.count
        input += "xref\n0 2\n0000000000 65535 f\n\(String(format: "%010d", objectOffset)) 00000 n\n"
        input += "trailer\n<< /Size 2 /Root 1 0 R >>\nstartxref\n\(xrefOffset)\n%%EOF"
        let file = try PDFFileStructure(Data(input.utf8))
        equal(try file.object(1)["Type"]?.nameValue, "Catalog")
        // Incremental saves keep older objects through /Prev and take newer versions first.
        input += "\n"
        let updatedObjectOffset = input.utf8.count
        input += "1 0 obj\n<< /Type /Catalog /Updated true >>\nendobj\n"
        let updatedXrefOffset = input.utf8.count
        input += "xref\n1 1\n\(String(format: "%010d", updatedObjectOffset)) 00000 n\n"
        input += "trailer\n<< /Size 2 /Root 1 0 R /Prev \(xrefOffset) >>\nstartxref\n\(updatedXrefOffset)\n%%EOF"
        let updated = try PDFFileStructure(Data(input.utf8))
        equal(try updated.object(1)["Updated"]?.serialized, "true")
    }

    static func testMalformedSignatureRangesDoNotCrash() {
        let cases = [
            "/ByteRange <", "/ByteRange (\\",
            "/ByteRange [0 1 9223372036854775807 1]",
            "%PDF-1.4\n<>\n/ByteRange [0 9 11 -1]",
            "%PDF-1.4\n<>\n/ByteRange [0 9 11 9223372036854775807]",
            "%PDF-1.4\n<>\n/ByteRange [0 9 /Invalid 11 0]",
        ]
        for input in cases { check(PDFSignatureVerifier.verify(Data(input.utf8)).isEmpty, input) }
    }

    static func testMalformedStreamLengthsUseBoundedFallback() throws {
        for length in [-1, Int.max] {
            var input = "%PDF-1.4\n"
            let rootOffset = input.utf8.count
            input += "1 0 obj\n<< /Type /Catalog /Metadata 2 0 R >>\nendobj\n"
            let streamOffset = input.utf8.count
            input += "2 0 obj\n<< /Length \(length) >>\nstream\nabc\nendstream\nendobj\n"
            let xrefOffset = input.utf8.count
            input += "xref\n0 3\n0000000000 65535 f\n\(String(format: "%010d", rootOffset)) 00000 n\n\(String(format: "%010d", streamOffset)) 00000 n\n"
            input += "trailer\n<< /Size 3 /Root 1 0 R >>\nstartxref\n\(xrefOffset)\n%%EOF"
            let data = Data(input.utf8)
            let writer = try unwrap(Writer(data))
            equal(writer.stream(2)?.1, Array("abc".utf8))
            check(PDFCompactor.compact(data) != nil)
        }
    }

    static func testEmptySignatureIsReportedAsInvalid() {
        // /Contents may legally precede /ByteRange, including with a large gap.
        let input = "%PDF-1.4\n<>" + String(repeating: " ", count: 5000) + "/ByteRange [0 9 11 0]"
        let results = PDFSignatureVerifier.verify(Data(input.utf8))
        equal(results.count, 1)
        equal(results.first?.status, .invalid)
        check(results.first?.problem != nil)
    }

    static func testUntrustedNamesBecomeSingleFileNames() {
        let cases = ["../../secret.txt": "secret.txt", "/tmp/test.txt": "test.txt",
                     "C:\\folder\\report.pdf": "report.pdf", "..": "Attachment", ".": "Attachment",
                     "": "Attachment", "nested/": "Attachment", "bad\u{0000}name.txt": "bad_name.txt",
                     "Отчёт.pdf": "Отчёт.pdf"]
        for (name, expected) in cases { equal(AttachmentExport.fileName(name), expected) }
    }

    static func testExtractionStaysInsideDirectoryAndPreservesExistingFiles() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        let destination = root.appendingPathComponent("attachments")
        try FileManager.default.createDirectory(at: destination, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let original = root.appendingPathComponent("report.txt")
        try Data("original".utf8).write(to: original)
        // A pre-existing symlink must not redirect extraction outside the destination either.
        try FileManager.default.createSymbolicLink(at: destination.appendingPathComponent("report.txt"), withDestinationURL: original)
        let extracted = try AttachmentExport.write(Data("first".utf8), name: "../report.txt", to: destination)
        let duplicate = try AttachmentExport.write(Data("second".utf8), name: "report.txt", to: destination)
        equal(extracted.lastPathComponent, "report (2).txt")
        equal(duplicate.lastPathComponent, "report (3).txt")
        equal(extracted.deletingLastPathComponent().standardizedFileURL, destination.standardizedFileURL)
        equal(try String(contentsOf: original, encoding: .utf8), "original")
        equal(try String(contentsOf: extracted, encoding: .utf8), "first")
        equal(try String(contentsOf: duplicate, encoding: .utf8), "second")
    }

    static func run() -> Int32 {
        failures = 0
        let checks: [(String, () throws -> Void)] = [
            ("testUnterminatedStringsAreRejected", testUnterminatedStringsAreRejected),
            ("testValidStringsAndNestedObjectsStillParse", testValidStringsAndNestedObjectsStillParse),
            ("testInvalidLexerOffsetsAreRejected", testInvalidLexerOffsetsAreRejected),
            ("testExcessiveNestingIsRejected", testExcessiveNestingIsRejected),
            ("testInvalidCrossReferenceTablesAreRejected", testInvalidCrossReferenceTablesAreRejected),
            ("testValidCrossReferenceTableAndObject", testValidCrossReferenceTableAndObject),
            ("testMalformedSignatureRangesDoNotCrash", testMalformedSignatureRangesDoNotCrash),
            ("testMalformedStreamLengthsUseBoundedFallback", testMalformedStreamLengthsUseBoundedFallback),
            ("testEmptySignatureIsReportedAsInvalid", testEmptySignatureIsReportedAsInvalid),
            ("testUntrustedNamesBecomeSingleFileNames", testUntrustedNamesBecomeSingleFileNames),
            ("testExtractionStaysInsideDirectoryAndPreservesExistingFiles", testExtractionStaysInsideDirectoryAndPreservesExistingFiles),
            ("testDERObjectIdentifiers", testDERObjectIdentifiers),
            ("testPAdESBaselineSignature", testPAdESBaselineSignature),
            ("testIncrementalStreamsRoundTrip", testIncrementalStreamsRoundTrip),
            ("testRevocationCoverageByCRL", testRevocationCoverageByCRL),
            ("testGOSTReferenceCMSIsParsed", testGOSTReferenceCMSIsParsed),
            ("testGOSTSignatureInPDFWithoutCSPIsNotChecked", testGOSTSignatureInPDFWithoutCSPIsNotChecked),
            ("testCertmgrListIsParsed", testCertmgrListIsParsed),
            ("testPowerOfAttorneyIsParsed", testPowerOfAttorneyIsParsed),
        ]
        for (name, run) in checks {
            let before = failures
            do { try run() } catch { check(false, "\(name): \(error)") }
            print("\(failures == before ? "PASS" : "FAIL") \(name)")
        }
        print("PDF42 selftest: \(checks.count) groups, \(failures) failures")
        return failures == 0 ? 0 : 1
    }
}
