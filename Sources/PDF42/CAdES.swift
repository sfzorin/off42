import CryptoKit
import Foundation
import Kit42
import Security

/// Minimal DER builder on top of `Timestamp.Node` (parser/encoder).
enum DER {
    typealias Node = Timestamp.Node

    static func tlv(_ tag: UInt8, _ body: [UInt8]) -> [UInt8] { Timestamp.tlv(tag, body) }
    static func seq(_ parts: [UInt8]...) -> [UInt8] { tlv(0x30, parts.flatMap { $0 }) }
    static func seq(_ parts: [[UInt8]]) -> [UInt8] { tlv(0x30, parts.flatMap { $0 }) }
    /// DER SET OF: elements sorted by their encodings.
    static func set(_ parts: [[UInt8]]) -> [UInt8] {
        tlv(0x31, parts.sorted { $0.lexicographicallyPrecedes($1) }.flatMap { $0 })
    }
    static func octet(_ b: [UInt8]) -> [UInt8] { tlv(0x04, b) }
    static let null: [UInt8] = [0x05, 0x00]

    static func int(_ n: Int) -> [UInt8] {
        var bytes: [UInt8] = []
        var v = n
        repeat { bytes.insert(UInt8(v & 0xFF), at: 0); v >>= 8 } while v > 0
        if bytes[0] & 0x80 != 0 { bytes.insert(0, at: 0) }
        return tlv(0x02, bytes)
    }

    static func oid(_ dotted: String) -> [UInt8] {
        let parts = dotted.split(separator: ".").compactMap { UInt64($0) }
        guard parts.count >= 2 else { return [] }
        var body: [UInt8] = [UInt8(parts[0] * 40 + parts[1])]
        for p in parts.dropFirst(2) {
            var chunk: [UInt8] = [UInt8(p & 0x7F)]
            var v = p >> 7
            while v > 0 { chunk.insert(UInt8(v & 0x7F) | 0x80, at: 0); v >>= 7 }
            body += chunk
        }
        return tlv(0x06, body)
    }

    static func oidString(_ content: [UInt8]) -> String {
        guard let first = content.first else { return "" }
        var parts = [Int(first) / 40, Int(first) % 40]
        var v = 0
        for b in content.dropFirst() {
            v = v << 7 | Int(b & 0x7F)
            if b & 0x80 == 0 { parts.append(v); v = 0 }
        }
        return parts.map(String.init).joined(separator: ".")
    }

    static func generalizedTime(_ date: Date) -> [UInt8] {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.timeZone = TimeZone(identifier: "UTC")
        f.dateFormat = "yyyyMMddHHmmss'Z'"
        return tlv(0x18, Array(f.string(from: date).utf8))
    }

    static func parseTime(_ node: Node) -> Date? {
        let s = String(decoding: node.content, as: UTF8.self)
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.timeZone = TimeZone(identifier: "UTC")
        f.dateFormat = node.tag == [0x17] ? "yyMMddHHmmss'Z'" : "yyyyMMddHHmmss'Z'"
        if let d = f.date(from: s) { return d }
        // Fractional seconds: drop them.
        if let dot = s.firstIndex(of: ".") { return f.date(from: String(s[..<dot]) + "Z") }
        return nil
    }

    static func parse(_ bytes: [UInt8]) -> Node? {
        var pos = 0
        return try? Timestamp.parse(bytes, &pos)
    }

    static func encode(_ n: Node) -> [UInt8] { Timestamp.encode(n) }

    /// Depth-first search for nodes with a tag.
    static func all(_ tag: [UInt8], in node: Node) -> [Node] {
        var found: [Node] = node.tag == tag ? [node] : []
        for c in node.children { found += all(tag, in: c) }
        return found
    }

    static func hex(_ b: [UInt8]) -> String { b.map { String(format: "%02X", $0) }.joined() }
}

