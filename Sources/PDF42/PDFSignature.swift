import CoreGraphics
import CryptoKit
import Foundation
import Kit42
import PDFKit
import Security

/// Digital signatures in the standard PDF way (CMS over /ByteRange): PAdES baseline (ETSI.CAdES.detached)
/// or legacy adbe.pkcs7.detached, compatible with Adobe Acrobat and other validators. PAdES.swift adds the
/// long-term levels (DSS, document timestamps).
enum PDFSigner {
    /// Where the key lives: the macOS Keychain (RSA/ECDSA), CryptoPro CSP (GOST) or a remote signing service (CSC).
    enum Signer {
        case keychain(SigningIdentity)
        case cryptoPro(CryptoPro.Certificate, CryptoPro.SignOptions)
        case remote(CMSRawSigner)
        /// Not chosen yet (set `Options.remote` before signing).
        case none

        var name: String {
            switch self {
            case .keychain(let id): return id.name
            case .cryptoPro(let cert, _): return cert.name
            case .remote(let r): return r.chain.first?.name ?? L("Unknown")
            case .none: return L("Unknown")
            }
        }
        var isGOST: Bool { if case .cryptoPro = self { return true } else { return false } }
    }

    struct Options {
        var signer: Signer
        var reason: String
        var location: String
        /// Visible signature: page index, rectangle in page space, rendered appearance.
        var visible: (page: Int, rect: CGRect, appearance: CGImage)?
        /// RFC 3161 time-stamping authority, e.g. http://timestamp.digicert.com
        var timestampURL: URL?
        /// Signature format; levels above B-T are completed by `PAdES.sign`. GOST signatures from CryptoPro CSP
        /// keep the format CSP produces (adbe.pkcs7.detached with CAdES inside).
        var level: PAdESLevel = .baselineB
        /// The Keychain identity, when signing with one.
        var identity: SigningIdentity? { if case .keychain(let id) = signer { return id }; return nil }
        /// Remote signing service (CSC API) instead of a local key.
        var remote: CMSRawSigner? {
            get { if case .remote(let r) = signer { return r }; return nil }
            set { if let newValue { signer = .remote(newValue) } }
        }
        var signerName: String { signer.name }

        init(signer: Signer, reason: String, location: String) {
            self.signer = signer
            self.reason = reason
            self.location = location
        }

        /// `identity` nil: set `remote` before signing.
        init(identity: SigningIdentity?, reason: String, location: String) {
            self.init(signer: identity.map(Signer.keychain) ?? .none, reason: reason, location: location)
        }
    }

    /// Reserved bytes for the CMS signature (hex doubles it). GOST CAdES from CSP carries the whole chain
    /// and, with a timestamp, the TSA certificates too.
    static let signatureCapacity = 16_384
    static let gostSignatureCapacity = 40_960

    /// Embeds files (e.g. a power of attorney and its signature) as an incremental update, so existing
    /// signatures stay valid. Used before signing, so the new signature covers them.
    static func embed(_ files: [EmbeddedFile], into base: Data) throws -> Data {
        guard !files.isEmpty else { return base }
        guard let pdf = PDFDocument(data: base), let writer = Writer(base) else { throw PDFStructureError.malformed("attachments") }
        var extras = DocumentExtras.read(base, pdf: pdf)
        for f in files {
            extras.files.removeAll { $0.name == f.name }
            extras.files.append(f)
        }
        extras.dirty = [.files]
        writer.write(extras: extras, pdf: pdf, pdfa: false)
        return writer.finish(base)
    }

