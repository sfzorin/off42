import CryptoKit
import Foundation
import Security

// XML Signature (XMLDSig) building blocks shared by the OOXML and ODF document signatures:
// a small XML tree, Canonical XML 1.0 (inclusive and exclusive, without comments), certificate details,
// signing through a pluggable raw signer and signature-value verification.

/// A parsed XML element. Text keeps the parser's normalisation (line ends, attribute values), as C14N requires.
public final class XNode {
    public enum Child { case element(XNode), text(String), pi(String, String) }
    public let name: String
    public var attributes: [(String, String)]
    public var children: [Child] = []
    public weak var parent: XNode?

    init(name: String, attributes: [(String, String)]) {
        self.name = name
        self.attributes = attributes
    }

    public var prefix: String { name.contains(":") ? String(name.split(separator: ":")[0]) : "" }
    public var localName: String { name.contains(":") ? String(name.split(separator: ":", maxSplits: 1)[1]) : name }
    public func attr(_ name: String) -> String? { attributes.first { $0.0 == name }?.1 }

    public var elements: [XNode] { children.compactMap { if case .element(let e) = $0 { return e } else { return nil } } }
    public func first(_ local: String) -> XNode? { elements.first { $0.localName == local } }
    public func all(_ local: String) -> [XNode] { elements.filter { $0.localName == local } }
    public var text: String {
        children.map {
            switch $0 {
            case .text(let s): return s
            case .element(let e): return e.text
            case .pi: return ""
            }
        }.joined()
    }

    /// Depth-first search, self included.
    public func descendants(_ match: (XNode) -> Bool) -> [XNode] {
        var out: [XNode] = match(self) ? [self] : []
        for e in elements { out += e.descendants(match) }
        return out
    }

    public func byID(_ id: String) -> XNode? {
        descendants { $0.attr("Id") == id || $0.attr("id") == id || $0.attr("ID") == id }.first
    }

    /// Namespace declarations in scope (prefix → URI; "" is the default namespace).
    public var namespacesInScope: [String: String] {
        var map = parent?.namespacesInScope ?? ["xml": "http://www.w3.org/XML/1998/namespace"]
        for (k, v) in attributes {
            if k == "xmlns" { map[""] = v } else if k.hasPrefix("xmlns:") { map[String(k.dropFirst(6))] = v }
        }
        return map
    }

    public var namespaceURI: String { namespacesInScope[prefix] ?? "" }

    /// Parses a document; returns its root element.
    public static func parse(_ data: Data) throws -> XNode {
        let builder = Builder()
        let parser = XMLParser(data: data)
        parser.shouldProcessNamespaces = false
        parser.shouldResolveExternalEntities = false
        parser.delegate = builder
        guard parser.parse(), let root = builder.root else {
            throw XMLDSigError.malformed(parser.parserError?.localizedDescription ?? "XML")
        }
        return root
    }

    private final class Builder: NSObject, XMLParserDelegate {
        var root: XNode?
        var stack: [XNode] = []
        func parser(_ parser: XMLParser, didStartElement elementName: String, namespaceURI: String?, qualifiedName: String?,
                    attributes: [String: String]) {
            let node = XNode(name: elementName, attributes: attributes.map { ($0.key, $0.value) })
            if let top = stack.last { node.parent = top; top.children.append(.element(node)) } else { root = node }
            stack.append(node)
        }
        func parser(_ parser: XMLParser, didEndElement elementName: String, namespaceURI: String?, qualifiedName: String?) {
            stack.removeLast()
        }
        func parser(_ parser: XMLParser, foundCharacters string: String) {
            guard let top = stack.last else { return }
            if case .text(let s)? = top.children.last { top.children[top.children.count - 1] = .text(s + string) } else { top.children.append(.text(string)) }
        }
        func parser(_ parser: XMLParser, foundProcessingInstructionWithTarget target: String, data: String?) {
            stack.last?.children.append(.pi(target, data ?? ""))
        }
    }
}

public enum XMLDSigError: LocalizedError {
    case malformed(String)
    case unsupported(String)
    case signing(String)

