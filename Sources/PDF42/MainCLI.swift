import AppKit
import Kit42
import PDFKit

/// Headless entry points report failure and replace destinations only after successful encoding.
enum PDFMainCLI {
    static func run(_ args: [String]) -> Int32? {
        guard args.count > 1, ["--compress", "--verify", "--sign", "--export", "--images-to-pdf", "--ocr"].contains(args[1]) else { return nil }
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
        do {
            switch args[1] {
            case "--verify":
                try require(args.count == 3, "Usage: --verify file.pdf")
                let (data, _) = try input(args[2])
                for s in PDFSignatureVerifier.verify(data) {
                    print("\(s.status) | \(s.signer) | whole=\(s.coversWholeFile) | \(s.reason ?? "-") | ts=\(s.timestamp.map { "\($0)" } ?? "none") | \(s.trustMessage ?? "")")
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
            default:
                try require((6...8).contains(args.count) && args.dropFirst(6).allSatisfy { ["visible", "tsa"].contains($0) },
                            "Usage: --sign in.pdf identity.p12 password out.pdf [visible] [tsa]")
                let (data, pdf) = try input(args[2])
                let identity = try CertificateStore.importPKCS12(Data(contentsOf: URL(fileURLWithPath: args[3])), password: args[4], toKeychain: false)
                guard let normalized = pdf.dataRepresentation() else { throw NSError(domain: NSCocoaErrorDomain, code: NSFileWriteUnknownError) }
                let base = PDFSignatureVerifier.verify(data).isEmpty ? DocumentWriter.apply(normalized, extras: DocumentExtras.read(data, pdf: pdf), pdf: pdf) : data
                var options = PDFSigner.Options(identity: identity, reason: "Signature test", location: "Moscow")
                if args.contains("visible") {
                    let size = CGSize(width: 220, height: 72)
                    guard let appearance = EditorWindowController.signatureAppearance(size: size, image: nil, name: identity.name, reason: "Signature test", date: Date()) else {
                        throw NSError(domain: NSCocoaErrorDomain, code: NSFileWriteUnknownError)
                    }
                    options.visible = (0, CGRect(x: 340, y: 40, width: size.width, height: size.height), appearance)
                }
                if args.contains("tsa") { options.timestampURL = URL(string: "http://timestamp.digicert.com") }
                let signed = try PDFSigner.sign(base, options: options)
                try signed.write(to: URL(fileURLWithPath: args[5]), options: .atomic)
                print("signed \(signed.count) bytes")
            }
            return 0
        } catch {
            FileHandle.standardError.write(Data((error.localizedDescription + "\n").utf8))
            return 1
        }
    }
}