enum OID {
    static let data = "1.2.840.113549.1.7.1"
    static let signedData = "1.2.840.113549.1.7.2"
    static let contentType = "1.2.840.113549.1.9.3"
    static let messageDigest = "1.2.840.113549.1.9.4"
    static let signingTime = "1.2.840.113549.1.9.5"
    static let signingCertificate = "1.2.840.113549.1.9.16.2.12"
    static let signingCertificateV2 = "1.2.840.113549.1.9.16.2.47"
    static let signatureTimeStamp = "1.2.840.113549.1.9.16.2.14"
    static let tstInfo = "1.2.840.113549.1.9.16.1.4"
    static let sha1 = "1.3.14.3.2.26"
    static let sha256 = "2.16.840.1.101.3.4.2.1"
    static let sha384 = "2.16.840.1.101.3.4.2.2"
    static let sha512 = "2.16.840.1.101.3.4.2.3"
    static let rsaEncryption = "1.2.840.113549.1.1.1"
    static let sha256WithRSA = "1.2.840.113549.1.1.11"
    static let ecPublicKey = "1.2.840.10045.2.1"
    static let ecdsaWithSHA256 = "1.2.840.10045.4.3.2"
    static let ocspBasic = "1.3.6.1.5.5.7.48.1.1"
    static let ocspNoCheck = "1.3.6.1.5.5.7.48.1.5"
    static let aia = "1.3.6.1.5.5.7.1.1"
    static let aiaOCSP = "1.3.6.1.5.5.7.48.1"
    static let aiaCAIssuers = "1.3.6.1.5.5.7.48.2"
    static let crlDistributionPoints = "2.5.29.31"
}

/// The parts of an X.509 certificate that PAdES needs.
struct X509: Hashable {
    let der: [UInt8]
    let serial: [UInt8]          // INTEGER content
    let issuer: [UInt8]          // Name, DER
    let subject: [UInt8]         // Name, DER
    let publicKey: [UInt8]       // subjectPublicKey BIT STRING content without the unused-bits byte
    let keyAlgorithm: String
    var notAfter: Date?
    var ocspURLs: [String] = []
    var caIssuerURLs: [String] = []
    var crlURLs: [String] = []
    var ocspNoCheck = false

    static func == (a: X509, b: X509) -> Bool { a.der == b.der }
    func hash(into h: inout Hasher) { h.combine(der) }

    var isSelfIssued: Bool { issuer == subject }
    var sha256: [UInt8] { [UInt8](SHA256.hash(data: Data(der))) }
    var name: String {
        SecCertificateCreateWithData(nil, Data(der) as CFData).flatMap { SecCertificateCopySubjectSummary($0) as String? } ?? "?"
    }

    init?(der: [UInt8]) {
        guard let cert = DER.parse(der), cert.children.count >= 3 else { return nil }
        let tbs = cert.children[0]
        var i = tbs.children.first?.tag == [0xA0] ? 1 : 0
        guard tbs.children.count >= i + 6 else { return nil }
        self.der = DER.encode(cert)
        serial = tbs.children[i].content; i += 1
        i += 1 // signature algorithm
        issuer = DER.encode(tbs.children[i]); i += 1
        let validity = tbs.children[i]; i += 1
        notAfter = validity.children.count == 2 ? DER.parseTime(validity.children[1]) : nil
        subject = DER.encode(tbs.children[i]); i += 1
        let spki = tbs.children[i]
        guard spki.children.count == 2 else { return nil }
        keyAlgorithm = spki.children[0].children.first.map { DER.oidString($0.content) } ?? ""
        publicKey = Array(spki.children[1].content.dropFirst())
        for ext in tbs.children.first(where: { $0.tag == [0xA3] })?.children.first?.children ?? [] {
            guard let oidNode = ext.children.first, let value = ext.children.last, value.tag == [0x04] else { continue }
            let id = DER.oidString(oidNode.content)
            if id == OID.ocspNoCheck { ocspNoCheck = true; continue }
            guard let inner = DER.parse(value.content) else { continue }
            if id == OID.aia {
                for desc in inner.children where desc.children.count == 2 {
                    let method = DER.oidString(desc.children[0].content)
                    guard desc.children[1].tag == [0x86] else { continue }
                    let url = String(decoding: desc.children[1].content, as: UTF8.self)
                    if method == OID.aiaOCSP { ocspURLs.append(url) }
                    if method == OID.aiaCAIssuers { caIssuerURLs.append(url) }
                }
            } else if id == OID.crlDistributionPoints {
                crlURLs += DER.all([0x86], in: inner).map { String(decoding: $0.content, as: UTF8.self) }
            }
        }
    }

    init?(_ cert: SecCertificate) { self.init(der: [UInt8](SecCertificateCopyData(cert) as Data)) }

    /// Certificates in a DER certificate or a PKCS#7 "certs-only" bundle (.p7c from caIssuers).
    static func certificates(in bytes: [UInt8]) -> [X509] {
        if let one = X509(der: bytes) { return [one] }
        if let pem = String(bytes: bytes, encoding: .ascii), pem.contains("-----BEGIN") {
            return pem.components(separatedBy: "-----BEGIN CERTIFICATE-----").dropFirst().compactMap {
                let b64 = $0.components(separatedBy: "-----END")[0].filter { !$0.isWhitespace }
                return Data(base64Encoded: b64).flatMap { X509(der: [UInt8]($0)) }
            }
        }
        guard let root = DER.parse(bytes) else { return [] }
        return CMSParts(root)?.certificates ?? []
    }
}

