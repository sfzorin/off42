import CryptoKit
import Foundation
import Kit42
import Security

/// Offline regressions for PAdES signing, DSS streams and revocation matching (no network).
extension PDFSelfTest {
    /// In-memory RSA key with a self-signed certificate built by the DER helpers.
    struct TestSigner: CMSRawSigner {
        let key: SecKey
        let chain: [X509]

        init(name: String) throws {
            let attrs: [String: Any] = [kSecAttrKeyType as String: kSecAttrKeyTypeRSA, kSecAttrKeySizeInBits as String: 2048]
            var error: Unmanaged<CFError>?
            guard let key = SecKeyCreateRandomKey(attrs as CFDictionary, &error), let pub = SecKeyCopyPublicKey(key),
                  let pkcs1 = SecKeyCopyExternalRepresentation(pub, nil) as Data? else { throw NSError(domain: "PDF42.SelfTest", code: 2) }
            self.key = key
            let name = DER.seq(DER.set([DER.seq(DER.oid("2.5.4.3"), DER.tlv(0x0C, Array(name.utf8)))]))
            let alg = DER.seq(DER.oid(OID.sha256WithRSA), DER.null)
            let spki = DER.seq(DER.seq(DER.oid(OID.rsaEncryption), DER.null), DER.tlv(0x03, [0] + [UInt8](pkcs1)))
            let validity = DER.seq(DER.tlv(0x17, Array("250101000000Z".utf8)), DER.tlv(0x17, Array("350101000000Z".utf8)))
            let tbs = DER.seq(DER.tlv(0xA0, DER.int(2)), DER.int(4242), alg, name, validity, name, spki)
            guard let sig = SecKeyCreateSignature(key, .rsaSignatureMessagePKCS1v15SHA256, Data(tbs) as CFData, &error) as Data?,
                  let cert = X509(der: DER.seq(tbs, alg, DER.tlv(0x03, [0] + [UInt8](sig)))) else { throw NSError(domain: "PDF42.SelfTest", code: 3) }
            chain = [cert]
        }

        func sign(attributes: [UInt8]) throws -> (signature: [UInt8], algorithm: [UInt8]) {
            var error: Unmanaged<CFError>?
            guard let sig = SecKeyCreateSignature(key, .rsaSignatureMessagePKCS1v15SHA256, Data(attributes) as CFData, &error) as Data? else {
                throw NSError(domain: "PDF42.SelfTest", code: 4)
            }
            return ([UInt8](sig), DER.seq(DER.oid(OID.sha256WithRSA), DER.null))
        }
    }

    /// One-page PDF with a classic xref table.
    static func minimalPDF() -> Data {
        var s = "%PDF-1.7\n"
        var offsets: [Int] = []
        for body in ["<< /Type /Catalog /Pages 2 0 R >>", "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
                     "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>"] {
            offsets.append(s.utf8.count)
            s += "\(offsets.count) 0 obj\n\(body)\nendobj\n"
        }
        let xref = s.utf8.count
        s += "xref\n0 4\n0000000000 65535 f\r\n" + offsets.map { String(format: "%010d 00000 n\r\n", $0) }.joined()
        s += "trailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n\(xref)\n%%EOF\n"
        return Data(s.utf8)
    }

    static func testDERObjectIdentifiers() {
        for oid in [OID.sha256, OID.signingCertificateV2, OID.ecdsaWithSHA256, "1.2.643.7.1.1.2.2"] {
            let encoded = DER.oid(oid)
            equal(DER.oidString(Array(encoded.dropFirst(2))), oid)
        }
        equal(DER.int(128), [0x02, 0x02, 0x00, 0x80])
        equal(DER.set([[0x04, 0x01, 0x02], [0x04, 0x01, 0x01]]), [0x31, 0x06, 0x04, 0x01, 0x01, 0x04, 0x01, 0x02])
    }

