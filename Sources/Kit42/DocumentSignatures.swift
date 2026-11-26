import Foundation
import Security

/// Digital signatures inside Office Open XML (.docx/.xlsx/.pptx, `_xmlsignatures/`, ECMA-376 Part 2 §13) and
/// OpenDocument (.odt/.ods/.odp, `META-INF/documentsignatures.xml`, ODF 1.2 Part 3 §5) packages:
/// XMLDSig with XAdES signed properties, RSA or ECDSA with SHA-256.
public enum DocumentSignatures {
    public enum Kind { case ooxml, odf }

    public static func kind(of zip: ZipArchive) -> Kind? {
        if zip["[Content_Types].xml"] != nil { return .ooxml }
        if zip["META-INF/manifest.xml"] != nil || zip["mimetype"] != nil { return .odf }
        return nil
    }

    /// Signatures in a package; empty when the file is not a signed package.
    public static func verify(_ data: Data) -> [DocumentSignature] {
        guard let zip = try? ZipArchive(data: data) else { return [] }
        switch kind(of: zip) {
        case .ooxml: return OOXMLSignature.verify(zip)
        case .odf: return ODFSignature.verify(zip)
        case nil: return []
        }
    }

    public static func isSigned(_ data: Data) -> Bool {
        guard let zip = try? ZipArchive(data: data) else { return false }
        return zip.order.contains { $0.hasPrefix("_xmlsignatures/") && $0.hasSuffix(".xml") } || zip["META-INF/documentsignatures.xml"] != nil
    }

    /// Adds a signature to the package and returns the new file contents. Existing signatures are kept.
    public static func sign(_ data: Data, signer: XMLRawSigner, comment: String = "", date: Date = Date()) throws -> Data {
        var zip = try ZipArchive(data: data)
        switch kind(of: zip) {
        case .ooxml: try OOXMLSignature.sign(&zip, signer: signer, comment: comment, date: date)
        case .odf: try ODFSignature.sign(&zip, signer: signer, comment: comment, date: date)
        case nil: throw XMLDSigError.unsupported(L("not an Office or OpenDocument file"))
        }
        return zip.data()
    }

    // MARK: Shared XMLDSig pieces

    static let dsig = "http://www.w3.org/2000/09/xmldsig#"
    static let xades = "http://uri.etsi.org/01903/v1.3.2#"

    static func esc(_ s: String) -> String { C14N.escapeText(s) }
    static func attr(_ s: String) -> String { C14N.escapeAttr(s) }

    static func reference(uri: String, type: String? = nil, transforms: [String] = [], digest: Data) -> String {
        var s = "<Reference" + (type.map { " Type=\"\(attr($0))\"" } ?? "") + " URI=\"\(attr(uri))\">"
        if !transforms.isEmpty { s += "<Transforms>" + transforms.joined() + "</Transforms>" }
        s += "<DigestMethod Algorithm=\"\(XMLDigest.sha256)\"></DigestMethod><DigestValue>\(digest.base64EncodedString())</DigestValue></Reference>"
        return s
    }

    static let c14nTransform = "<Transform Algorithm=\"\(C14N.inclusive)\"></Transform>"

    /// XAdES SignedProperties (XAdES-BES with the signing certificate and an implied policy).
    static func qualifyingProperties(target: String, id: String, signer: XMLRawSigner, date: Date) throws -> String {
        guard let leaf = signer.leaf else { throw XMLDSigError.signing(L("The certificate cannot be read")) }
        return "<xd:QualifyingProperties xmlns:xd=\"\(xades)\" Target=\"#\(target)\"><xd:SignedProperties Id=\"\(id)\">"
            + "<xd:SignedSignatureProperties><xd:SigningTime>\(XMLTime.string(date))</xd:SigningTime>"
            + "<xd:SigningCertificate><xd:Cert><xd:CertDigest><DigestMethod Algorithm=\"\(XMLDigest.sha256)\"></DigestMethod>"
            + "<DigestValue>\(leaf.sha256.base64EncodedString())</DigestValue></xd:CertDigest><xd:IssuerSerial>"
            + "<X509IssuerName>\(esc(leaf.issuerName))</X509IssuerName><X509SerialNumber>\(leaf.serialDecimal)</X509SerialNumber>"
            + "</xd:IssuerSerial></xd:Cert></xd:SigningCertificate>"
            + "<xd:SignaturePolicyIdentifier><xd:SignaturePolicyImplied></xd:SignaturePolicyImplied></xd:SignaturePolicyIdentifier>"
            + "</xd:SignedSignatureProperties></xd:SignedProperties></xd:QualifyingProperties>"
    }