/// Selected parts of a CMS SignedData (ContentInfo).
struct CMSParts {
    var certificates: [X509] = []
    var signerInfo: DER.Node?
    var signedAttributes: [String: DER.Node] = [:]   // OID → attribute values SET
    var unsignedAttributes: [String: [DER.Node]] = [:]
    var signatureValue: [UInt8] = []
    var encapsulatedType = ""
    var encapsulatedContent: [UInt8]?
    var signerSerial: [UInt8]?
    var signerIssuer: [UInt8]?

    init?(_ root: DER.Node) {
        guard root.children.count >= 2, DER.oidString(root.children[0].content) == OID.signedData,
              let sd = root.children[1].children.first else { return nil }
        if let encap = sd.children.first(where: { $0.tag == [0x30] }) {
            encapsulatedType = encap.children.first.map { DER.oidString($0.content) } ?? ""
            if encap.children.count > 1, let octets = encap.children[1].children.first {
                encapsulatedContent = octets.constructed ? octets.children.flatMap(\.content) : octets.content
            }
        }
        if let certs = sd.children.first(where: { $0.tag == [0xA0] }) {
            certificates = certs.children.compactMap { X509(der: DER.encode($0)) }
        }
        guard let infos = sd.children.last(where: { $0.tag == [0x31] }), let si = infos.children.first else { return }
        signerInfo = si
        if si.children.count > 1, si.children[1].tag == [0x30], si.children[1].children.count == 2 {
            signerIssuer = DER.encode(si.children[1].children[0])
            signerSerial = si.children[1].children[1].content
        }
        for attr in si.children.first(where: { $0.tag == [0xA0] })?.children ?? [] where attr.children.count == 2 {
            signedAttributes[DER.oidString(attr.children[0].content)] = attr.children[1]
        }
        for attr in si.children.first(where: { $0.tag == [0xA1] })?.children ?? [] where attr.children.count == 2 {
            unsignedAttributes[DER.oidString(attr.children[0].content), default: []] += attr.children[1].children
        }
        signatureValue = si.children.last(where: { $0.tag == [0x04] })?.content ?? []
    }

    init?(bytes: [UInt8]) {
        guard let root = DER.parse(bytes) else { return nil }
        self.init(root)
    }

    var signerCertificate: X509? {
        certificates.first { $0.serial == signerSerial && $0.issuer == signerIssuer } ?? certificates.first
    }

    /// Timestamp tokens in the signature-time-stamp unsigned attribute.
    var timestampTokens: [CMSParts] {
        (unsignedAttributes[OID.signatureTimeStamp] ?? []).compactMap { CMSParts($0) }
    }

    /// genTime of a TimeStampToken (TSTInfo).
    var timestampTime: Date? {
        guard encapsulatedType == OID.tstInfo, let content = encapsulatedContent, let info = DER.parse(content) else { return nil }
        return info.children.first { $0.tag == [0x18] }.flatMap(DER.parseTime)
    }

    /// messageImprint of a TimeStampToken: (hash algorithm OID, digest).
    var timestampImprint: (String, [UInt8])? {
        guard encapsulatedType == OID.tstInfo, let content = encapsulatedContent, let info = DER.parse(content),
              info.children.count > 2, info.children[2].children.count == 2 else { return nil }
        let imprint = info.children[2]
        return (imprint.children[0].children.first.map { DER.oidString($0.content) } ?? "", imprint.children[1].content)
    }
}

/// Produces the signature value over the DER of the signed attributes.
protocol CMSRawSigner {
    /// Signing certificate first, then the rest of the chain (as far as known).
    var chain: [X509] { get }
    /// Returns the signature and the DER AlgorithmIdentifier of the signature algorithm.
    func sign(attributes: [UInt8]) throws -> (signature: [UInt8], algorithm: [UInt8])
}

/// A Keychain or .p12 identity.
struct KeychainSigner: CMSRawSigner {
    let identity: SecIdentity
    let chain: [X509]

