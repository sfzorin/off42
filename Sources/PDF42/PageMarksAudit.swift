import AppKit
import Kit42
import PDFKit

/// UI regression for page marks (`K42_DO=auditPageMarks` with `K42_SNAPSHOT`, run by `scripts/audit-ui.py`):
/// adds every kind of mark through the editor, checks geometry and fresh impressions, Undo/Redo, the saved file
/// reopened in PDFKit (appearance streams with multiply blending) and the e-signature mark made from a real signature.
extension EditorWindowController {
    @objc func auditPageMarks(_ sender: Any?) {
        guard let pdf, let undo else { print("FAIL page marks: no document"); return }
        var passed = 0, total = 0
        func check(_ ok: Bool, _ name: String) {
            total += 1
            if ok { passed += 1 } else { print("FAIL page marks: \(name)") }
        }
        func fingerprint(_ image: CGImage) -> Int {
            guard let data = image.dataProvider?.data as Data? else { return 0 }
            return data.hashValue
        }

        // Five pages, one of them turned 90°, so placement is checked on a rotated page too.
        while pdf.pageCount < 5 { pdf.insert(PDFPage.blank(), at: pdf.pageCount) }
        pdf.page(at: 3)?.rotation = 90
        sidebar.thumbnails.reload()
        pdfView.layoutDocumentView()
        undo.removeAllActions()
        let pages = (0..<pdf.pageCount).compactMap(pdf.page(at:))
        let ours: (PDFAnnotation) -> Bool = { $0 is ImageStampAnnotation || $0 is VectorMarkAnnotation }
        func count() -> Int { pages.reduce(0) { $0 + $1.annotations.filter(ours).count } }

        // Pure helpers
        check(PageMarks.russianPlural(1, "лист", "листа", "листов") == "лист" && PageMarks.russianPlural(22, "лист", "листа", "листов") == "листа"
              && PageMarks.russianPlural(12, "лист", "листа", "листов") == "листов" && PageMarks.russianPlural(111, "лист", "листа", "листов") == "листов"
              && PageMarks.russianPlural(25, "лист", "листа", "листов") == "листов" && PageMarks.russianPlural(21, "лист", "листа", "листов") == "лист",
              "Russian plural of sheets")
        let note = PageMarks.certificationText(sheets: 12, signatory: "", date: Date(timeIntervalSince1970: 1_793_000_000), language: .ru)
        check(note.joined(separator: " ").contains("12 (двенадцать) листов"), "note text: \(note)")
        check(PageMarks.certificationText(sheets: 1, signatory: "", date: Date(), language: .en).joined().contains("1 (one) sheet in total"), "English note")
        let columns = PageMarks.sliceColumns(width: 1001, count: 7)
        check(columns.first?.lowerBound == 0 && columns.last?.upperBound == 1001 && zip(columns, columns.dropFirst()).allSatisfy { $0.upperBound == $1.lowerBound },
              "slices cover the seal without gaps")

        guard let template = SealTemplate.named("ru.ooo") else { print("FAIL page marks: no seal template"); return }
        let seal = GeneratedMark.seal(SealSpec(template: template, values: [:], ink: InkColor.violet.stampPad, designSeed: 3))
        let signature = GeneratedMark.signature("Иван Иванов", ink: InkColor.blue.pen,
                                                design: SignatureGenerator.design(form: .initialSurname, variant: 1, seed: 77))

        // Library picker: with an audit library (K42_STAMP_LIBRARY) the marks are added to it and chosen from it,
        // as in the dialogs; every call gives a new impression.
        if ProcessInfo.processInfo.environment["K42_STAMP_LIBRARY"] != nil {
            let library = StampLibrary.shared
            if library.items.isEmpty {
                if let preview = seal.impression(1) { _ = try? library.add(preview, mark: seal) }
                if let preview = signature.impression(1) { _ = try? library.add(preview, mark: signature) }
            }
            let stamps = LibraryMarkPopup(prefer: .seal, key: "auditSeal")
            let signatures = LibraryMarkPopup(prefer: .signature, allowsNone: true, key: "auditSignature")
            check(stamps.selectedURL.flatMap(StampLibrary.mark(at:))?.kind == .seal, "stamp picker prefers a stamp")
            check(signatures.selectedURL.flatMap(StampLibrary.mark(at:))?.kind == .signature, "signature picker prefers a signature")
            if let a = stamps.impression(), let b = stamps.impression() { check(fingerprint(a) != fingerprint(b), "library gives a new impression each time") }
            else { check(false, "library impressions") }
        }

        // 1. Cross-page seal on all pages.
        guard let impression = seal.impression(5) else { print("FAIL page marks: seal"); return }
        let size: CGFloat = 40 * PageGeometry.mm
        let cross = PageMarks.crossPageSeal(impression, pages: pages, size: size, level: 0.18)
        placeMarks(cross.map { ($0.0, $0.1 as PDFAnnotation) }, name: L("Cross-Page Seal"))
        check(cross.count == pages.count, "one slice per page")
        let displayRects = cross.map { PageMarks.displayRect($0.1.bounds, on: $0.0) }
        check(zip(cross, displayRects).allSatisfy { abs($1.maxX - PageMarks.displaySize($0.0).width) < 0.5 }, "slices touch the right edge: \(displayRects)")
        check(abs(displayRects.map(\.width).reduce(0, +) - size) < 0.5, "slice widths add up to the seal")
        check(cross.map { $0.1.image.width }.reduce(0, +) == impression.width, "slices add up to the impression's pixels")
        check(cross[3].1.rotation == 90 && abs(cross[3].1.bounds.height - displayRects[3].width) < 0.5, "slice on the rotated page")

        // 2. Initials: a new impression on every page.
        let initials = PageMarks.initials(pages: pages, corner: .bottomRight, width: 25 * PageGeometry.mm, margin: 10 * PageGeometry.mm,
                                          impression: { signature.impression() })
        placeMarks(initials.map { ($0.0, $0.1 as PDFAnnotation) }, name: L("Initial Every Page"))
        check(initials.count == pages.count, "initials on every page")
        check(Set(initials.map { fingerprint($0.1.image) }).count == pages.count, "every initial is a new impression")
        check(initials.allSatisfy { page, mark in
            let r = PageMarks.displayRect(mark.bounds, on: page), s = PageMarks.displaySize(page)
            return abs(s.width - r.maxX - 10 * PageGeometry.mm) < 0.5 && abs(r.minY - 10 * PageGeometry.mm) < 0.5
        }, "initials in the bottom-right corner, rotated page included")

        // 3. Certification note on the last page.
        let last = pages[pages.count - 1]
        let noteMarks = PageMarks.certificationNote(on: last, sheets: pages.count, signatory: "Генеральный директор И. И. Иванов", date: Date(),
                                                    language: .ru, seal: seal.impression(), sealSize: 40 * PageGeometry.mm,
                                                    signature: signature.impression())
        placeMarks(noteMarks.map { (last, $0) }, name: L("Certification Note"))
        check(noteMarks.count == 3 && (noteMarks.first?.contents ?? "").contains("\(pages.count) ("), "note with stamp and signature")

        // 4. E-signature mark from a real signature: sign a copy with a throwaway certificate, verify, build the mark.
        var eMarks = 0
        do {
            let identity = try Self.auditIdentity()
            guard let base = PDFDocument(data: pdf.dataRepresentation() ?? Data())?.dataRepresentation() else { throw CocoaError(.fileWriteUnknown) }
            let signed = try PDFSigner.sign(base, options: PDFSigner.Options(identity: identity, reason: "Audit", location: ""))
            let found = PDFSignatureVerifier.verify(signed)
            check(found.count == 1 && found.first?.signerCertificate != nil, "signature read back with its certificate")
            if let info = found.first, let cert = info.signerCertificate, let fromIdentity = PageMarks.CertificateDetails(identity: identity) {
                let details = PageMarks.CertificateDetails(certificate: cert, signedAt: info.date)
                check(!details.serial.isEmpty && details.serial == fromIdentity.serial, "serial from the signature matches the certificate")
                check(details.owner == "Audit Signer" && details.validFrom != nil && (details.validTo ?? .distantPast) > (details.validFrom ?? .distantFuture),
                      "owner and validity: \(details)")
                for language in PageMarks.MarkLanguage.allCases {
                    let text = PageMarks.eSignatureText(details, language: language)
                    check(text.rows.count == 4 && text.rows[0].1 == details.serial && text.rows[1].1 == "Audit Signer", "mark text \(language)")
                }
                check(PageMarks.eSignatureText(details, language: .ru).heading.joined(separator: " ") == "ДОКУМЕНТ ПОДПИСАН ЭЛЕКТРОННОЙ ПОДПИСЬЮ", "GOST heading")
                let marks = [pages[0], pages[3]].map { ($0, PageMarks.eSignatureMark(details, language: .ru, corner: .bottomLeft, on: $0) as PDFAnnotation) }
                placeMarks(marks, name: L("Electronic Signature Mark"))
                eMarks = marks.count
            }
        } catch { check(false, "e-signature: \(error.localizedDescription)") }

        let expected = cross.count + initials.count + noteMarks.count + eMarks
        check(count() == expected, "marks on the pages: \(count()) of \(expected)")

        // Undo/Redo once the event's undo group is closed; then save and reopen.
        DispatchQueue.main.async { [self] in
            undo.undo()
            check(count() == 0, "Undo removes every mark (\(count()) left)")
            undo.redo()
            check(count() == expected, "Redo restores every mark (\(count()))")

            guard let data = try? pdfDoc?.data(ofType: "com.adobe.pdf"), let reopened = PDFDocument(data: data) else {
                check(false, "save")
                print("PDF page marks UI regressions: \(passed)/\(total) passed")
                return
            }
            let stamps = (0..<reopened.pageCount).map { reopened.page(at: $0)?.annotations.filter { $0.isKind(.stamp) }.count ?? 0 }
            let perPage = pages.map { $0.annotations.filter(ours).count }
            check(stamps == perPage, "saved file keeps the marks per page: \(stamps) vs \(perPage)")
            let text = String(decoding: data, as: UTF8.self)
            check(text.contains("/Multiply"), "appearance streams use multiply blending")
            // The reopened pages show ink where the marks are.
            func inked(_ page: PDFPage, at display: CGRect) -> Bool {
                let image = page.thumbnail(of: NSSize(width: 600, height: 600), for: .cropBox)
                guard let rep = image.tiffRepresentation.flatMap(NSBitmapImageRep.init(data:)) else { return false }
                let s = PageMarks.displaySize(page)
                let kx = CGFloat(rep.pixelsWide) / s.width, ky = CGFloat(rep.pixelsHigh) / s.height
                var dark = 0
                for y in stride(from: display.minY, to: display.maxY, by: 1) {
                    for x in stride(from: display.minX, to: display.maxX, by: 1) {
                        let px = Int(x * kx), py = rep.pixelsHigh - 1 - Int(y * ky)
                        guard px >= 0, py >= 0, px < rep.pixelsWide, py < rep.pixelsHigh, let c = rep.colorAt(x: px, y: py) else { continue }
                        if c.brightnessComponent < 0.85 { dark += 1 }
                    }
                }
                return dark > 3
            }
            if let p0 = reopened.page(at: 0), let p3 = reopened.page(at: 3) {
                check(inked(p0, at: displayRects[0]), "seal slice visible on the reopened first page")
                check(inked(p3, at: displayRects[3]), "seal slice visible on the reopened rotated page")
            }
            // Thumbnails draw live annotations without the page rotation unless the mark compensates.
            check(inked(pages[3], at: displayRects[3]), "seal slice in the live thumbnail of the rotated page")
            check(inked(pages[3], at: PageMarks.displayRect(initials[3].1.bounds, on: pages[3]).insetBy(dx: 4, dy: 2)),
                  "initials in the live thumbnail of the rotated page")
            // Renders of the live and the reopened pages for review (K42_UI_TEST_DIR).
            if let dir = ProcessInfo.processInfo.environment["K42_UI_TEST_DIR"] {
                try? data.write(to: URL(fileURLWithPath: dir).appendingPathComponent("page-marks-saved.pdf"))
                for (tag, doc) in [("live", pdf), ("saved", reopened)] {
                    for i in 0..<doc.pageCount {
                        guard let page = doc.page(at: i) else { continue }
                        let image = page.thumbnail(of: NSSize(width: 900, height: 900), for: .cropBox)
                        if let png = image.tiffRepresentation.flatMap(NSBitmapImageRep.init(data:))?.representation(using: .png, properties: [:]) {
                            try? png.write(to: URL(fileURLWithPath: dir).appendingPathComponent("page-marks-\(tag)-\(i + 1).png"))
                        }
                    }
                }
            }
            // Debug run: the snapshot hook quits afterwards, which must not wait on a "save changes?" review.
            pdfDoc?.updateChangeCount(.changeCleared)
            pdfView.layoutDocumentView()
            pdfView.needsDisplay = true
            updateSubtitleNow()
            print("PDF page marks UI regressions: \(passed)/\(total) passed")
        }
    }

    /// A throwaway self-signed identity held in memory only (never added to the Keychain).
    static func auditIdentity() throws -> SigningIdentity {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        func openssl(_ args: [String]) throws {
            let p = Process()
            p.executableURL = URL(fileURLWithPath: "/usr/bin/openssl")
            p.arguments = args
            p.currentDirectoryURL = dir
            p.standardOutput = Pipe()
            p.standardError = Pipe()
            try p.run()
            p.waitUntilExit()
            guard p.terminationStatus == 0 else { throw CocoaError(.executableLoad) }
        }
        try openssl(["req", "-x509", "-newkey", "rsa:2048", "-sha256", "-days", "365", "-nodes", "-keyout", "key.pem", "-out", "cert.pem",
                     "-subj", "/CN=Audit Signer", "-addext", "keyUsage=critical,digitalSignature,nonRepudiation"])
        try openssl(["pkcs12", "-export", "-inkey", "key.pem", "-in", "cert.pem", "-out", "id.p12", "-passout", "pass:audit"])
        return try CertificateStore.importPKCS12(try Data(contentsOf: dir.appendingPathComponent("id.p12")), password: "audit", toKeychain: false)
    }
}