    public var errorDescription: String? {
        switch self {
        case .malformed(let s): return L("The signature data is damaged: %@", s)
        case .unsupported(let s): return L("Unsupported signature feature: %@", s)
        case .signing(let s): return s
        }
    }
}

/// Canonical XML 1.0 (http://www.w3.org/TR/2001/REC-xml-c14n-20010315) and Exclusive XML Canonicalization 1.0
/// for an element subtree (a document subset whose apex is `node`), comments omitted.
public enum C14N {
    public static let inclusive = "http://www.w3.org/TR/2001/REC-xml-c14n-20010315"
    public static let inclusiveWithComments = "http://www.w3.org/TR/2001/REC-xml-c14n-20010315#WithComments"
    public static let exclusive = "http://www.w3.org/2001/10/xml-exc-c14n#"
    public static let c14n11 = "http://www.w3.org/2006/12/xml-c14n11"

    public static func isSupported(_ algorithm: String) -> Bool {
        [inclusive, inclusiveWithComments, exclusive, c14n11].contains(algorithm)
    }

    public static func canonicalize(_ node: XNode, algorithm: String = inclusive) -> Data {
        var out = ""
        let exclusiveMode = algorithm == exclusive
        // Namespaces rendered by the nearest output ancestor: none for the apex.
        write(node, rendered: [:], apex: true, exclusive: exclusiveMode, into: &out)
        return Data(out.utf8)
    }

    /// The whole document: the root element (processing instructions around it are rare in Office files and dropped).
    public static func canonicalizeDocument(_ root: XNode) -> Data { canonicalize(root) }

    private static func write(_ e: XNode, rendered: [String: String], apex: Bool, exclusive: Bool, into out: inout String) {
        let scope = e.namespacesInScope.filter { $0.key != "xml" }
        var decls: [(String, String)] = []
        if exclusive {
            var used = Set([e.prefix])
            for (k, _) in e.attributes where !k.hasPrefix("xmlns") && k.contains(":") { used.insert(String(k.split(separator: ":")[0])) }
            for p in used where p != "xml" {
                let uri = scope[p] ?? ""
                if rendered[p] != uri, !(p.isEmpty && uri.isEmpty && rendered[p] == nil) { decls.append((p, uri)) }
            }
        } else {
            for (p, uri) in scope where rendered[p] != uri {
                if p.isEmpty, uri.isEmpty, (rendered[""] ?? "").isEmpty { continue }
                decls.append((p, uri))
            }
            if scope[""] == nil, let d = rendered[""], !d.isEmpty { decls.append(("", "")) }
        }
        decls.sort { $0.0 < $1.0 }
        var next = rendered
        for (p, uri) in decls { next[p] = uri }

        var attrs: [(uri: String, local: String, name: String, value: String)] = e.attributes.compactMap { k, v in
            if k == "xmlns" || k.hasPrefix("xmlns:") { return nil }
            if k.contains(":") {
                let parts = k.split(separator: ":", maxSplits: 1).map(String.init)
                return (scope[parts[0]] ?? (parts[0] == "xml" ? "http://www.w3.org/XML/1998/namespace" : ""), parts[1], k, v)
            }
            return ("", k, k, v)
        }
        // Inclusive C14N carries xml:* attributes of ancestors onto the apex.
        if apex, !exclusive {
            var a = e.parent
            while let p = a {
                for (k, v) in p.attributes where k.hasPrefix("xml:") && !attrs.contains(where: { $0.name == k }) {
                    attrs.append(("http://www.w3.org/XML/1998/namespace", String(k.dropFirst(4)), k, v))
                }
                a = p.parent
            }
        }
        attrs.sort { $0.uri != $1.uri ? $0.uri < $1.uri : $0.local < $1.local }

        out += "<" + e.name
        for (p, uri) in decls { out += (p.isEmpty ? " xmlns" : " xmlns:" + p) + "=\"" + escapeAttr(uri) + "\"" }
        for a in attrs { out += " " + a.name + "=\"" + escapeAttr(a.value) + "\"" }
        out += ">"
        for child in e.children {
            switch child {
            case .text(let s): out += escapeText(s)
            case .element(let c): write(c, rendered: next, apex: false, exclusive: exclusive, into: &out)
            case .pi(let t, let d): out += "<?" + t + (d.isEmpty ? "" : " " + d) + "?>"
            }
        }
        out += "</" + e.name + ">"
    }

