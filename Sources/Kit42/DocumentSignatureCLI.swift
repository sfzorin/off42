import Foundation

/// Command-line hooks for Office/OpenDocument signatures, shared by DOC42 and XLS42:
///   --sign-document in out (--p12 file --password pw | --identity name | --pkcs11 module --pin pin [--slot n] [--key id])
///                   [--comment text]
///   --verify-document file      prints one line per signature; exit status 0 only when all are valid
public enum DocumentSignatureCLI {
    /// Returns an exit code when the arguments are a signature command, else nil.
    public static func run(_ args: [String]) -> Int32? {
        func value(_ name: String) -> String? {
            guard let i = args.firstIndex(of: name), i + 1 < args.count else { return nil }
            return args[i + 1]
        }
        // --c14n file.xml [Id]: canonical form of the element with that Id (or the whole document), for tests.
        if let i = args.firstIndex(of: "--c14n"), i + 1 < args.count {
            guard let data = FileManager.default.contents(atPath: args[i + 1]), let root = try? XNode.parse(data) else { return 2 }
            let node = i + 2 < args.count ? root.byID(args[i + 2]) : root
            FileHandle.standardOutput.write(node.map { C14N.canonicalize($0) } ?? Data())
            return node == nil ? 1 : 0
        }
        // --pkcs11-list module: module architecture, slots, tokens and certificates (no login).
        if let i = args.firstIndex(of: "--pkcs11-list"), i + 1 < args.count {
            let module = args[i + 1]
            print("module \(module) [\(PKCS11Modules.architectures(module).joined(separator: ", "))], host \(PKCS11Modules.hostArchitecture)")
            let token = PKCS11Token(module: module)
            do {
                let certs = try token.certificates()
                print("library \(token.info.manufacturer) \(token.info.library) (bridge \(token.info.arch))")
                for s in token.info.slots { print("slot \(s.slot): \(s.description) token=\(s.token ?? "-")") }
                for c in certs {
                    let qualified = TurkishESign.isQualified(c.der) ? " qualified" : ""
                    print("cert slot=\(c.slot) id=\(c.id) \(c.displayName) nonRepudiation=\(c.keyUsage.nonRepudiation)\(qualified)")
                }
                return 0
            } catch {
                print("error: \(error.localizedDescription)")
                return 1
            }
        }
        if let i = args.firstIndex(of: "--verify-document"), i + 1 < args.count {
            guard let data = FileManager.default.contents(atPath: args[i + 1]) else { print("cannot read \(args[i + 1])"); return 2 }
            let list = DocumentSignatures.verify(data)
            if list.isEmpty { print("no signatures"); return 1 }
            for s in list { print(line(s)) }
            let strict = args.contains("--require-trust")
            return list.allSatisfy { $0.status == .valid || (!strict && $0.status == .untrusted) } ? 0 : 1
        }
        if let i = args.firstIndex(of: "--sign-document"), i + 2 < args.count {
            do {
                guard let data = FileManager.default.contents(atPath: args[i + 1]) else { throw XMLDSigError.signing("cannot read \(args[i + 1])") }
                let signer = try signerFromArguments(value)
                let signed = try DocumentSignatures.sign(data, signer: signer, comment: value("--comment") ?? "")
                try signed.write(to: URL(fileURLWithPath: args[i + 2]))
                print("signed \(signed.count) bytes")
                return 0
            } catch {
                print("error: \(error.localizedDescription)")
                return 1
            }
        }
        return nil
    }

    public static func signerFromArguments(_ value: (String) -> String?) throws -> XMLRawSigner {
        if let p12 = value("--p12") {
            guard let data = FileManager.default.contents(atPath: p12) else { throw XMLDSigError.signing("cannot read \(p12)") }
            let id = try CertificateStore.importPKCS12(data, password: value("--password") ?? "", toKeychain: false)
            return try IdentitySigner(id.identity)
        }
        if let name = value("--identity") {
            guard let id = CertificateStore.identities().first(where: { $0.displayName.localizedCaseInsensitiveContains(name) }) else {
                throw XMLDSigError.signing("no Keychain identity matches \(name)")
            }
            return try IdentitySigner(id.identity)
        }
        if let module = value("--pkcs11") {
            let token = PKCS11Token(module: module)
            let certs = try token.certificates()
            let slot = value("--slot").flatMap(Int.init)
            let key = value("--key")
            // Private keys are usually visible only after login: prefer a certificate whose key is listed, else take the
            // first non-repudiation (qualified signature) certificate, else the first one.
            let candidates = certs.filter { (slot == nil || $0.slot == slot) && (key == nil || $0.id == key) }
            guard let cert = candidates.first(where: \.hasPrivateKey) ?? candidates.first(where: { $0.keyUsage.nonRepudiation }) ?? candidates.first else {
                throw XMLDSigError.signing("no certificate on the token")
            }
            return PKCS11Signer(token: token, certificate: cert, pin: value("--pin") ?? "")
        }
        throw XMLDSigError.signing("give --p12, --identity or --pkcs11")
    }

    static func line(_ s: DocumentSignature) -> String {
        let status: String
        switch s.status {
        case .valid: status = "VALID"
        case .untrusted: status = "INTACT-UNTRUSTED"
        case .invalid: status = "INVALID"
        }
        var parts = [status, s.format, "signer=\(s.signer)"]
        if let d = s.date { parts.append("date=\(XMLTime.string(d))") }
        if let p = s.problem { parts.append("problem=\(p)") }
        if let t = s.trustMessage { parts.append("trust=\(t)") }
        if let c = s.comment { parts.append("comment=\(c)") }
        return parts.joined(separator: " | ")
    }
}
