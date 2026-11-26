import CryptoKit
import Foundation
import LocalAuthentication
import Security

/// Signing identity (certificate + private key) from the user's Keychain.
public struct SigningIdentity {
    public let identity: SecIdentity
    public let name: String
    public let email: String?

    init?(_ identity: SecIdentity) {
        var cert: SecCertificate?
        guard SecIdentityCopyCertificate(identity, &cert) == errSecSuccess, let cert else { return nil }
        self.identity = identity
        name = SecCertificateCopySubjectSummary(cert) as String? ?? L("Unnamed")
        var emails: CFArray?
        SecCertificateCopyEmailAddresses(cert, &emails)
        email = (emails as? [String])?.first
    }

    public var displayName: String { email.map { "\(name) <\($0)>" } ?? name }
}

public enum CertificateStore {
    public static func identities() -> [SigningIdentity] {
        let query: [String: Any] = [
            kSecClass as String: kSecClassIdentity,
            kSecMatchLimit as String: kSecMatchLimitAll,
            kSecReturnRef as String: true,
        ]
        var result: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess,
              let array = result as? [SecIdentity] else { return [] }
        var seen = Set<String>()
        return array.compactMap(SigningIdentity.init).filter { seen.insert($0.displayName).inserted }
    }

    /// Imports a .p12/.pfx. With `toKeychain` the identity is also added to the login Keychain.
    public static func importPKCS12(_ data: Data, password: String, toKeychain: Bool) throws -> SigningIdentity {
        var options: [String: Any] = [kSecImportExportPassphrase as String: password]
        if !toKeychain, #available(macOS 15.0, *) { options[kSecImportToMemoryOnly as String] = true }
        var items: CFArray?
        let status = SecPKCS12Import(data as CFData, options as CFDictionary, &items)
        guard status == errSecSuccess else { throw error(status, L("Could not open the certificate file (wrong password?)")) }
        guard let dict = (items as? [[String: Any]])?.first,
              let ref = dict[kSecImportItemIdentity as String] else { throw error(errSecItemNotFound, L("The file contains no certificate with a private key")) }
        let identity = ref as! SecIdentity
        if toKeychain {
            let add = SecItemAdd([kSecValueRef as String: identity] as CFDictionary, nil)
            guard add == errSecSuccess || add == errSecDuplicateItem else { throw error(add, L("Could not add the certificate to the Keychain")) }
        }
        guard let signing = SigningIdentity(identity) else { throw error(errSecItemNotFound, L("The certificate cannot be read")) }
        return signing
    }

    /// Creates a self-signed signing certificate with the system `openssl` and adds it to the Keychain.
    public static func createSelfSigned(name: String, email: String, organization: String, years: Int = 5) throws -> SigningIdentity {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }

        func clean(_ s: String) -> String { s.replacingOccurrences(of: "/", with: " ").replacingOccurrences(of: "=", with: " ") }
        var subject = "/CN=\(clean(name))"
        if !organization.isEmpty { subject += "/O=\(clean(organization))" }
        if !email.isEmpty { subject += "/emailAddress=\(clean(email))" }
        let password = UUID().uuidString

        try run(["req", "-x509", "-newkey", "rsa:2048", "-sha256", "-days", "\(years * 365)", "-nodes", "-utf8",
                 "-keyout", "key.pem", "-out", "cert.pem", "-subj", subject,
                 "-addext", "keyUsage=critical,digitalSignature,nonRepudiation"], in: dir)
        try run(["pkcs12", "-export", "-inkey", "key.pem", "-in", "cert.pem", "-out", "id.p12",
                 "-passout", "pass:\(password)", "-name", clean(name)], in: dir)
        let data = try Data(contentsOf: dir.appendingPathComponent("id.p12"))
        return try importPKCS12(data, password: password, toKeychain: true)
    }

    private static func run(_ args: [String], in dir: URL) throws {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/usr/bin/openssl")
        p.arguments = args
        p.currentDirectoryURL = dir
        let err = Pipe()
        p.standardError = err
        p.standardOutput = Pipe()
        try p.run()
        p.waitUntilExit()
        guard p.terminationStatus == 0 else {
            let msg = String(data: err.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
            throw NSError(domain: "Suite42", code: Int(p.terminationStatus),
                          userInfo: [NSLocalizedDescriptionKey: "openssl: \(msg.trimmingCharacters(in: .whitespacesAndNewlines))"])
        }
    }

    static func plain(_ text: String) -> NSError {
        NSError(domain: "Suite42.Certificates", code: 1, userInfo: [NSLocalizedDescriptionKey: text])
    }

    static func error(_ status: OSStatus, _ text: String) -> NSError {
        let detail = SecCopyErrorMessageString(status, nil) as String? ?? "\(status)"
        return NSError(domain: NSOSStatusErrorDomain, code: Int(status),
                       userInfo: [NSLocalizedDescriptionKey: "\(text): \(detail)"])
    }
}