    /// Appends a signature as an incremental update. `base` must use a classic xref table
    /// (any file saved by PDF42/PDFKit, or a file signed by PDF42).
    static func sign(_ base: Data, options: Options) throws -> Data {
        let gost = options.signer.isGOST
        // GOST: CSP's CMS under adbe.pkcs7.detached, and "CryptoPro PDF" names the preferred handler (Acrobat with
        // the CryptoPro PDF plug-in). Other signers: PAdES (ETSI.CAdES.detached) or the legacy format.
        let subFilter = gost || options.level == .legacy ? "adbe.pkcs7.detached" : "ETSI.CAdES.detached"
        let filter = gost ? "/CryptoPro#20PDF" : "/Adobe.PPKLite"
        var tail = "/M \(pdfDate(Date())) /Name \(pdfTextString(options.signerName))"
        if !options.reason.isEmpty { tail += " /Reason \(pdfTextString(options.reason))" }
        if !options.location.isEmpty { tail += " /Location \(pdfTextString(options.location))" }
        tail += " /Prop_Build << /App << /Name /PDF42 >> >>"
        let capacity = gost ? gostSignatureCapacity
            : options.level == .legacy && options.remote == nil ? signatureCapacity : 2 * signatureCapacity
        return try appendSignatureField(base, visible: options.visible, head: "/Type /Sig /Filter \(filter) /SubFilter /\(subFilter)",
                                        tail: tail, capacity: capacity) { signed in
            var cms: Data
            switch options.signer {
            case .cryptoPro(_, var g):
                // CSP adds the timestamp itself (CAdES-T).
                g.tsa = options.timestampURL ?? g.tsa
                return try CryptoPro.signDetached(signed, options: g)
            case .remote(let remote):
                cms = try CAdES.sign(signed, signer: remote)
            case .keychain(let identity):
                cms = options.level == .legacy ? try cmsSign(signed, identity: identity.identity)
                    : try CAdES.sign(signed, signer: try KeychainSigner(identity.identity))
            case .none:
                throw CertificateError.failed(L("no certificate"))
            }
            if let url = options.timestampURL { cms = try Timestamp.add(to: cms, tsa: url) }
            return cms
        }
    }

