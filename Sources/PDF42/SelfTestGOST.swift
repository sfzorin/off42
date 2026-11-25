import Foundation
import Kit42

/// GOST parsing regressions (no CryptoPro CSP needed): TC 26 reference CMS, certmgr output, МЧД XML.
extension PDFSelfTest {
    /// TC 26 reference SignedData (GOST R 34.10-2012 512-bit signer, Streebog-512, attached content), from the
    /// test vectors published with the open-source gost-engine (tcl_tests/tc26_cms/signed_a111.pem).
    static let tc26SignedA111 = "MIIENwYJKoZIhvcNAQcCoIIEKDCCBCQCAQExDDAKBggqhQMHAQECAzA7BgkqhkiG9w0BBwGgLgQsyu7t8vDu6/zt++kg7/Do7OXwIOTr/yDx8vDz6vLz8PsgU2lnbmVkRGF0YS6gggI6MIICNjCCAeOgAwIBAgIEAYy6hDAKBggqhQMHAQEDAjA4MQ0wCwYDVQQKEwRUSzI2MScwJQYDVQQDEx5DQSBUSzI2OiBHT1NUIDM0LjEwLTEyIDI1Ni1iaXQwHhcNMDEwMTAxMDAwMDAwWhcNNDkxMjMxMDAwMDAwWjA7MQ0wCwYDVQQKEwRUSzI2MSowKAYDVQQDEyFPUklHSU5BVE9SOiBHT1NUIDM0LjEwLTEyIDUxMi1iaXQwgaowIQYIKoUDBwEBAQIwFQYJKoUDBwECAQIBBggqhQMHAQECAwOBhAAEgYC0i7davCkOGGVcYqFPtS1fUIROzB0fYARIe0tclTRpare/qzRuVRapqzzO+K21LDpYVfDPs2Sqa13ZN+Ts/JUlv59qCFB2cYpFyB/0kh4+K79yvz7r8+4WE0EmZf8T3ae/J1Jo6xGunecH1/G4hMts9HYLnxbwJDMNVGuIHV6gzqOBhTCBgjBhBgNVHQEEWjBYgBSA2Qz3mfhmTZNTiY7AnnEtp6cxEqE6MDgxDTALBgNVBAoTBFRLMjYxJzAlBgNVBAMTHkNBIFRLMjY6IEdPU1QgMzQuMTAtMTIgMjU2LWJpdIIEAYy6gTAdBgNVHQ4EFgQUK+l9HAscONGxzCcRpxRAmFHvlXowCgYIKoUDBwEBAwIDQQAbjA0Q41/rIKOOvjHKsAsoEJM+WJf6/PKXg2JaStthmw99bdtwwkU/qDbcje2tF6mt+XWyQBXwvfeES1GFY9fJMYIBlDCCAZACAQEwQDA4MQ0wCwYDVQQKEwRUSzI2MScwJQYDVQQDEx5DQSBUSzI2OiBHT1NUIDM0LjEwLTEyIDI1Ni1iaXQCBAGMuoQwCgYIKoUDBwEBAgOgga0wGAYJKoZIhvcNAQkDMQsGCSqGSIb3DQEHATAcBgkqhkiG9w0BCQUxDxcNMTkwMzIwMTk1NTIyWjAiBgkqhkiG9w0BCWIxFQQTU2lnbmVkIGF0dHIncyB2YWx1ZTBPBgkqhkiG9w0BCQQxQgRAUdPHEukF5BIfo9DoQIMdnB0ZLkzq0RueEUZSNv07A7C+GKWiG62fueArg8uPCHPTUN6d/42p33fgMkEwH7f7cDAKBggqhQMHAQEBAgSBgGUnVka8FvTlClmOtj/FUUacBdE/nEBeMLOO/535VDYrXlftPE6zQf/4ghS7TQG2VRGQ3GWD+L3+W09A7d5uyyTEbvgtdllUG0OyqFwKmJEaYsMin87SFVs0cn1PGV1fOKeLluZabLx5whxd+mzlpekL5i6ImRX+TpERxrA/xSe5"

    static func testGOSTReferenceCMSIsParsed() throws {
        let cms = try unwrap(CMSInfo(Data(base64Encoded: tc26SignedA111)!))
        check(cms.isGOST, "GOST detected")
        equal(cms.signers.count, 1)
        let signer = try unwrap(cms.signers.first)
        equal(signer.digestAlgorithm, "1.2.643.7.1.1.2.3")
        equal(signer.certificate?.commonName, "ORIGINATOR: GOST 34.10-12 512-bit")
        equal(signer.certificate?.publicKeyAlgorithm, "1.2.643.7.1.1.1.2")
        equal(signer.signingTime?.timeIntervalSince1970, 1_553_111_722, "signingTime 2019-03-20 19:55:22Z")
        check(cms.content?.isEmpty == false, "attached content")
        // PEM armour is accepted too.
        let pem = "-----BEGIN CMS-----\n" + tc26SignedA111 + "\n-----END CMS-----\n"
        check(CMSInfo(Data(pem.utf8))?.isGOST == true, "PEM input")
    }

