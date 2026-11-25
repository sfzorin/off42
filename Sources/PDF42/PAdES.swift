import Compression
import CryptoKit
import Foundation
import Kit42

/// Signature formats offered when signing.
enum PAdESLevel: Int, CaseIterable, Comparable {
    case legacy, baselineB, baselineT, baselineLT, baselineLTA

    static func < (a: PAdESLevel, b: PAdESLevel) -> Bool { a.rawValue < b.rawValue }

    var title: String {
        switch self {
        case .legacy: return L("Basic PDF signature (adbe.pkcs7.detached)")
        case .baselineB: return "PAdES B-B"
        case .baselineT: return L("PAdES B-T (with timestamp)")
        case .baselineLT: return L("PAdES B-LT (long-term validation)")
        case .baselineLTA: return L("PAdES B-LTA (long-term archive)")
        }
    }

    var cliName: String { ["pkcs7", "B-B", "B-T", "B-LT", "B-LTA"][rawValue] }
    static func named(_ s: String) -> PAdESLevel? { allCases.first { $0.cliName.lowercased() == s.lowercased() } }
}

/// PAdES baseline profiles (ETSI EN 319 142-1): DSS (validation data) and document timestamps.
enum PAdES {
    struct Result {
        var data: Data
        var warnings: [String] = []
    }

    /// Signs at `options.level`: CMS signature (+ signature timestamp), then the DSS (B-LT) and a
    /// document timestamp (B-LTA). Without a TSA the level is capped at B-B.
    static func sign(_ base: Data, options: PDFSigner.Options) throws -> Result {
        var options = options
        if options.timestampURL == nil, options.level > .baselineB { options.level = .baselineB }
        var result = Result(data: try PDFSigner.sign(base, options: options))
        if options.level >= .baselineLT { result = try addValidationData(result.data) }
        if options.level == .baselineLTA, let tsa = options.timestampURL {
            let stamped = try addDocumentTimestamp(result.data, tsa: tsa)
            let ltv = try addValidationData(stamped)
            result = Result(data: ltv.data, warnings: result.warnings + ltv.warnings)
        }
        return result
    }

    // MARK: Signatures in a file

    struct EmbeddedSignature {
        var range: [Int]                  // /ByteRange
        var contents: [UInt8]             // decoded /Contents including the zero padding
        var cms: CMSParts?
        var isDocumentTimestamp: Bool
        var subFilter: String?

        var coveredEnd: Int { range[2] + range[3] }
        /// DSS /VRI key: uppercase hex SHA-1 of the /Contents bytes.
        var vriKey: String { DER.hex([UInt8](Insecure.SHA1.hash(data: Data(contents)))) }
        /// Signer certificate and timestamp signer certificates.
        var leaves: [X509] {
            guard let cms else { return [] }
            return [cms.signerCertificate].compactMap { $0 } + cms.timestampTokens.compactMap(\.signerCertificate)
        }
        var certificates: [X509] { (cms?.certificates ?? []) + (cms?.timestampTokens.flatMap(\.certificates) ?? []) }
    }

    static func signatures(in bytes: [UInt8]) -> [EmbeddedSignature] {
        var found: [EmbeddedSignature] = []
        var pos = 0
        while let br = PDFSigner.find("/ByteRange", in: bytes, from: pos) {
            pos = br + 10
            var lexer = PDFLexer(bytes: bytes, pos: pos)
            guard let items = (try? lexer.parse())?.items, items.count == 4 else { continue }
            let r = items.compactMap(\.int)
            guard r.count == 4, r[0] == 0, r[1] > 0, r[2] > r[1], r[2] <= bytes.count, r[3] >= 0, r[3] <= bytes.count - r[2],
                  bytes[r[1]] == UInt8(ascii: "<"), bytes[r[2] - 1] == UInt8(ascii: ">") else { continue }
            let contents = hexDecode(bytes[(r[1] + 1)..<(r[2] - 1)])
            let region = PDFSignatureVerifier.dictionaryRegion(bytes, around: br, contents: r[1]..<r[2])
            var subFilter: String?
            if let p = PDFSigner.find("/SubFilter", in: region, from: 0) {
                var lx = PDFLexer(bytes: region, pos: p + 10)
                subFilter = (try? lx.parse())?.nameValue
            }
            found.append(EmbeddedSignature(range: r, contents: contents, cms: CMSParts(bytes: contents),
                                           isDocumentTimestamp: subFilter == "ETSI.RFC3161", subFilter: subFilter))
        }
        return found
    }