    /// Appends a signature field (invisible, or visible on `visible.page`) whose value dictionary is
    /// `<< head /ByteRange … /Contents <…> tail >>`, then fills /Contents with `contents(signedBytes)`.
    /// `base` must use a classic xref table (any file saved by PDF42/PDFKit, or a file signed by PDF42).
    static func appendSignatureField(_ base: Data, visible: (page: Int, rect: CGRect, appearance: CGImage)?, head: String, tail: String,
                                     capacity: Int, contents: (Data) throws -> Data) throws -> Data {
        let file = try PDFFileStructure(base)
        guard case .ref(let rootNum, _)? = file.trailer["Root"], let size = file.trailer["Size"]?.int else {
            throw PDFStructureError.malformed("no /Root")
        }
        var catalog = try file.object(rootNum)

        // Target page (first, or the visible signature's page)
        guard case .ref(let pagesRoot, _)? = catalog["Pages"] else { throw PDFStructureError.malformed("no /Pages") }
        var pageNums: [Int] = []
        func collect(_ num: Int, depth: Int) throws {
            guard depth < 64 else { return }
            let n = try file.object(num)
            if n["Type"]?.nameValue == "Page" { pageNums.append(num); return }
            for case .ref(let kid, _) in try file.resolve(n["Kids"])?.items ?? [] { try collect(kid, depth: depth + 1) }
        }
        try collect(pagesRoot, depth: 0)
        guard !pageNums.isEmpty else { throw PDFStructureError.malformed("page tree") }
        let pageNum = pageNums[min(max(0, visible?.page ?? 0), pageNums.count - 1)]
        let node = try file.object(pageNum)

        var nextNum = size
        let sigNum = nextNum; nextNum += 1
        let widgetNum = nextNum; nextNum += 1
        let widgetRef = PDFObject.ref(widgetNum, 0)
        var updates: [Int: String] = [:]
        var binary: [Int: [UInt8]] = [:]

        // Page /Annots += widget
        switch node["Annots"] {
        case .array(let a)?:
            updates[pageNum] = node.setting("Annots", .array(a + [widgetRef])).serialized
        case .ref(let n, _)?:
            let arr = try file.object(n).items ?? []
            updates[n] = PDFObject.array(arr + [widgetRef]).serialized
        default:
            updates[pageNum] = node.setting("Annots", .array([widgetRef])).serialized
        }

        // Catalog /AcroForm /Fields += widget, /SigFlags 3
        func addField(_ form: PDFObject) throws -> PDFObject {
            var form = form
            switch form["Fields"] {
            case .array(let a)?: form = form.setting("Fields", .array(a + [widgetRef]))
            case .ref(let n, _)?:
                let arr = try file.object(n).items ?? []
                updates[n] = PDFObject.array(arr + [widgetRef]).serialized
            default: form = form.setting("Fields", .array([widgetRef]))
            }
            return form.setting("SigFlags", .raw("3"))
        }
        switch catalog["AcroForm"] {
        case .ref(let n, _)?:
            updates[n] = try addField(try file.object(n)).serialized
        case .dict?:
            catalog = catalog.setting("AcroForm", try addField(catalog["AcroForm"]!))
            updates[rootNum] = catalog.serialized
        default:
            catalog = catalog.setting("AcroForm", try addField(.dict([])))
            updates[rootNum] = catalog.serialized
        }

        let existingForm = try file.resolve(try file.object(rootNum)["AcroForm"])
        let fieldCount = try file.resolve(existingForm?["Fields"])?.items?.count ?? 0
        var rectText = "[0 0 0 0]"
        var appearance = ""
        if let visible {
            let r = visible.rect
            rectText = String(format: "[%.2f %.2f %.2f %.2f]", r.minX, r.minY, r.maxX, r.maxY)
            let imgNum = nextNum, maskNum = nextNum + 1, apNum = nextNum + 2
            nextNum += 3
            let (rgb, alpha, w, h) = Self.pixels(visible.appearance)
            let rgbZ = [UInt8](zlibCompress(Data(rgb))), alphaZ = [UInt8](zlibCompress(Data(alpha)))
            binary[maskNum] = Array("<< /Type /XObject /Subtype /Image /Width \(w) /Height \(h) /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode /Length \(alphaZ.count) >>\nstream\n".utf8) + alphaZ + Array("\nendstream".utf8)
            binary[imgNum] = Array("<< /Type /XObject /Subtype /Image /Width \(w) /Height \(h) /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /SMask \(maskNum) 0 R /Length \(rgbZ.count) >>\nstream\n".utf8) + rgbZ + Array("\nendstream".utf8)
            let content = String(format: "q %.2f 0 0 %.2f 0 0 cm /Img Do Q", r.width, r.height)
            updates[apNum] = "<< /Type /XObject /Subtype /Form /BBox [0 0 \(String(format: "%.2f %.2f", r.width, r.height))] /Resources << /XObject << /Img \(imgNum) 0 R >> >> /Length \(content.utf8.count) >>\nstream\n\(content)\nendstream"
            appearance = " /AP << /N \(apNum) 0 R >>"
        }
        updates[widgetNum] = "<< /Type /Annot /Subtype /Widget /FT /Sig /T \(pdfTextString("Signature\(fieldCount + 1)")) /F 132 /Rect \(rectText)\(appearance) /P \(pageNum) \(file.gen(pageNum)) R /V \(sigNum) 0 R >>"

        let byteRangePlaceholder = "[0 0000000000 0000000000 0000000000]"
        let contentsPlaceholder = "<" + String(repeating: "0", count: capacity * 2) + ">"
        updates[sigNum] = "<< \(head) /ByteRange \(byteRangePlaceholder) /Contents \(contentsPlaceholder) \(tail) >>"

        // Write the update
        var out = [UInt8](base)
        if out.last != 0x0A { out.append(0x0A) }
        var entries: [(Int, Int)] = []
        var sigOffset = 0
        for num in Set(updates.keys).union(binary.keys).sorted() {
            entries.append((num, out.count))
            if num == sigNum { sigOffset = out.count }
            let gen = num >= size ? 0 : file.gen(num)
            out += Array("\(num) \(gen) obj\n".utf8) + (binary[num] ?? Array(updates[num]!.utf8)) + Array("\nendobj\n".utf8)
        }
        let xrefOffset = out.count
        var xref = "xref\n"
        var i = 0
        while i < entries.count {
            var j = i
            while j + 1 < entries.count, entries[j + 1].0 == entries[j].0 + 1 { j += 1 }
            xref += "\(entries[i].0) \(j - i + 1)\n"
            for k in i...j {
                let num = entries[k].0
                xref += String(format: "%010d %05d n\r\n", entries[k].1, num >= size ? 0 : file.gen(num))
            }
            i = j + 1
        }
        let trailer = file.trailer.removing("XRefStm").removing("Prev")
            .setting("Size", .raw("\(nextNum)")).setting("Prev", .raw("\(file.startxref)"))
        xref += "trailer\n\(trailer.serialized)\nstartxref\n\(xrefOffset)\n%%EOF\n"
        out += Array(xref.utf8)

        // Patch /ByteRange and sign
        guard let brPos = find("/ByteRange ", in: out, from: sigOffset),
              let contentsPos = find("/Contents <", in: out, from: sigOffset) else {
            throw PDFStructureError.malformed("signature placeholder")
        }
        let a = contentsPos + "/Contents ".utf8.count
        let b = a + contentsPlaceholder.utf8.count
        let range = "[0 \(a) \(b) \(out.count - b)]"
        let padded = range.padding(toLength: byteRangePlaceholder.count, withPad: " ", startingAt: 0)
        let brStart = brPos + "/ByteRange ".utf8.count
        out.replaceSubrange(brStart..<brStart + padded.utf8.count, with: Array(padded.utf8))

        let signed = Data(out[0..<a] + out[b..<out.count])
        let cms = try contents(signed)
        guard cms.count <= capacity else { throw PDFStructureError.unsupported(L("the signature is too large (%d bytes)", cms.count)) }
        let hex = cms.map { String(format: "%02X", $0) }.joined()
        out.replaceSubrange(a + 1..<a + 1 + hex.utf8.count, with: Array(hex.utf8))
        return Data(out)
    }