    init(_ identity: SecIdentity) throws {
        self.identity = identity
        var cert: SecCertificate?
        try PDFSigner.check(SecIdentityCopyCertificate(identity, &cert), "certificate")
        guard let cert, let leaf = X509(cert) else { throw CertificateError.failed("certificate") }
        var chain = [leaf]
        var trust: SecTrust?
        if SecTrustCreateWithCertificates(cert, SecPolicyCreateBasicX509(), &trust) == errSecSuccess, let trust {
            _ = SecTrustEvaluateWithError(trust, nil)
            for c in (SecTrustCopyCertificateChain(trust) as? [SecCertificate] ?? []).dropFirst() {
                if let x = X509(c), !chain.contains(x) { chain.append(x) }
            }
        }
        // Complete the chain from the certificates' caIssuers URLs (a .p12 often holds only the signer).
        while let last = chain.last, !last.isSelfIssued, chain.count < 8 {
            let found = last.caIssuerURLs.lazy.compactMap { URL(string: $0) }
                .compactMap { try? Revocation.fetch($0, timeout: 10) }
                .compactMap { Revocation.issuer(of: last, in: X509.certificates(in: [UInt8]($0))) }.first
            guard let issuer = found, !chain.contains(issuer) else { break }
            chain.append(issuer)
        }
        self.chain = chain
    }

    func sign(attributes: [UInt8]) throws -> (signature: [UInt8], algorithm: [UInt8]) {
        var key: SecKey?
        try PDFSigner.check(SecIdentityCopyPrivateKey(identity, &key), L("signing key"))
        guard let key else { throw CertificateError.failed(L("signing key")) }
        let type = (SecKeyCopyAttributes(key) as? [String: Any])?[kSecAttrKeyType as String] as? String
        let isEC = type == (kSecAttrKeyTypeECSECPrimeRandom as String)
        let algorithm: SecKeyAlgorithm = isEC ? .ecdsaSignatureMessageX962SHA256 : .rsaSignatureMessagePKCS1v15SHA256
        var error: Unmanaged<CFError>?
        guard let sig = SecKeyCreateSignature(key, algorithm, Data(attributes) as CFData, &error) as Data? else {
            let message = (error?.takeRetainedValue() as Error?)?.localizedDescription ?? "?"
            throw CertificateError.failed(L("signature (access to the key denied?)") + " " + message)
        }
        return ([UInt8](sig), isEC ? DER.seq(DER.oid(OID.ecdsaWithSHA256)) : DER.seq(DER.oid(OID.sha256WithRSA), DER.null))
    }
}

/// CMS SignedData for PAdES baseline signatures (ETSI.CAdES.detached): content-type, message-digest and
/// signing-certificate-v2 signed attributes, no signing-time (the claimed time is the PDF /M entry).
enum CAdES {
    static let sha256AlgID = DER.seq(DER.oid(OID.sha256), DER.null)

    static func signedAttributes(digest: [UInt8], certificate: X509) -> [[UInt8]] {
        let essCertID = DER.seq(DER.octet(certificate.sha256),
                                DER.seq(DER.seq(DER.tlv(0xA4, certificate.issuer)), DER.tlv(0x02, certificate.serial)))
        return [
            DER.seq(DER.oid(OID.contentType), DER.set([DER.oid(OID.data)])),
            DER.seq(DER.oid(OID.messageDigest), DER.set([DER.octet(digest)])),
            DER.seq(DER.oid(OID.signingCertificateV2), DER.set([DER.seq(DER.seq(essCertID))])),
        ]
    }

    static func sign(_ content: Data, signer: CMSRawSigner) throws -> Data {
        guard let leaf = signer.chain.first else { throw CertificateError.failed("certificate") }
        let digest = [UInt8](SHA256.hash(data: content))
        let attributes = signedAttributes(digest: digest, certificate: leaf)
        let toBeSigned = DER.set(attributes)
        let (signature, algorithm) = try signer.sign(attributes: toBeSigned)
        let signerInfo = DER.seq([
            DER.int(1),
            DER.seq(leaf.issuer, DER.tlv(0x02, leaf.serial)),
            sha256AlgID,
            [0xA0] + Array(toBeSigned.dropFirst()),     // [0] IMPLICIT, same content as the signed SET
            algorithm,
            DER.octet(signature),
        ])
        let signedData = DER.seq([
            DER.int(1),
            DER.set([sha256AlgID]),
            DER.seq(DER.oid(OID.data)),
            DER.tlv(0xA0, signer.chain.flatMap(\.der)),
            DER.set([signerInfo]),
        ])
        return Data(DER.seq(DER.oid(OID.signedData), DER.tlv(0xA0, signedData)))
    }
}
