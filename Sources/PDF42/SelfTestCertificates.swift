import CryptoKit
import Foundation
import Kit42

/// "Get a Certificate…": PKCS#10 requests, pairing certificates with keys, and Secure Enclave PAdES signing.
extension PDFSelfTest {
    static func testCertificateRequestAndEnclaveSignature() throws {
        guard KeyVault.secureEnclaveAvailable else { print("  (skipped: no Secure Enclave)"); return }
        let saved = KeyVault.directory
        KeyVault.directory = FileManager.default.temporaryDirectory.appendingPathComponent("pdf42-keys-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: KeyVault.directory); KeyVault.directory = saved }

        let keyName = "Self-test"
        var key = try KeyVault.create(label: keyName, storage: .secureEnclave, userPresence: false)
        equal(key.algorithm, .p256)
        equal(key.status, .needsRequest)
        var subject = CertificateSubject()
        subject.commonName = "Ayşe Yılmaz"
        subject.email = "ayse@example.com"
        subject.country = "TR"
        subject.serialNumber = "PNOTR-12345678901"
        let request = [UInt8](try KeyVault.certificationRequest(for: &key, subject: subject))
        equal(key.status, .waitingForCertificate)

        // PKCS#10: info, ecdsa-with-SHA256, and a signature that verifies with the request's own public key.
        let csr = try unwrap(DER.parse(request))
        equal(csr.children.count, 3)
        let info = csr.children[0]
        equal(DER.encode(info.children[0]), DER.int(0))
        equal(DER.oidString(csr.children[1].children[0].content), OID.ecdsaWithSHA256)
        let spki = DER.encode(info.children[2])
        equal(Data(spki), try KeyVault.publicKeyInfo(key))
        let publicKey = try P256.Signing.PublicKey(derRepresentation: spki)
        let signature = try P256.Signing.ECDSASignature(derRepresentation: Array(csr.children[2].content.dropFirst()))
        check(publicKey.isValidSignature(signature, for: Data(DER.encode(info))), "CSR signature does not verify")
        let subjectText = String(decoding: DER.encode(info.children[1]), as: UTF8.self)
        check(subjectText.contains("Ayşe Yılmaz") && subjectText.contains("PNOTR-12345678901"), "subject fields missing")
        let requested = DER.all([0x06], in: info.children[3]).map { DER.oidString($0.content) }
        check(requested.contains("2.5.29.15") && requested.contains("2.5.29.17"), "key usage / e-mail not requested")

        // A certificate for another key is refused; the key's own self-signed certificate is paired.
        let other = try TestSigner(name: "Other")
        check((try? KeyVault.importCertificate(Data(other.chain[0].der), into: key, persist: false)) == nil, "foreign certificate accepted")
        try KeyVault.selfSign(&key, subject: subject)
        equal(key.status, .ready)
        equal(KeyVault.enclaveIdentities().map(\.id), [key.id])

        // ECDSA P-256 CMS in PAdES B-B, checked by the PDF42 verifier.
        let options = PDFSigner.Options(signer: .secureEnclave(key), reason: "Enclave", location: "")
        let signed = try PAdES.sign(minimalPDF(), options: options).data
        let results = PDFSignatureVerifier.verify(signed)
        equal(results.count, 1)
        equal(results.first?.signer, "Ayşe Yılmaz")
        equal(results.first?.level, "PAdES B-B")
        check(results.first?.status != .invalid, results.first?.problem ?? "invalid")
        let cms = try unwrap(PAdES.signatures(in: [UInt8](signed)).first?.cms)
        let algorithm = try unwrap(cms.signerInfo?.children.dropLast().last?.children.first)
        equal(DER.oidString(algorithm.content), OID.ecdsaWithSHA256)
        try KeyVault.delete(key)
        check(KeyVault.all().isEmpty, "key record left behind")
    }
}
