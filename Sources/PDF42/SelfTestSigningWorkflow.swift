import AppKit
import CryptoKit
import Kit42
import PDFKit

/// Offline regressions for the signing workflow: handwritten appearance, batch signing, verification report
/// and the certificate of completion (QR round trip, hash prefix, signatures preserved).
extension PDFSelfTest {
    static func testSignatureAppearanceUsesFreshImpressions() throws {
        let mark = GeneratedMark.signature("Ivan Petrov", ink: InkColor.blue.pen, design: 12345)
        let a = try unwrap(mark.impression()), b = try unwrap(mark.impression())
        check(pixels(a) != pixels(b), "two impressions of a generated signature must differ")
        let details = SignatureAppearance.Details(name: "Ivan Petrov", reason: "Approved", certificate: try TestSigner(name: "Appearance").chain[0],
                                                  level: .baselineT)
        let size = CGSize(width: 240, height: 80)
        let withPen = try unwrap(SignatureAppearance.render(size: size, handwriting: a, details: details))
        let textOnly = try unwrap(SignatureAppearance.render(size: size, handwriting: nil, details: details))
        equal(withPen.width, 720)
        check(pixels(withPen) != pixels(textOnly), "the handwriting must be drawn")
        // Same layout with a blank picture: the difference on the left half is the pen signature.
        let blankCtx = CGContext(data: nil, width: a.width, height: a.height, bitsPerComponent: 8, bytesPerRow: 0,
                                 space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
        let blank = try unwrap(SignatureAppearance.render(size: size, handwriting: blankCtx.makeImage(), details: details))
        check(inkCoverage(withPen, left: true) > inkCoverage(blank, left: true) + 0.005, "pen signature on the left")
        equal(inkCoverage(withPen, left: false), inkCoverage(blank, left: false), "details column unchanged by the pen")
        let lines = SignatureAppearance.lines(details)
        check(lines.contains { $0.contains("Appearance") }, "issuer line")
        check(lines.contains("PAdES B-T"), "level line")
    }

    static func testBatchSigningSignsEveryFileAndReportsFailures() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("pdf42-batch-\(UUID().uuidString)")
        let out = dir.appendingPathComponent("out")
        try FileManager.default.createDirectory(at: dir.appendingPathComponent("nested"), withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        try minimalPDF().write(to: dir.appendingPathComponent("one.pdf"))
        try minimalPDF().write(to: dir.appendingPathComponent("nested/two.pdf"))
        try Data("not a pdf".utf8).write(to: dir.appendingPathComponent("nested/broken.pdf"))
        try Data("ignored".utf8).write(to: dir.appendingPathComponent("notes.txt"))

        let files = BatchSigner.collect([dir, dir.appendingPathComponent("one.pdf")])
        equal(Set(files.map(\.lastPathComponent)), ["broken.pdf", "two.pdf", "one.pdf"])
        equal(files.count, 3, "folders are walked, duplicates and non-PDFs dropped")

        let signer = try TestSigner(name: "Batch Signer")
        var options = PDFSigner.Options(identity: nil, reason: "Batch", location: "")
        options.remote = signer
        options.level = .baselineB
        var settings = BatchSigner.Settings(options: options, outputFolder: out, suffix: "signed", visiblePage: .last)
        settings.generatedMark = .signature("Batch Signer", ink: InkColor.blue.pen, design: 7)
        var seen: [String] = []
        let outcomes = BatchSigner.run(files, settings: settings) { _, url in seen.append(url.lastPathComponent); return true }
        equal(seen.count, 3)
        equal(outcomes.filter(\.succeeded).count, 2)
        check(outcomes.first { $0.input.lastPathComponent == "broken.pdf" }?.error != nil, "a damaged file is reported, not fatal")
        var appearances: [[UInt8]] = []
        for o in outcomes where o.succeeded {
            let url = try unwrap(o.output)
            check(url.lastPathComponent.hasSuffix(" (signed).pdf"), url.lastPathComponent)
            let data = try Data(contentsOf: url)
            let infos = PDFSignatureVerifier.verify(data)
            equal(infos.count, 1)
            check(infos.first?.status != .invalid && infos.first?.coversWholeFile == true, infos.first?.problem ?? "invalid")
            equal(infos.first?.level, "PAdES B-B")
            // Visible widget with an image appearance.
            let file = try PDFFileStructure(data)
            let images = file.offsets.keys.compactMap { try? file.rawObject($0) }.filter { $0.0["Subtype"]?.nameValue == "Image" && $0.0["SMask"] != nil }
            equal(images.count, 1, "one appearance image per file")
            if let raw = images.first?.1 { appearances.append(raw) }
        }
        check(appearances.count == 2 && appearances[0] != appearances[1], "each file gets its own impression")
        // Never overwrites: a second run makes numbered copies.
        let again = BatchSigner.run(files.filter { $0.lastPathComponent == "one.pdf" }, settings: settings)
        check(again.first?.output?.lastPathComponent.hasSuffix("(signed) 2.pdf") == true, again.first?.output?.lastPathComponent ?? "no output")
        // Cancel stops before the next file.
        let stopped = BatchSigner.run(files, settings: settings) { i, _ in i < 1 }
        equal(stopped.count, 1)
        check(BatchSigner.summary(stopped, total: 3).contains("2"), "summary counts unprocessed files")
    }

    static func testVerificationReportListsSignatureDetails() throws {
        let signer = try TestSigner(name: "Report Signer")
        var options = PDFSigner.Options(identity: nil, reason: "Report reason", location: "Ankara")
        options.remote = signer
        options.level = .baselineB
        let signed = try PAdES.sign(minimalPDF(), options: options).data
        let model = VerificationReport.model(signed, fileName: "signed.pdf")
        equal(model.signatures.count, 1)
        equal(model.entries.first?.revision, 1)
        equal(model.entries.first?.chain.first?.certificate, signer.chain[0])
        equal(model.sha256, DER.hex([UInt8](SHA256.hash(data: signed))))
        let pdf = try unwrap(PDFDocument(data: VerificationReport.render(model)))
        let text = (0..<pdf.pageCount).compactMap { pdf.page(at: $0)?.string }.joined(separator: "\n")
        for needle in ["Report Signer", "PAdES B-B", "Report reason", "Ankara", "ETSI.CAdES.detached", signer.chain[0].serialHex,
                       String(model.sha256.prefix(8))] {
            check(text.contains(needle), "report text lacks \(needle)")
        }
        // An unsigned file still gets a report.
        let empty = try unwrap(PDFDocument(data: VerificationReport.render(VerificationReport.model(minimalPDF(), fileName: "plain.pdf"))))
        check(empty.pageCount == 1, "report for an unsigned file")
    }

    static func testCompletionCertificateRoundTrip() throws {
        let signer = try TestSigner(name: "Completion Signer")
        var options = PDFSigner.Options(identity: nil, reason: "", location: "")
        options.remote = signer
        options.level = .baselineB
        let signed = try PAdES.sign(minimalPDF(), options: options).data
        let completed = try CompletionCertificate.append(to: signed, fileName: "done.pdf")

        // The signed file is an untouched prefix; the page is added; the signature stays intact for its revision.
        check(completed.prefix(signed.count) == signed, "incremental: original bytes kept")
        equal(PDFDocument(data: completed)?.pageCount, 2)
        let infos = PDFSignatureVerifier.verify(completed)
        equal(infos.count, 1)
        check(infos.first?.status != .invalid && infos.first?.coversWholeFile == false, infos.first?.problem ?? "invalid")
        check(PDFDocument(data: completed)?.page(at: 1)?.string?.contains("Completion Signer") == true, "signer on the page")

        // QR code read back from the rendered page.
        let check1 = try unwrap(CompletionCertificate.check(completed))
        equal(check1.source, .qr)
        check(check1.matches, "hash matches")
        equal(check1.payload.length, signed.count)
        equal(check1.payload.sha256, DER.hex([UInt8](SHA256.hash(data: signed))).lowercased())
        equal(check1.payload.signers.map(\.name), ["Completion Signer"])

        // A changed byte in the covered part is detected.
        var tampered = [UInt8](completed)
        let i = try unwrap(PDFSigner.find("MediaBox [0 0 200", in: tampered, from: 0))
        tampered[i + 15] = UInt8(ascii: "3")
        check(CompletionCertificate.check(Data(tampered))?.matches == false, "tampering detected")

        // Payload text round trip, including the printed one-line fallback.
        let p = check1.payload
        equal(CompletionCertificate.Payload.parse(p.text)?.sha256, p.sha256)
        equal(CompletionCertificate.Payload.parse("x " + p.machineLine + " y")?.length, p.length)
        check(CompletionCertificate.Payload.parse("PDF42-COC:1 L:10 H:zz") == nil, "malformed hash rejected")

        // Unsigned documents work too.
        let plain = try CompletionCertificate.append(to: minimalPDF(), fileName: "plain.pdf")
        check(CompletionCertificate.check(plain)?.matches == true, "unsigned document")
    }

    private static func pixels(_ image: CGImage) -> [UInt8] {
        var data = [UInt8](repeating: 0, count: image.width * image.height * 4)
        let ctx = CGContext(data: &data, width: image.width, height: image.height, bitsPerComponent: 8, bytesPerRow: image.width * 4,
                            space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
        ctx.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
        return data
    }

    /// Share of opaque pixels in the left or right half.
    private static func inkCoverage(_ image: CGImage, left: Bool) -> Double {
        let p = pixels(image), w = image.width, h = image.height
        var ink = 0
        for y in 0..<h { for x in (left ? 0..<w / 2 : w / 2..<w) where p[(y * w + x) * 4 + 3] > 40 { ink += 1 } }
        return Double(ink) / Double(w * h / 2)
    }
}