    static func hexDecode(_ hex: ArraySlice<UInt8>) -> [UInt8] {
        var out: [UInt8] = []
        out.reserveCapacity(hex.count / 2)
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

    // MARK: DSS

    struct DSSContents {
        var certs: [[UInt8]] = []
        var ocsps: [[UInt8]] = []
        var crls: [[UInt8]] = []
        /// File offset of the DSS dictionary (or of the catalog holding it).
        var offset: Int?
    }

    static func catalog(_ file: PDFFileStructure) throws -> (num: Int, dict: PDFObject) {
        guard case .ref(let n, _)? = file.trailer["Root"] else { throw PDFStructureError.malformed("no /Root") }
        return (n, try file.object(n))
    }

    static func readDSS(_ file: PDFFileStructure) -> DSSContents? {
        guard let (rootNum, cat) = try? catalog(file), let entry = cat["DSS"], let dss = try? file.resolve(entry) else { return nil }
        var out = DSSContents()
        if case .ref(let n, _) = entry { out.offset = file.offsets[n]?.offset } else { out.offset = file.offsets[rootNum]?.offset }
        func streams(_ key: String) -> [[UInt8]] {
            ((try? file.resolve(dss[key]))?.items ?? []).compactMap {
                guard case .ref(let n, _) = $0 else { return nil }
                return try? file.streamData(n)
            }
        }
        out.certs = streams("Certs")
        out.ocsps = streams("OCSPs")
        out.crls = streams("CRLs")
        return out
    }

    /// Adds (or extends) the document security store with validation data for every signature and
    /// document timestamp in the file: "Enable Long-Term Validation".
    static func addValidationData(_ data: Data) throws -> Result {
        let file = try PDFFileStructure(data)
        let sigs = signatures(in: file.bytes).filter { $0.cms != nil }
        guard !sigs.isEmpty else { throw PDFStructureError.unsupported(L("the document has no signatures")) }
        let existing = readDSS(file)
        var pool = sigs.flatMap(\.certificates)
        pool += (existing?.certs ?? []).compactMap { X509(der: $0) }

        var perSignature: [(String, Revocation.ValidationData)] = []
        var all = Revocation.ValidationData()
        var known = Revocation.ValidationData()
        known.ocsps = existing?.ocsps ?? []
        known.crls = existing?.crls ?? []
        for sig in sigs {
            let vd = Revocation.collect(leaves: sig.leaves, pool: pool, known: known)
            known.merge(vd)
            pool += vd.certificates
            perSignature.append((sig.vriKey, vd))
            all.merge(vd)
        }
        var update = IncrementalUpdate(file: file)
        let (rootNum, cat) = try catalog(file)
        let oldDSS = try file.resolve(cat["DSS"])

        // Existing streams by content, so nothing is stored twice.
        var refs: [String: [[UInt8]: Int]] = ["Certs": [:], "OCSPs": [:], "CRLs": [:]]
        var arrays: [String: [PDFObject]] = ["Certs": [], "OCSPs": [], "CRLs": []]
        for key in ["Certs", "OCSPs", "CRLs"] {
            for item in (try file.resolve(oldDSS?[key]))?.items ?? [] {
                arrays[key]!.append(item)
                if case .ref(let n, _) = item, let bytes = try? file.streamData(n) { refs[key]![bytes] = n }
            }
        }
        func ref(_ key: String, _ bytes: [UInt8]) -> Int {
            if let n = refs[key]![bytes] { return n }
            let n = update.addStream(bytes)
            refs[key]![bytes] = n
            arrays[key]!.append(.ref(n, 0))
            return n
        }
        var vri: [(String, PDFObject)] = []
        if case .dict(let pairs)? = try file.resolve(oldDSS?["VRI"]) { vri = pairs }
        let now = PDFSigner.pdfDate(Date())
        for (key, vd) in perSignature {
            let entry = PDFObject.dict([
                ("Cert", .array(vd.certificates.map { .ref(ref("Certs", $0.der), 0) })),
                ("OCSP", .array(vd.ocsps.map { .ref(ref("OCSPs", $0), 0) })),
                ("CRL", .array(vd.crls.map { .ref(ref("CRLs", $0), 0) })),
                ("TU", .raw(now)),
            ])
            vri.removeAll { $0.0 == key }
            vri.append((key, entry))
        }
        var dss: [(String, PDFObject)] = [("Type", .name("DSS"))]
        for key in ["Certs", "OCSPs", "CRLs"] where !arrays[key]!.isEmpty { dss.append((key, .array(arrays[key]!))) }
        dss.append(("VRI", .dict(vri)))
        let dssNum = update.add(Array(PDFObject.dict(dss).serialized.utf8))
        var newCatalog = cat.setting("DSS", .ref(dssNum, 0))
        if cat["Extensions"] == nil {
            newCatalog = newCatalog.setting("Extensions", .raw("<< /ESIC << /BaseVersion /1.7 /ExtensionLevel 5 >> >>"))
        }
        update.replace(rootNum, Array(newCatalog.serialized.utf8))
        return Result(data: Data(update.write()), warnings: all.warnings)
    }

    // MARK: Document timestamp

    /// Adds an RFC 3161 document timestamp (ETSI.RFC3161) covering the whole file: B-LTA, or a
    /// timestamp-only seal of an unsigned document.
    static func addDocumentTimestamp(_ data: Data, tsa: URL) throws -> Data {
        try PDFSigner.appendSignatureField(data, visible: nil, head: "/Type /DocTimeStamp /Filter /Adobe.PPKLite /SubFilter /ETSI.RFC3161",
                                           tail: "/Prop_Build << /App << /Name /PDF42 >> >>", capacity: PDFSigner.signatureCapacity) { signed in
            Data(try Timestamp.requestToken(digest: [UInt8](SHA256.hash(data: signed)), tsa: tsa))
        }
    }

    // MARK: Analysis (verification)

    struct Analysis {
        var level: String?
        var ltvEnabled: Bool?
        var ltvProblem: String?
    }

    /// PAdES level and LTV status of each /ByteRange signature in `data` (same order as the verifier).
    static func analyze(_ data: Data) -> [Analysis] {
        let bytes = [UInt8](data)
        let sigs = signatures(in: bytes)
        guard !sigs.isEmpty else { return [] }
        let dss = (try? PDFFileStructure(data)).flatMap(readDSS)
        let dssCerts = (dss?.certs ?? []).compactMap { X509(der: $0) }
        // Offsets of the DSS of every revision (each %%EOF), to tell which signatures and timestamps it follows.
        var dssOffsets = Set<Int>()
        var p = 0
        while let eof = PDFSigner.find("%%EOF", in: bytes, from: p) {
            p = eof + 5
            if let f = try? PDFFileStructure(Data(bytes[0..<p])), let off = readDSS(f)?.offset { dssOffsets.insert(off) }
        }
        return sigs.map { sig in
            var a = Analysis()
            guard let cms = sig.cms else { return a }
            let pool = sigs.flatMap(\.certificates) + dssCerts
            // LTV: revocation data for every non-root certificate on the signer and timestamp paths.
            let paths = sig.leaves.flatMap { Revocation.chain(of: $0, pool: pool) }
            let missing = paths.filter { !$0.isSelfIssued && !Revocation.covered($0, ocsps: dss?.ocsps ?? [], crls: dss?.crls ?? []) }
            let incomplete = paths.contains { !$0.isSelfIssued && Revocation.issuer(of: $0, in: pool) == nil }
            if dss == nil {
                a.ltvEnabled = false
                a.ltvProblem = L("no validation data (DSS) in the document")
            } else if incomplete {
                a.ltvEnabled = false
                a.ltvProblem = L("the certificate chain is incomplete")
            } else if let m = missing.first {
                a.ltvEnabled = false
                a.ltvProblem = L("no revocation data for “%@”", m.name)
            } else { a.ltvEnabled = true }

            if sig.isDocumentTimestamp {
                a.level = L("Document timestamp")
                return a
            }
            guard sig.subFilter == "ETSI.CAdES.detached",
                  cms.signedAttributes[OID.signingCertificateV2] != nil || cms.signedAttributes[OID.signingCertificate] != nil else {
                a.level = sig.subFilter == "ETSI.CAdES.detached" ? L("CAdES without signing certificate (not PAdES)") : L("Basic PDF signature (not PAdES)")
                return a
            }
            var level = "PAdES B-B"
            let hasTimestamp = !cms.timestampTokens.isEmpty || sigs.contains { $0.isDocumentTimestamp && $0.range[1] > sig.coveredEnd }
            if hasTimestamp {
                level = "PAdES B-T"
                let added = dssOffsets.filter { $0 >= sig.coveredEnd }
                if a.ltvEnabled == true, let first = added.min() {
                    level = "PAdES B-LT"
                    if sigs.contains(where: { $0.isDocumentTimestamp && $0.range[1] > first }) { level = "PAdES B-LTA" }
                }
            }
            a.level = level
            return a
        }
    }
}

/// Objects appended to a file as one incremental update (classic xref table).
struct IncrementalUpdate {
    let file: PDFFileStructure
    private(set) var nextNum: Int
    private var objects: [Int: [UInt8]] = [:]