// MARK: - Signing keys and certificate requests ("Get a Certificate…")

/// Where a signing key lives.
public enum KeyStorage: String, Codable {
    /// Non-exportable P-256 key wrapped by this Mac's Secure Enclave; only the wrapped blob is stored on disk.
    case secureEnclave
    /// Key pair in the login Keychain; an issued certificate imported into the Keychain forms a normal identity.
    case keychain
}

public enum KeyAlgorithm: String, Codable, CaseIterable {
    case p256, rsa2048, rsa3072

    public var title: String {
        switch self {
        case .p256: return "ECDSA P-256"
        case .rsa2048: return "RSA 2048"
        case .rsa3072: return "RSA 3072"
        }
    }

    var isEC: Bool { self == .p256 }
}

/// Subject of a certificate request. Empty fields are left out.
public struct CertificateSubject: Codable, Equatable {
    public var commonName = ""
    public var givenName = ""
    public var surname = ""
    public var email = ""
    public var organization = ""
    public var organizationalUnit = ""
    public var locality = ""
    public var state = ""
    /// ISO 3166 two-letter code.
    public var country = ""
    /// Personal or company identifier in the subject `serialNumber` (e.g. eIDAS "PNOTR-12345678901").
    public var serialNumber = ""

    public init() {}
}

/// A signing key created by "Get a Certificate…", with its request and (once issued) its certificate.
public struct StoredKey: Codable, Equatable {
    public enum Status { case needsRequest, waitingForCertificate, ready }

    public let id: String
    public var label: String
    public let storage: KeyStorage
    public let algorithm: KeyAlgorithm
    /// Secure Enclave: Touch ID or the login password before every signature.
    public let userPresence: Bool
    public let created: Date
    /// Secure Enclave key blob (encrypted by the Secure Enclave; useless on another Mac).
    var enclaveKey: Data?
    public var subject: CertificateSubject?
    /// PKCS#10 request, DER.
    public var request: Data?
    /// Issued certificate, DER, then any intermediate CA certificates that came with it.
    public var certificate: Data?
    public var chain: [Data] = []

    public var status: Status { certificate != nil ? .ready : request != nil ? .waitingForCertificate : .needsRequest }

    public var secCertificate: SecCertificate? { certificate.flatMap { SecCertificateCreateWithData(nil, $0 as CFData) } }

    public var displayName: String {
        guard let cert = secCertificate else { return label }
        let name = SecCertificateCopySubjectSummary(cert) as String? ?? label
        var emails: CFArray?
        SecCertificateCopyEmailAddresses(cert, &emails)
        return (emails as? [String])?.first.map { "\(name) <\($0)>" } ?? name
    }

    public var certificateExpiry: Date? {
        guard let cert = secCertificate,
              let values = SecCertificateCopyValues(cert, [kSecOIDX509V1ValidityNotAfter] as CFArray, nil) as? [String: Any],
              let entry = values[kSecOIDX509V1ValidityNotAfter as String] as? [String: Any],
              let number = entry[kSecPropertyKeyValue as String] as? NSNumber else { return nil }
        return Date(timeIntervalSinceReferenceDate: number.doubleValue)
    }
}

