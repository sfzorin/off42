import AppKit
import CryptoKit
import Kit42
import PDFKit

/// Offline "certificate of completion": a last page appended as an incremental update, with the SHA-256 of
/// the document as it was (every byte before the page), signer names, signing times and a QR code holding the
/// hash. Nothing is sent anywhere. Because the update is appended, the hashed bytes stay a prefix of the file:
/// anyone can check them with `head -c LENGTH file.pdf | shasum -a 256`, and existing signatures stay valid
/// for their revisions.
enum CompletionCertificate {
    static let magic = "PDF42-COC:1"

    struct Signer: Equatable {
        var name: String
        var time: Date?
        var trustedTime: Bool
        var isTimestamp: Bool
    }

    struct Payload: Equatable {
        var fileName: String
        var length: Int
        var sha256: String
        var created: Date
        var signers: [Signer]

        /// Line-based text stored in the QR code.
        var text: String {
            var lines = [CompletionCertificate.magic, "L:\(length)", "H:\(sha256)", "T:\(iso(created))", "F:\(clip(fileName, 60))"]
            for s in signers.prefix(8) {
                lines.append("S:\(clip(s.name, 40).replacingOccurrences(of: "|", with: "/"))|\(s.time.map(iso) ?? "-")|\(s.isTimestamp ? "D" : s.trustedTime ? "T" : "C")")
            }
            return lines.joined(separator: "\n")
        }

        /// One-line form printed on the page (fallback when the QR code cannot be read).
        var machineLine: String { "\(CompletionCertificate.magic) L:\(length) H:\(sha256)" }

        static func parse(_ text: String) -> Payload? {
            guard text.contains(CompletionCertificate.magic) else { return nil }
            var p = Payload(fileName: "", length: -1, sha256: "", created: Date(timeIntervalSince1970: 0), signers: [])
            // Accept both the QR lines and the printed single line.
            let tokens = text.split(whereSeparator: { $0 == "\n" }).flatMap { line -> [Substring] in
                line.hasPrefix("S:") || line.hasPrefix("F:") ? [line] : line.split(separator: " ")
            }
            for t in tokens {
                let value = String(t.dropFirst(2))
                switch t.prefix(2) {
                case "L:": p.length = Int(value) ?? -1
                case "H:": p.sha256 = value.lowercased()
                case "T:": p.created = isoParser.date(from: value) ?? p.created
                case "F:": p.fileName = value
                case "S:":
                    let f = value.split(separator: "|", omittingEmptySubsequences: false).map(String.init)
                    guard f.count == 3 else { continue }
                    p.signers.append(Signer(name: f[0], time: isoParser.date(from: f[1]), trustedTime: f[2] == "T", isTimestamp: f[2] == "D"))
                default: break
                }
            }
            guard p.length >= 0, p.sha256.count == 64, p.sha256.allSatisfy(\.isHexDigit) else { return nil }
            return p
        }
    }

    struct Check {
        enum Source { case qr, text }
        var payload: Payload
        var source: Source
        var matches: Bool
    }

