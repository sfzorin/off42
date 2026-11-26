import CryptoKit
import Foundation
import Security

/// Smart-card signing through the PKCS#11 module that the card's middleware installed (AKİS for Turkish e-imza cards,
/// SafeNet, OpenSC, …). Nothing is bundled: the module runs in the P11Bridge42 helper process, under Rosetta when the
/// vendor ships only an Intel build.
public enum PKCS11Modules {
    public struct Known {
        public let name: String
        public let paths: [String]
    }

    /// Where common middleware installs its module on macOS.
    public static let known: [Known] = [
        Known(name: "AKİS (TÜBİTAK BİLGEM)", paths: ["/usr/local/lib/libakisp11.dylib", "/usr/lib/libakisp11.dylib",
                                                   "/Library/AKIS/libakisp11.dylib", "/Applications/AKIS.app/Contents/MacOS/libakisp11.dylib"]),
        Known(name: "SafeNet eToken", paths: ["/usr/local/lib/libeTPkcs11.dylib", "/Library/Frameworks/eToken.framework/Versions/A/libeToken.dylib"]),
        Known(name: "Gemalto / Thales IDPrime", paths: ["/usr/local/lib/libIDPrimePKCS11.dylib", "/Library/Frameworks/IDPrimePKCS11.framework/IDPrimePKCS11"]),
        Known(name: "OpenSC", paths: ["/Library/OpenSC/lib/opensc-pkcs11.so", "/opt/homebrew/lib/opensc-pkcs11.so", "/usr/local/lib/opensc-pkcs11.so"]),
        Known(name: "SoftHSM 2 (test)", paths: ["/opt/homebrew/lib/softhsm/libsofthsm2.so", "/usr/local/lib/softhsm/libsofthsm2.so"]),
    ]

    /// Installed modules: (display name, path).
    public static func installed() -> [(String, String)] {
        #if APP_STORE
        return []  // the App Store variant has no P11Bridge42 helper
        #else
        var out: [(String, String)] = []
        for k in known { for p in k.paths where FileManager.default.fileExists(atPath: p) { out.append((k.name, p)) } }
        if let custom = UserDefaults.standard.string(forKey: "pkcs11.module"), FileManager.default.fileExists(atPath: custom),
           !out.contains(where: { $0.1 == custom }) {
            out.append(((custom as NSString).lastPathComponent, custom))
        }
        return out
        #endif
    }

    /// CPU architectures in a Mach-O file (thin or universal).
    public static func architectures(_ path: String) -> [String] {
        guard let h = FileHandle(forReadingAtPath: path), let head = try? h.read(upToCount: 4096), head.count >= 8 else { return [] }
        let b = [UInt8](head)
        func be32(_ i: Int) -> UInt32 { UInt32(b[i]) << 24 | UInt32(b[i + 1]) << 16 | UInt32(b[i + 2]) << 8 | UInt32(b[i + 3]) }
        func le32(_ i: Int) -> UInt32 { UInt32(b[i]) | UInt32(b[i + 1]) << 8 | UInt32(b[i + 2]) << 16 | UInt32(b[i + 3]) << 24 }
        func name(_ cpu: UInt32) -> String {
            switch cpu {
            case 0x0100_0007: return "x86_64"
            case 0x0100_000C: return "arm64"
            case 7: return "i386"
            default: return String(format: "cpu%x", cpu)
            }
        }
        switch be32(0) {
        case 0xCAFE_BABE, 0xCAFE_BABF:
            let n = Int(be32(4)), size = be32(0) == 0xCAFE_BABF ? 32 : 20
            return (0..<min(n, 16)).compactMap { i in 8 + i * size + 4 <= b.count ? name(be32(8 + i * size)) : nil }
        default:
            if le32(0) == 0xFEED_FACF || le32(0) == 0xFEED_FACE { return [name(le32(4))] }
            return []
        }
    }

    public static var hostArchitecture: String {
        #if arch(arm64)
        return "arm64"
        #else
        return "x86_64"
        #endif
    }

    /// The bridge helper: inside the app bundle, else next to the executable (development builds).
    public static func bridgeURL() -> URL? {
        #if APP_STORE
        return nil
        #else
        if let env = ProcessInfo.processInfo.environment["SUITE42_P11BRIDGE"] { return URL(fileURLWithPath: env) }
        let candidates = [Bundle.main.bundleURL.appendingPathComponent("Contents/Helpers/P11Bridge42"),
                          Bundle.main.executableURL?.deletingLastPathComponent().appendingPathComponent("P11Bridge42")].compactMap { $0 }
        return candidates.first { FileManager.default.isExecutableFile(atPath: $0.path) }
        #endif
    }
}

public enum PKCS11Error: LocalizedError {
    case noBridge
    case architecture(module: String, archs: [String])
    case rosetta
    case module(String)
    case pin(String)
    case token(String)