    /// RGB and alpha planes (8-bit, not premultiplied) of an image, top row first.
    static func pixels(_ image: CGImage) -> ([UInt8], [UInt8], Int, Int) {
        let w = image.width, h = image.height
        var rgba = [UInt8](repeating: 0, count: w * h * 4)
        let ctx = CGContext(data: &rgba, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
                            space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
        ctx.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
        var rgb = [UInt8](repeating: 0, count: w * h * 3), alpha = [UInt8](repeating: 0, count: w * h)
        for i in 0..<(w * h) {
            let a = rgba[i * 4 + 3]
            alpha[i] = a
            for c in 0..<3 {
                let v = rgba[i * 4 + c]
                rgb[i * 3 + c] = a == 0 ? 255 : UInt8(min(255, Int(v) * 255 / Int(a)))
            }
        }
        return (rgb, alpha, w, h)
    }

    static func cmsSign(_ data: Data, identity: SecIdentity) throws -> Data {
        var encoder: CMSEncoder?
        try check(CMSEncoderCreate(&encoder), "CMS")
        guard let encoder else { throw CertificateError.failed("CMS") }
        try check(CMSEncoderAddSigners(encoder, identity), "signing key")
        try check(CMSEncoderSetHasDetachedContent(encoder, true), "CMS")
        try check(CMSEncoderSetSignerAlgorithm(encoder, kCMSEncoderDigestAlgorithmSHA256), "SHA-256")
        try check(CMSEncoderAddSignedAttributes(encoder, .attrSigningTime), "signing time")
        try check(CMSEncoderSetCertificateChainMode(encoder, .chainWithRoot), "certificate chain")
        try data.withUnsafeBytes { try check(CMSEncoderUpdateContent(encoder, $0.baseAddress!, data.count), "data") }
        var out: CFData?
        try check(CMSEncoderCopyEncodedContent(encoder, &out), L("signature (access to the key denied?)"))
        guard let out else { throw CertificateError.failed("empty signature") }
        return out as Data
    }

    static func check(_ status: OSStatus, _ what: String) throws {
        guard status == errSecSuccess else {
            let msg = SecCopyErrorMessageString(status, nil) as String? ?? "\(status)"
            throw CertificateError.failed("\(what): \(msg)")
        }
    }

    static func pdfDate(_ date: Date) -> String {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.dateFormat = "yyyyMMddHHmmss"
        let tz = TimeZone.current.secondsFromGMT(for: date)
        let sign = tz >= 0 ? "+" : "-"
        return String(format: "(D:%@%@%02d'%02d')", f.string(from: date), sign, abs(tz) / 3600, abs(tz) % 3600 / 60)
    }

    static func find(_ s: String, in bytes: [UInt8], from: Int) -> Int? {
        let needle = Array(s.utf8)
        var i = from
        while i + needle.count <= bytes.count {
            if bytes[i] == needle[0], Array(bytes[i..<i + needle.count]) == needle { return i }
            i += 1
        }
        return nil
    }
}

enum CertificateError: LocalizedError {
    case failed(String)
    var errorDescription: String? {
        if case .failed(let s) = self { return L("Could not sign: %@", s) }
        return nil
    }
}

/// Result of checking one signature.
struct SignatureInfo {
    /// `notChecked`: a GOST signature while CryptoPro CSP is not installed.
    enum Status { case valid, untrusted, invalid, notChecked }

