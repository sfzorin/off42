import AppKit
import Kit42
import PDFKit

/// Keys and certificate requests ("Get a Certificate…") from the command line; `SUITE42_KEY_DIR` overrides the key folder.
enum KeyCLI {
    static func run(_ args: [String]) -> Int32? {
        guard args.count > 1, ["--key-new", "--key-list", "--key-csr", "--key-import", "--key-sign", "--key-delete"].contains(args[1]) else {
            return nil
        }
        func require(_ value: Bool, _ message: String) throws {
            if !value { throw NSError(domain: "PDF42.CLI", code: 1, userInfo: [NSLocalizedDescriptionKey: message]) }
        }
        func argument(_ key: String, from start: Int) -> String? {
            args.dropFirst(start).first { $0.hasPrefix(key + "=") }.map { String($0.dropFirst(key.count + 1)) }
        }
        func storedKey(_ id: String) throws -> StoredKey {
            guard let key = KeyVault.all().first(where: { $0.id == id || $0.id.hasPrefix(id) }) else {
                throw NSError(domain: "PDF42.CLI", code: 1, userInfo: [NSLocalizedDescriptionKey: "No key \(id)"])
            }
            return key
        }
        do {
            switch args[1] {
            case "--key-new":
                try require(args.count >= 4, "Usage: --key-new enclave|keychain LABEL [p256|rsa2048|rsa3072] [no-presence]")
                try require(["enclave", "keychain"].contains(args[2]), "Unknown key storage: \(args[2])")
                let storage: KeyStorage = args[2] == "enclave" ? .secureEnclave : .keychain
                let algorithm = args.dropFirst(4).compactMap(KeyAlgorithm.init(rawValue:)).first ?? .p256
                let key = try KeyVault.create(label: args[3], storage: storage, algorithm: algorithm, userPresence: !args.contains("no-presence"))
                print(key.id)
            case "--key-list":
                for k in KeyVault.all() {
                    print("\(k.id) | \(k.storage.rawValue) | \(k.algorithm.title) | presence=\(k.userPresence) | \(k.status) | \(k.displayName)")
                }
            case "--key-csr":
                try require(args.count >= 5, "Usage: --key-csr ID out.csr CN=… [O=… OU=… L=… ST=… C=… email=… serial=… given=… surname=…]")
                var key = try storedKey(args[2])
                var subject = CertificateSubject()
                let value = { (name: String) in argument(name, from: 4) ?? "" }
                subject.commonName = value("CN")
                subject.organization = value("O")
                subject.organizationalUnit = value("OU")
                subject.locality = value("L")
                subject.state = value("ST")
                subject.country = value("C")
                subject.email = value("email")
                subject.serialNumber = value("serial")
                subject.givenName = value("given")
                subject.surname = value("surname")
                let der = try KeyVault.certificationRequest(for: &key, subject: subject)
                try KeyVault.pem(der).write(toFile: args[3], atomically: true, encoding: .utf8)
                print("request \(der.count) bytes")
            case "--key-import":
                try require(args.count == 4, "Usage: --key-import ID|any certificate-file")
                let key = try KeyVault.importCertificate(try Data(contentsOf: URL(fileURLWithPath: args[3])),
                                                         into: args[2] == "any" ? nil : storedKey(args[2]))
                print("paired with \(key.id): \(key.displayName), chain \(key.chain.count)")
            case "--key-sign":
                try require(args.count >= 5, "Usage: --key-sign in.pdf ID out.pdf [tsa | tsa=URL] [level=…] [reason=…]")
                let key = try storedKey(args[3])
                // Fails early (no certificate yet) before the document is touched.
                _ = try StoredKeySigner(key)
                let data = try Data(contentsOf: URL(fileURLWithPath: args[2]))
                guard let pdf = PDFDocument(data: data), !pdf.isLocked, pdf.pageCount > 0 else {
                    throw NSError(domain: NSCocoaErrorDomain, code: NSFileReadCorruptFileError)
                }
                var options = PDFSigner.Options(signer: .secureEnclave(key), reason: argument("reason", from: 5) ?? "Secure Enclave signature test", location: "")
                if args.dropFirst(5).contains("tsa") { options.timestampURL = URL(string: "http://timestamp.digicert.com") }
                if let tsa = argument("tsa", from: 5) {
                    guard let url = URL(string: tsa), ["http", "https"].contains(url.scheme?.lowercased() ?? "") else {
                        throw PDFStructureError.unsupported("Invalid HTTP or HTTPS URL")
                    }
                    options.timestampURL = url
                }
                if let level = argument("level", from: 5) {
                    guard let named = PAdESLevel.named(level) else { throw PDFStructureError.unsupported("Unknown PAdES level") }
                    options.level = named
                } else { options.level = options.timestampURL != nil ? .baselineT : .baselineB }
                if options.level == .baselineB { options.timestampURL = nil }
                let signed = try PAdES.sign(PDFMainCLI.signingData(data, pdf: pdf), options: options)
                signed.warnings.forEach { print("warning: \($0)") }
                try signed.data.write(to: URL(fileURLWithPath: args[4]), options: .atomic)
                print("signed with stored key \(signed.data.count) bytes, \(options.level.cliName)")
            default:
                try require(args.count == 3, "Usage: --key-delete ID")
                try KeyVault.delete(storedKey(args[2]))
            }
            return 0
        } catch {
            FileHandle.standardError.write(Data(("error: " + error.localizedDescription + "\n").utf8))
            return 1
        }
    }
}