    /// Fills the DigestValue of same-document references, signs SignedInfo and returns the finished XML.
    /// `xml` must contain `<SignedInfo>…</SignedInfo>` with `@@DIGEST:id@@` placeholders and `@@SIGNATUREVALUE@@`.
    static func finish(_ xml: String, sameDocumentIDs: [String], signatureID: String, signer: XMLRawSigner) throws -> String {
        var xml = xml
        let draft = try XNode.parse(Data(xml.utf8))
        for id in sameDocumentIDs {
            guard let node = draft.byID(id) else { throw XMLDSigError.malformed(id) }
            let digest = try XMLDigest.digest(C14N.canonicalize(node), XMLDigest.sha256)
            xml = xml.replacingOccurrences(of: "@@DIGEST:\(id)@@", with: digest.base64EncodedString())
        }
        let doc = try XNode.parse(Data(xml.utf8))
        guard let signature = doc.byID(signatureID), let signedInfo = signature.first("SignedInfo") else {
            throw XMLDSigError.malformed("SignedInfo")
        }
        let value = try signer.signSHA256(C14N.canonicalize(signedInfo))
        return xml.replacingOccurrences(of: "@@SIGNATUREVALUE@@", with: value.base64EncodedString())
    }

    static func keyInfo(_ signer: XMLRawSigner, issuerSerial: Bool) -> String {
        var s = "<KeyInfo><X509Data>"
        if issuerSerial, let leaf = signer.leaf {
            s += "<X509IssuerSerial><X509IssuerName>\(esc(leaf.issuerName))</X509IssuerName><X509SerialNumber>\(leaf.serialDecimal)</X509SerialNumber></X509IssuerSerial>"
        }
        for c in signer.certificates { s += "<X509Certificate>\(c.base64EncodedString())</X509Certificate>" }
        return s + "</X509Data></KeyInfo>"
    }

    /// Verifies SignedInfo's signature and same-document references; `external` resolves other URIs.
    /// Returns the signature report; `problem` is set when the cryptographic check fails.
    static func verifyCore(_ signature: XNode, root: XNode, format: String,
                           external: (String, [XNode]) throws -> Data?) -> DocumentSignature {
        var report = DocumentSignature(status: .invalid, signer: L("Unknown"), format: format)
        let certs = signature.descendants { $0.localName == "X509Certificate" }
            .compactMap { Data(base64Encoded: $0.text.filter { !$0.isWhitespace }) }
            .compactMap { SecCertificateCreateWithData(nil, $0 as CFData) }
        // The signing certificate: the one XAdES names, else the first one.
        var leaf = certs.first
        if let digestNode = signature.descendants({ $0.localName == "CertDigest" }).first,
           let value = digestNode.first("DigestValue").flatMap({ Data(base64Encoded: $0.text.filter { !$0.isWhitespace }) }),
           let method = digestNode.first("DigestMethod")?.attr("Algorithm") {
            leaf = certs.first { (try? XMLDigest.digest(SecCertificateCopyData($0) as Data, method)) == value } ?? leaf
        }
        if let leaf {
            report.signer = SecCertificateCopySubjectSummary(leaf) as String? ?? L("Unknown")
            report.email = CertificateTrust.email(leaf)
            report.certificate = SecCertificateCopyData(leaf) as Data
        }
        if let t = signature.descendants({ $0.localName == "SigningTime" }).first ?? signature.descendants({ $0.localName == "Value" && $0.parent?.localName == "SignatureTime" }).first
            ?? signature.descendants({ $0.localName == "date" }).first {
            report.date = XMLTime.parse(t.text)
        }
        if let c = signature.descendants({ $0.localName == "SignatureComments" || $0.localName == "desc" || $0.localName == "description" }).first, !c.text.isEmpty {
            report.comment = c.text
        }
        guard let leaf else { report.problem = L("no certificate"); return report }
        guard let signedInfo = signature.first("SignedInfo"),
              let c14n = signedInfo.first("CanonicalizationMethod")?.attr("Algorithm"),
              let method = signedInfo.first("SignatureMethod")?.attr("Algorithm"),
              let value = signature.first("SignatureValue").flatMap({ Data(base64Encoded: $0.text.filter { !$0.isWhitespace }) }) else {
            report.problem = L("incomplete signature"); return report
        }
        guard C14N.isSupported(c14n) else { report.problem = L("unsupported canonicalization"); return report }
        do {
            for ref in signedInfo.all("Reference") {
                guard try checkReference(ref, root: root, external: external) else {
                    report.problem = L("“%@” was changed after signing", displayName(ref.attr("URI") ?? ""))
                    return report
                }
            }
        } catch {
            report.problem = error.localizedDescription
            return report
        }
        guard XMLSignatureMethod.verify(value, over: C14N.canonicalize(signedInfo, algorithm: c14n), method: method, certificate: leaf) else {
            report.problem = L("the signature value does not match")
            return report
        }
        report.trustMessage = CertificateTrust.evaluate(leaf, extra: certs.filter { $0 !== leaf }, at: report.date)
        report.status = report.trustMessage == nil ? .valid : .untrusted
        return report
    }

    static func displayName(_ uri: String) -> String {
        let path = uri.components(separatedBy: "?")[0]
        return path.hasPrefix("#") ? L("signature properties") : (path.removingPercentEncoding ?? path).trimmingCharacters(in: CharacterSet(charactersIn: "/"))
    }