    var signer = L("Unknown")
    var email: String?
    var date: Date?
    var reason: String?
    var location: String?
    var status: Status = .invalid
    var coversWholeFile = false
    var trustMessage: String?
    var problem: String?
    /// Trusted time from an RFC 3161 timestamp, if the signature has one.
    var timestamp: Date?
    /// A document timestamp (ETSI.RFC3161) rather than a signature.
    var isDocumentTimestamp = false
    /// "PAdES B-B" … "PAdES B-LTA", or a description of a non-PAdES format.
    var level: String?
    /// Whether the document holds revocation data for the whole certificate path (nil: not checked).
    var ltvEnabled: Bool?
    var ltvProblem: String?

    var ltvSummary: String? {
        guard let ltvEnabled else { return nil }
        return ltvEnabled ? L("Long-term validation: enabled") : L("Long-term validation: not enabled (%@)", ltvProblem ?? "")
    }
    /// GOST signature (checked through CryptoPro CSP).
    var isGOST = false
    /// Extra lines: algorithm, SNILS / INN / OGRN of a qualified certificate.
    var details: [String] = []
    /// Results of checking an attached power of attorney (МЧД).
    var powerOfAttorney: [PowerOfAttorney.Finding] = []
    /// Signer certificate as parsed from the CMS (for GOST signatures).
    var certificate: CertificateInfo?
    var filter: String?

    var summary: String {
        switch status {
        case .valid where isDocumentTimestamp: return coversWholeFile ? L("Document timestamp is valid") : L("Document timestamp is valid for its version")
        case .valid: return coversWholeFile ? L("Signature is valid") : L("Valid for its version; the document was extended after signing")
        case .untrusted: return L("The signature is intact, but the certificate is not verified")
        case .invalid: return problem ?? L("Signature is invalid")
        case .notChecked: return L("GOST signature: not checked, CryptoPro CSP is not installed")
        }
    }
}

enum PDFSignatureVerifier {
    /// `attachments`: embedded files, searched for a power of attorney (МЧД) and its signature.
    static func verify(_ data: Data, attachments: [EmbeddedFile] = []) -> [SignatureInfo] {
        let bytes = [UInt8](data)
        var results: [SignatureInfo] = []
        var pos = 0
        while let br = PDFSigner.find("/ByteRange", in: bytes, from: pos) {
            pos = br + 10
            var lexer = PDFLexer(bytes: bytes, pos: pos)
            guard let items = (try? lexer.parse())?.items, items.count == 4 else { continue }
            let range = items.compactMap(\.int)
            guard range.count == 4 else { continue }
            let (a, b, c, d) = (range[0], range[1], range[2], range[3])
            guard a == 0, b > 0, c > b, c <= bytes.count, d >= 0, d <= bytes.count - c,
                  c - b >= 2, bytes[b] == UInt8(ascii: "<"), bytes[c - 1] == UInt8(ascii: ">") else { continue }
            var info = SignatureInfo()
            info.coversWholeFile = c + d == bytes.count
            readDictionary(bytes, around: br, contents: b..<c, into: &info)
            let cms = hexDecode(bytes[(b + 1)..<(c - 1)])
            let signed = Data(bytes[0..<b] + bytes[c..<(c + d)])
            if info.isDocumentTimestamp { checkDocumentTimestamp(cms: cms, signed: signed, into: &info) }
            else { check(cms: cms, signed: signed, into: &info) }
            results.append(info)
        }
        let analysis = PAdES.analyze(data)
        if analysis.count == results.count {
            for i in results.indices {
                results[i].level = analysis[i].level
                results[i].ltvEnabled = analysis[i].ltvEnabled
                results[i].ltvProblem = analysis[i].ltvProblem
            }
        }
        if !results.isEmpty, let poa = attachments.first(where: { PowerOfAttorney.looksLike($0.data) }),
           let parsed = PowerOfAttorney(xml: poa.data) {
            let sig = attachments.first { $0.name != poa.name && ($0.name == poa.name + ".sig" || $0.name == poa.name + ".p7s"
                || ($0.name.hasPrefix((poa.name as NSString).deletingPathExtension) && ($0.name.hasSuffix(".sig") || $0.name.hasSuffix(".p7s")))) }
            // The МЧД concerns the representative's signatures; others (e.g. the principal's own) are left alone,
            // unless no signature matches, which is then reported on all of them.
            let matching = results.indices.filter { parsed.isRepresentative(results[$0].certificate) }
            for i in matching.isEmpty ? Array(results.indices) : matching {
                results[i].powerOfAttorney = parsed.check(signer: results[i].certificate, signedAt: results[i].date,
                                                          xml: poa.data, poaSignature: sig?.data)
            }
        }
        return results
    }

