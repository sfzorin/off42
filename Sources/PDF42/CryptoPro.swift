import Foundation
import Kit42

/// GOST signatures through the user's own CryptoPro CSP (never bundled): the `cryptcp`, `certmgr` and `csptest`
/// command-line tools from /opt/cprocsp. PDF42 performs no GOST cryptography itself.
enum CryptoPro {
    /// Tools directory. `SUITE42_CPROCSP_BIN` overrides it (non-standard installs, regression scripts).
    static var binDirectory: URL {
        if let custom = ProcessInfo.processInfo.environment["SUITE42_CPROCSP_BIN"], !custom.isEmpty {
            return URL(fileURLWithPath: custom)
        }
        return URL(fileURLWithPath: "/opt/cprocsp/bin")
    }

    static func tool(_ name: String) -> URL { binDirectory.appendingPathComponent(name) }

    static var isInstalled: Bool { FileManager.default.isExecutableFile(atPath: tool("cryptcp").path) }

    static let downloadURL = URL(string: "https://www.cryptopro.ru/products/csp/downloads")!
    static let testCAURL = URL(string: "https://testgost2012.cryptopro.ru/certsrv/")!

    static var installHelp: String {
        L("GOST signatures use CryptoPro CSP 5.0 for macOS, which PDF42 does not include. Download it from cryptopro.ru (free registration), install the package with administrator rights and restart PDF42. A new install runs for 90 days on a trial licence; after that, enter a purchased serial number. Install your certificate with its key container (token or folder), and the root certificate of your CA.")
    }

    enum Failure: LocalizedError {
        case notInstalled
        case failed(String)
        var errorDescription: String? {
            switch self {
            case .notInstalled: return L("CryptoPro CSP is not installed.") + " " + CryptoPro.installHelp
            case .failed(let s): return s
            }
        }
    }

    // MARK: - Certificates and containers

    struct Certificate {
        var thumbprint: String
        var subject: String
        var issuer: String
        var notAfter: String
        var container: String?
        var hasPrivateKey: Bool
        /// Parsed certificate (exported from the store), when available.
        var info: CertificateInfo?

        var name: String { info?.personName ?? info?.commonName ?? Self.field("CN", in: subject) ?? subject }
        var displayName: String {
            var s = info?.displayName ?? name
            if let snils = info?.snils { s += " · " + L("SNILS %@", snils) }
            else if let inn = info?.inn ?? info?.innLE { s += " · " + L("INN %@", inn) }
            return s
        }

        static func field(_ key: String, in dn: String) -> String? {
            for part in dn.components(separatedBy: ", ") {
                let kv = part.split(separator: "=", maxSplits: 1)
                if kv.count == 2, kv[0].trimmingCharacters(in: .whitespaces) == key { return String(kv[1]) }
            }
            return nil
        }
    }

    /// Certificates in the personal store (uMy). With `signingOnly`, only those linked to a private key.
    static func certificates(signingOnly: Bool = true) throws -> [Certificate] {
        guard isInstalled else { throw Failure.notInstalled }
        let out = try run("certmgr", ["-list", "-store", "uMy"], allowFailure: true).output
        var list = parseCertificateList(out)
        if signingOnly { list = list.filter(\.hasPrivateKey) }
        for i in list.indices { list[i].info = exportCertificate(thumbprint: list[i].thumbprint) }
        return list
    }