    init(file: PDFFileStructure) {
        self.file = file
        nextNum = file.trailer["Size"]?.int ?? 0
    }

    mutating func add(_ body: [UInt8]) -> Int {
        let n = nextNum
        nextNum += 1
        objects[n] = body
        return n
    }

    mutating func addStream(_ bytes: [UInt8]) -> Int {
        add(Array("<< /Length \(bytes.count) >>\nstream\n".utf8) + bytes + Array("\nendstream".utf8))
    }

    mutating func replace(_ num: Int, _ body: [UInt8]) { objects[num] = body }

    func write() -> [UInt8] {
        let size = file.trailer["Size"]?.int ?? 0
        var out = file.bytes
        if out.last != 0x0A { out.append(0x0A) }
        var entries: [(Int, Int)] = []
        for num in objects.keys.sorted() {
            entries.append((num, out.count))
            let gen = num >= size ? 0 : file.gen(num)
            out += Array("\(num) \(gen) obj\n".utf8) + objects[num]! + Array("\nendobj\n".utf8)
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
            .setting("Size", .raw("\(max(size, nextNum))")).setting("Prev", .raw("\(file.startxref)"))
        xref += "trailer\n\(trailer.serialized)\nstartxref\n\(xrefOffset)\n%%EOF\n"
        return out + Array(xref.utf8)
    }
}

extension PDFFileStructure {
    /// Decoded data of a stream object (no filter or /FlateDecode).
    func streamData(_ num: Int) throws -> [UInt8] {
        guard let entry = offsets[num], entry.offset > 0 else { throw PDFStructureError.malformed("no object \(num)") }
        var lexer = PDFLexer(bytes: bytes, pos: entry.offset)
        _ = lexer.word(); _ = lexer.word()
        try lexer.expect("obj")
        let dict = try lexer.parse()
        try lexer.expect("stream")
        if lexer.pos < bytes.count, bytes[lexer.pos] == 0x0D { lexer.pos += 1 }
        if lexer.pos < bytes.count, bytes[lexer.pos] == 0x0A { lexer.pos += 1 }
        guard let length = try resolve(dict["Length"])?.int, length >= 0, lexer.pos + length <= bytes.count else {
            throw PDFStructureError.malformed("stream length")
        }
        let raw = Array(bytes[lexer.pos..<lexer.pos + length])
        let filter = try resolve(dict["Filter"])
        let name = filter?.nameValue ?? filter?.items?.first?.nameValue
        switch name {
        case nil: return raw
        case "FlateDecode"?: return try Self.inflate(raw)
        default: throw PDFStructureError.unsupported("filter \(name ?? "")")
        }
    }

    /// zlib (RFC 1950) → bytes.
    static func inflate(_ zlib: [UInt8]) throws -> [UInt8] {
        guard zlib.count > 2 else { throw PDFStructureError.malformed("flate") }
        let raw = Array(zlib.dropFirst(2))
        var capacity = max(4096, raw.count * 8)
        while capacity <= 64 << 20 {
            var out = [UInt8](repeating: 0, count: capacity)
            let n = compression_decode_buffer(&out, capacity, raw, raw.count, nil, COMPRESSION_ZLIB)
            if n > 0, n < capacity { return Array(out[0..<n]) }
            if n == 0 { break }
            capacity *= 4
        }
        throw PDFStructureError.malformed("flate")
    }
}