    static let isoParser: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime]
        return f
    }()

    static func iso(_ d: Date) -> String { isoParser.string(from: d) }
    static func clip(_ s: String, _ n: Int) -> String { s.count > n ? String(s.prefix(n - 1)) + "…" : s }

    // MARK: Creating

    /// Bytes the certificate will cover: the file itself when it can take an incremental update, otherwise (an
    /// unsigned file with a compressed cross-reference) a normalised copy.
    static func base(_ data: Data) throws -> Data {
        if (try? PDFFileStructure(data)) != nil { return data }
        guard PDFSignatureVerifier.verify(data).isEmpty, let pdf = PDFDocument(data: data), !pdf.isEncrypted,
              let normalized = pdf.dataRepresentation() else {
            throw PDFStructureError.unsupported(L("compressed cross-reference stream. Save the document in PDF42 and sign the copy."))
        }
        return normalized
    }

    static func payload(for base: Data, fileName: String, created: Date = Date()) -> Payload {
        let signers = PDFSignatureVerifier.verify(base).map {
            Signer(name: $0.signer, time: $0.timestamp ?? $0.date, trustedTime: $0.timestamp != nil, isTimestamp: $0.isDocumentTimestamp)
        }
        return Payload(fileName: fileName, length: base.count, sha256: DER.hex([UInt8](SHA256.hash(data: base))).lowercased(),
                       created: created, signers: signers)
    }

    /// Appends the certificate page. Returns the new file; the input is a prefix of it.
    static func append(to data: Data, fileName: String, created: Date = Date()) throws -> Data {
        let base = try base(data)
        let infos = PDFSignatureVerifier.verify(base)
        let payload = payload(for: base, fileName: fileName, created: created)
        let pageSize = PDFDocument(data: base)?.page(at: max(0, (PDFDocument(data: base)?.pageCount ?? 1) - 1))
            .map { $0.bounds(for: .mediaBox).size }.flatMap { $0.width >= 400 && $0.height >= 560 && $0.height > $0.width ? $0 : nil }
            ?? CGSize(width: 595, height: 842)
        let page = render(payload, infos: infos, pageSize: pageSize)
        return try appendPage(from: page, to: base)
    }

    /// The certificate as a one-page PDF.
    static func render(_ p: Payload, infos: [SignatureInfo], pageSize: CGSize) -> Data {
        let r = ReportWriter(pageSize: pageSize, title: L("Certificate of Completion"))
        r.newPage()
        r.text(L("Certificate of Completion"), size: 22, weight: .bold, color: ReportWriter.accent, after: 2)
        r.text(L("Created %@ on this computer by PDF42, without any online service.", VerificationReport.stamp(p.created)),
               size: 9, color: ReportWriter.muted, after: 14)

        // QR code on the right of the document block.
        let qrSide: CGFloat = 132
        let top = r.y
        if let qr = ReportWriter.qrCode(p.text) {
            r.image(qr, in: CGRect(x: r.pageSize.width - r.margin - qrSide, y: top, width: qrSide, height: qrSide), interpolate: false)
        }
        r.text(L("Document"), size: 13, weight: .semibold, after: 6)
        let labelWidth: CGFloat = 110
        let narrow = r.width - qrSide - 16
        func row(_ label: String, _ value: String, mono: Bool = false) {
            // Keep clear of the QR code.
            let l = NSAttributedString(string: label, attributes: ReportWriter.attributes(size: 9, weight: .medium, color: ReportWriter.muted))
            let v = NSAttributedString(string: value, attributes: ReportWriter.attributes(size: mono ? 8.5 : 9.5, mono: mono))
            let h = max(r.height(l, width: labelWidth - 6), r.height(v, width: narrow - labelWidth))
            l.draw(with: CGRect(x: r.margin, y: r.y, width: labelWidth - 6, height: h), options: [.usesLineFragmentOrigin])
            v.draw(with: CGRect(x: r.margin + labelWidth, y: r.y, width: narrow - labelWidth, height: h), options: [.usesLineFragmentOrigin])
            r.space(h + 3)
        }
        row(L("File"), p.fileName)
        row(L("Covered bytes"), L("%@ (everything before this page)", p.length.formatted()))
        row("SHA-256", VerificationReport.grouped(p.sha256), mono: true)
        row(L("Signatures"), "\(infos.filter { !$0.isDocumentTimestamp }.count)")
        row(L("Document timestamps"), "\(infos.filter(\.isDocumentTimestamp).count)")
        r.space(max(0, top + qrSide + 10 - r.y))

        r.rule()
        r.text(L("Signing history"), size: 13, weight: .semibold, after: 6)
        if infos.isEmpty { r.text(L("The document has no digital signatures."), size: 10, color: ReportWriter.muted) }
        for (i, s) in infos.enumerated() {
            let (label, color) = VerificationReport.statusBadge(s)
            r.badge(label, color: color, text: s.isDocumentTimestamp ? L("%d. Document timestamp: %@", i + 1, s.signer) : "\(i + 1). \(s.signer)")
            if let email = s.email { r.field(L("Email"), email, indent: 12) }
            if let t = s.timestamp { r.field(L("Trusted time"), VerificationReport.stamp(t), indent: 12) }
            if let d = s.date, s.timestamp == nil || !s.isDocumentTimestamp, d != s.timestamp {
                r.field(L("Signing time (claimed)"), VerificationReport.stamp(d), indent: 12)
            }
            if let level = s.level { r.field(L("Format"), level, indent: 12) }
            if let reason = s.reason { r.field(L("Reason"), reason, indent: 12) }
            r.field(L("Status when issued"), s.summary, indent: 12)
            r.space(4)
        }
        r.rule()
        r.text(L("How to verify offline"), size: 13, weight: .semibold, after: 6)
        r.text(L("1. The QR code holds the document hash and length. PDF42 checks it with “Verify Certificate of Completion” or `PDF42 --verify-completion FILE.pdf`."), size: 9.5)
        r.text(L("2. Without PDF42, hash the first %@ bytes of this file and compare with the SHA-256 above:", p.length.formatted()), size: 9.5, after: 2)
        r.text("head -c \(p.length) \"\(p.fileName)\" | shasum -a 256", size: 9, mono: true, indent: 12, after: 6)
        r.text(L("3. The signatures themselves are verified as usual; this page was added after them, so viewers report that the document was extended after signing. The page itself is not signed: it records, it does not certify."),
               size: 9.5, after: 10)
        r.text(p.machineLine, size: 6.5, color: ReportWriter.muted, mono: true)
        return r.finish()
    }

    // MARK: Incremental page append

    /// Copies the first page of `pageData` (with fonts, images and other resources) to the end of `base` as
    /// an incremental update: the base bytes are untouched.
    static func appendPage(from pageData: Data, to base: Data) throws -> Data {
        let src = try PDFFileStructure(pageData)
        let file = try PDFFileStructure(base)
        guard case .ref(let srcRoot, _)? = src.trailer["Root"], case .ref(let srcPages, _)? = try src.object(srcRoot)["Pages"],
              case .ref(let srcPage, _)? = try src.resolve(try src.object(srcPages)["Kids"])?.items?.first else {
            throw PDFStructureError.malformed("certificate page")
        }
        var update = IncrementalUpdate(file: file)
        var mapping: [Int: Int] = [:]
        var queue: [Int] = []
        func map(_ obj: PDFObject) -> PDFObject {
            switch obj {
            case .ref(let n, _):
                if mapping[n] == nil { mapping[n] = update.add([]); queue.append(n) }
                return .ref(mapping[n]!, 0)
            case .array(let items): return .array(items.map(map))
            case .dict(let pairs): return .dict(pairs.map { ($0.0, map($0.1)) })
            default: return obj
            }
        }
        // The page itself: new parent, everything else copied.
        let (pageDict, _) = try src.rawObject(srcPage)
        let (rootNum, catalog) = try PAdES.catalog(file)
        _ = rootNum
        guard case .ref(let pagesNum, _)? = catalog["Pages"] else { throw PDFStructureError.malformed("no /Pages") }
        let newPage = map(pageDict.removing("Parent")).setting("Parent", .ref(pagesNum, file.gen(pagesNum)))
        let pageNum = update.add(Array(newPage.serialized.utf8))
        while let n = queue.first {
            queue.removeFirst()
            let (dict, stream) = try src.rawObject(n)
            var body = dict
            if let stream { body = body.setting("Length", .raw("\(stream.count)")) }
            let mapped = map(body)
            var bytes = Array(mapped.serialized.utf8)
            if let stream { bytes += Array("\nstream\n".utf8) + stream + Array("\nendstream".utf8) }
            update.replace(mapping[n]!, bytes)
        }
        // Page tree: add the page to the root's kids.
        let pages = try file.object(pagesNum)
        let count = (try file.resolve(pages["Count"])?.int) ?? 0
        if case .ref(let kidsNum, _)? = pages["Kids"] {
            let kids = (try file.object(kidsNum).items ?? []) + [.ref(pageNum, 0)]
            update.replace(kidsNum, Array(PDFObject.array(kids).serialized.utf8))
            update.replace(pagesNum, Array(pages.setting("Count", .raw("\(count + 1)")).serialized.utf8))
        } else {
            let kids = (pages["Kids"]?.items ?? []) + [.ref(pageNum, 0)]
            update.replace(pagesNum, Array(pages.setting("Kids", .array(kids)).setting("Count", .raw("\(count + 1)")).serialized.utf8))
        }
        return Data(update.write())
    }

    // MARK: Checking

    /// Finds the certificate (QR code first, then the printed line) on the last pages and checks the hash.
    static func check(_ data: Data) -> Check? {
        guard let doc = CGPDFDocument(CGDataProvider(data: data as CFData)!), doc.numberOfPages > 0 else { return nil }
        let pdf = PDFDocument(data: data)
        for index in stride(from: doc.numberOfPages, to: max(0, doc.numberOfPages - 3), by: -1) {
            if let page = doc.page(at: index), let image = rasterize(page) {
                for text in ReportWriter.readQRCodes(image) {
                    if let p = Payload.parse(text) { return Check(payload: p, source: .qr, matches: matches(p, data)) }
                }
            }
            if let text = pdf?.page(at: index - 1)?.string, let range = text.range(of: magic) {
                let line = text[range.lowerBound...].prefix(160)
                if let p = Payload.parse(String(line).replacingOccurrences(of: "\n", with: " ")) {
                    return Check(payload: p, source: .text, matches: matches(p, data))
                }
            }
        }
        return nil
    }

    static func matches(_ p: Payload, _ data: Data) -> Bool {
        guard p.length > 0, p.length < data.count else { return false }
        return DER.hex([UInt8](SHA256.hash(data: data.prefix(p.length)))).lowercased() == p.sha256
    }

    static func rasterize(_ page: CGPDFPage, scale: CGFloat = 2.5) -> CGImage? {
        let box = page.getBoxRect(.mediaBox)
        let w = Int(box.width * scale), h = Int(box.height * scale)
        guard w > 0, h > 0, let ctx = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0,
                                                space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue) else { return nil }
        ctx.setFillColor(CGColor(gray: 1, alpha: 1))
        ctx.fill(CGRect(x: 0, y: 0, width: w, height: h))
        ctx.scaleBy(x: scale, y: scale)
        ctx.translateBy(x: -box.minX, y: -box.minY)
        ctx.drawPDFPage(page)
        return ctx.makeImage()
    }
}