    static func testPAdESBaselineSignature() throws {
        let signer = try TestSigner(name: "PDF42 PAdES self-test")
        var options = PDFSigner.Options(identity: nil, reason: "Test", location: "")
        options.remote = signer
        options.level = .baselineB
        let signed = try PAdES.sign(minimalPDF(), options: options).data
        let results = PDFSignatureVerifier.verify(signed)
        equal(results.count, 1)
        equal(results.first?.level, "PAdES B-B")
        check(results.first?.status != .invalid, results.first?.problem ?? "invalid")
        equal(results.first?.signer, "PDF42 PAdES self-test")
        equal(results.first?.ltvEnabled, false)

        // CMS: ETSI.CAdES.detached with signing-certificate-v2 over the signer certificate, no signing-time.
        let sig = try unwrap(PAdES.signatures(in: [UInt8](signed)).first)
        equal(sig.subFilter, "ETSI.CAdES.detached")
        let cms = try unwrap(sig.cms)
        check(cms.signedAttributes[OID.signingTime] == nil, "signing-time must be absent")
        let essHash = cms.signedAttributes[OID.signingCertificateV2].flatMap { DER.all([0x04], in: $0).first?.content }
        equal(essHash, signer.chain[0].sha256)

        // Any change to the signed bytes invalidates it.
        var tampered = [UInt8](signed)
        let i = try unwrap(PDFSigner.find("MediaBox [0 0 200", in: tampered, from: 0))
        tampered[i + 15] = UInt8(ascii: "3")
        equal(PDFSignatureVerifier.verify(Data(tampered)).first?.status, .invalid)

        // Legacy format stays available.
        options.level = .legacy
        let legacy = try PAdES.sign(minimalPDF(), options: options).data
        equal(PAdES.signatures(in: [UInt8](legacy)).first?.subFilter, "adbe.pkcs7.detached")
        equal(PDFSignatureVerifier.verify(legacy).first?.level, L("Basic PDF signature (not PAdES)"))
    }

    static func testIncrementalStreamsRoundTrip() throws {
        let file = try PDFFileStructure(minimalPDF())
        var update = IncrementalUpdate(file: file)
        let plain = update.addStream(Array("plain stream".utf8))
        let packed = try unwrap(zlibCompress(Array("deflated stream".utf8)))
        let flate = update.add(Array("<< /Length \(packed.count) /Filter /FlateDecode >>\nstream\n".utf8) + packed + Array("\nendstream".utf8))
        let out = try PDFFileStructure(Data(update.write()))
        equal(try out.streamData(plain), Array("plain stream".utf8))
        equal(try out.streamData(flate), Array("deflated stream".utf8))
        equal(out.trailer["Size"]?.int, 6)
        equal(out.trailer["Prev"]?.int, file.startxref)
    }

    static func testRevocationCoverageByCRL() throws {
        let cert = try TestSigner(name: "Leaf").chain[0]
        let issuerName = cert.issuer
        func crl(revoking serials: [[UInt8]]) -> [UInt8] {
            var tbs = DER.int(1) + DER.seq(DER.oid(OID.sha256WithRSA), DER.null) + issuerName + DER.tlv(0x17, Array("260101000000Z".utf8))
            if !serials.isEmpty {
                tbs += DER.seq(serials.map { DER.seq(DER.tlv(0x02, $0), DER.tlv(0x17, Array("260101000000Z".utf8))) })
            }
            return DER.seq(DER.seq(tbs), DER.seq(DER.oid(OID.sha256WithRSA), DER.null), DER.tlv(0x03, [0, 1]))
        }
        check(Revocation.covered(cert, ocsps: [], crls: [crl(revoking: [])]), "CRL without the serial covers the certificate")
        check(!Revocation.covered(cert, ocsps: [], crls: [crl(revoking: [cert.serial])]), "a CRL that revokes the serial does not")
        check(!Revocation.covered(cert, ocsps: [], crls: []), "no data, no coverage")
    }
}