    /// Digest check of one Reference.
    static func checkReference(_ ref: XNode, root: XNode, external: (String, [XNode]) throws -> Data?) throws -> Bool {
        guard let uri = ref.attr("URI"), let method = ref.first("DigestMethod")?.attr("Algorithm"),
              let expected = ref.first("DigestValue").flatMap({ Data(base64Encoded: $0.text.filter { !$0.isWhitespace }) }) else {
            throw XMLDSigError.malformed("Reference")
        }
        let transforms = ref.first("Transforms")?.all("Transform") ?? []
        let octets: Data
        if uri.hasPrefix("#") {
            guard let node = root.byID(String(uri.dropFirst())) else { throw XMLDSigError.malformed(uri) }
            let algorithm = transforms.compactMap { $0.attr("Algorithm") }.last { C14N.isSupported($0) } ?? C14N.inclusive
            octets = C14N.canonicalize(node, algorithm: algorithm)
        } else {
            guard let data = try external(uri, transforms) else { return false }
            octets = data
        }
        return try XMLDigest.digest(octets, method) == expected
    }

    /// Applies a chain of c14n transforms to an XML stream from the package.
    static func applyXMLTransforms(_ data: Data, _ transforms: [XNode]) throws -> Data {
        guard let c14n = transforms.compactMap({ $0.attr("Algorithm") }).last(where: C14N.isSupported) else { return data }
        return C14N.canonicalize(try XNode.parse(data), algorithm: c14n)
    }
}

// MARK: - Office Open XML

enum OOXMLSignature {
    static let originType = "http://schemas.openxmlformats.org/package/2006/relationships/digital-signature/origin"
    static let signatureType = "http://schemas.openxmlformats.org/package/2006/relationships/digital-signature/signature"
    static let relationshipTransform = "http://schemas.openxmlformats.org/package/2006/RelationshipTransform"
    static let relsContentType = "application/vnd.openxmlformats-package.relationships+xml"
    static let originContentType = "application/vnd.openxmlformats-package.digital-signature-origin"
    static let signatureContentType = "application/vnd.openxmlformats-package.digital-signature-xmlsignature+xml"
    static let mdssi = "http://schemas.openxmlformats.org/package/2006/digital-signature"
    static let relsNS = "http://schemas.openxmlformats.org/package/2006/relationships"

    /// Relationship types whose targets Office signs (the last path component of the type URI, as Apache POI lists them).
    /// Core and extended properties, thumbnails and the signature origin are deliberately not signed.
    static let signedRelationships: Set<String> = [
        "activeXControl", "aFChunk", "attachedTemplate", "attachedToolbars", "audio", "calcChain", "chart", "chartColorStyle",
        "chartLayout", "chartsheet", "chartStyle", "chartUserShapes", "commentAuthors", "comments", "connections", "connectorXml",
        "control", "ctrlProp", "customData", "customProperty", "customXml", "diagram", "diagramColors", "diagramColorsHeader",
        "diagramData", "diagramDrawing", "diagramLayout", "diagramLayoutHeader", "diagramQuickStyle", "diagramQuickStyleHeader",
        "dialogsheet", "dictionary", "documentParts", "downRev", "drawing", "endnotes", "externalLink", "externalLinkPath", "font",
        "fontTable", "footer", "footnotes", "functionPrototypes", "glossaryDocument", "graphicFrameDoc", "groupShapeXml",
        "handoutMaster", "hdphoto", "header", "hyperlink", "image", "ink", "inkXml", "keyMapCustomizations", "legacyDiagramText",
        "legacyDocTextInfo", "mailMergeHeaderSource", "mailMergeRecipientData", "mailMergeSource", "media", "notesMaster",
        "notesSlide", "numbering", "officeDocument", "oleObject", "package", "pictureXml", "pivotCacheDefinition",
        "pivotCacheRecords", "pivotTable", "powerPivotData", "presProps", "printerSettings", "queryTable", "recipientData",
        "settings", "shapeXml", "sharedStrings", "sheetMetadata", "slicer", "slicerCache", "slide", "slideLayout", "slideMaster",
        "slideUpdateInfo", "slideUpdateUrl", "smartTags", "styles", "stylesWithEffects", "table", "tableSingleCells",
        "tableStyles", "tags", "theme", "themeOverride", "timeline", "timelineCache", "transform", "ui/altText", "ui/buttonSize",
        "ui/controlID", "ui/description", "ui/enabled", "ui/extensibility", "ui/helperText", "ui/imageID", "ui/imageMso",
        "ui/keyTip", "ui/label", "ui/lcid", "ui/loud", "ui/pressed", "ui/progID", "ui/ribbonID", "ui/showImage", "ui/showLabel",
        "ui/supertip", "ui/target", "ui/text", "ui/title", "ui/tooltip", "ui/userCustomization", "ui/visible", "userXmlData",
        "vbaProject", "video", "viewProps", "vmlDrawing", "volatileDependencies", "webSettings", "wordVbaData", "worksheet",
        "wsSortMap", "xlBinaryIndex", "xlExternalLinkPath/xlAlternateStartup", "xlExternalLinkPath/xlLibrary",
        "xlExternalLinkPath/xlPathMissing", "xlExternalLinkPath/xlStartup", "xlIntlMacrosheet", "xlMacrosheet",
    ]