extension PDFFileStructure {
    /// An object and, for a stream, its raw (still encoded) data.
    func rawObject(_ num: Int) throws -> (PDFObject, [UInt8]?) {
        guard let entry = offsets[num], entry.offset > 0 else { throw PDFStructureError.malformed("no object \(num)") }
        var lexer = PDFLexer(bytes: bytes, pos: entry.offset)
        _ = lexer.word(); _ = lexer.word()
        try lexer.expect("obj")
        let object = try lexer.parse()
        lexer.skipWhitespace()
        guard lexer.peek("stream") else { return (object, nil) }
        try lexer.expect("stream")
        if lexer.pos < bytes.count, bytes[lexer.pos] == 0x0D { lexer.pos += 1 }
        if lexer.pos < bytes.count, bytes[lexer.pos] == 0x0A { lexer.pos += 1 }
        guard let length = try resolve(object["Length"])?.int, length >= 0, lexer.pos + length <= bytes.count else {
            throw PDFStructureError.malformed("stream length")
        }
        return (object, Array(bytes[lexer.pos..<lexer.pos + length]))
    }
}

extension EditorWindowController {
    /// Signature ▸ Add Certificate of Completion…
    @objc func addCompletionCertificateAction(_ sender: Any?) {
        guard let doc = pdfDoc, let pdf else { return }
        let signed = !doc.signatures.isEmpty
        if signed && doc.isDocumentEdited {
            presentError(PDFStructureError.unsupported(L("undo your changes first: the certificate is added to the signed version")))
            return
        }
        prepareForSave()
        let base: Data
        if signed, let original = doc.originalData { base = original }
        else if let data = PageOverlay.bakedData(pdf) { base = data }
        else { return }
        let name = doc.fileURL?.deletingPathExtension().lastPathComponent ?? doc.displayName ?? L("Document")
        do {
            let data = try CompletionCertificate.append(to: base, fileName: name + " (" + L("completed") + ").pdf")
            saveSignedCopy(PAdES.Result(data: data), suffix: L("completed"))
        } catch { presentError(error) }
    }

    /// Signature ▸ Verify Certificate of Completion
    @objc func verifyCompletionCertificateAction(_ sender: Any?) {
        guard let doc = pdfDoc, let data = doc.originalData ?? doc.fileURL.flatMap({ try? Data(contentsOf: $0) }) else { return }
        let alert = NSAlert()
        if let c = CompletionCertificate.check(data) {
            alert.messageText = c.matches ? L("The certificate of completion matches") : L("The certificate of completion does not match")
            alert.informativeText = L("SHA-256 of the first %@ bytes: %@", c.payload.length.formatted(), c.payload.sha256)
                + "\n" + (c.source == .qr ? L("Read from the QR code.") : L("Read from the page text."))
            alert.icon = Theme42.symbol(c.matches ? "checkmark.seal.fill" : "xmark.seal.fill", size: 40)
        } else {
            alert.messageText = L("No certificate of completion found")
        }
        if let window { alert.beginSheetModal(for: window) } else { alert.runModal() }
    }
}