    public var errorDescription: String? {
        switch self {
        case .noBridge: return L("The smart-card helper (P11Bridge42) is missing from the application.")
        case .architecture(let m, let a):
            return L("The card module %@ is built for %@ only and cannot be loaded on this Mac.", (m as NSString).lastPathComponent, a.joined(separator: ", "))
        case .rosetta:
            return L("The card module is built for Intel processors. Install Rosetta (Terminal: softwareupdate --install-rosetta) or a newer Apple-silicon version of the card software.")
        case .module(let s): return L("The card module reported an error: %@", s)
        case .pin(let s): return s
        case .token(let s): return s
        }
    }

    /// Text for PKCS#11 return values a user can act on.
    static func message(_ rv: Int, call: String) -> PKCS11Error {
        switch rv {
        case 0xA0: return .pin(L("Wrong PIN."))
        case 0xA4: return .pin(L("The PIN is locked. Unlock it with the PUK in the card software."))
        case 0xA3: return .pin(L("The PIN has expired. Change it in the card software."))
        case 0xA1, 0xA2: return .pin(L("The PIN has the wrong length."))
        case 0xE0, 0x30, 0x32: return .token(L("No card in the reader, or the card was removed."))
        case 0x5, 0x6: return .module(L("general failure (%@)", call))
        case 0x70, 0x71: return .token(L("The card does not support this signing method."))
        case 0x101: return .pin(L("The card needs a PIN."))
        default: return .module(String(format: "%@ CKR 0x%lX", call, rv))
        }
    }
}

/// A certificate on a token.
public struct PKCS11Certificate {
    public let slot: Int
    public let id: String
    public let label: String
    public let der: Data
    public let hasPrivateKey: Bool
    public let tokenLabel: String

    public var details: CertificateDetails? { CertificateDetails(der: der) }
    public var displayName: String {
        let cn = details?.commonName ?? label
        return tokenLabel.isEmpty ? cn : "\(cn) — \(tokenLabel)"
    }

    /// Signing certificates: nonRepudiation or digitalSignature key usage (the authentication certificate of an
    /// e-imza card has digitalSignature only, the qualified one nonRepudiation).
    public var keyUsage: (digitalSignature: Bool, nonRepudiation: Bool) {
        guard let ext = details?.extensions["2.5.29.15"], let bits = ASN1.parse(ext), bits.content.count >= 2 else { return (true, false) }
        return (bits.content[1] & 0x80 != 0, bits.content[1] & 0x40 != 0)
    }
}

public struct PKCS11TokenInfo {
    public var library = ""
    public var manufacturer = ""
    public var arch = ""
    public var slots: [(slot: Int, description: String, token: String?, flags: UInt)] = []
}

/// One PKCS#11 module, driven through the bridge. Each call is a short-lived helper process.
public final class PKCS11Token {
    public let module: String
    public private(set) var info = PKCS11TokenInfo()

    public init(module: String) { self.module = module }

    /// Launch arguments for the bridge: native, or x86_64 under Rosetta for Intel-only modules.
    func launch() throws -> (URL, [String]) {
        guard let bridge = PKCS11Modules.bridgeURL() else { throw PKCS11Error.noBridge }
        let moduleArchs = PKCS11Modules.architectures(module)
        let host = PKCS11Modules.hostArchitecture
        if moduleArchs.isEmpty || moduleArchs.contains(host) || moduleArchs.contains("arm64e") && host == "arm64" {
            return (bridge, [])
        }
        if moduleArchs.contains("x86_64") {
            guard PKCS11Modules.architectures(bridge.path).contains("x86_64") else { throw PKCS11Error.architecture(module: module, archs: moduleArchs) }
            return (URL(fileURLWithPath: "/usr/bin/arch"), ["-x86_64", bridge.path])
        }
        throw PKCS11Error.architecture(module: module, archs: moduleArchs)
    }

    func call(_ request: [String: Any]) throws -> [String: Any] {
        var request = request
        request["module"] = module
        let (exe, args) = try launch()
        let p = Process()
        p.executableURL = exe
        p.arguments = args
        let input = Pipe(), output = Pipe(), errors = Pipe()
        p.standardInput = input
        p.standardOutput = output
        p.standardError = errors
        try p.run()
        input.fileHandleForWriting.write(try JSONSerialization.data(withJSONObject: request))
        try? input.fileHandleForWriting.close()
        let data = output.fileHandleForReading.readDataToEndOfFile()
        let stderr = String(decoding: errors.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
        p.waitUntilExit()
        if stderr.contains("Bad CPU type") { throw PKCS11Error.rosetta }
        guard let response = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] else {
            if args.first == "-x86_64" { throw PKCS11Error.rosetta }
            throw PKCS11Error.module(stderr.isEmpty ? L("the helper stopped (status %d)", p.terminationStatus) : stderr)
        }
        if let error = response["error"] as? String {
            if let rv = (response["rv"] as? NSNumber)?.intValue { throw PKCS11Error.message(rv, call: error) }
            throw PKCS11Error.module(error)
        }
        return response
    }