    static func testGOSTSignatureInPDFWithoutCSPIsNotChecked() throws {
        let cms = Data(base64Encoded: tc26SignedA111)!
        let hex = cms.map { String(format: "%02X", $0) }.joined() + String(repeating: "0", count: 64)
        let head = "%PDF-1.7\n1 0 obj\n<< /Type /Sig /Filter /CryptoPro#20PDF /SubFilter /adbe.pkcs7.detached /ByteRange "
        let placeholder = "[0 0000000000 0000000000 0000000000]"
        let prefix = head + placeholder + " /Contents "
        let a = prefix.utf8.count, b = a + hex.utf8.count + 2
        let tail = " /M (D:20260101000000+03'00') >>\nendobj\n%%EOF\n"
        let range = "[0 \(a) \(b) \(tail.utf8.count)]".padding(toLength: placeholder.count, withPad: " ", startingAt: 0)
        let pdf = head + range + " /Contents <" + hex + ">" + tail
        let saved = ProcessInfo.processInfo.environment["SUITE42_CPROCSP_BIN"]
        setenv("SUITE42_CPROCSP_BIN", "/nonexistent-cprocsp", 1)
        defer { if let saved { setenv("SUITE42_CPROCSP_BIN", saved, 1) } else { unsetenv("SUITE42_CPROCSP_BIN") } }
        let results = PDFSignatureVerifier.verify(Data(pdf.utf8))
        equal(results.count, 1)
        let s = try unwrap(results.first)
        equal(s.status, .notChecked)
        check(s.isGOST, "GOST")
        check(s.signer.contains("ORIGINATOR"), s.signer)
        check(s.coversWholeFile, "whole file")
    }

    static func testCertmgrListIsParsed() {
        let english = """
        Certmgr 1.1 (c) "Crypto-Pro",  2007-2024.
        =============================================================================
        1-------
        Issuer              : CN=CRYPTO-PRO Test Center 2, O=CRYPTO-PRO LLC, C=RU
        Subject             : CN=Тестов Тест, SNILS=12345678901
        SHA1 Hash           : 0x4a2c0d8b3e1f00112233445566778899aabbccdd
        Not valid after     : 01/03/2027  09:10:00 UTC
        PrivateKey Link     : Yes
        Container           : HDIMAGE\\test.000\\1234
        2-------
        Subject             : CN=Partner
        SHA1 Hash           : 0x0000000000000000000000000000000000000001
        PrivateKey Link     : No
        =============================================================================
        [ErrorCode: 0x00000000]
        """
        let list = CryptoPro.parseCertificateList(english)
        equal(list.count, 2)
        equal(list.first?.thumbprint, "4A2C0D8B3E1F00112233445566778899AABBCCDD")
        equal(list.first?.hasPrivateKey, true)
        equal(list.first?.name, "Тестов Тест")
        equal(list.last?.hasPrivateKey, false)
        let russian = """
        1-------
        Издатель            : CN=УЦ
        Субъект             : CN=Иванов
        SHA1 отпечаток      : 0x00112233445566778899aabbccddeeff00112233
        Истекает            : 01/03/2027  09:10:00 UTC
        Ссылка на ключ      : Есть
        Контейнер           : HDIMAGE\\ivanov.000\\0001
        """
        let ru = CryptoPro.parseCertificateList(russian)
        equal(ru.count, 1)
        equal(ru.first?.hasPrivateKey, true)
        equal(ru.first?.container, "HDIMAGE\\ivanov.000\\0001")
        var r = CryptoPro.Result(status: 1, output: "x\n[ErrorCode: 0x200001f9]\n")
        equal(r.errorCode, 0x2000_01F9)
        r.output = "Signed message is created.\n[ErrorCode: 0x00000000]"
        equal(r.errorCode, 0)
    }

    static func testPowerOfAttorneyIsParsed() throws {
        let xml = """
        <?xml version="1.0" encoding="UTF-8"?>
        <Доверенность ВерсФорм="EMCHD_1"><Документ>
        <СвДов НомДовер="abc-42" ДатаВыдДовер="2026-01-01" СрокДейст="2026-12-31"/>
        <Доверит><РосОргДовер><СвРосОрг НаимОрг="ООО Ромашка" ИННЮЛ="7701234567" ОГРН="1027700000000"/>
        <ЛицоБезДов><СвФЛ СНИЛС="999-999-999 99"><ФИО Фамилия="Директоров" Имя="Пётр"/></СвФЛ></ЛицоБезДов></РосОргДовер></Доверит>
        <Пред><СведФизЛ СНИЛС="123-456-789 01" ИННФЛ="771234567890"><ФИО Фамилия="Иванов" Имя="Иван"/></СведФизЛ></Пред>
        <Полном><МашПолн КодПолн="NALOG" НаимПолн="Декларации"/></Полном>
        </Документ></Доверенность>
        """
        let poa = try unwrap(PowerOfAttorney(xml: Data(xml.utf8)))
        equal(poa.number, "abc-42")
        equal(poa.principalINN, "7701234567")
        equal(poa.principalName, "ООО Ромашка")
        equal(poa.representativeSNILS, "12345678901", "the director's SNILS is not the representative's")
        equal(poa.representativeName, "Иванов Иван")
        equal(poa.powers, ["NALOG — Декларации"])
        let inside = try unwrap(PowerOfAttorney.date("2026-12-31"))
        check(!poa.check(signer: nil, signedAt: inside, xml: Data(xml.utf8), poaSignature: nil).contains { $0.text.contains("expired") }, "last day is valid")
        let after = inside.addingTimeInterval(2 * 86_400)
        check(poa.check(signer: nil, signedAt: after, xml: Data(xml.utf8), poaSignature: nil).contains { $0.level == .error }, "expired")
        check(PowerOfAttorney(xml: Data("<Root><a/></Root>".utf8)) == nil, "other XML is rejected")
    }
}
