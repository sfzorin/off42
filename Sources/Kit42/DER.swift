import CryptoKit
import Foundation

/// Minimal DER/BER reader for certificates and CMS. It only reads structure for display and matching
/// (subject, serial, OIDs, signing time); no cryptography is performed here.
public struct DERNode {
    public let tag: UInt8
    /// Whole element (tag, length, value).
    public let raw: Data
    public let value: Data
    public let children: [DERNode]

    public var isConstructed: Bool { tag & 0x20 != 0 }
    /// Context-specific tag number ([0], [1], …) or nil.
    public var contextTag: Int? { tag & 0xC0 == 0x80 ? Int(tag & 0x1F) : nil }
    public subscript(i: Int) -> DERNode? { i < children.count ? children[i] : nil }

    /// Parses one element at the start of `data` (trailing bytes are ignored).
    public static func parse(_ data: Data) -> DERNode? {
        let bytes = [UInt8](data)
        var pos = 0
        return read(bytes, &pos, end: bytes.count, depth: 0)
    }

    private static func read(_ b: [UInt8], _ pos: inout Int, end: Int, depth: Int) -> DERNode? {
        guard depth < 40, pos + 2 <= end else { return nil }
        let start = pos
        let tag = b[pos]
        guard tag & 0x1F != 0x1F else { return nil } // high tag numbers are not used here
        pos += 1
        let first = b[pos]; pos += 1
        var children: [DERNode] = []
        if first == 0x80 {
            // Indefinite length (BER): children up to 00 00.
            guard tag & 0x20 != 0 else { return nil }
            let valueStart = pos
            while true {
                guard pos + 2 <= end else { return nil }
                if b[pos] == 0, b[pos + 1] == 0 { break }
                guard let child = read(b, &pos, end: end, depth: depth + 1) else { return nil }
                children.append(child)
            }
            let value = Data(b[valueStart..<pos])
            pos += 2
            return DERNode(tag: tag, raw: Data(b[start..<pos]), value: value, children: children)
        }
        var length = 0
        if first & 0x80 == 0 { length = Int(first) } else {
            let n = Int(first & 0x7F)
            guard n >= 1, n <= 4, pos + n <= end else { return nil }
            for _ in 0..<n { length = length << 8 | Int(b[pos]); pos += 1 }
        }
        guard length >= 0, pos + length <= end else { return nil }
        let valueStart = pos, valueEnd = pos + length
        // Constructed types, and OCTET/BIT STRINGs that happen to wrap DER (extensions), are parsed further.
        if tag & 0x20 != 0 {
            var p = valueStart
            while p < valueEnd {
                guard let child = read(b, &p, end: valueEnd, depth: depth + 1) else { return nil }
                children.append(child)
            }
        }
        pos = valueEnd
        return DERNode(tag: tag, raw: Data(b[start..<valueEnd]), value: Data(b[valueStart..<valueEnd]), children: children)
    }

    /// Dotted object identifier, if this is an OID.
    public var oid: String? {
        guard tag == 0x06, !value.isEmpty else { return nil }
        let b = [UInt8](value)
        var parts: [String] = []
        var n: UInt64 = 0
        for (i, byte) in b.enumerated() {
            n = n << 7 | UInt64(byte & 0x7F)
            if byte & 0x80 == 0 {
                if parts.isEmpty && i < b.count {
                    let a = min(n / 40, 2)
                    parts.append("\(a)"); parts.append("\(n - a * 40)")
                } else { parts.append("\(n)") }
                n = 0
            }
        }
        return parts.joined(separator: ".")
    }

    /// Text of string types (UTF8, Printable, IA5, Numeric, BMP, Teletex as Latin-1).
    public var string: String? {
        switch tag {
        case 0x0C, 0x13, 0x16, 0x12, 0x1A: return String(data: value, encoding: .utf8) ?? String(data: value, encoding: .isoLatin1)
        case 0x1E: return String(data: value, encoding: .utf16BigEndian)
        case 0x14: return String(data: value, encoding: .isoLatin1)
        default: return nil
        }
    }

    /// UTCTime / GeneralizedTime.
    public var date: Date? {
        guard tag == 0x17 || tag == 0x18, let s = String(data: value, encoding: .ascii) else { return nil }
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.timeZone = TimeZone(identifier: "UTC")
        var text = s.replacingOccurrences(of: "Z", with: "")
        if let dot = text.firstIndex(of: ".") { text = String(text[..<dot]) }
        f.dateFormat = tag == 0x17 ? (text.count == 10 ? "yyMMddHHmm" : "yyMMddHHmmss") : "yyyyMMddHHmmss"
        return f.date(from: text)
    }
}