    static func escapeText(_ s: String) -> String {
        var r = ""
        r.reserveCapacity(s.count)
        for ch in s.unicodeScalars {
            switch ch {
            case "&": r += "&amp;"
            case "<": r += "&lt;"
            case ">": r += "&gt;"
            case "\r": r += "&#xD;"
            default: r.unicodeScalars.append(ch)
            }
        }
        return r
    }

    static func escapeAttr(_ s: String) -> String {
        var r = ""
        for ch in s.unicodeScalars {
            switch ch {
            case "&": r += "&amp;"
            case "<": r += "&lt;"
            case "\"": r += "&quot;"
            case "\t": r += "&#x9;"
            case "\n": r += "&#xA;"
            case "\r": r += "&#xD;"
            default: r.unicodeScalars.append(ch)
            }
        }
        return r
    }
}

// MARK: - Digests and signature algorithms

public enum XMLDigest {
    public static let sha1 = "http://www.w3.org/2000/09/xmldsig#sha1"
    public static let sha256 = "http://www.w3.org/2001/04/xmlenc#sha256"
    public static let sha384 = "http://www.w3.org/2001/04/xmldsig-more#sha384"
    public static let sha512 = "http://www.w3.org/2001/04/xmlenc#sha512"

    public static func digest(_ data: Data, _ algorithm: String) throws -> Data {
        switch algorithm {
        case sha1: return Data(Insecure.SHA1.hash(data: data))
        case sha256: return Data(SHA256.hash(data: data))
        case sha384: return Data(SHA384.hash(data: data))
        case sha512: return Data(SHA512.hash(data: data))
        default: throw XMLDSigError.unsupported(algorithm)
        }
    }
}

public enum XMLSignatureMethod {
    public static let rsaSHA1 = "http://www.w3.org/2000/09/xmldsig#rsa-sha1"
    public static let rsaSHA256 = "http://www.w3.org/2001/04/xmldsig-more#rsa-sha256"
    public static let rsaSHA384 = "http://www.w3.org/2001/04/xmldsig-more#rsa-sha384"
    public static let rsaSHA512 = "http://www.w3.org/2001/04/xmldsig-more#rsa-sha512"
    public static let ecdsaSHA1 = "http://www.w3.org/2001/04/xmldsig-more#ecdsa-sha1"
    public static let ecdsaSHA256 = "http://www.w3.org/2001/04/xmldsig-more#ecdsa-sha256"
    public static let ecdsaSHA384 = "http://www.w3.org/2001/04/xmldsig-more#ecdsa-sha384"
    public static let ecdsaSHA512 = "http://www.w3.org/2001/04/xmldsig-more#ecdsa-sha512"

    static func secAlgorithm(_ method: String) -> (SecKeyAlgorithm, ec: Bool)? {
        switch method {
        case rsaSHA1: return (.rsaSignatureMessagePKCS1v15SHA1, false)
        case rsaSHA256: return (.rsaSignatureMessagePKCS1v15SHA256, false)
        case rsaSHA384: return (.rsaSignatureMessagePKCS1v15SHA384, false)
        case rsaSHA512: return (.rsaSignatureMessagePKCS1v15SHA512, false)
        case ecdsaSHA1: return (.ecdsaSignatureMessageX962SHA1, true)
        case ecdsaSHA256: return (.ecdsaSignatureMessageX962SHA256, true)
        case ecdsaSHA384: return (.ecdsaSignatureMessageX962SHA384, true)
        case ecdsaSHA512: return (.ecdsaSignatureMessageX962SHA512, true)
        default: return nil
        }
    }