    /// Certificates on all present tokens, and module/slot information in `info`.
    public func certificates() throws -> [PKCS11Certificate] {
        let r = try call(["op": "list"])
        let i = r["info"] as? [String: Any]
        info.library = i?["library"] as? String ?? ""
        info.manufacturer = i?["manufacturer"] as? String ?? ""
        info.arch = r["arch"] as? String ?? ""
        var labels: [Int: String] = [:]
        info.slots = (r["slots"] as? [[String: Any]] ?? []).map { s in
            let slot = (s["slot"] as? NSNumber)?.intValue ?? 0
            let token = s["token"] as? [String: Any]
            labels[slot] = token?["label"] as? String
            return (slot, s["description"] as? String ?? "", token?["label"] as? String, (token?["flags"] as? NSNumber)?.uintValue ?? 0)
        }
        return (r["certificates"] as? [[String: Any]] ?? []).compactMap { c in
            guard let der = (c["der"] as? String).flatMap({ Data(base64Encoded: $0) }) else { return nil }
            let slot = (c["slot"] as? NSNumber)?.intValue ?? 0
            return PKCS11Certificate(slot: slot, id: c["id"] as? String ?? "", label: c["label"] as? String ?? "", der: der,
                                     hasPrivateKey: c["hasKey"] as? Bool ?? false, tokenLabel: labels[slot] ?? "")
        }
    }

    /// PIN state flags of a slot's token (CKF_USER_PIN_COUNT_LOW / FINAL_TRY / LOCKED, protected authentication path).
    public func pinWarning(slot: Int) -> String? {
        guard let flags = info.slots.first(where: { $0.slot == slot })?.flags else { return nil }
        if flags & 0x40000 != 0 { return L("The PIN is locked. Unlock it with the PUK in the card software.") }
        if flags & 0x20000 != 0 { return L("Last attempt: a wrong PIN now locks the card.") }
        if flags & 0x10000 != 0 { return L("A wrong PIN was entered before; few attempts are left.") }
        return nil
    }

    public func hasPinPad(slot: Int) -> Bool {
        (info.slots.first { $0.slot == slot }?.flags ?? 0) & 0x100 != 0
    }

    /// Raw signature: RSA PKCS#1 v1.5 over a DigestInfo, or ECDSA r‖s over a hash. `pin` nil uses the PIN pad.
    public func sign(_ data: Data, certificate: PKCS11Certificate, pin: String?, ecdsa: Bool) throws -> Data {
        var request: [String: Any] = ["op": "sign", "slot": certificate.slot, "id": certificate.id, "label": certificate.label,
                                      "mechanism": ecdsa ? "ecdsa" : "rsa-pkcs", "data": data.base64EncodedString()]
        if let pin { request["pin"] = pin }
        let r = try call(request)
        guard let sig = (r["signature"] as? String).flatMap({ Data(base64Encoded: $0) }) else { throw PKCS11Error.module("no signature") }
        return sig
    }

    /// DigestInfo prefix for SHA-256 (RFC 8017 §9.2).
    public static let sha256DigestInfo: [UInt8] = [0x30, 0x31, 0x30, 0x0D, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, 0x05, 0x00, 0x04, 0x20]
}

/// XMLDSig signer backed by a token key.
public struct PKCS11Signer: XMLRawSigner {
    public let token: PKCS11Token
    public let certificate: PKCS11Certificate
    public let pin: String?
    public var chain: [Data] = []

    public init(token: PKCS11Token, certificate: PKCS11Certificate, pin: String?, chain: [Data] = []) {
        self.token = token
        self.certificate = certificate
        self.pin = pin
        self.chain = chain
    }

    public var certificates: [Data] { [certificate.der] + chain.filter { $0 != certificate.der } }
    public var isEC: Bool {
        guard let cert = SecCertificateCreateWithData(nil, certificate.der as CFData), let key = SecCertificateCopyKey(cert),
              let type = (SecKeyCopyAttributes(key) as? [String: Any])?[kSecAttrKeyType as String] as? String else { return false }
        return type == (kSecAttrKeyTypeECSECPrimeRandom as String)
    }

    public func signSHA256(_ data: Data) throws -> Data {
        let hash = Data(SHA256.hash(data: data))
        return try signDigest(hash)
    }

    /// Signs a SHA-256 hash. RSA: PKCS#1 v1.5 DigestInfo; EC: r‖s as the token returns it.
    public func signDigest(_ hash: Data) throws -> Data {
        isEC ? try token.sign(hash, certificate: certificate, pin: pin, ecdsa: true)
            : try token.sign(Data(PKCS11Token.sha256DigestInfo) + hash, certificate: certificate, pin: pin, ecdsa: false)
    }
}
