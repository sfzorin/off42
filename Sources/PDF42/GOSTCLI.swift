import Foundation
import Kit42
import PDFKit

/// Command-line GOST signing through CryptoPro CSP:
///   PDF42 --gost-status
///   PDF42 --gost-certs [all]                      certificates in uMy (with a key unless "all") and key containers
///   PDF42 --gost-sign in.pdf out.pdf --thumbprint SHA1 [--pin PIN] [--poa poa.xml] [--tsa URL] [--reason TEXT] [--visible]
///   PDF42 --gost-sign-file file [--out file.sig] --thumbprint SHA1 [--pin PIN] [--tsa URL]
///   PDF42 --gost-verify file.pdf                  every signature in the PDF (GOST through CSP, others through macOS)
///   PDF42 --gost-verify file --sig file.sig [--poa poa.xml]
///   PDF42 --poa-check poa.xml [--sig document.sig] [--date YYYY-MM-DD]
/// Exit codes for verification: 0 all valid, 1 invalid, 2 intact but certificate not trusted, 3 not checked (no CSP),
/// 4 power-of-attorney mismatch.
enum GOSTCLI {
    static func run(_ args: [String]) -> Int32? {
        guard args.count >= 2, args[1].hasPrefix("--gost-") || args[1] == "--poa-check" else { return nil }
        let a = Array(args.dropFirst(2))
        func option(_ name: String) -> String? {
            guard let i = a.firstIndex(of: name), i + 1 < a.count else { return nil }
            return a[i + 1]
        }
        let positional = positionals(a)
        do {
            switch args[1] {
            case "--gost-status":
                return status()
            case "--gost-certs":
                return try listCertificates(all: a.contains("all"))
            case "--gost-sign":
                guard positional.count >= 2, let tp = option("--thumbprint") else { return usage() }
                return try signPDF(input: positional[0], output: positional[1], thumbprint: tp, pin: option("--pin"),
                                   poa: option("--poa"), tsa: option("--tsa"), reason: option("--reason") ?? "", visible: a.contains("--visible"))
            case "--gost-sign-file":
                guard let file = positional.first, let tp = option("--thumbprint") else { return usage() }
                let out = URL(fileURLWithPath: option("--out") ?? file + ".sig")
                let tsa = option("--tsa").flatMap(URL.init(string:))
                try CryptoPro.signDetached(file: URL(fileURLWithPath: file), to: out,
                                           options: CryptoPro.SignOptions(thumbprint: tp, pin: option("--pin"), tsa: tsa))
                let size = (try? out.resourceValues(forKeys: [.fileSizeKey]))?.fileSize ?? 0
                print("signed: \(out.path) (\(size) bytes, \(tsa == nil ? "CAdES-BES" : "CAdES-T"))")
                return 0
            case "--gost-verify":
                guard let file = positional.first else { return usage() }
                if let sig = option("--sig") { return try verifyDetached(file: file, signature: sig, poa: option("--poa")) }
                return verifyPDF(file)
            case "--poa-check":
                guard let file = positional.first else { return usage() }
                return try checkPowerOfAttorney(file, signature: option("--sig"), date: option("--date"))
            default:
                return usage()
            }
        } catch {
            fputs("error: \(error.localizedDescription)\n", stderr)
            return 1
        }
    }

    /// Arguments that are not options or option values.
    static func positionals(_ a: [String]) -> [String] {
        let valued: Set<String> = ["--thumbprint", "--pin", "--poa", "--tsa", "--reason", "--out", "--sig", "--date"]
        var out: [String] = []
        var i = 0
        while i < a.count {
            if valued.contains(a[i]) { i += 2; continue }
            if !a[i].hasPrefix("--"), a[i] != "all" { out.append(a[i]) }
            i += 1
        }
        return out
    }

    static func usage() -> Int32 {
        fputs("""
        usage: PDF42 --gost-status | --gost-certs [all]
               PDF42 --gost-sign in.pdf out.pdf --thumbprint SHA1 [--pin PIN] [--poa poa.xml] [--tsa URL] [--reason TEXT] [--visible]
               PDF42 --gost-sign-file file [--out file.sig] --thumbprint SHA1 [--pin PIN] [--tsa URL]
               PDF42 --gost-verify file.pdf | --gost-verify file --sig file.sig [--poa poa.xml]
               PDF42 --poa-check poa.xml [--sig document.sig] [--date YYYY-MM-DD]

        """, stderr)
        return 64
    }