    static func isSigned(_ type: String) -> Bool {
        guard let r = type.range(of: "/relationships/", options: .backwards) else { return false }
        return signedRelationships.contains(String(type[r.upperBound...]))
    }

    struct Relationship { var id, type, target: String; var external: Bool }

    static func relationships(_ data: Data) -> [Relationship] {
        guard let root = try? XNode.parse(data) else { return [] }
        return root.all("Relationship").compactMap {
            guard let id = $0.attr("Id"), let type = $0.attr("Type"), let target = $0.attr("Target") else { return nil }
            return Relationship(id: id, type: type, target: target, external: $0.attr("TargetMode") == "External")
        }
    }

    /// Content type of a part from [Content_Types].xml.
    static func contentTypes(_ zip: ZipArchive) -> (String) -> String? {
        let root = zip["[Content_Types].xml"].flatMap { try? XNode.parse($0) }
        var defaults: [String: String] = [:], overrides: [String: String] = [:]
        for d in root?.all("Default") ?? [] { if let e = d.attr("Extension"), let c = d.attr("ContentType") { defaults[e.lowercased()] = c } }
        for o in root?.all("Override") ?? [] { if let p = o.attr("PartName"), let c = o.attr("ContentType") { overrides[p.lowercased()] = c } }
        return { part in
            overrides[part.lowercased()] ?? defaults[(part as NSString).pathExtension.lowercased()]
        }
    }

    /// "/word/_rels/document.xml.rels" → "/word/document.xml"; "/_rels/.rels" → "/".
    static func sourcePart(ofRels rels: String) -> String {
        let dir = (rels as NSString).deletingLastPathComponent           // /word/_rels
        let parent = (dir as NSString).deletingLastPathComponent         // /word
        let name = String((rels as NSString).lastPathComponent.dropLast(5)) // document.xml
        return name.isEmpty ? "/" : (parent == "/" ? "/" + name : parent + "/" + name)
    }

    static func resolve(_ target: String, from source: String) -> String {
        if target.hasPrefix("/") { return normalize(target) }
        let base = source == "/" ? "/" : (source as NSString).deletingLastPathComponent
        return normalize((base as NSString).appendingPathComponent(target.removingPercentEncoding ?? target))
    }

    static func normalize(_ path: String) -> String {
        var parts: [Substring] = []
        for p in path.split(separator: "/") {
            if p == "." || p.isEmpty { continue }
            if p == ".." { if !parts.isEmpty { parts.removeLast() } } else { parts.append(p) }
        }
        return "/" + parts.joined(separator: "/")
    }

    /// The ECMA-376 relationship transform: selected relationships, sorted by Id, with an explicit TargetMode, canonical.
    static func relationshipTransform(_ data: Data, sourceIDs: Set<String>, sourceTypes: Set<String>) -> Data {
        let selected = relationships(data).filter { sourceIDs.contains($0.id) || sourceTypes.contains($0.type) }
            .sorted { Array($0.id.utf8).lexicographicallyPrecedes(Array($1.id.utf8)) }
        var s = "<Relationships xmlns=\"\(relsNS)\">"
        for r in selected {
            s += "<Relationship Id=\"\(C14N.escapeAttr(r.id))\" Target=\"\(C14N.escapeAttr(r.target))\" TargetMode=\"\(r.external ? "External" : "Internal")\" Type=\"\(C14N.escapeAttr(r.type))\"></Relationship>"
        }
        return Data((s + "</Relationships>").utf8)
    }

    struct ManifestEntry {
        var uri: String
        var part: String
        var sourceIDs: [String] = []
        var isRels: Bool { !sourceIDs.isEmpty }
    }

    /// Parts and relationships a signature should cover (Apache POI's OOXMLSignatureFacet selection, which Office accepts).
    static func expectedManifest(_ zip: ZipArchive) -> [ManifestEntry] {
        let contentType = contentTypes(zip)
        var entries: [ManifestEntry] = []
        var digested = Set<String>()
        for name in zip.order where name.hasSuffix(".rels") && !name.hasPrefix("_xmlsignatures/") {
            let relsPart = "/" + name
            let source = sourcePart(ofRels: relsPart)
            var ids: [String] = []
            for r in relationships(zip[name] ?? Data()) where isSigned(r.type) {
                ids.append(r.id)
                if r.external { continue }
                let part = resolve(r.target, from: source)
                guard !digested.contains(part), zip[String(part.dropFirst())] != nil, let ct = contentType(part) else { continue }
                if r.type.hasSuffix("customXml"), !(ct == "inkml+xml" || ct == "text/xml") { continue }
                digested.insert(part)
                entries.append(ManifestEntry(uri: part + "?ContentType=" + ct, part: part))
            }
            if !ids.isEmpty {
                entries.append(ManifestEntry(uri: relsPart + "?ContentType=" + relsContentType, part: relsPart, sourceIDs: ids))
            }
        }
        return entries.sorted { $0.uri < $1.uri }
    }