/// Signing keys for certificate requests: creation (Secure Enclave or Keychain), PKCS#10 requests,
/// pairing issued certificates with their keys, and signing.
public enum KeyVault {
    /// Records live here (one JSON file per key). `SUITE42_KEY_DIR` overrides it for tests.
    public static var directory: URL = {
        if let dir = ProcessInfo.processInfo.environment["SUITE42_KEY_DIR"], !dir.isEmpty { return URL(fileURLWithPath: dir) }
        #if APP_STORE
        return AppStoreEdition.sharedSupportDirectory.appendingPathComponent("Signing Keys", isDirectory: true)
        #else
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return base.appendingPathComponent("Suite42/Signing Keys", isDirectory: true)
        #endif
    }()

    public static var secureEnclaveAvailable: Bool { SecureEnclave.isAvailable }

    // MARK: Records

    public static func all() -> [StoredKey] {
        let files = (try? FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: nil)) ?? []
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        return files.filter { $0.pathExtension == "json" }
            .compactMap { try? decoder.decode(StoredKey.self, from: Data(contentsOf: $0)) }
            .sorted { $0.created < $1.created }
    }

    /// Secure Enclave keys that have a certificate: identities for signing (Keychain keys appear in `CertificateStore`).
    public static func enclaveIdentities() -> [StoredKey] {
        all().filter { $0.storage == .secureEnclave && $0.certificate != nil }
    }

    public static func key(id: String) -> StoredKey? { all().first { $0.id == id } }

    static func save(_ key: StoredKey) throws {
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true,
                                                attributes: [.posixPermissions: 0o700])
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        let url = directory.appendingPathComponent("\(key.id).json")
        try encoder.encode(key).write(to: url, options: .atomic)
        try? FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
    }

    /// Removes the record and, for Keychain keys, the private key (an imported certificate stays in the Keychain).
    public static func delete(_ key: StoredKey) throws {
        if key.storage == .keychain {
            let status = SecItemDelete([kSecClass as String: kSecClassKey,
                                        kSecAttrApplicationTag as String: tag(key.id)] as CFDictionary)
            guard status == errSecSuccess || status == errSecItemNotFound else {
                throw CertificateStore.error(status, L("Could not delete the key"))
            }
        }
        try FileManager.default.removeItem(at: directory.appendingPathComponent("\(key.id).json"))
    }

    // MARK: Keys

    /// Creates a key. Secure Enclave keys are always P-256.
    public static func create(label: String, storage: KeyStorage, algorithm: KeyAlgorithm = .p256,
                              userPresence: Bool = true, persist: Bool = true) throws -> StoredKey {
        let id = UUID().uuidString
        var key = StoredKey(id: id, label: label, storage: storage, algorithm: storage == .secureEnclave ? .p256 : algorithm,
                            userPresence: storage == .secureEnclave && userPresence, created: Date())
        switch storage {
        case .secureEnclave:
            guard SecureEnclave.isAvailable else {
                throw CertificateStore.plain(L("This Mac has no Secure Enclave"))
            }
            var flags: SecAccessControlCreateFlags = [.privateKeyUsage]
            if userPresence { flags.insert(.userPresence) }
            var error: Unmanaged<CFError>?
            guard let access = SecAccessControlCreateWithFlags(nil, kSecAttrAccessibleWhenUnlockedThisDeviceOnly, flags, &error) else {
                throw error!.takeRetainedValue() as Error
            }
            key.enclaveKey = try SecureEnclave.P256.Signing.PrivateKey(accessControl: access).dataRepresentation
        case .keychain:
            let attributes: [String: Any] = [
                kSecAttrKeyType as String: algorithm.isEC ? kSecAttrKeyTypeECSECPrimeRandom : kSecAttrKeyTypeRSA,
                kSecAttrKeySizeInBits as String: algorithm == .p256 ? 256 : algorithm == .rsa2048 ? 2048 : 3072,
                kSecPrivateKeyAttrs as String: [
                    kSecAttrIsPermanent as String: true,
                    kSecAttrApplicationTag as String: tag(id),
                    kSecAttrLabel as String: label,
                ] as [String: Any],
            ]
            var error: Unmanaged<CFError>?
            guard SecKeyCreateRandomKey(attributes as CFDictionary, &error) != nil else {
                throw error!.takeRetainedValue() as Error
            }
        }
        if persist { try save(key) }
        return key
    }

    private static func tag(_ id: String) -> Data { Data("com.sfzorin.suite42.key.\(id)".utf8) }

    private static func keychainKey(_ key: StoredKey) throws -> SecKey {
        var result: CFTypeRef?
        let status = SecItemCopyMatching([kSecClass as String: kSecClassKey,
                                          kSecAttrKeyClass as String: kSecAttrKeyClassPrivate,
                                          kSecAttrApplicationTag as String: tag(key.id),
                                          kSecReturnRef as String: true] as CFDictionary, &result)
        guard status == errSecSuccess, let result else { throw CertificateStore.error(status, L("The key is not in the Keychain")) }
        return result as! SecKey
    }

    private static func enclaveKey(_ key: StoredKey, reason: String?) throws -> SecureEnclave.P256.Signing.PrivateKey {
        guard let blob = key.enclaveKey else { throw CertificateStore.error(errSecItemNotFound, L("The key is missing")) }
        let context = LAContext()
        if let reason { context.localizedReason = reason }
        return try SecureEnclave.P256.Signing.PrivateKey(dataRepresentation: blob, authenticationContext: context)
    }

    /// Public key as an X.509 SubjectPublicKeyInfo (DER).
    public static func publicKeyInfo(_ key: StoredKey) throws -> Data {
        switch key.storage {
        case .secureEnclave:
            return try enclaveKey(key, reason: nil).publicKey.derRepresentation
        case .keychain:
            guard let pub = SecKeyCopyPublicKey(try keychainKey(key)) else { throw CertificateStore.error(errSecItemNotFound, L("The key is missing")) }
            var error: Unmanaged<CFError>?
            guard let raw = SecKeyCopyExternalRepresentation(pub, &error) as Data? else { throw error!.takeRetainedValue() as Error }
            return Data(spki(raw: [UInt8](raw), ec: key.algorithm.isEC))
        }
    }

    private static func spki(raw: [UInt8], ec: Bool) -> [UInt8] {
        let algorithm = ec ? RequestDER.seq(RequestDER.oid("1.2.840.10045.2.1"), RequestDER.oid("1.2.840.10045.3.1.7"))
                           : RequestDER.seq(RequestDER.oid("1.2.840.113549.1.1.1"), RequestDER.null)
        return RequestDER.seq(algorithm, RequestDER.bitString(raw))
    }

    /// Signs `message` with SHA-256: an X9.62 DER ECDSA signature or a PKCS#1 v1.5 RSA signature.
    /// A Secure Enclave key with user presence asks for Touch ID or the password, showing `reason`.
    public static func sign(_ message: Data, with key: StoredKey, reason: String? = nil) throws -> Data {
        switch key.storage {
        case .secureEnclave:
            return try enclaveKey(key, reason: reason).signature(for: message).derRepresentation
        case .keychain:
            var error: Unmanaged<CFError>?
            let algorithm: SecKeyAlgorithm = key.algorithm.isEC ? .ecdsaSignatureMessageX962SHA256 : .rsaSignatureMessagePKCS1v15SHA256
            guard let sig = SecKeyCreateSignature(try keychainKey(key), algorithm, message as CFData, &error) as Data? else {
                throw error!.takeRetainedValue() as Error
            }
            return sig
        }
    }

    /// DER AlgorithmIdentifier of the signatures `sign` makes.
    public static func signatureAlgorithm(_ key: StoredKey) -> Data {
        Data(key.algorithm.isEC ? RequestDER.seq(RequestDER.oid("1.2.840.10045.4.3.2"))
                                : RequestDER.seq(RequestDER.oid("1.2.840.113549.1.1.11"), RequestDER.null))
    }

    // MARK: Requests

    /// Builds a PKCS#10 certificate request signed by the key, stores it with the key and returns it (DER).
    /// It asks for a signing certificate: key usage digitalSignature + nonRepudiation, the e-mail as subjectAltName.
    @discardableResult
    public static func certificationRequest(for key: inout StoredKey, subject: CertificateSubject, persist: Bool = true) throws -> Data {
        guard !subject.commonName.trimmingCharacters(in: .whitespaces).isEmpty else {
            throw CertificateStore.plain(L("The request needs a name"))
        }
        var extensions = [RequestDER.seq(RequestDER.oid("2.5.29.15"), RequestDER.bool(true), RequestDER.octet(RequestDER.bitString([0xC0], unused: 6)))]
        let email = subject.email.trimmingCharacters(in: .whitespaces)
        if !email.isEmpty {
            extensions.append(RequestDER.seq(RequestDER.oid("2.5.29.17"), RequestDER.octet(RequestDER.seq(RequestDER.tlv(0x81, Array(email.utf8))))))
        }
        let extensionRequest = RequestDER.seq(RequestDER.oid("1.2.840.113549.1.9.14"), RequestDER.tlv(0x31, RequestDER.seq(extensions)))
        let info = RequestDER.seq(RequestDER.int(0), name(subject), [UInt8](try publicKeyInfo(key)), RequestDER.tlv(0xA0, extensionRequest))
        let signature = try sign(Data(info), with: key, reason: L("create a certificate request"))
        let request = Data(RequestDER.seq(info, [UInt8](signatureAlgorithm(key)), RequestDER.bitString([UInt8](signature))))
        key.subject = subject
        key.request = request
        if persist { try save(key) }
        return request
    }

    /// Gives the key a self-signed signing certificate (no CA): for internal use and tests.
    @discardableResult
    public static func selfSign(_ key: inout StoredKey, subject: CertificateSubject, years: Int = 5, persist: Bool = true) throws -> Data {
        guard !subject.commonName.trimmingCharacters(in: .whitespaces).isEmpty else { throw CertificateStore.plain(L("The request needs a name")) }
        func utc(_ date: Date) -> [UInt8] {
            let f = DateFormatter()
            f.locale = Locale(identifier: "en_US_POSIX")
            f.timeZone = TimeZone(identifier: "UTC")
            f.dateFormat = "yyMMddHHmmss'Z'"
            return RequestDER.tlv(0x17, Array(f.string(from: date).utf8))
        }
        let now = Date()
        let end = Calendar(identifier: .gregorian).date(byAdding: .year, value: years, to: now) ?? now
        var serial = [UInt8](repeating: 0, count: 16)
        _ = SecRandomCopyBytes(kSecRandomDefault, serial.count, &serial)
        serial[0] &= 0x7F
        var extensions = [RequestDER.seq(RequestDER.oid("2.5.29.19"), RequestDER.bool(true), RequestDER.octet(RequestDER.seq())),
                          RequestDER.seq(RequestDER.oid("2.5.29.15"), RequestDER.bool(true), RequestDER.octet(RequestDER.bitString([0xC0], unused: 6)))]
        let email = subject.email.trimmingCharacters(in: .whitespaces)
        if !email.isEmpty {
            extensions.append(RequestDER.seq(RequestDER.oid("2.5.29.17"), RequestDER.octet(RequestDER.seq(RequestDER.tlv(0x81, Array(email.utf8))))))
        }
        let algorithm = [UInt8](signatureAlgorithm(key))
        let issuer = name(subject)
        let tbs = RequestDER.seq(RequestDER.tlv(0xA0, RequestDER.int(2)), RequestDER.tlv(0x02, serial), algorithm, issuer,
                           RequestDER.seq(utc(now.addingTimeInterval(-300)), utc(end)), issuer, [UInt8](try publicKeyInfo(key)),
                           RequestDER.tlv(0xA3, RequestDER.seq(extensions)))
        let signature = try sign(Data(tbs), with: key, reason: L("create a certificate"))
        let certificate = Data(RequestDER.seq(tbs, algorithm, RequestDER.bitString([UInt8](signature))))
        key.subject = subject
        key = try importCertificate(certificate, into: key, persist: persist)
        return certificate
    }

    /// X.501 Name in the usual order (C, ST, L, O, OU, serialNumber, givenName, surname, CN, emailAddress).
    static func name(_ s: CertificateSubject) -> [UInt8] {
        func trim(_ v: String) -> String { v.trimmingCharacters(in: .whitespacesAndNewlines) }
        let fields: [(String, String, UInt8)] = [
            ("2.5.4.6", trim(s.country).uppercased(), 0x13),
            ("2.5.4.8", trim(s.state), 0x0C),
            ("2.5.4.7", trim(s.locality), 0x0C),
            ("2.5.4.10", trim(s.organization), 0x0C),
            ("2.5.4.11", trim(s.organizationalUnit), 0x0C),
            ("2.5.4.5", trim(s.serialNumber), 0x13),
            ("2.5.4.42", trim(s.givenName), 0x0C),
            ("2.5.4.4", trim(s.surname), 0x0C),
            ("2.5.4.3", trim(s.commonName), 0x0C),
            ("1.2.840.113549.1.9.1", trim(s.email), 0x16),
        ]
        return RequestDER.seq(fields.filter { !$0.1.isEmpty }.map { oid, value, tag in
            // PrintableString only allows a small set; fall back to UTF8String.
            let printable = value.allSatisfy { $0.isASCII && ($0.isLetter || $0.isNumber || " '()+,-./:=?".contains($0)) }
            let stringTag: UInt8 = tag == 0x13 && !printable ? 0x0C : tag
            return RequestDER.tlv(0x31, RequestDER.seq(RequestDER.oid(oid), RequestDER.tlv(stringTag, Array(value.utf8))))
        })
    }

    public static func pem(_ der: Data, label: String = "CERTIFICATE REQUEST") -> String {
        let b64 = der.base64EncodedString(options: [.lineLength64Characters, .endLineWithLineFeed])
        return "-----BEGIN \(label)-----\n\(b64)\n-----END \(label)-----\n"
    }

    // MARK: Certificates

    /// Certificates in a DER/PEM certificate or a PKCS#7 bundle (.cer, .crt, .pem, .p7b, .p7c).
    public static func certificates(in data: Data) -> [SecCertificate] {
        if let one = SecCertificateCreateWithData(nil, data as CFData) { return [one] }
        var format = SecExternalFormat.formatUnknown
        var type = SecExternalItemType.itemTypeUnknown
        var items: CFArray?
        guard SecItemImport(data as CFData, nil, &format, &type, [], nil, nil, &items) == errSecSuccess else { return [] }
        return (items as? [AnyObject] ?? []).compactMap { item in
            CFGetTypeID(item) == SecCertificateGetTypeID() ? (item as! SecCertificate) : nil
        }
    }

    /// Pairs an issued certificate with its key (any stored key when `key` is nil): the certificate whose
    /// public key matches becomes the key's certificate, the other certificates its chain. Keychain keys also
    /// get the certificate in the Keychain, which makes a normal identity there.
    public static func importCertificate(_ data: Data, into key: StoredKey? = nil, persist: Bool = true) throws -> StoredKey {
        let certs = certificates(in: data)
        guard !certs.isEmpty else { throw CertificateStore.plain(L("The file contains no certificates")) }
        let candidates = key.map { [$0] } ?? all()
        for var candidate in candidates {
            guard let ours = try? publicKeyInfo(candidate) else { continue }
            guard let leaf = certs.first(where: { publicKeyInfo(of: $0) == ours }) else { continue }
            candidate.certificate = SecCertificateCopyData(leaf) as Data
            candidate.chain = certs.filter { $0 !== leaf }.map { SecCertificateCopyData($0) as Data }
            if candidate.storage == .keychain, persist {
                for cert in certs {
                    let status = SecItemAdd([kSecClass as String: kSecClassCertificate, kSecValueRef as String: cert] as CFDictionary, nil)
                    guard status == errSecSuccess || status == errSecDuplicateItem else {
                        throw CertificateStore.error(status, L("Could not add the certificate to the Keychain"))
                    }
                }
            }
            if persist { try save(candidate) }
            return candidate
        }
        throw CertificateStore.plain(key == nil
            ? L("The certificate does not belong to any key created here")
            : L("The certificate does not belong to this key"))
    }

    /// SubjectPublicKeyInfo of a certificate (DER).
    static func publicKeyInfo(of cert: SecCertificate) -> Data? {
        let der = [UInt8](SecCertificateCopyData(cert) as Data)
        guard let tbs = RequestDER.children(der).first else { return nil }
        let parts = RequestDER.children(tbs)
        let offset = parts.first?.first == 0xA0 ? 1 : 0
        guard parts.count > offset + 5 else { return nil }
        return Data(parts[offset + 5])
    }
}

