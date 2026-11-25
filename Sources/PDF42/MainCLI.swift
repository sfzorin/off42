import AppKit
import Kit42
import PDFKit

/// Headless entry points report failure and replace destinations only after successful encoding.
enum PDFMainCLI {
    static func run(_ args: [String]) -> Int32? {
        guard args.count > 1, ["--compress", "--verify", "--sign", "--export", "--images-to-pdf", "--ocr",
                                 "--ltv", "--doc-timestamp", "--csc-credentials", "--csc-sign"].contains(args[1]) else { return nil }
        func require(_ value: Bool, _ message: String) throws {
            if !value { throw NSError(domain: "PDF42.CLI", code: 1, userInfo: [NSLocalizedDescriptionKey: message]) }
        }
        func input(_ path: String) throws -> (Data, PDFDocument) {
            let data = try Data(contentsOf: URL(fileURLWithPath: path))
            guard let pdf = PDFDocument(data: data), !pdf.isLocked, pdf.pageCount > 0 else {
                throw NSError(domain: NSCocoaErrorDomain, code: NSFileReadCorruptFileError)
            }
            return (data, pdf)
        }
        let optionStart = args[1] == "--csc-credentials" ? 3 : 6
        let options = Array(args.dropFirst(optionStart))
        func argument(_ key: String) -> String? {
            options.first { $0.hasPrefix(key + "=") }.map { String($0.dropFirst(key.count + 1)) }
        }
        func validateOptions(flags: Set<String> = [], keys: Set<String> = []) throws {
            var seen: Set<String> = []
            for value in options {
                let name = String(value.prefix { $0 != "=" })
                try require(flags.contains(value) || (value.contains("=") && keys.contains(name)), "Unknown option: \(name)")
                try require(seen.insert(name).inserted, "Duplicate option: \(name)")
            }
        }
        func httpURL(_ value: String) throws -> URL {
            guard let url = URL(string: value.trimmingCharacters(in: .whitespacesAndNewlines)),
                  ["http", "https"].contains(url.scheme?.lowercased() ?? ""), url.host?.isEmpty == false else {
                throw PDFStructureError.unsupported("Invalid HTTP or HTTPS URL")
            }
            return url
        }
        func applySignArguments(_ value: inout PDFSigner.Options) throws {
            if options.contains("tsa") { value.timestampURL = try httpURL("http://timestamp.digicert.com") }
            if let tsa = argument("tsa") { value.timestampURL = try httpURL(tsa) }
            if let level = argument("level") {
                guard let named = PAdESLevel.named(level) else { throw PDFStructureError.unsupported("Unknown PAdES level") }
                value.level = named
            } else { value.level = value.timestampURL != nil ? .baselineT : .baselineB }
            if value.level == .baselineB { value.timestampURL = nil }
        }
        func cscClient(_ url: String) throws -> CSCClient {
            var config = CSCClient.Config(baseURL: try httpURL(url))
            config.accessToken = argument("token")
            config.username = argument("user")
            config.password = argument("password")
            config.clientID = argument("client-id")
            config.clientSecret = argument("client-secret")
            if let rawPort = argument("port") {
                guard let port = UInt16(rawPort), port > 0 else { throw PDFStructureError.unsupported("Invalid redirect port") }
                config.redirectPort = port
            }
            config.openURL = { url in
                print("Open to authorize: \(url.absoluteString)")
                fflush(stdout)
                if !options.contains("no-browser") { NSWorkspace.shared.open(url) }
            }
            return CSCClient(config)
        }
        let cscKeys: Set<String> = ["token", "user", "password", "client-id", "client-secret", "port"]
        do {
            switch args[1] {
            case "--verify":
                try require(args.count == 3, "Usage: --verify file.pdf")
                let (data, pdf) = try input(args[2])
                for s in PDFSignatureVerifier.verify(data, attachments: DocumentExtras.read(data, pdf: pdf).files) {
                    let ltv = s.ltvEnabled.map { $0 ? "yes" : "no (\(s.ltvProblem ?? ""))" } ?? "-"
                    print("\(s.isDocumentTimestamp ? "doc-timestamp" : "signature") | \(s.status) | \(s.signer) | whole=\(s.coversWholeFile) | \(s.reason ?? "-") | ts=\(s.timestamp.map { "\($0)" } ?? "none") | level=\(s.level ?? "-") | ltv=\(ltv) | \(s.problem ?? s.trustMessage ?? "")")
                }
            case "--compress":
                try require((4...5).contains(args.count), "Usage: --compress in.pdf out.pdf [smallest|balanced|high]")
                let level = args.count == 5 ? PDFCompressor.Level.named(args[4]) : .balanced
                try require(level != nil, "Unknown compression level")
                let (original, pdf) = try input(args[2])
                let compressed = PDFCompressor.compress(pdf, level: level!, original: original.count)
                let data = compressed.map { DocumentWriter.apply($0, extras: DocumentExtras.read(original, pdf: pdf), pdf: pdf) } ?? original
                let output = data.count < original.count ? data : original
                try output.write(to: URL(fileURLWithPath: args[3]), options: .atomic)
                print("\(original.count) -> \(output.count) bytes")
            case "--export":
                try require(args.count == 4, "Usage: --export in.pdf out.docx|rtf|html|txt")
                let (_, pdf) = try input(args[2]), out = URL(fileURLWithPath: args[3])
                let data = try PDFOutput.textData(pdf, format: out.pathExtension)
                try data.write(to: out, options: .atomic)
                print("\(out.pathExtension): \(data.count) bytes")
            case "--images-to-pdf":
                try require(args.count >= 4, "Usage: --images-to-pdf out.pdf images…")
                let pdf = PDFDocument()
                for path in args.dropFirst(3) {
                    guard let image = NSImage(contentsOfFile: path), let page = PDFDoc.page(from: image) else {
                        throw NSError(domain: NSCocoaErrorDomain, code: NSFileReadCorruptFileError)
                    }
                    pdf.insert(page, at: pdf.pageCount)
                }
                guard let data = pdf.dataRepresentation() else { throw NSError(domain: NSCocoaErrorDomain, code: NSFileWriteUnknownError) }
                try data.write(to: URL(fileURLWithPath: args[2]), options: .atomic)
                print("pages: \(pdf.pageCount)")
            case "--ocr":
                try require(args.count == 4, "Usage: --ocr in.pdf out.pdf")
                let (data, pdf) = try input(args[2])
                let extras = DocumentExtras.read(data, pdf: pdf)
                for i in 0..<pdf.pageCount {
                    if let page = pdf.page(at: i), let recognized = EditorWindowController.ocrPage(page) { PageOverlay.replace([(i, recognized)], in: pdf) }
                }
                guard let output = pdf.dataRepresentation() else { throw NSError(domain: NSCocoaErrorDomain, code: NSFileWriteUnknownError) }
                try DocumentWriter.apply(output, extras: extras, pdf: pdf).write(to: URL(fileURLWithPath: args[3]), options: .atomic)
                print("ocr pages: \(pdf.pageCount)")
            case "--ltv":
                try require(args.count == 4, "Usage: --ltv in.pdf out.pdf")
                let (data, _) = try input(args[2])
                let result = try PAdES.addValidationData(data)
                result.warnings.forEach { print("warning: \($0)") }
                try result.data.write(to: URL(fileURLWithPath: args[3]), options: .atomic)
                print("ltv \(result.data.count) bytes")
            case "--doc-timestamp":
                try require((4...5).contains(args.count), "Usage: --doc-timestamp in.pdf out.pdf [TSA-URL]")
                let (data, pdf) = try input(args[2])
                let signed = !PDFSignatureVerifier.verify(data).isEmpty
                let base = try signingData(data, pdf: pdf)
                let tsa = try httpURL(args.count == 5 ? args[4] : "http://timestamp.digicert.com")
                var result = PAdES.Result(data: try PAdES.addDocumentTimestamp(base, tsa: tsa))
                if signed { result = try PAdES.addValidationData(result.data) }
                result.warnings.forEach { print("warning: \($0)") }
                try result.data.write(to: URL(fileURLWithPath: args[3]), options: .atomic)
                print("timestamped \(result.data.count) bytes")
            case "--csc-credentials":
                try require(args.count >= 3, "Usage: --csc-credentials SERVICE-URL [auth…]")
                try validateOptions(flags: ["no-browser"], keys: cscKeys)
                let client = try cscClient(args[2])
                if let info = try? client.loadInfo() { print("service: \(info["name"] ?? "?") | auth: \(client.authTypes.joined(separator: ","))") }
                for c in try client.listCredentials() {
                    print("\(c.id) | \(c.chain.first?.name ?? "-") | chain=\(c.chain.count) | auth=\(c.authMode) pin=\(c.needsPIN) otp=\(c.needsOTP) | algo=\(c.algorithms.joined(separator: ","))")
                }
            case "--csc-sign":
                try require(args.count >= 6, "Usage: --csc-sign in.pdf out.pdf SERVICE-URL CREDENTIAL-ID [auth…] [pin=…] [otp=…] [tsa=URL] [level=…]")
                try validateOptions(flags: ["no-browser", "tsa"], keys: cscKeys.union(["pin", "otp", "tsa", "level", "reason"]))
                let (data, pdf) = try input(args[2])
                let client = try cscClient(args[4])
                let credential = try client.credential(args[5])
                let signer = CSCSigner(client: client, credential: credential) { _ in (argument("pin"), argument("otp")) }
                var value = PDFSigner.Options(signer: .remote(signer), reason: argument("reason") ?? "Remote signature test", location: "")
                try applySignArguments(&value)
                let signed = try PAdES.sign(signingData(data, pdf: pdf), options: value)
                signed.warnings.forEach { print("warning: \($0)") }
                try signed.data.write(to: URL(fileURLWithPath: args[3]), options: .atomic)
                print("signed remotely \(signed.data.count) bytes, \(value.level.cliName)")
            default:
                try require(args.count >= 6, "Usage: --sign in.pdf identity.p12 password out.pdf [visible] [tsa | tsa=URL] [level=pkcs7|B-B|B-T|B-LT|B-LTA]")
                try validateOptions(flags: ["visible", "tsa"], keys: ["tsa", "level"])
                let (data, pdf) = try input(args[2])
                let identity = try CertificateStore.importPKCS12(Data(contentsOf: URL(fileURLWithPath: args[3])), password: args[4], toKeychain: false)
                let base = try signingData(data, pdf: pdf)
                var value = PDFSigner.Options(identity: identity, reason: "Signature test", location: "Moscow")
                if options.contains("visible") {
                    let size = CGSize(width: 220, height: 72)
                    guard let appearance = EditorWindowController.signatureAppearance(size: size, image: nil, name: identity.name, reason: "Signature test", date: Date()) else {
                        throw NSError(domain: NSCocoaErrorDomain, code: NSFileWriteUnknownError)
                    }
                    value.visible = (0, CGRect(x: 340, y: 40, width: size.width, height: size.height), appearance)
                }
                try applySignArguments(&value)
                let signed = try PAdES.sign(base, options: value)
                signed.warnings.forEach { print("warning: \($0)") }
                try signed.data.write(to: URL(fileURLWithPath: args[5]), options: .atomic)
                print("signed \(signed.data.count) bytes, \(value.level.cliName)")
            }
            return 0
        } catch {
            FileHandle.standardError.write(Data((error.localizedDescription + "\n").utf8))
            return 1
        }
    }

    /// Preserve signed bytes verbatim; retain document metadata, bookmarks, labels and attachments when normalising unsigned input.
    static func signingData(_ data: Data, pdf: PDFDocument) throws -> Data {
        if !PDFSignatureVerifier.verify(data).isEmpty { return data }
        guard let normalized = pdf.dataRepresentation() else {
            throw NSError(domain: NSCocoaErrorDomain, code: NSFileWriteUnknownError)
        }
        return DocumentWriter.apply(normalized, extras: DocumentExtras.read(data, pdf: pdf), pdf: pdf)
    }
}