    /// RFC 3161 document timestamp: the token's message imprint must match the covered bytes.
    private static func checkDocumentTimestamp(cms: Data, signed: Data, into info: inout SignatureInfo) {
        let message = derTrimmed(cms)
        guard let token = CMSParts(bytes: [UInt8](message)), let (alg, digest) = token.timestampImprint else {
            info.problem = L("The signature is damaged (CMS cannot be read)")
            return
        }
        info.signer = token.signerCertificate?.name ?? info.signer
        info.timestamp = token.timestampTime
        info.date = token.timestampTime
        let actual: [UInt8]
        switch alg {
        case OID.sha256: actual = [UInt8](SHA256.hash(data: signed))
        case OID.sha384: actual = [UInt8](SHA384.hash(data: signed))
        case OID.sha512: actual = [UInt8](SHA512.hash(data: signed))
        case OID.sha1: actual = [UInt8](Insecure.SHA1.hash(data: signed))
        default: info.problem = L("The signature could not be verified"); return
        }
        guard actual == digest else { info.problem = L("The document was changed after signing"); return }
        // Security.framework treats id-ct-TSTInfo content as detached: pass the TSTInfo explicitly.
        func decode(_ detached: [UInt8]?) -> CMSDecoder? {
            var d: CMSDecoder?
            guard CMSDecoderCreate(&d) == errSecSuccess, let d,
                  message.withUnsafeBytes({ CMSDecoderUpdateMessage(d, $0.baseAddress!, message.count) }) == errSecSuccess else { return nil }
            if let detached { CMSDecoderSetDetachedContent(d, Data(detached) as CFData) }
            return CMSDecoderFinalizeMessage(d) == errSecSuccess ? d : nil
        }
        var status: CMSSignerStatus = .unsigned
        var trust: SecTrust?
        var code: OSStatus = 0
        for detached in [nil, token.encapsulatedContent] {
            guard let d = decode(detached) else { continue }
            trust = nil
            CMSDecoderCopySignerStatus(d, 0, SecPolicyCreateBasicX509(), true, &status, &trust, &code)
            if status != .needsDetachedContent { break }
        }
        if let trust {
            var error: CFError?
            if !SecTrustEvaluateWithError(trust, &error) { info.trustMessage = (error as Error?)?.localizedDescription }
        }
        switch status {
        case .valid: info.status = .valid
        case .invalidCert: info.status = .untrusted
        default: info.status = .invalid; info.problem = L("The signature could not be verified")
        }
    }

    /// GOST CMS: signer details from the CMS itself, the cryptographic check through CryptoPro CSP.
    static func checkGOST(cms: Data, signed: Data, parsed: CMSInfo?, into info: inout SignatureInfo) {
        info.isGOST = true
        describe(parsed, into: &info)
        guard CryptoPro.isInstalled else {
            info.status = .notChecked
            info.trustMessage = CryptoPro.installHelp
            return
        }
        do {
            let v = try CryptoPro.verifyDetached(signed, signature: cms)
            switch v.status {
            case .valid: info.status = .valid
            case .untrusted: info.status = .untrusted; info.trustMessage = v.message
            case .invalid: info.status = .invalid; info.problem = L("The document was changed after signing") + (v.message.map { "\n" + $0 } ?? "")
            }
        } catch {
            info.status = .invalid
            info.problem = error.localizedDescription
        }
    }

