#!/usr/bin/env python3
"""Independent XMLDSig check of Office Open XML and OpenDocument signatures.

Does not share code with Suite42: canonicalisation is libxml2's (lxml, C14N 1.0), signature math is OpenSSL's
(`cryptography`). For OOXML it applies the ECMA-376 relationship transform and checks every Manifest reference;
for ODF every stream reference. It also checks that the XAdES SigningCertificate digest names the KeyInfo certificate.

Usage: verify_xmldsig.py file.docx|xlsx|odt|ods [...]
Exit status 0 when every signature in every file verifies cryptographically (trust is not evaluated).
"""
import base64
from copy import deepcopy
import hashlib
import sys
import urllib.parse
import zipfile

from cryptography import x509
from cryptography.hazmat.primitives import hashes
from cryptography.hazmat.primitives.asymmetric import ec, padding, utils
from lxml import etree

DS = "http://www.w3.org/2000/09/xmldsig#"
NS = {"ds": DS, "xd": "http://uri.etsi.org/01903/v1.3.2#",
      "mdssi": "http://schemas.openxmlformats.org/package/2006/digital-signature"}
C14N = {"http://www.w3.org/TR/2001/REC-xml-c14n-20010315": (False, False),
        "http://www.w3.org/TR/2001/REC-xml-c14n-20010315#WithComments": (False, True),
        "http://www.w3.org/2001/10/xml-exc-c14n#": (True, False)}
DIGESTS = {"http://www.w3.org/2000/09/xmldsig#sha1": hashlib.sha1,
           "http://www.w3.org/2001/04/xmlenc#sha256": hashlib.sha256,
           "http://www.w3.org/2001/04/xmldsig-more#sha384": hashlib.sha384,
           "http://www.w3.org/2001/04/xmlenc#sha512": hashlib.sha512}
SIGS = {"http://www.w3.org/2000/09/xmldsig#rsa-sha1": ("rsa", hashes.SHA1()),
        "http://www.w3.org/2001/04/xmldsig-more#rsa-sha256": ("rsa", hashes.SHA256()),
        "http://www.w3.org/2001/04/xmldsig-more#rsa-sha512": ("rsa", hashes.SHA512()),
        "http://www.w3.org/2001/04/xmldsig-more#ecdsa-sha256": ("ec", hashes.SHA256())}
REL_TRANSFORM = "http://schemas.openxmlformats.org/package/2006/RelationshipTransform"
RELS_NS = "http://schemas.openxmlformats.org/package/2006/relationships"


def c14n(node, algorithm):
    exclusive, comments = C14N[algorithm]
    if isinstance(node, etree._Element) and node.getparent() is not None:
        # libxml2 2.13/2.14 emits spurious xmlns="" when canonicalising a document subset whose default namespace is
        # declared on an ancestor. Canonicalise a standalone copy instead: the apex carries every in-scope namespace
        # (inclusive C14N renders them all there anyway), descendants are copied unchanged.
        copy = etree.Element(node.tag, attrib=dict(node.attrib), nsmap=node.nsmap if not exclusive else None)
        copy.text = node.text
        for child in node:
            copy.append(deepcopy(child))
        node = copy
    return etree.tostring(node, method="c14n", exclusive=exclusive, with_comments=comments)


def rel_transform(data, ids, types):
    root = etree.fromstring(data)
    rels = [r for r in root.findall("{%s}Relationship" % RELS_NS) if r.get("Id") in ids or r.get("Type") in types]
    out = etree.Element("{%s}Relationships" % RELS_NS, nsmap={None: RELS_NS})
    for r in sorted(rels, key=lambda r: r.get("Id").encode()):
        e = etree.SubElement(out, "{%s}Relationship" % RELS_NS)
        for a in ("Id", "Type", "Target"):
            e.set(a, r.get(a))
        e.set("TargetMode", r.get("TargetMode") or "Internal")
    return etree.tostring(out, method="c14n")


def digest_ok(ref, octets):
    method = ref.find("ds:DigestMethod", NS).get("Algorithm")
    expected = base64.b64decode("".join(ref.find("ds:DigestValue", NS).text.split()))
    return DIGESTS[method](octets).digest() == expected