    static func sign(_ zip: inout ZipArchive, signer: XMLRawSigner, comment: String, date: Date) throws {
        // Origin part and relationships (once per package).
        var types = zip.string("[Content_Types].xml") ?? ""
        if !types.contains("Extension=\"sigs\"") {
            types = types.replacingOccurrences(of: "</Types>", with: "<Default Extension=\"sigs\" ContentType=\"\(originContentType)\"/></Types>")
        }
        var rootRels = zip.string("_rels/.rels") ?? "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"\(relsNS)\"></Relationships>"
        var origin = relationships(Data(rootRels.utf8)).first { $0.type == originType }.map { resolve($0.target, from: "/") }
        if origin == nil {
            let id = freeID(relationships(Data(rootRels.utf8)))
            rootRels = insertRelationship(rootRels, id: id, type: originType, target: "_xmlsignatures/origin.sigs")
            zip["_rels/.rels"] = Data(rootRels.utf8)
            origin = "/_xmlsignatures/origin.sigs"
        }
        let originPart = origin!
        if zip[String(originPart.dropFirst())] == nil { zip[String(originPart.dropFirst())] = Data() }
        let originRelsName = String(((originPart as NSString).deletingLastPathComponent as NSString)
            .appendingPathComponent("_rels/" + (originPart as NSString).lastPathComponent + ".rels").dropFirst())
        var originRels = zip.string(originRelsName) ?? "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"\(relsNS)\"></Relationships>"
        let existing = Set(zip.order.filter { $0.hasPrefix("_xmlsignatures/") })
        var n = 1
        while existing.contains("_xmlsignatures/sig\(n).xml") { n += 1 }
        let sigName = "_xmlsignatures/sig\(n).xml"
        originRels = insertRelationship(originRels, id: freeID(relationships(Data(originRels.utf8))), type: signatureType,
                                        target: (sigName as NSString).lastPathComponent)
        types = types.replacingOccurrences(of: "</Types>", with: "<Override PartName=\"/\(sigName)\" ContentType=\"\(signatureContentType)\"/></Types>")

        // Manifest of signed parts, computed on the package as it will be written.
        zip["[Content_Types].xml"] = Data(types.utf8)
        zip[originRelsName] = Data(originRels.utf8)
        // Office and Apache POI drop whitespace between relationships in the relationship transform, LibreOffice keeps
        // it. Relationship parts without such whitespace give the same digest in both (LibreOffice rewrites them too).
        for name in zip.order where name.hasSuffix(".rels") {
            if let data = zip[name], let compact = compactRelationships(data) { zip[name] = compact }
        }
        var manifest = ""
        for e in expectedManifest(zip) {
            let raw = zip[String(e.part.dropFirst())] ?? Data()
            if e.isRels {
                let refs = e.sourceIDs.map { "<mdssi:RelationshipReference xmlns:mdssi=\"\(mdssi)\" SourceId=\"\(C14N.escapeAttr($0))\"></mdssi:RelationshipReference>" }.joined()
                let digest = try XMLDigest.digest(relationshipTransform(raw, sourceIDs: Set(e.sourceIDs), sourceTypes: []), XMLDigest.sha256)
                manifest += DocumentSignatures.reference(uri: e.uri, transforms: ["<Transform Algorithm=\"\(relationshipTransform)\">\(refs)</Transform>", DocumentSignatures.c14nTransform], digest: digest)
            } else {
                manifest += DocumentSignatures.reference(uri: e.uri, digest: try XMLDigest.digest(raw, XMLDigest.sha256))
            }
        }

        let esc = DocumentSignatures.esc
        let objectType = "http://www.w3.org/2000/09/xmldsig#Object"
        let signedInfo = "<SignedInfo><CanonicalizationMethod Algorithm=\"\(C14N.inclusive)\"></CanonicalizationMethod>"
            + "<SignatureMethod Algorithm=\"\(signer.signatureMethod)\"></SignatureMethod>"
            + "<Reference Type=\"\(objectType)\" URI=\"#idPackageObject\"><DigestMethod Algorithm=\"\(XMLDigest.sha256)\"></DigestMethod><DigestValue>@@DIGEST:idPackageObject@@</DigestValue></Reference>"
            + "<Reference Type=\"\(objectType)\" URI=\"#idOfficeObject\"><DigestMethod Algorithm=\"\(XMLDigest.sha256)\"></DigestMethod><DigestValue>@@DIGEST:idOfficeObject@@</DigestValue></Reference>"
            + "<Reference Type=\"http://uri.etsi.org/01903#SignedProperties\" URI=\"#idSignedProperties\"><Transforms>\(DocumentSignatures.c14nTransform)</Transforms><DigestMethod Algorithm=\"\(XMLDigest.sha256)\"></DigestMethod><DigestValue>@@DIGEST:idSignedProperties@@</DigestValue></Reference>"
            + "</SignedInfo>"
        let packageObject = "<Object Id=\"idPackageObject\"><Manifest>\(manifest)</Manifest><SignatureProperties>"
            + "<SignatureProperty Id=\"idSignatureTime\" Target=\"#idPackageSignature\"><mdssi:SignatureTime xmlns:mdssi=\"\(mdssi)\">"
            + "<mdssi:Format>YYYY-MM-DDThh:mm:ssTZD</mdssi:Format><mdssi:Value>\(XMLTime.string(date))</mdssi:Value></mdssi:SignatureTime>"
            + "</SignatureProperty></SignatureProperties></Object>"
        let officeObject = "<Object Id=\"idOfficeObject\"><SignatureProperties><SignatureProperty Id=\"idOfficeV1Details\" Target=\"#idPackageSignature\">"
            + "<SignatureInfoV1 xmlns=\"http://schemas.microsoft.com/office/2006/digsig\"><SetupID></SetupID><SignatureText></SignatureText>"
            + "<SignatureImage></SignatureImage><SignatureComments>\(esc(comment))</SignatureComments><WindowsVersion>10.0</WindowsVersion>"
            + "<OfficeVersion>16.0</OfficeVersion><ApplicationVersion>16.0</ApplicationVersion><Monitors>1</Monitors>"
            + "<HorizontalResolution>1920</HorizontalResolution><VerticalResolution>1080</VerticalResolution><ColorDepth>32</ColorDepth>"
            + "<SignatureProviderId>{00000000-0000-0000-0000-000000000000}</SignatureProviderId><SignatureProviderUrl></SignatureProviderUrl>"
            + "<SignatureProviderDetails>9</SignatureProviderDetails><ManifestHashAlgorithm>\(XMLDigest.sha256)</ManifestHashAlgorithm>"
            + "<SignatureType>1</SignatureType></SignatureInfoV1></SignatureProperty></SignatureProperties></Object>"
        let xadesObject = "<Object>" + (try DocumentSignatures.qualifyingProperties(target: "idPackageSignature", id: "idSignedProperties", signer: signer, date: date)) + "</Object>"
        let draft = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?><Signature xmlns=\"\(DocumentSignatures.dsig)\" Id=\"idPackageSignature\">"
            + signedInfo + "<SignatureValue>@@SIGNATUREVALUE@@</SignatureValue>" + DocumentSignatures.keyInfo(signer, issuerSerial: false)
            + packageObject + officeObject + xadesObject + "</Signature>"
        let xml = try DocumentSignatures.finish(draft, sameDocumentIDs: ["idPackageObject", "idOfficeObject", "idSignedProperties"],
                                                signatureID: "idPackageSignature", signer: signer)
        zip[sigName] = Data(xml.utf8)
    }