    static func status() -> Int32 {
        print("tools: \(CryptoPro.binDirectory.path)")
        guard CryptoPro.isInstalled else {
            print("CryptoPro CSP: not installed")
            print(CryptoPro.installHelp)
            print("download: \(CryptoPro.downloadURL.absoluteString)")
            print("test CA: \(CryptoPro.testCAURL.absoluteString)")
            return 3
        }
        print("CryptoPro CSP: installed, version \(CryptoPro.versionInfo() ?? "unknown")")
        if let license = CryptoPro.licenseInfo() { print("licence:\n\(license)") }
        return 0
    }

    static func listCertificates(all: Bool) throws -> Int32 {
        let certs = try CryptoPro.certificates(signingOnly: !all)
        print("certificates: \(certs.count)")
        for c in certs {
            print("\(c.thumbprint) | \(c.displayName) | key=\(c.hasPrivateKey ? "yes" : "no") | until \(c.notAfter) | \(c.container ?? "-")")
            if let i = c.info { print("    issuer: \(i.issuerName) | \(GOST.algorithmName(i.publicKeyAlgorithm))") }
        }
        let containers = try CryptoPro.containers()
        print("containers: \(containers.count)")
        containers.forEach { print("    \($0)") }
        return 0
    }

    static func signPDF(input: String, output: String, thumbprint: String, pin: String?, poa: String?, tsa: String?,
                        reason: String, visible: Bool) throws -> Int32 {
        guard CryptoPro.isInstalled else { throw CryptoPro.Failure.notInstalled }
        let data = try Data(contentsOf: URL(fileURLWithPath: input))
        // Same normalisation as the UI: PDFKit rewrite (classic xref) unless the file is already signed.
        var base = data
        if PDFSignatureVerifier.verify(data).isEmpty {
            guard let pdf = PDFDocument(data: data), !pdf.isLocked, pdf.pageCount > 0 else { throw PDFStructureError.malformed("cannot open \(input)") }
            base = try PDFMainCLI.signingData(data, pdf: pdf)
        }
        let all = try CryptoPro.certificates(signingOnly: false)
        let cert = all.first { $0.thumbprint.caseInsensitiveCompare(thumbprint) == .orderedSame }
            ?? CryptoPro.Certificate(thumbprint: thumbprint.uppercased(), subject: "", issuer: "", notAfter: "", container: nil, hasPrivateKey: true, info: nil)
        if let poa {
            let url = URL(fileURLWithPath: poa)
            let xml = try Data(contentsOf: url)
            guard PowerOfAttorney(xml: xml) != nil else { throw PDFStructureError.unsupported("\(poa) is not a power of attorney (МЧД)") }
            var files = [EmbeddedFile(name: url.lastPathComponent, data: xml, desc: "Power of attorney (МЧД)", modified: Date(), mime: "application/xml")]
            if let sig = PowerOfAttorney.signatureFile(next: url) {
                files.append(EmbeddedFile(name: sig.lastPathComponent, data: try Data(contentsOf: sig), desc: "Signature of the power of attorney",
                                          modified: Date(), mime: "application/pkcs7-signature"))
            }
            base = try PDFSigner.embed(files, into: base)
            print("attached: \(files.map(\.name).joined(separator: ", "))")
        }
        var options = PDFSigner.Options(signer: .cryptoPro(cert, CryptoPro.SignOptions(thumbprint: thumbprint, pin: pin)),
                                        reason: reason, location: "")
        options.timestampURL = tsa.flatMap(URL.init(string:))
        if visible {
            let size = CGSize(width: 220, height: 72)
            guard let appearance = EditorWindowController.signatureAppearance(size: size, image: nil, name: cert.name, reason: reason, date: Date()) else {
                throw NSError(domain: NSCocoaErrorDomain, code: NSFileWriteUnknownError)
            }
            options.visible = (0, CGRect(x: 340, y: 40, width: size.width, height: size.height), appearance)
        }
        let signed = try PDFSigner.sign(base, options: options)
        try signed.write(to: URL(fileURLWithPath: output), options: .atomic)
        print("signed: \(output) (\(signed.count) bytes)")
        return 0
    }

    static func verifyPDF(_ path: String) -> Int32 {
        guard let data = try? Data(contentsOf: URL(fileURLWithPath: path)) else { fputs("cannot read \(path)\n", stderr); return 1 }
        let files = PDFDocument(data: data).map { DocumentExtras.read(data, pdf: $0).files } ?? []
        let results = PDFSignatureVerifier.verify(data, attachments: files)
        if results.isEmpty { print("no signatures"); return 1 }
        for (i, s) in results.enumerated() { printSignature(s, index: i + 1) }
        return exitCode(results)
    }