/// X.509 certificate fields used for display and power-of-attorney matching, including the Russian
/// qualified-certificate attributes (SNILS, INN, OGRN).
public struct CertificateInfo {
    public var subject: [(oid: String, value: String)] = []
    public var issuer: [(oid: String, value: String)] = []
    public var serial = Data()
    public var notBefore: Date?
    public var notAfter: Date?
    public var publicKeyAlgorithm = ""
    public var signatureAlgorithm = ""
    /// SHA-1 of the DER certificate, uppercase hex (the CryptoPro "thumbprint").
    public var thumbprint = ""
    public var der = Data()

    public static let oidCN = "2.5.4.3", oidSurname = "2.5.4.4", oidGivenName = "2.5.4.42", oidOrganization = "2.5.4.10",
                      oidTitle = "2.5.4.12", oidEmail = "1.2.840.113549.1.9.1"
    public static let oidSNILS = "1.2.643.100.3", oidOGRN = "1.2.643.100.1", oidOGRNIP = "1.2.643.100.5",
                      oidINN = "1.2.643.3.131.1.1", oidINNLE = "1.2.643.100.4"

    public init?(der: Data) {
        guard let cert = DERNode.parse(der), let tbs = cert[0], tbs.tag == 0x30 else { return nil }
        self.der = cert.raw
        thumbprint = Insecure.SHA1.hash(data: cert.raw).map { String(format: "%02X", $0) }.joined()
        var i = 0
        if tbs[0]?.contextTag == 0 { i = 1 } // version
        guard let serialNode = tbs[i], serialNode.tag == 0x02 else { return nil }
        serial = serialNode.value
        signatureAlgorithm = tbs[i + 1]?[0]?.oid ?? ""
        issuer = Self.name(tbs[i + 2])
        notBefore = tbs[i + 3]?[0]?.date
        notAfter = tbs[i + 3]?[1]?.date
        subject = Self.name(tbs[i + 4])
        publicKeyAlgorithm = tbs[i + 5]?[0]?[0]?.oid ?? ""
    }

    static func name(_ node: DERNode?) -> [(oid: String, value: String)] {
        guard let node else { return [] }
        return node.children.flatMap { set in
            set.children.compactMap { atv -> (oid: String, value: String)? in
                guard let oid = atv[0]?.oid, let v = atv[1]?.string else { return nil }
                return (oid, v.trimmingCharacters(in: .whitespaces))
            }
        }
    }

    public func subjectValue(_ oid: String) -> String? { subject.first { $0.oid == oid }?.value }
    public var commonName: String { subjectValue(Self.oidCN) ?? subject.first?.value ?? "" }
    public var issuerName: String { issuer.first { $0.oid == Self.oidCN }?.value ?? issuer.first?.value ?? "" }
    public var snils: String? { subjectValue(Self.oidSNILS).map(Self.digits) }
    /// Personal INN (12 digits). Some CAs put a 10-digit organisation INN padded to 12 with "00".
    public var inn: String? { subjectValue(Self.oidINN).map(Self.digits) }
    public var innLE: String? { subjectValue(Self.oidINNLE).map(Self.digits) }
    public var ogrn: String? { (subjectValue(Self.oidOGRN) ?? subjectValue(Self.oidOGRNIP)).map(Self.digits) }
    public var organization: String? { subjectValue(Self.oidOrganization) }
    /// Surname + given names (the SN and G attributes), as in Russian qualified certificates.
    public var personName: String? {
        let parts = [subjectValue(Self.oidSurname), subjectValue(Self.oidGivenName)].compactMap { $0 }
        return parts.isEmpty ? nil : parts.joined(separator: " ")
    }

    public var isGOST: Bool { GOST.isGOST(publicKeyAlgorithm) || GOST.isGOST(signatureAlgorithm) }

    /// Name for display: person, organisation, SNILS / INN when present.
    public var displayName: String {
        var s = personName ?? commonName
        if let org = organization, !org.isEmpty, org != s, commonName != s || personName == nil { s += " · \(org)" }
        return s
    }

    static func digits(_ s: String) -> String { s.filter(\.isNumber) }
}

/// GOST algorithm identifiers (TC 26 and the older CryptoPro arc).
public enum GOST {
    public static func isGOST(_ oid: String) -> Bool {
        oid.hasPrefix("1.2.643.7.1.1.") || oid.hasPrefix("1.2.643.2.2.")
    }