    /// Parses `certmgr -list` output (English or Russian labels).
    static func parseCertificateList(_ text: String) -> [Certificate] {
        var result: [Certificate] = []
        var fields: [String: String] = [:]
        func flush() {
            defer { fields = [:] }
            guard let tp = fields["thumbprint"], tp.count == 40 else { return }
            let link = (fields["link"] ?? "").lowercased()
            result.append(Certificate(thumbprint: tp, subject: fields["subject"] ?? "", issuer: fields["issuer"] ?? "",
                                      notAfter: fields["notAfter"] ?? "", container: fields["container"],
                                      hasPrivateKey: link.hasPrefix("yes") || link.hasPrefix("есть") || link.hasPrefix("да"), info: nil))
        }
        let keys: [(String, [String])] = [
            ("thumbprint", ["sha1 hash", "sha1 thumbprint", "sha1 отпечаток", "отпечаток sha1"]),
            ("subject", ["subject", "субъект", "владелец"]),
            ("issuer", ["issuer", "издатель", "кем выдан"]),
            ("notAfter", ["not valid after", "истекает", "действителен по"]),
            ("link", ["privatekey link", "ссылка на ключ", "private key link"]),
            ("container", ["container", "контейнер"]),
        ]
        for raw in text.components(separatedBy: .newlines) {
            let line = raw.trimmingCharacters(in: .whitespaces)
            // Entries start with "1-------", "2-------" …
            if line.range(of: #"^\d+-{3,}"#, options: .regularExpression) != nil { flush(); continue }
            if line.hasPrefix("=====") { flush(); continue }
            guard let colon = line.firstIndex(of: ":") else { continue }
            let label = line[..<colon].trimmingCharacters(in: .whitespaces).lowercased()
            let value = line[line.index(after: colon)...].trimmingCharacters(in: .whitespaces)
            guard let key = keys.first(where: { $0.1.contains(label) })?.0 else { continue }
            if key == "thumbprint" {
                var hex = value.lowercased().hasPrefix("0x") ? String(value.dropFirst(2)) : value
                hex = hex.filter(\.isHexDigit).uppercased()
                fields[key] = hex
            } else {
                fields[key] = value
            }
        }
        flush()
        return result
    }

    static func exportCertificate(thumbprint: String) -> CertificateInfo? {
        let dir = temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: dir) }
        let dest = dir.appendingPathComponent("cert.cer")
        _ = try? run("certmgr", ["-export", "-store", "uMy", "-thumbprint", thumbprint.lowercased(), "-dest", dest.path], allowFailure: true)
        guard let data = try? Data(contentsOf: dest) else { return nil }
        return CertificateInfo(der: CMSInfo.decodeArmour(data))
    }

    /// Key containers visible to CSP (folders, tokens), as fully qualified names.
    static func containers() throws -> [String] {
        guard isInstalled else { throw Failure.notInstalled }
        let out = try run("csptest", ["-keyset", "-enum_cont", "-fqcn", "-verifyc", "-uniq"], allowFailure: true).output
        return out.components(separatedBy: .newlines).compactMap { line in
            let l = line.trimmingCharacters(in: .whitespaces)
            guard l.hasPrefix("\\\\.\\") else { return nil }
            return String(l.split(separator: "|").first ?? Substring(l))
        }
    }

    /// Installs the certificate stored in a key container into the personal store, linked to the key.
    static func installCertificate(container: String, pin: String?) throws {
        var args = ["-inst", "-store", "uMy", "-cont", container, "-silent"]
        if let pin, !pin.isEmpty { args += ["-pin", pin] }
        let r = try run("certmgr", args, allowFailure: true)
        guard r.errorCode == 0 || (r.errorCode == nil && r.status == 0) else { throw Failure.failed(summary(r.output)) }
    }

    /// Product version and licence, for the status line (best effort).
    static func versionInfo() -> String? {
        guard isInstalled, let r = try? run("csptest", ["-keyset", "-verifycontext"], allowFailure: true) else { return nil }
        if let range = r.output.range(of: #"Ver:\s*[0-9.]+"#, options: .regularExpression) {
            return String(r.output[range]).replacingOccurrences(of: "Ver:", with: "").trimmingCharacters(in: .whitespaces)
        }
        return nil
    }

    static func licenseInfo() -> String? {
        let cpconfig = binDirectory.deletingLastPathComponent().appendingPathComponent("sbin/cpconfig")
        guard FileManager.default.isExecutableFile(atPath: cpconfig.path),
              let r = try? run(cpconfig, ["-license", "-view"], allowFailure: true) else { return nil }
        let lines = r.output.components(separatedBy: .newlines).map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { !$0.isEmpty && !$0.hasPrefix("[") }
        return lines.isEmpty ? nil : lines.joined(separator: "\n")
    }

    // MARK: - Signing and verification

    struct SignOptions {
        var thumbprint: String
        /// Container PIN; nil lets CSP ask (or use a saved PIN).
        var pin: String?
        /// Time-stamp server for CAdES-T; nil gives CAdES-BES.
        var tsa: URL?
    }

    /// Detached CAdES signature (DER) of a file.
    static func signDetached(file: URL, to signature: URL, options: SignOptions) throws {
        guard isInstalled else { throw Failure.notInstalled }
        var args = ["-sign", "-detached", "-der", "-thumbprint", options.thumbprint.lowercased()]
        if let tsa = options.tsa { args += ["-cadest", "-cadestsa", tsa.absoluteString] } else { args += ["-cadesbes"] }
        if let pin = options.pin, !pin.isEmpty { args += ["-pin", pin] }
        try? FileManager.default.removeItem(at: signature)
        let r = try run("cryptcp", args + [file.path, signature.path], allowFailure: true)
        guard r.errorCode == 0 || (r.errorCode == nil && r.status == 0),
              let size = (try? signature.resourceValues(forKeys: [.fileSizeKey]))?.fileSize, size > 0 else {
            throw Failure.failed(L("CryptoPro could not sign: %@", summary(r.output)))
        }
    }

    static func signDetached(_ data: Data, options: SignOptions) throws -> Data {
        let dir = temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: dir) }
        let input = dir.appendingPathComponent("data.bin"), output = dir.appendingPathComponent("data.sig")
        try data.write(to: input)
        try signDetached(file: input, to: output, options: options)
        return try Data(contentsOf: output)
    }

    struct Verification {
        enum Status { case valid, untrusted, invalid }
        var status: Status
        /// CSP's explanation (error text or chain problem).
        var message: String?
    }

    static func verifyDetached(file: URL, signature: URL) throws -> Verification {
        guard isInstalled else { throw Failure.notInstalled }
        let base = ["-verify", "-verall", "-detached"]
        let full = try run("cryptcp", base + [file.path, "-f", signature.path], allowFailure: true)
        if full.errorCode == 0 { return Verification(status: .valid, message: nil) }
        let problem = summary(full.output)
        // Signature mathematics without the chain: tells a damaged signature from an untrusted certificate.
        let math = try run("cryptcp", base + ["-nochain", file.path, "-f", signature.path], allowFailure: true)
        if math.errorCode == 0 {
            return Verification(status: .untrusted, message: problem)
        }
        return Verification(status: .invalid, message: summary(math.output))
    }

    static func verifyDetached(_ data: Data, signature: Data) throws -> Verification {
        let dir = temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: dir) }
        let input = dir.appendingPathComponent("data.bin"), sig = dir.appendingPathComponent("data.sig")
        try data.write(to: input)
        try signature.write(to: sig)
        return try verifyDetached(file: input, signature: sig)
    }

    // MARK: - Running the tools

    struct Result {
        var status: Int32
        var output: String
        /// Value of "[ErrorCode: 0x…]" in the output.
        var errorCode: UInt32? {
            guard let r = output.range(of: #"\[ErrorCode:\s*0x([0-9A-Fa-f]+)\]"#, options: .regularExpression) else { return nil }
            let hex = output[r].split(separator: "x").last?.filter(\.isHexDigit) ?? ""
            return UInt32(hex, radix: 16)
        }
    }

    static func run(_ name: String, _ args: [String], allowFailure: Bool = false, timeout: TimeInterval = 120) throws -> Result {
        try run(tool(name), args, allowFailure: allowFailure, timeout: timeout)
    }

    static func run(_ executable: URL, _ args: [String], allowFailure: Bool = false, timeout: TimeInterval = 120) throws -> Result {
        guard FileManager.default.isExecutableFile(atPath: executable.path) else { throw Failure.notInstalled }
        let p = Process()
        p.executableURL = executable
        p.arguments = args
        var env = ProcessInfo.processInfo.environment
        env["LANG"] = "en_US.UTF-8"
        env["LC_ALL"] = "en_US.UTF-8"
        p.environment = env
        let pipe = Pipe()
        p.standardOutput = pipe
        p.standardError = pipe
        p.standardInput = FileHandle.nullDevice
        var collected = Data()
        let group = DispatchGroup()
        group.enter()
        DispatchQueue.global().async {
            collected = pipe.fileHandleForReading.readDataToEndOfFile()
            group.leave()
        }
        try p.run()
        if group.wait(timeout: .now() + timeout) == .timedOut {
            p.terminate()
            _ = group.wait(timeout: .now() + 5)
            throw Failure.failed(L("%@ did not finish in %d seconds (waiting for a PIN or a token?)", executable.lastPathComponent, Int(timeout)))
        }
        p.waitUntilExit()
        let text = String(data: collected, encoding: .utf8) ?? String(data: collected, encoding: .windowsCP1251) ?? ""
        let result = Result(status: p.terminationStatus, output: text)
        if !allowFailure, p.terminationStatus != 0 { throw Failure.failed(summary(text)) }
        return result
    }

    /// The informative lines of a tool's output (no banner, no progress dots).
    static func summary(_ output: String) -> String {
        let lines = output.components(separatedBy: .newlines).map { $0.trimmingCharacters(in: .whitespaces) }.filter { l in
            !l.isEmpty && !l.hasPrefix("CryptCP") && !l.hasPrefix("Certmgr") && !l.contains("(c) \"Crypto-Pro\"")
                && !l.hasPrefix("Utility for") && !l.hasPrefix("program for") && !l.hasPrefix("=====")
                && l.range(of: #"^[.\s]+$"#, options: .regularExpression) == nil && !l.hasPrefix("Folder") && !l.hasPrefix("Папка")
        }
        return lines.suffix(6).joined(separator: "\n")
    }

    static func temporaryDirectory() -> URL {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("PDF42-GOST-" + UUID().uuidString)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }
}