    /// Signer, time and qualified-certificate fields parsed from the CMS (no verification).
    static func describe(_ parsed: CMSInfo?, into info: inout SignatureInfo) {
        guard let signer = parsed?.signers.first else { return }
        if let t = signer.signingTime { info.date = t }
        if let c = signer.certificate {
            info.certificate = c
            info.signer = c.displayName
            if let s = c.snils { info.details.append(L("SNILS %@", s)) }
            if let i = c.inn { info.details.append(L("INN %@", i)) }
            if let i = c.innLE, i != c.inn { info.details.append(L("Organisation INN %@", i)) }
            if let o = c.ogrn { info.details.append(L("OGRN %@", o)) }
            info.details.append(L("Issued by %@", c.issuerName))
            if let until = c.notAfter { info.details.append(L("Certificate valid until %@", until.formatted(date: .long, time: .omitted))) }
        }
        let alg = signer.signatureAlgorithm.isEmpty ? signer.certificate?.publicKeyAlgorithm ?? "" : signer.signatureAlgorithm
        if !alg.isEmpty { info.details.append(L("Algorithm: %@", GOST.algorithmName(alg))) }
    }

    private static func check(cms: Data, signed: Data, into info: inout SignatureInfo) {
        let message = derTrimmed(cms)
        guard !message.isEmpty else {
            info.problem = L("The signature is damaged (CMS cannot be read)")
            return
        }
        let parsed = CMSInfo(message)
        if parsed?.isGOST == true || info.filter == "CryptoPro PDF" || info.filter == "CryptoPro#20PDF" {
            checkGOST(cms: message, signed: signed, parsed: parsed, into: &info)
            return
        }
        func decoder(detached: Data?) -> CMSDecoder? {
            var d: CMSDecoder?
            guard CMSDecoderCreate(&d) == errSecSuccess, let d else { return nil }
            let ok = message.withUnsafeBytes { CMSDecoderUpdateMessage(d, $0.baseAddress!, message.count) } == errSecSuccess
            if let detached { CMSDecoderSetDetachedContent(d, detached as CFData) }
            guard ok, CMSDecoderFinalizeMessage(d) == errSecSuccess else { return nil }
            return d
        }
        guard let probe = decoder(detached: nil) else {
            info.problem = L("The signature is damaged (CMS cannot be read)")
            return
        }
        // Detached (adbe.pkcs7.detached, ETSI.CAdES.detached) has no embedded content;
        // adbe.pkcs7.sha1 embeds the SHA-1 of the byte range.
        var embedded: CFData?
        CMSDecoderCopyContent(probe, &embedded)
        var dec = probe
        if let content = embedded as Data?, !content.isEmpty {
            if content != Data(Insecure.SHA1.hash(data: signed)) {
                info.problem = L("The document was changed after signing")
            }
        } else {
            guard let d = decoder(detached: signed) else { info.problem = L("The signature is damaged"); return }
            dec = d
        }

        var cert: SecCertificate?
        if CMSDecoderCopySignerCert(dec, 0, &cert) == errSecSuccess, let cert {
            info.signer = SecCertificateCopySubjectSummary(cert) as String? ?? info.signer
            var emails: CFArray?
            SecCertificateCopyEmailAddresses(cert, &emails)
            info.email = (emails as? [String])?.first
        }
        var time: CFAbsoluteTime = 0
        if CMSDecoderCopySignerSigningTime(dec, 0, &time) == errSecSuccess {
            info.date = Date(timeIntervalSinceReferenceDate: time)
        }
        var ts: CFAbsoluteTime = 0
        if CMSDecoderCopySignerTimestamp(dec, 0, &ts) == errSecSuccess { info.timestamp = Date(timeIntervalSinceReferenceDate: ts) }

        var status: CMSSignerStatus = .unsigned
        var trust: SecTrust?
        var verifyCode: OSStatus = 0
        CMSDecoderCopySignerStatus(dec, 0, SecPolicyCreateBasicX509(), true, &status, &trust, &verifyCode)
        if let trust {
            var error: CFError?
            if !SecTrustEvaluateWithError(trust, &error) {
                info.trustMessage = (error as Error?)?.localizedDescription
            }
        }
        if info.problem != nil { info.status = .invalid; return }
        switch status {
        case .valid: info.status = .valid
        case .invalidCert: info.status = .untrusted
        case .invalidSignature: info.status = .invalid; info.problem = L("The document was changed after signing")
        default: info.status = .invalid; info.problem = L("The signature could not be verified")
        }
    }