    static func verifyDetached(file: String, signature: String, poa: String?) throws -> Int32 {
        let sigData = try Data(contentsOf: URL(fileURLWithPath: signature))
        let parsed = CMSInfo(sigData)
        var info = SignatureInfo()
        info.coversWholeFile = true
        PDFSignatureVerifier.describe(parsed, into: &info)
        info.isGOST = parsed?.isGOST ?? false
        if CryptoPro.isInstalled {
            let v = try CryptoPro.verifyDetached(file: URL(fileURLWithPath: file), signature: URL(fileURLWithPath: signature))
            switch v.status {
            case .valid: info.status = .valid
            case .untrusted: info.status = .untrusted; info.trustMessage = v.message
            case .invalid: info.status = .invalid; info.problem = L("The document was changed after signing") + (v.message.map { "\n" + $0 } ?? "")
            }
        } else {
            info.status = .notChecked
        }
        if let poa {
            let url = URL(fileURLWithPath: poa)
            let xml = try Data(contentsOf: url)
            guard let p = PowerOfAttorney(xml: xml) else { throw PDFStructureError.unsupported("\(poa) is not a power of attorney (МЧД)") }
            let poaSig = PowerOfAttorney.signatureFile(next: url).flatMap { try? Data(contentsOf: $0) }
            info.powerOfAttorney = p.check(signer: info.certificate, signedAt: info.date, xml: xml, poaSignature: poaSig)
        }
        printSignature(info, index: 1)
        return exitCode([info])
    }

    static func checkPowerOfAttorney(_ path: String, signature: String?, date: String?) throws -> Int32 {
        let url = URL(fileURLWithPath: path)
        let xml = try Data(contentsOf: url)
        guard let p = PowerOfAttorney(xml: xml) else { print("not a power of attorney (МЧД)"); return 1 }
        print("format: \(p.format)")
        print("number: \(p.number)")
        print("issued: \(p.issued.map(day) ?? "-") expires: \(p.expires.map(day) ?? "-")")
        print("principal: \(p.principalName ?? "-") INN \(p.principalINN ?? "-") OGRN \(p.principalOGRN ?? "-")")
        print("representative: \(p.representativeName ?? "-") SNILS \(p.representativeSNILS ?? "-") INN \(p.representativeINN ?? "-")")
        p.powers.forEach { print("power: \($0)") }
        let sigInfo = signature.flatMap { try? Data(contentsOf: URL(fileURLWithPath: $0)) }.flatMap(CMSInfo.init)
        let signer = sigInfo?.signers.first
        let when = date.flatMap(PowerOfAttorney.date) ?? signer?.signingTime
        let findings = p.check(signer: signer?.certificate, signedAt: when, xml: xml,
                               poaSignature: PowerOfAttorney.signatureFile(next: url).flatMap { try? Data(contentsOf: $0) })
        findings.forEach { print("  \(mark($0.level)) \($0.text)") }
        return findings.contains { $0.level == .error } ? 4 : 0
    }

    static func printSignature(_ s: SignatureInfo, index: Int) {
        print("#\(index) \(s.status) | \(s.signer) | gost=\(s.isGOST) | whole=\(s.coversWholeFile) | \(s.summary)")
        if let d = s.date { print("    signed: \(ISO8601DateFormatter().string(from: d))") }
        if let r = s.reason { print("    reason: \(r)") }
        s.details.forEach { print("    \($0)") }
        if let t = s.trustMessage, s.status != .valid { print("    trust: \(t.replacingOccurrences(of: "\n", with: " / "))") }
        if let p = s.problem { print("    problem: \(p.replacingOccurrences(of: "\n", with: " / "))") }
        if !s.powerOfAttorney.isEmpty {
            print("    power of attorney:")
            s.powerOfAttorney.forEach { print("      \(mark($0.level)) \($0.text)") }
        }
    }

    static func mark(_ level: PowerOfAttorney.Finding.Level) -> String {
        switch level { case .ok: return "ok"; case .warning: return "warning"; case .error: return "ERROR"; case .info: return "-" }
    }

    static func exitCode(_ results: [SignatureInfo]) -> Int32 {
        if results.contains(where: { $0.status == .invalid }) { return 1 }
        if results.contains(where: { $0.powerOfAttorney.contains { $0.level == .error } }) { return 4 }
        if results.contains(where: { $0.status == .notChecked }) { return 3 }
        if results.contains(where: { $0.status == .untrusted }) { return 2 }
        return 0
    }

    static func day(_ d: Date) -> String {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.timeZone = TimeZone(identifier: "Europe/Moscow")
        f.dateFormat = "yyyy-MM-dd"
        return f.string(from: d)
    }
}