    /// The relationships part without whitespace between elements; nil when it has none or cannot be read.
    static func compactRelationships(_ data: Data) -> Data? {
        guard let root = try? XNode.parse(data), root.localName == "Relationships",
              root.children.contains(where: { if case .text = $0 { return true } else { return false } }),
              root.elements.allSatisfy({ $0.localName == "Relationship" && $0.elements.isEmpty }) else { return nil }
        var s = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"\(relsNS)\">"
        for r in root.elements {
            s += "<Relationship" + r.attributes.map { " \($0.0)=\"\(xmlEscape($0.1))\"" }.joined() + "/>"
        }
        return Data((s + "</Relationships>").utf8)
    }

    static func freeID(_ rels: [Relationship]) -> String {
        let ids = Set(rels.map(\.id))
        var n = rels.count + 1
        while ids.contains("rId\(n)") { n += 1 }
        return "rId\(n)"
    }

    static func insertRelationship(_ xml: String, id: String, type: String, target: String) -> String {
        let rel = "<Relationship Id=\"\(id)\" Type=\"\(type)\" Target=\"\(xmlEscape(target))\"/>"
        if let r = xml.range(of: "</Relationships>", options: .backwards) { return xml.replacingCharacters(in: r, with: rel + "</Relationships>") }
        // <Relationships .../> (empty, self-closing)
        if let r = xml.range(of: "/>", options: .backwards) { return xml.replacingCharacters(in: r, with: ">" + rel + "</Relationships>") }
        return xml
    }

    /// Signature parts listed by the origin, else every XML file in _xmlsignatures/.
    static func signatureParts(_ zip: ZipArchive) -> [String] {
        let rootRels = relationships(zip["_rels/.rels"] ?? Data())
        if let origin = rootRels.first(where: { $0.type == originType }).map({ resolve($0.target, from: "/") }) {
            let relsName = String(((origin as NSString).deletingLastPathComponent as NSString)
                .appendingPathComponent("_rels/" + (origin as NSString).lastPathComponent + ".rels").dropFirst())
            let parts = relationships(zip[relsName] ?? Data()).filter { $0.type == signatureType }
                .map { String(resolve($0.target, from: origin).dropFirst()) }.filter { zip[$0] != nil }
            if !parts.isEmpty { return parts }
        }
        return zip.order.filter { $0.hasPrefix("_xmlsignatures/") && $0.hasSuffix(".xml") }
    }