    /// Checks a SignatureValue over `data` with the certificate's public key. XMLDSig ECDSA values are r‖s.
    public static func verify(_ signature: Data, over data: Data, method: String, certificate: SecCertificate) -> Bool {
        guard let (algorithm, ec) = secAlgorithm(method), let key = SecCertificateCopyKey(certificate) else { return false }
        let value = ec ? ECDSAValue.der(fromRaw: signature) : signature
        return SecKeyVerifySignature(key, algorithm, data as CFData, value as CFData, nil)
    }
}

/// Conversion between the X9.62 DER encoding of an ECDSA signature and the fixed r‖s form used by XMLDSig.
public enum ECDSAValue {
    public static func raw(fromDER der: Data, keyBytes: Int) -> Data {
        guard let seq = ASN1.parse([UInt8](der)), seq.children.count == 2 else { return der }
        func fit(_ b: [UInt8]) -> [UInt8] {
            let t = Array(b.drop { $0 == 0 })
            return [UInt8](repeating: 0, count: max(0, keyBytes - t.count)) + t.suffix(keyBytes)
        }
        return Data(fit(seq.children[0].content) + fit(seq.children[1].content))
    }

    public static func der(fromRaw raw: Data) -> Data {
        let b = [UInt8](raw)
        let half = b.count / 2
        func int(_ x: ArraySlice<UInt8>) -> [UInt8] {
            var v = Array(x.drop { $0 == 0 })
            if v.isEmpty { v = [0] }
            if v[0] & 0x80 != 0 { v.insert(0, at: 0) }
            return ASN1.tlv(0x02, v)
        }
        return Data(ASN1.tlv(0x30, int(b[..<half]) + int(b[half...])))
    }
}

// MARK: - Minimal ASN.1 reader for certificate fields

/// A DER reader for the few certificate fields XAdES needs (issuer DN, serial, policies).
public enum ASN1 {
    public struct Node {
        public let tag: UInt8
        public let content: [UInt8]
        public let encoded: [UInt8]
        public var children: [Node] { tag & 0x20 != 0 ? ASN1.sequence(content) : [] }
    }

    public static func parse(_ b: [UInt8]) -> Node? {
        guard let (node, used) = read(b[...]), used == b.count else { return nil }
        return node
    }

    static func read(_ b: ArraySlice<UInt8>) -> (Node, Int)? {
        let s = b.startIndex
        guard b.count >= 2 else { return nil }
        let tag = b[s]
        var len = Int(b[s + 1]), head = 2
        if len & 0x80 != 0 {
            let n = len & 0x7F
            guard n > 0, n <= 4, b.count >= 2 + n else { return nil }
            len = 0
            for i in 0..<n { len = len << 8 | Int(b[s + 2 + i]) }
            head += n
        }
        guard b.count >= head + len else { return nil }
        return (Node(tag: tag, content: Array(b[(s + head)..<(s + head + len)]), encoded: Array(b[s..<(s + head + len)])), head + len)
    }

    static func sequence(_ b: [UInt8]) -> [Node] {
        var out: [Node] = []
        var rest = b[...]
        while !rest.isEmpty, let (n, used) = read(rest) {
            out.append(n)
            rest = rest.dropFirst(used)
        }
        return out
    }

    public static func tlv(_ tag: UInt8, _ body: [UInt8]) -> [UInt8] {
        let n = body.count
        if n < 0x80 { return [tag, UInt8(n)] + body }
        var len: [UInt8] = []
        var v = n
        while v > 0 { len.insert(UInt8(v & 0xFF), at: 0); v >>= 8 }
        return [tag, 0x80 | UInt8(len.count)] + len + body
    }

    public static func oid(_ content: [UInt8]) -> String {
        guard let first = content.first else { return "" }
        var parts = [Int(first) / 40, Int(first) % 40]
        var v = 0
        for byte in content.dropFirst() {
            v = v << 7 | Int(byte & 0x7F)
            if byte & 0x80 == 0 { parts.append(v); v = 0 }
        }
        return parts.map(String.init).joined(separator: ".")
    }