/// Small DER encoder/reader for the request and certificate parsing above.
enum RequestDER {
    static func tlv(_ tag: UInt8, _ body: [UInt8]) -> [UInt8] {
        var length: [UInt8]
        if body.count < 0x80 { length = [UInt8(body.count)] } else {
            var n = body.count, bytes: [UInt8] = []
            while n > 0 { bytes.insert(UInt8(n & 0xFF), at: 0); n >>= 8 }
            length = [0x80 | UInt8(bytes.count)] + bytes
        }
        return [tag] + length + body
    }
    static func seq(_ parts: [UInt8]...) -> [UInt8] { tlv(0x30, parts.flatMap { $0 }) }
    static func seq(_ parts: [[UInt8]]) -> [UInt8] { tlv(0x30, parts.flatMap { $0 }) }
    static func octet(_ b: [UInt8]) -> [UInt8] { tlv(0x04, b) }
    static func bitString(_ b: [UInt8], unused: UInt8 = 0) -> [UInt8] { tlv(0x03, [unused] + b) }
    static func bool(_ v: Bool) -> [UInt8] { [0x01, 0x01, v ? 0xFF : 0x00] }
    static let null: [UInt8] = [0x05, 0x00]
    static func int(_ n: Int) -> [UInt8] {
        var bytes: [UInt8] = [], v = n
        repeat { bytes.insert(UInt8(v & 0xFF), at: 0); v >>= 8 } while v > 0
        if bytes[0] & 0x80 != 0 { bytes.insert(0, at: 0) }
        return tlv(0x02, bytes)
    }
    static func oid(_ dotted: String) -> [UInt8] {
        let parts = dotted.split(separator: ".").compactMap { UInt64($0) }
        var body: [UInt8] = [UInt8(parts[0] * 40 + parts[1])]
        for p in parts.dropFirst(2) {
            var chunk: [UInt8] = [UInt8(p & 0x7F)], v = p >> 7
            while v > 0 { chunk.insert(UInt8(v & 0x7F) | 0x80, at: 0); v >>= 7 }
            body += chunk
        }
        return tlv(0x06, body)
    }

    /// Complete TLV elements in `bytes` (one level).
    static func elements(_ bytes: [UInt8]) -> [[UInt8]] {
        var out: [[UInt8]] = [], i = 0
        while i + 2 <= bytes.count {
            let start = i
            i += 1
            var length = Int(bytes[i]); i += 1
            if length & 0x80 != 0 {
                let n = length & 0x7F
                guard n <= 4, i + n <= bytes.count else { return out }
                length = bytes[i..<i + n].reduce(0) { $0 << 8 | Int($1) }
                i += n
            }
            guard i + length <= bytes.count else { return out }
            i += length
            out.append(Array(bytes[start..<i]))
        }
        return out
    }

    /// Children of a constructed element.
    static func children(_ element: [UInt8]) -> [[UInt8]] {
        guard let whole = elements(element).first, whole.count >= 2 else { return [] }
        var header = 2
        if whole[1] & 0x80 != 0 { header += Int(whole[1] & 0x7F) }
        return elements(Array(whole[header...]))
    }
}