    static func verify(_ zip: ZipArchive) -> [DocumentSignature] {
        signatureParts(zip).compactMap { name in
            guard let data = zip[name] else { return nil }
            guard let root = try? XNode.parse(data) else {
                return DocumentSignature(status: .invalid, signer: L("Unknown"), problem: L("damaged signature part"), format: "XMLDSig")
            }
            let signature = root.localName == "Signature" ? root : (root.descendants { $0.localName == "Signature" }.first ?? root)
            let hasXAdES = !signature.descendants { $0.localName == "SignedProperties" }.isEmpty
            let format = hasXAdES ? "XAdES (Office Open XML)" : "XMLDSig (Office Open XML)"
            var report = DocumentSignatures.verifyCore(signature, root: signature, format: format) { _, _ in nil }
            guard report.problem == nil else { return report }
            // The package object must be signed; its manifest references the parts.
            guard let packageObject = signature.byID("idPackageObject") ?? signature.descendants({ $0.localName == "Manifest" }).first?.parent,
                  let manifestNode = packageObject.first("Manifest"),
                  signature.first("SignedInfo")?.all("Reference").contains(where: { $0.attr("URI") == "#" + (packageObject.attr("Id") ?? "") }) == true else {
                report.status = .invalid
                report.problem = L("the package manifest is not signed")
                return report
            }
            var covered = Set<String>()
            var coveredIDs: [String: Set<String>] = [:]
            for ref in manifestNode.all("Reference") {
                let uri = ref.attr("URI") ?? ""
                let part = normalize((uri.components(separatedBy: "?")[0]).removingPercentEncoding ?? uri)
                covered.insert(part)
                do {
                    let ok = try DocumentSignatures.checkReference(ref, root: signature) { _, transforms in
                        guard let raw = zip[String(part.dropFirst())] else { return nil }
                        var data = raw
                        for t in transforms {
                            if t.attr("Algorithm") == relationshipTransform {
                                let ids = Set(t.all("RelationshipReference").compactMap { $0.attr("SourceId") })
                                let types = Set(t.all("RelationshipsGroupReference").compactMap { $0.attr("SourceType") })
                                coveredIDs[part, default: []].formUnion(ids)
                                data = relationshipTransform(data, sourceIDs: ids, sourceTypes: types)
                            } else if let a = t.attr("Algorithm"), C14N.isSupported(a) {
                                data = C14N.canonicalize(try XNode.parse(data), algorithm: a)
                            } else {
                                throw XMLDSigError.unsupported(t.attr("Algorithm") ?? "transform")
                            }
                        }
                        return data
                    }
                    if !ok {
                        report.status = .invalid
                        report.problem = L("“%@” was changed after signing", DocumentSignatures.displayName(uri))
                        return report
                    }
                } catch {
                    report.status = .invalid
                    report.problem = error.localizedDescription
                    return report
                }
            }
            // Parts or relationships that a signature should cover but this one does not (added after signing).
            for e in expectedManifest(zip) {
                if e.isRels {
                    let missing = Set(e.sourceIDs).subtracting(coveredIDs[e.part] ?? [])
                    if !missing.isEmpty { report.unsignedParts.append(e.part.dropFirst() + " (" + missing.sorted().joined(separator: ", ") + ")") }
                } else if !covered.contains(e.part) {
                    report.unsignedParts.append(String(e.part.dropFirst()))
                }
            }
            if !report.unsignedParts.isEmpty, report.status != .invalid {
                report.status = .invalid
                report.problem = L("content was added after signing: %@", report.unsignedParts.prefix(3).joined(separator: ", "))
            }
            return report
        }
    }
}

// MARK: - OpenDocument

enum ODFSignature {
    static let signaturesName = "META-INF/documentsignatures.xml"
    static let containerNS = "urn:oasis:names:tc:opendocument:xmlns:digitalsignature:1.0"
    static let excluded: Set<String> = [signaturesName, "META-INF/macrosignatures.xml", "META-INF/packagesignatures.xml"]

    /// Streams a document signature covers: everything but the signature files. LibreOffice treats a signature that
    /// covers META-INF/manifest.xml (its "OOo 3.2" form) as valid only if the mimetype stream is signed as well.
    static func signedStreams(_ zip: ZipArchive) -> [String] {
        zip.order.filter { !excluded.contains($0) && !$0.hasSuffix("/") }
    }

    static func encode(_ name: String) -> String {
        name.addingPercentEncoding(withAllowedCharacters: CharacterSet.urlPathAllowed.subtracting(CharacterSet(charactersIn: "#?"))) ?? name
    }

    static func digest(_ name: String, _ data: Data) throws -> (Data, Bool) {
        if name.lowercased().hasSuffix(".xml"), let root = try? XNode.parse(data) {
            return (try XMLDigest.digest(C14N.canonicalizeDocument(root), XMLDigest.sha256), true)
        }
        return (try XMLDigest.digest(data, XMLDigest.sha256), false)
    }