    /// The signature dictionary that holds this /ByteRange, without the /Contents value.
    static func dictionaryRegion(_ bytes: [UInt8], around pos: Int, contents: Range<Int>) -> [UInt8] {
        let start = min(contents.lowerBound, max(0, pos - 4096))
        let before = Array(bytes[start..<contents.lowerBound])
        let after = Array(bytes[contents.upperBound..<min(bytes.count, contents.upperBound + 4096)])
        var region = before + after
        if let objStart = PDFFileStructure.lastIndex(of: " obj", in: before) { region = Array(before[objStart...]) + after }
        if let end = PDFSigner.find("endobj", in: region, from: 0) { region = Array(region[..<end]) }
        return region
    }

    /// Reads /Reason, /Location, /M, /Name and the kind (signature or document timestamp) of the dictionary.
    private static func readDictionary(_ bytes: [UInt8], around pos: Int, contents: Range<Int>, into info: inout SignatureInfo) {
        let region = dictionaryRegion(bytes, around: pos, contents: contents)
        info.isDocumentTimestamp = PDFSigner.find("/ETSI.RFC3161", in: region, from: 0) != nil
        func value(_ key: String) -> String? {
            guard let p = PDFSigner.find("/\(key)", in: region, from: 0) else { return nil }
            var lexer = PDFLexer(bytes: region, pos: p + key.utf8.count + 1)
            guard case .raw(let raw)? = try? lexer.parse() else { return nil }
            let s = decodePDFString(raw)
            return s.isEmpty ? nil : s
        }
        if let p = PDFSigner.find("/Filter", in: region, from: 0) {
            var lexer = PDFLexer(bytes: region, pos: p + 7)
            info.filter = (try? lexer.parse())?.nameValue
        }
        info.reason = value("Reason")
        info.location = value("Location")
        if let name = value("Name") { info.signer = name }
        if let m = value("M") { info.date = parsePDFDate(m) }
    }

    static func parsePDFDate(_ s: String) -> Date? {
        let digits = s.replacingOccurrences(of: "D:", with: "").prefix(14)
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.dateFormat = "yyyyMMddHHmmss"
        return f.date(from: String(digits))
    }

    private static func hexDecode(_ hex: ArraySlice<UInt8>) -> Data {
        var out = Data(capacity: hex.count / 2)
        var high: UInt8?
        for c in hex {
            let v: UInt8
            switch c {
            case 0x30...0x39: v = c - 0x30
            case 0x41...0x46: v = c - 0x41 + 10
            case 0x61...0x66: v = c - 0x61 + 10
            default: continue
            }
            if let h = high { out.append(h << 4 | v); high = nil } else { high = v }
        }
        return out
    }

    /// Drops the zero padding after the CMS message (BER, possibly indefinite-length).
    private static func derTrimmed(_ data: Data) -> Data {
        let b = [UInt8](data)
        guard let length = berLength(b, at: 0) else { return data }
        return data.prefix(length)
    }

    /// Total size of the BER element starting at `i`, or nil if malformed.
    private static func berLength(_ b: [UInt8], at start: Int) -> Int? {
        var i = start
        guard i < b.count else { return nil }
        if b[i] & 0x1F == 0x1F {
            i += 1
            while i < b.count, b[i] & 0x80 != 0 { i += 1 }
        }
        i += 1
        guard i < b.count else { return nil }
        let first = b[i]
        i += 1
        if first == 0x80 {
            // Indefinite length: children until the 00 00 end-of-contents marker.
            while i + 1 < b.count {
                if b[i] == 0, b[i + 1] == 0 { return i + 2 - start }
                guard let child = berLength(b, at: i) else { return nil }
                i += child
            }
            return nil
        }
        var len = 0
        if first & 0x80 == 0 { len = Int(first) } else {
            let n = Int(first & 0x7F)
            guard n <= 4, i + n <= b.count else { return nil }
            for k in 0..<n { len = len << 8 | Int(b[i + k]) }
            i += n
        }
        return i + len - start <= b.count ? i + len - start : nil
    }
}
