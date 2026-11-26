import Foundation

/// Headless regression checks for Office Open XML signature verification (run by `XLS42 --selftest`).
public enum DocumentSignatureSelfTest {
    /// `_rels/.rels` and its manifest Reference exactly as LibreOffice 25.2.4 wrote them when signing a DOC42 .docx
    /// (scripts/office-signatures/fixtures/libreoffice-25.2-signed.docx). LibreOffice itself reports that signature
    /// valid; its relationship-transform digest counts the line break before </Relationships>.
    static let libreOfficeRels = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">"
        + "<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/>"
        + "<Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/>"
        + "<Relationship Id=\"rId3\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/digital-signature/origin\" Target=\"_xmlsignatures/origin.sigs\"/>\n"
        + "</Relationships>"
    static let libreOfficeReference = "<Reference xmlns=\"http://www.w3.org/2000/09/xmldsig#\" URI=\"/_rels/.rels?ContentType=application/vnd.openxmlformats-package.relationships+xml\">"
        + "<Transforms><Transform Algorithm=\"http://schemas.openxmlformats.org/package/2006/RelationshipTransform\">"
        + "<mdssi:RelationshipReference xmlns:mdssi=\"http://schemas.openxmlformats.org/package/2006/digital-signature\" SourceId=\"rId1\"/></Transform>"
        + "<Transform Algorithm=\"http://www.w3.org/TR/2001/REC-xml-c14n-20010315\"/></Transforms>"
        + "<DigestMethod Algorithm=\"http://www.w3.org/2001/04/xmlenc#sha256\"/><DigestValue>jFHL+zfAGaCsbMXJYixpbic6BmOqNm0T+vhRyWkp9Y0=</DigestValue></Reference>"

    /// Returns the number of failed checks.
    public static func run() -> Int {
        var failed = 0
        func check(_ name: String, _ ok: Bool) {
            if !ok { failed += 1 }
            print(ok ? "ok  " : "FAIL", "office signature:", name)
        }
        let rels = Data(libreOfficeRels.utf8)
        let ids: Set<String> = ["rId1"]
        let standard = OOXMLSignature.relationshipTransform(rels, sourceIDs: ids, sourceTypes: [])
        let legacy = OOXMLSignature.relationshipTransform(rels, sourceIDs: ids, sourceTypes: [], libxmlsecTextNodes: true)
        check("LibreOffice relationship transform writes the text node as <text></text>",
              String(decoding: legacy, as: UTF8.self) == "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><text></text>"
                + "<Relationship Id=\"rId1\" Target=\"word/document.xml\" TargetMode=\"Internal\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\"></Relationship></Relationships>")
        check("ECMA-376 relationship transform drops the text node", !String(decoding: standard, as: UTF8.self).contains("<text>"))
        check("no text nodes: both forms agree",
              OOXMLSignature.relationshipTransform(Data(libreOfficeRels.replacingOccurrences(of: "\n<", with: "<").utf8), sourceIDs: ids, sourceTypes: [], libxmlsecTextNodes: true)
                == OOXMLSignature.relationshipTransform(Data(libreOfficeRels.replacingOccurrences(of: "\n<", with: "<").utf8), sourceIDs: ids, sourceTypes: []))
        guard let ref = try? XNode.parse(Data(libreOfficeReference.utf8)) else { check("reference parses", false); return failed }
        func matches(_ data: Data?) -> Bool {
            var covered = Set<String>()
            return (try? OOXMLSignature.manifestReferenceMatches(ref, part: data, coveredIDs: &covered)) == true && covered == ids
        }
        check("LibreOffice 25.2 manifest digest of _rels/.rels matches", matches(rels))
        check("unsigned relationship may change",
              matches(Data(libreOfficeRels.replacingOccurrences(of: "docProps/core.xml", with: "docProps/other.xml").utf8)))
        check("changed signed relationship target is rejected",
              !matches(Data(libreOfficeRels.replacingOccurrences(of: "word/document.xml", with: "word/evil.xml").utf8)))
        check("changed signed relationship type is rejected",
              !matches(Data(libreOfficeRels.replacingOccurrences(of: "relationships/officeDocument", with: "relationships/attachedTemplate").utf8)))
        check("external target mode is rejected",
              !matches(Data(libreOfficeRels.replacingOccurrences(of: "Target=\"word/document.xml\"", with: "Target=\"word/document.xml\" TargetMode=\"External\"").utf8)))
        check("removed signed relationship is rejected",
              !matches(Data(libreOfficeRels.replacingOccurrences(of: "rId1", with: "rId9").utf8)))
        check("missing part is rejected", !matches(nil))
        return failed
    }
}