    /// Big-endian unsigned integer as decimal digits.
    public static func decimal(_ bytes: [UInt8]) -> String {
        var digits = [UInt8](bytes.drop { $0 == 0 })
        if digits.isEmpty { return "0" }
        var out: [Character] = []
        while !digits.isEmpty {
            var rem = 0
            var next: [UInt8] = []
            for d in digits {
                let cur = rem * 256 + Int(d)
                let q = cur / 10
                rem = cur % 10
                if !next.isEmpty || q > 0 { next.append(UInt8(q)) }
            }
            out.append(Character(String(rem)))
            digits = next
        }
        return String(out.reversed())
    }

    /// Decimal digits as a big-endian unsigned integer.
    public static func bytes(decimal: String) -> [UInt8] {
        var out: [UInt8] = [0]
        for ch in decimal where ch.isNumber {
            var carry = Int(String(ch)) ?? 0
            for i in stride(from: out.count - 1, through: 0, by: -1) {
                let v = Int(out[i]) * 10 + carry
                out[i] = UInt8(v & 0xFF)
                carry = v >> 8
            }
            while carry > 0 { out.insert(UInt8(carry & 0xFF), at: 0); carry >>= 8 }
        }
        return Array(out.drop { $0 == 0 })
    }
}

/// Certificate fields used by XAdES properties and the signature reports.
public struct CertificateDetails {
    public let der: Data
    public let serial: [UInt8]
    public let issuerDER: [UInt8]
    public let subjectDER: [UInt8]
    public let notBefore: Date?
    public let notAfter: Date?
    public var policies: [String] = []
    public var qcStatements: [String] = []
    public var extensions: [String: [UInt8]] = [:]

    public init?(der: Data) {
        guard let cert = ASN1.parse([UInt8](der)), let tbs = cert.children.first else { return nil }
        let f = tbs.children
        var i = f.first?.tag == 0xA0 ? 1 : 0
        guard f.count >= i + 6 else { return nil }
        self.der = der
        serial = f[i].content; i += 2
        issuerDER = f[i].encoded; i += 1
        let validity = f[i].children; i += 1
        notBefore = validity.first.flatMap(CertificateDetails.time)
        notAfter = validity.count > 1 ? CertificateDetails.time(validity[1]) : nil
        subjectDER = f[i].encoded
        for ext in f.first(where: { $0.tag == 0xA3 })?.children.first?.children ?? [] {
            guard let id = ext.children.first, let value = ext.children.last, value.tag == 0x04 else { continue }
            let oid = ASN1.oid(id.content)
            extensions[oid] = value.content
            if oid == "2.5.29.32", let seq = ASN1.parse(value.content) {
                policies = seq.children.compactMap { $0.children.first.map { ASN1.oid($0.content) } }
            }
            if oid == "1.3.6.1.5.5.7.1.3", let seq = ASN1.parse(value.content) {
                qcStatements = seq.children.compactMap { $0.children.first.map { ASN1.oid($0.content) } }
            }
        }
    }

    public var serialDecimal: String { ASN1.decimal(serial) }
    public var issuerName: String { CertificateDetails.rfc2253(issuerDER) }
    public var subjectName: String { CertificateDetails.rfc2253(subjectDER) }
    public var subjectAttributes: [(String, String)] { CertificateDetails.attributes(subjectDER) }
    public var issuerAttributes: [(String, String)] { CertificateDetails.attributes(issuerDER) }
    public var sha1: Data { Data(Insecure.SHA1.hash(data: der)) }
    public var sha256: Data { Data(SHA256.hash(data: der)) }

    static func time(_ n: ASN1.Node) -> Date? {
        let s = String(decoding: n.content, as: UTF8.self)
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.timeZone = TimeZone(identifier: "UTC")
        f.dateFormat = n.tag == 0x17 ? "yyMMddHHmmss'Z'" : "yyyyMMddHHmmss'Z'"
        return f.date(from: s)
    }