    public static func algorithmName(_ oid: String) -> String {
        switch oid {
        case "1.2.643.7.1.1.1.1": return "ГОСТ Р 34.10-2012 256"
        case "1.2.643.7.1.1.1.2": return "ГОСТ Р 34.10-2012 512"
        case "1.2.643.7.1.1.2.2": return "ГОСТ Р 34.11-2012 256"
        case "1.2.643.7.1.1.2.3": return "ГОСТ Р 34.11-2012 512"
        case "1.2.643.7.1.1.3.2": return "ГОСТ Р 34.10-2012 256 / 34.11-2012"
        case "1.2.643.7.1.1.3.3": return "ГОСТ Р 34.10-2012 512 / 34.11-2012"
        case "1.2.643.2.2.19": return "ГОСТ Р 34.10-2001"
        case "1.2.643.2.2.9": return "ГОСТ Р 34.11-94"
        case "1.2.643.2.2.3": return "ГОСТ Р 34.10-2001 / 34.11-94"
        default: return oid
        }
    }
}

/// Structure of a CMS SignedData (PKCS #7) message: content, certificates and signer attributes.
public struct CMSInfo {
    public struct Signer {
        public var certificate: CertificateInfo?
        public var digestAlgorithm = ""
        public var signatureAlgorithm = ""
        public var signingTime: Date?
        /// Value of the messageDigest signed attribute.
        public var messageDigest: Data?
    }

    public var certificates: [CertificateInfo] = []
    public var signers: [Signer] = []
    /// Encapsulated content (attached signature), nil when detached.
    public var content: Data?

    public var isGOST: Bool {
        signers.contains { GOST.isGOST($0.digestAlgorithm) || GOST.isGOST($0.signatureAlgorithm) || $0.certificate?.isGOST == true }
    }

    static let signedData = "1.2.840.113549.1.7.2"

    /// Accepts DER/BER or Base64 (with or without PEM armour).
    public init?(_ input: Data) {
        let data = Self.decodeArmour(input)
        guard let root = DERNode.parse(data), root.tag == 0x30, root[0]?.oid == Self.signedData,
              let sd = root[1]?[0], sd.tag == 0x30 else { return nil }
        // SignedData: version, digestAlgorithms, encapContentInfo, [0] certificates, [1] crls, signerInfos
        if let encap = sd[2], let wrapped = encap[1], wrapped.contextTag == 0 {
            // eContent: OCTET STRING, possibly constructed (BER chunks).
            content = wrapped.children.map { node -> Data in
                node.children.isEmpty ? node.value : node.children.reduce(Data()) { $0 + $1.value }
            }.reduce(Data(), +)
        }
        for node in sd.children.dropFirst(3) {
            if node.contextTag == 0 {
                certificates = node.children.compactMap { $0.tag == 0x30 ? CertificateInfo(der: $0.raw) : nil }
            }
        }
        guard let infos = sd.children.last, infos.tag == 0x31 else { return nil }
        for si in infos.children {
            var s = Signer()
            // version, sid, digestAlgorithm, [0] signedAttrs, signatureAlgorithm, signature
            let sid = si[1]
            if let sid, sid.tag == 0x30, let serial = sid[1]?.value {
                s.certificate = certificates.first { $0.serial == serial }
            } else if let sid, sid.contextTag == 0 {
                let ski = sid.value
                s.certificate = certificates.first { $0.der.range(of: ski) != nil }
            }
            if s.certificate == nil, certificates.count == 1 { s.certificate = certificates[0] }
            s.digestAlgorithm = si[2]?[0]?.oid ?? ""
            var next = 3
            if let attrs = si[3], attrs.contextTag == 0 {
                next = 4
                for attr in attrs.children {
                    switch attr[0]?.oid {
                    case "1.2.840.113549.1.9.5": s.signingTime = attr[1]?[0]?.date
                    case "1.2.840.113549.1.9.4": s.messageDigest = attr[1]?[0]?.value
                    default: break
                    }
                }
            }
            s.signatureAlgorithm = si[next]?[0]?.oid ?? ""
            signers.append(s)
        }
    }

    /// Base64 / PEM to DER; binary input is returned unchanged.
    public static func decodeArmour(_ input: Data) -> Data {
        guard let first = input.first, first != 0x30 else { return input }
        guard var text = String(data: input, encoding: .ascii) else { return input }
        text = text.split(separator: "\n").filter { !$0.hasPrefix("-----") }.joined()
        text = text.filter { !$0.isWhitespace }
        return Data(base64Encoded: text) ?? input
    }
}