    static func sign(_ zip: inout ZipArchive, signer: XMLRawSigner, comment: String, date: Date) throws {
        let uid = UUID().uuidString.replacingOccurrences(of: "-", with: "")
        let sigID = "ID_" + uid
        let dateID = "ID_" + uid + "_date", descID = "ID_" + uid + "_desc", propsID = "idSignedProperties_" + uid
        var refs = ""
        for name in signedStreams(zip) {
            guard let data = zip[name] else { continue }
            let (value, xml) = try digest(name, data)
            refs += DocumentSignatures.reference(uri: encode(name), transforms: xml ? [DocumentSignatures.c14nTransform] : [], digest: value)
        }
        refs += "<Reference URI=\"#\(dateID)\"><DigestMethod Algorithm=\"\(XMLDigest.sha256)\"></DigestMethod><DigestValue>@@DIGEST:\(dateID)@@</DigestValue></Reference>"
        if !comment.isEmpty {
            refs += "<Reference URI=\"#\(descID)\"><DigestMethod Algorithm=\"\(XMLDigest.sha256)\"></DigestMethod><DigestValue>@@DIGEST:\(descID)@@</DigestValue></Reference>"
        }
        refs += "<Reference Type=\"http://uri.etsi.org/01903#SignedProperties\" URI=\"#\(propsID)\"><Transforms>\(DocumentSignatures.c14nTransform)</Transforms><DigestMethod Algorithm=\"\(XMLDigest.sha256)\"></DigestMethod><DigestValue>@@DIGEST:\(propsID)@@</DigestValue></Reference>"

        // dc:date in the form LibreOffice writes (local time without a zone).
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.timeZone = TimeZone(identifier: "UTC")
        f.dateFormat = "yyyy-MM-dd'T'HH:mm:ss'.000000000'"
        var objects = "<Object><SignatureProperties><SignatureProperty Id=\"\(dateID)\" Target=\"#\(sigID)\">"
            + "<dc:date xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\(f.string(from: date))</dc:date></SignatureProperty></SignatureProperties></Object>"
        if !comment.isEmpty {
            objects += "<Object><SignatureProperties><SignatureProperty Id=\"\(descID)\" Target=\"#\(sigID)\">"
                + "<dc:description xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\(DocumentSignatures.esc(comment))</dc:description></SignatureProperty></SignatureProperties></Object>"
        }
        objects += "<Object>" + (try DocumentSignatures.qualifyingProperties(target: sigID, id: propsID, signer: signer, date: date)) + "</Object>"
        let signature = "<Signature xmlns=\"\(DocumentSignatures.dsig)\" Id=\"\(sigID)\"><SignedInfo>"
            + "<CanonicalizationMethod Algorithm=\"\(C14N.inclusive)\"></CanonicalizationMethod>"
            + "<SignatureMethod Algorithm=\"\(signer.signatureMethod)\"></SignatureMethod>\(refs)</SignedInfo>"
            + "<SignatureValue>@@SIGNATUREVALUE@@</SignatureValue>" + DocumentSignatures.keyInfo(signer, issuerSerial: true) + objects + "</Signature>"

        var container = zip.string(signaturesName) ?? "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<document-signatures xmlns=\"\(containerNS)\"></document-signatures>"
        if container.contains("<document-signatures xmlns=\"\(containerNS)\"/>") {
            container = container.replacingOccurrences(of: "<document-signatures xmlns=\"\(containerNS)\"/>", with: "<document-signatures xmlns=\"\(containerNS)\"></document-signatures>")
        }
        guard let end = container.range(of: "</document-signatures>", options: .backwards) else { throw XMLDSigError.malformed(signaturesName) }
        let draft = container.replacingCharacters(in: end, with: signature + "</document-signatures>")
        let ids = [dateID] + (comment.isEmpty ? [] : [descID]) + [propsID]
        zip[signaturesName] = Data(try DocumentSignatures.finish(draft, sameDocumentIDs: ids, signatureID: sigID, signer: signer).utf8)
        // ODF 1.2 lists META-INF/documentsignatures.xml only optionally; LibreOffice does not, so neither do we.
    }

    static func verify(_ zip: ZipArchive) -> [DocumentSignature] {
        guard let data = zip[signaturesName] else { return [] }
        guard let root = try? XNode.parse(data) else {
            return [DocumentSignature(status: .invalid, signer: L("Unknown"), problem: L("damaged signature part"), format: "ODF")]
        }
        return root.all("Signature").map { signature in
            let format = signature.descendants { $0.localName == "SignedProperties" }.isEmpty ? "XMLDSig (OpenDocument)" : "XAdES (OpenDocument)"
            var signed = Set<String>()
            var report = DocumentSignatures.verifyCore(signature, root: signature, format: format) { uri, transforms in
                let name = uri.removingPercentEncoding ?? uri
                signed.insert(name)
                guard let raw = zip[name] else { return nil }
                return try DocumentSignatures.applyXMLTransforms(raw, transforms)
            }
            if report.problem == nil {
                // Signatures from before ODF 1.2 (OpenOffice 3.0 and older) cover neither the manifest nor the mimetype.
                let legacy = !signed.contains("META-INF/manifest.xml")
                report.unsignedParts = signedStreams(zip).filter {
                    !signed.contains($0) && !(legacy && ($0 == "mimetype" || $0 == "META-INF/manifest.xml"))
                }
                if !report.unsignedParts.isEmpty {
                    report.status = .invalid
                    report.problem = L("content was added after signing: %@", report.unsignedParts.prefix(3).joined(separator: ", "))
                }
            }
            return report
        }
    }
}