    static let shortNames: [String: String] = [
        "2.5.4.3": "CN", "2.5.4.6": "C", "2.5.4.7": "L", "2.5.4.8": "ST", "2.5.4.10": "O", "2.5.4.11": "OU",
        "2.5.4.9": "STREET", "0.9.2342.19200300.100.1.25": "DC", "0.9.2342.19200300.100.1.1": "UID",
        "2.5.4.5": "SERIALNUMBER", "1.2.840.113549.1.9.1": "E", "2.5.4.4": "SN", "2.5.4.42": "G", "2.5.4.12": "T",
        "2.5.4.97": "2.5.4.97",
    ]

    /// (attribute type, value) in certificate order.
    static func attributes(_ name: [UInt8]) -> [(String, String)] {
        guard let seq = ASN1.parse(name) else { return [] }
        return seq.children.flatMap { rdn in
            rdn.children.compactMap { atv -> (String, String)? in
                guard atv.children.count == 2 else { return nil }
                let oid = ASN1.oid(atv.children[0].content)
                let v = atv.children[1]
                let value: String
                switch v.tag {
                case 0x1E: // BMPString
                    value = String(decoding: stride(from: 0, to: v.content.count - 1, by: 2).map { UInt16(v.content[$0]) << 8 | UInt16(v.content[$0 + 1]) }, as: UTF16.self)
                default: value = String(decoding: v.content, as: UTF8.self)
                }
                return (shortNames[oid] ?? oid, value)
            }
        }
    }

    /// RFC 2253 string (last RDN first), as Office writes in X509IssuerName.
    public static func rfc2253(_ name: [UInt8]) -> String {
        attributes(name).reversed().map { k, v in
            var e = ""
            for (i, ch) in v.enumerated() {
                if ",+\"\\<>;".contains(ch) || (i == 0 && (ch == " " || ch == "#")) || (i == v.count - 1 && ch == " ") { e += "\\" }
                e.append(ch)
            }
            return "\(k)=\(e)"
        }.joined(separator: ", ")
    }

    public var commonName: String {
        subjectAttributes.first { $0.0 == "CN" }?.1 ?? subjectName
    }
}

// MARK: - Signers

/// Produces XMLDSig signature values. Implemented by Keychain/.p12 identities and by PKCS#11 tokens.
public protocol XMLRawSigner {
    /// Signing certificate first, then intermediates (if known).
    var certificates: [Data] { get }
    /// True for EC keys (XMLDSig ECDSA), false for RSA.
    var isEC: Bool { get }
    /// Signs `data` with SHA-256. RSA: PKCS#1 v1.5. EC: r‖s.
    func signSHA256(_ data: Data) throws -> Data
}

/// A Keychain or .p12 identity.
public struct IdentitySigner: XMLRawSigner {
    public let identity: SecIdentity
    public let certificates: [Data]
    public let isEC: Bool
    private let key: SecKey

    public init(_ identity: SecIdentity) throws {
        self.identity = identity
        var cert: SecCertificate?
        var key: SecKey?
        guard SecIdentityCopyCertificate(identity, &cert) == errSecSuccess, let cert,
              SecIdentityCopyPrivateKey(identity, &key) == errSecSuccess, let key else {
            throw XMLDSigError.signing(L("The certificate has no usable private key"))
        }
        self.key = key
        let type = (SecKeyCopyAttributes(key) as? [String: Any])?[kSecAttrKeyType as String] as? String
        isEC = type == (kSecAttrKeyTypeECSECPrimeRandom as String)
        var chain = [SecCertificateCopyData(cert) as Data]
        var trust: SecTrust?
        if SecTrustCreateWithCertificates(cert, SecPolicyCreateBasicX509(), &trust) == errSecSuccess, let trust {
            _ = SecTrustEvaluateWithError(trust, nil)
            for c in (SecTrustCopyCertificateChain(trust) as? [SecCertificate] ?? []).dropFirst() {
                let d = SecCertificateCopyData(c) as Data
                if !chain.contains(d) { chain.append(d) }
            }
        }
        certificates = chain
    }