def check_signature(sig, zf, kind):
    problems = []
    ids = {e.get("Id"): e for e in sig.iter() if e.get("Id")}
    si = sig.find("ds:SignedInfo", NS)
    referenced = []
    for ref in si.findall("ds:Reference", NS):
        uri = ref.get("URI")
        transforms = [t.get("Algorithm") for t in ref.findall("ds:Transforms/ds:Transform", NS)]
        if uri.startswith("#"):
            alg = next((t for t in transforms if t in C14N), "http://www.w3.org/TR/2001/REC-xml-c14n-20010315")
            octets = c14n(ids[uri[1:]], alg)
        else:
            name = urllib.parse.unquote(uri)
            referenced.append(name)
            octets = zf.read(name)
            alg = next((t for t in transforms if t in C14N), None)
            if alg:
                octets = c14n(etree.fromstring(octets).getroottree(), alg)
        if not digest_ok(ref, octets):
            problems.append("digest mismatch: " + uri)
    if kind == "ooxml":
        manifest = sig.find("ds:Object/ds:Manifest", NS)
        if manifest is None:
            problems.append("no manifest")
        for ref in manifest.findall("ds:Reference", NS) if manifest is not None else []:
            part = urllib.parse.unquote(ref.get("URI").split("?")[0]).lstrip("/")
            data = zf.read(part)
            for t in ref.findall("ds:Transforms/ds:Transform", NS):
                alg = t.get("Algorithm")
                if alg == REL_TRANSFORM:
                    rids = {r.get("SourceId") for r in t.findall("mdssi:RelationshipReference", NS)}
                    types = {r.get("SourceType") for r in t.findall("mdssi:RelationshipsGroupReference", NS)}
                    data = rel_transform(data, rids, types)
                elif alg in C14N:
                    data = c14n(etree.fromstring(data).getroottree(), alg)
            if not digest_ok(ref, data):
                problems.append("manifest digest mismatch: " + part)
    else:
        skip = {"mimetype", "META-INF/documentsignatures.xml", "META-INF/macrosignatures.xml"}
        missing = [n for n in zf.namelist() if n not in skip and not n.endswith("/") and n not in referenced]
        if missing:
            problems.append("unsigned streams: " + ", ".join(missing))

    certs = [x509.load_der_x509_certificate(base64.b64decode("".join(c.text.split())))
             for c in sig.findall("ds:KeyInfo/ds:X509Data/ds:X509Certificate", NS)]
    cert = certs[0]
    cd = sig.find(".//xd:SigningCertificate/xd:Cert/xd:CertDigest", NS)
    if cd is not None:
        method = cd.find("ds:DigestMethod", NS).get("Algorithm")
        want = base64.b64decode(cd.find("ds:DigestValue", NS).text)
        from cryptography.hazmat.primitives.serialization import Encoding
        match = [c for c in certs if DIGESTS[method](c.public_bytes(Encoding.DER)).digest() == want]
        if not match:
            problems.append("XAdES SigningCertificate digest names no KeyInfo certificate")
        else:
            cert = match[0]
    alg = si.find("ds:CanonicalizationMethod", NS).get("Algorithm")
    method = si.find("ds:SignatureMethod", NS).get("Algorithm")
    value = base64.b64decode("".join(sig.find("ds:SignatureValue", NS).text.split()))
    kind_, h = SIGS[method]
    data = c14n(si, alg)
    try:
        if kind_ == "rsa":
            cert.public_key().verify(value, data, padding.PKCS1v15(), h)
        else:
            n = len(value) // 2
            der = utils.encode_dss_signature(int.from_bytes(value[:n], "big"), int.from_bytes(value[n:], "big"))
            cert.public_key().verify(der, data, ec.ECDSA(h))
    except Exception as e:  # noqa: BLE001
        problems.append("SignatureValue does not verify (%s)" % type(e).__name__)
    return cert.subject.rfc4514_string(), problems


def main(paths):
    ok = True
    for path in paths:
        zf = zipfile.ZipFile(path)
        names = zf.namelist()
        if "[Content_Types].xml" in names:
            kind, parts = "ooxml", [n for n in names if n.startswith("_xmlsignatures/") and n.endswith(".xml")]
        else:
            kind, parts = "odf", [n for n in names if n == "META-INF/documentsignatures.xml"]
        sigs = []
        for p in parts:
            root = etree.fromstring(zf.read(p))
            sigs += [root] if root.tag == "{%s}Signature" % DS else root.findall("ds:Signature", NS)
        if not sigs:
            print("%s: no signatures" % path)
            ok = False
        for s in sigs:
            subject, problems = check_signature(s, zf, kind)
            print("%s: %s %s%s" % (path, "OK" if not problems else "FAIL", subject, "" if not problems else " — " + "; ".join(problems)))
            ok = ok and not problems
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
