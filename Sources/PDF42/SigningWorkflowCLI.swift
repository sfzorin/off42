import AppKit
import CryptoKit
import Kit42

/// Command-line signing workflow:
///   PDF42 --batch-sign identity.p12 password OUTDIR FILE-OR-FOLDER… [level=B-B|B-T|B-LT|B-LTA|pkcs7] [tsa | tsa=URL]
///         [visible | visible=first|last] [handwriting=LIBRARY.png | signature-name="Name"] [suffix=signed]
///         [reason=…] [location=…] [completion]
///   PDF42 --verify-report in.pdf report.pdf
///   PDF42 --completion in.pdf out.pdf          (append the certificate of completion)
///   PDF42 --verify-completion file.pdf         (exit 0 when the recorded hash matches)
enum SigningWorkflowCLI {
    static func run(_ args: [String]) -> Int32? {
        guard args.count >= 2 else { return nil }
        func value(_ key: String) -> String? { args.first { $0.hasPrefix(key + "=") }.map { String($0.dropFirst(key.count + 1)) } }
        do {
            switch args[1] {
            case "--batch-sign" where args.count >= 6:
                let identity = try CertificateStore.importPKCS12(try Data(contentsOf: URL(fileURLWithPath: args[2])), password: args[3], toKeychain: false)
                var options = PDFSigner.Options(identity: identity, reason: value("reason") ?? "", location: value("location") ?? "")
                if args.contains("tsa") { options.timestampURL = URL(string: "http://timestamp.digicert.com") }
                if let t = value("tsa") { options.timestampURL = URL(string: t) }
                options.level = value("level").flatMap(PAdESLevel.named) ?? (options.timestampURL != nil ? .baselineT : .baselineB)
                if options.level == .baselineB { options.timestampURL = nil }
                let inputs = args.dropFirst(5).filter { !$0.contains("=") && !["tsa", "visible", "completion"].contains($0) }.map { URL(fileURLWithPath: $0) }
                var settings = BatchSigner.Settings(options: options, outputFolder: URL(fileURLWithPath: args[4]), suffix: value("suffix") ?? "signed")
                if args.contains("visible") { settings.visiblePage = .first }
                if let v = value("visible") { settings.visiblePage = v == "last" ? .last : .first }
                if let path = value("handwriting") { settings.handwriting = URL(fileURLWithPath: path) }
                settings.appendCompletion = args.contains("completion")
                var generated: GeneratedMark?
                if let name = value("signature-name") {
                    generated = .signature(name, ink: InkColor.blue.pen, design: SHA256.hash(data: Data(name.utf8)).prefix(8).reduce(UInt64(0)) { $0 << 8 | UInt64($1) } | 1)
                }
                settings.generatedMark = generated
                let files = BatchSigner.collect(inputs)
                guard !files.isEmpty else { print("no PDF files"); return 1 }
                let outcomes = BatchSigner.run(files, settings: settings) { i, url in
                    print("[\(i + 1)/\(files.count)] \(url.lastPathComponent)")
                    return true
                }
                print(BatchSigner.summary(outcomes, total: files.count))
                return outcomes.allSatisfy(\.succeeded) && outcomes.count == files.count ? 0 : 1
            case "--verify-report" where args.count >= 4:
                let input = URL(fileURLWithPath: args[2])
                let model = VerificationReport.model(try Data(contentsOf: input), fileName: input.lastPathComponent)
                let data = VerificationReport.render(model)
                try data.write(to: URL(fileURLWithPath: args[3]))
                print("report: \(model.signatures.count) signatures, \(model.timestamps.count) document timestamps, \(data.count) bytes")
                return 0
            case "--completion" where args.count >= 4:
                let input = URL(fileURLWithPath: args[2]), output = URL(fileURLWithPath: args[3])
                let data = try CompletionCertificate.append(to: try Data(contentsOf: input), fileName: output.lastPathComponent)
                try data.write(to: output)
                let check = CompletionCertificate.check(data)
                print("completion: \(data.count) bytes, covered=\(check?.payload.length ?? -1), sha256=\(check?.payload.sha256 ?? "-")")
                return 0
            case "--verify-completion" where args.count >= 3:
                let data = try Data(contentsOf: URL(fileURLWithPath: args[2]))
                guard let c = CompletionCertificate.check(data) else { print("no certificate of completion"); return 2 }
                print("\(c.matches ? "MATCH" : "MISMATCH") | source=\(c.source) | length=\(c.payload.length) | sha256=\(c.payload.sha256) | signers=\(c.payload.signers.map(\.name).joined(separator: "; "))")
                return c.matches ? 0 : 1
            default:
                return nil
            }
        } catch {
            print("error: \(error.localizedDescription)")
            return 1
        }
    }
}