    public func signSHA256(_ data: Data) throws -> Data {
        var error: Unmanaged<CFError>?
        let algorithm: SecKeyAlgorithm = isEC ? .ecdsaSignatureMessageX962SHA256 : .rsaSignatureMessagePKCS1v15SHA256
        guard let sig = SecKeyCreateSignature(key, algorithm, data as CFData, &error) as Data? else {
            let message = (error?.takeRetainedValue() as Error?)?.localizedDescription ?? "?"
            throw XMLDSigError.signing(L("Signing failed (access to the key denied?)") + " " + message)
        }
        guard isEC else { return sig }
        let bits = (SecKeyCopyAttributes(key) as? [String: Any])?[kSecAttrKeySizeInBits as String] as? Int ?? 256
        return ECDSAValue.raw(fromDER: sig, keyBytes: (bits + 7) / 8)
    }
}

extension XMLRawSigner {
    var signatureMethod: String { isEC ? XMLSignatureMethod.ecdsaSHA256 : XMLSignatureMethod.rsaSHA256 }
    var leaf: CertificateDetails? { certificates.first.flatMap { CertificateDetails(der: $0) } }
}

// MARK: - Verification result shared by document signatures

/// One signature found in an Office or OpenDocument file.
public struct DocumentSignature {
    public enum Status { case valid, untrusted, invalid }
    public var status: Status
    public var signer: String
    public var email: String?
    public var date: Date?
    public var certificate: Data?
    /// Integrity problem (digest/signature mismatch), if any.
    public var problem: String?
    /// Certificate trust message when status is .untrusted.
    public var trustMessage: String?
    /// Package parts the signature does not cover (added or changed outside the signed set).
    public var unsignedParts: [String] = []
    public var comment: String?
    public var format: String

    public var summary: String {
        switch status {
        case .valid: return unsignedParts.isEmpty ? L("Signature is valid") : L("Signature is valid, but some parts are not signed")
        case .untrusted: return L("Signature is intact; the certificate is not trusted")
        case .invalid: return L("Signature is invalid: %@", problem ?? L("the document was changed after signing"))
        }
    }
}

enum CertificateTrust {
    /// nil when the chain is trusted, else the system's message.
    static func evaluate(_ leaf: SecCertificate, extra: [SecCertificate], at date: Date?) -> String? {
        var trust: SecTrust?
        guard SecTrustCreateWithCertificates(([leaf] + extra) as CFArray, SecPolicyCreateBasicX509(), &trust) == errSecSuccess,
              let trust else { return L("The certificate cannot be checked") }
        if let date { SecTrustSetVerifyDate(trust, date as CFDate) }
        var error: CFError?
        if SecTrustEvaluateWithError(trust, &error) { return nil }
        return (error as Error?)?.localizedDescription ?? L("The certificate is not trusted")
    }

    static func email(_ cert: SecCertificate) -> String? {
        var emails: CFArray?
        SecCertificateCopyEmailAddresses(cert, &emails)
        return (emails as? [String])?.first
    }
}

/// ISO 8601 in UTC with seconds, as Office and LibreOffice write signing times.
enum XMLTime {
    static func string(_ date: Date) -> String {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime]
        return f.string(from: date)
    }

    static func parse(_ s: String) -> Date? {
        let t = s.trimmingCharacters(in: .whitespacesAndNewlines)
        let f = ISO8601DateFormatter()
        for o: ISO8601DateFormatter.Options in [[.withInternetDateTime], [.withInternetDateTime, .withFractionalSeconds]] {
            f.formatOptions = o
            if let d = f.date(from: t) { return d }
        }
        // LibreOffice: 2024-01-02T10:11:12.123456789 (no zone) — read as UTC.
        let g = DateFormatter()
        g.locale = Locale(identifier: "en_US_POSIX")
        g.timeZone = TimeZone(identifier: "UTC")
        g.dateFormat = "yyyy-MM-dd'T'HH:mm:ss"
        if let d = g.date(from: String(t.prefix(19))) { return d }
        return nil
    }
}
