import CryptoKit
import Foundation
import Kit42
import PDFKit
import Security

/// PAdES signing with a smart card through its PKCS#11 module (Turkish e-imza: AKİS cards with Kamu SM, E-Tuğra or
/// TÜRKTRUST qualified certificates; any other PKCS#11 token works the same way). The card computes the RSA or ECDSA
/// signature over the CMS signed attributes; the chain is completed from the system and the certificates' caIssuers
/// URLs so that B-LT/B-LTA can embed revocation data for it.
struct PKCS11CMSSigner: CMSRawSigner {
    let signer: PKCS11Signer
    let chain: [X509]

    init(token: PKCS11Token, certificate: PKCS11Certificate, pin: String?) throws {
        guard let leaf = X509(der: [UInt8](certificate.der)) else { throw CertificateError.failed(L("certificate")) }
        var chain = [leaf]
        if let cert = SecCertificateCreateWithData(nil, certificate.der as CFData) {
            var trust: SecTrust?
            if SecTrustCreateWithCertificates(cert, SecPolicyCreateBasicX509(), &trust) == errSecSuccess, let trust {
                _ = SecTrustEvaluateWithError(trust, nil)
                for c in (SecTrustCopyCertificateChain(trust) as? [SecCertificate] ?? []).dropFirst() {
                    if let x = X509(c), !chain.contains(x) { chain.append(x) }
                }
            }
        }
        // Turkish providers publish their intermediate (and root) certificates at the caIssuers URL.
        while let last = chain.last, !last.isSelfIssued, chain.count < 8 {
            let found = last.caIssuerURLs.lazy.compactMap { URL(string: $0) }
                .compactMap { try? Revocation.fetch($0, timeout: 10) }
                .compactMap { Revocation.issuer(of: last, in: X509.certificates(in: [UInt8]($0))) }.first
            guard let issuer = found, !chain.contains(issuer) else { break }
            chain.append(issuer)
        }
        self.chain = chain
        signer = PKCS11Signer(token: token, certificate: certificate, pin: pin, chain: chain.dropFirst().map { Data($0.der) })
    }

    func sign(attributes: [UInt8]) throws -> (signature: [UInt8], algorithm: [UInt8]) {
        let value = try signer.signDigest(Data(SHA256.hash(data: Data(attributes))))
        if signer.isEC {
            return ([UInt8](ECDSAValue.der(fromRaw: value)), DER.seq(DER.oid(OID.ecdsaWithSHA256)))
        }
        return ([UInt8](value), DER.seq(DER.oid(OID.sha256WithRSA), DER.null))
    }
}

enum EImzaCLI {
    /// Smart-card (PKCS#11) signing and the Turkish qualified-signature report; nil for other commands.
    static func run(_ args: [String]) -> Int32? {
        guard args.count > 1 else { return nil }
        switch args[1] {
        case "--pkcs11-sign" where args.count >= 5: return sign(args)
        case "--verify-tr" where args.count >= 3: return verify(args[2])
        case "--pkcs11-list", "--verify-document": return args.count >= 3 ? DocumentSignatureCLI.run(Array(args.dropFirst())) : nil
        default: return nil
        }
    }

    /// PDF42 --pkcs11-sign in.pdf out.pdf MODULE pin=… [slot=N] [key=hex] [level=B-B|B-T|B-LT|B-LTA] [tsa | tsa=URL] [reason=…] [location=…]
    static func sign(_ args: [String]) -> Int32 {
        let options = args.dropFirst(5)
        func argument(_ key: String) -> String? {
            options.first { $0.hasPrefix(key + "=") }.map { String($0.dropFirst(key.count + 1)) }
        }
        func apply(_ value: inout PDFSigner.Options) throws {
            if options.contains("tsa") { value.timestampURL = URL(string: "http://timestamp.digicert.com") }
            if let tsa = argument("tsa") {
                guard let url = URL(string: tsa), ["http", "https"].contains(url.scheme?.lowercased() ?? "") else {
                    throw PDFStructureError.unsupported("Invalid HTTP or HTTPS URL")
                }
                value.timestampURL = url
            }
            if let level = argument("level") {
                guard let named = PAdESLevel.named(level) else { throw PDFStructureError.unsupported("Unknown PAdES level") }
                value.level = named
            } else { value.level = value.timestampURL != nil ? .baselineT : .baselineB }
            if value.level == .baselineB { value.timestampURL = nil }
        }
        do {
            let input = try Data(contentsOf: URL(fileURLWithPath: args[2]))
            let token = PKCS11Token(module: args[4])
            let certs = try token.certificates()
            let slot = argument("slot").flatMap(Int.init), key = argument("key")
            let candidates = certs.filter { (slot == nil || $0.slot == slot) && (key == nil || $0.id == key) }
            guard let cert = candidates.first(where: \.hasPrivateKey) ?? candidates.first(where: { $0.keyUsage.nonRepudiation }) ?? candidates.first else {
                print("error: no certificate on the token")
                return 1
            }
            guard let pdf = PDFDocument(data: input), !pdf.isLocked, pdf.pageCount > 0 else {
                throw NSError(domain: NSCocoaErrorDomain, code: NSFileReadCorruptFileError)
            }
            let signer = try PKCS11CMSSigner(token: token, certificate: cert, pin: argument("pin"))
            var value = PDFSigner.Options(signer: .smartCard(signer), reason: argument("reason") ?? "e-imza", location: argument("location") ?? "")
            try apply(&value)
            let signed = try PAdES.sign(PDFMainCLI.signingData(input, pdf: pdf), options: value)
            signed.warnings.forEach { print("warning: \($0)") }
            try signed.data.write(to: URL(fileURLWithPath: args[3]), options: .atomic)
            print("signed with \(cert.displayName) \(signed.data.count) bytes, \(value.level.cliName), chain=\(signer.chain.count)")
            return 0
        } catch {
            print("error: \(error.localizedDescription)")
            return 1
        }
    }

    /// PDF42 --verify-tr file.pdf: signatures with their Turkish qualified-certificate status.
    static func verify(_ path: String) -> Int32 {
        guard let data = FileManager.default.contents(atPath: path) else { return 2 }
        let list = PDFSignatureVerifier.verify(data)
        if list.isEmpty { print("no signatures"); return 1 }
        for s in list {
            var line = "\(s.status) | \(s.signer) | \(s.level ?? "-")"
            if let der = s.certificateDER { line += " | " + (TurkishESign.describe(der) ?? "not a Turkish certificate") }
            if let p = s.problem { line += " | problem=\(p)" }
            print(line)
        }
        return list.allSatisfy { $0.status != .invalid } ? 0 : 1
    }
}

